#!/usr/bin/env python3
"""Compare libass and Rassa semantics without requiring raster pixel identity."""

from __future__ import annotations

import argparse
import dataclasses
import math
import os
from pathlib import Path
import re
import struct
import subprocess
import sys


@dataclasses.dataclass(frozen=True)
class Image:
    kind: int
    color: int
    x: int
    y: int
    width: int
    height: int
    lit: int
    inner: tuple[int, int, int, int] | None

    def visible_bounds(self) -> tuple[int, int, int, int] | None:
        if self.inner is None:
            return None
        left, top, right, bottom = self.inner
        return self.x + left, self.y + top, self.x + right + 1, self.y + bottom + 1


@dataclasses.dataclass(frozen=True)
class Frame:
    styles: int
    events: int
    play_res: tuple[int, int]
    active: int
    images: tuple[Image, ...]


def png_size(path: Path) -> tuple[int, int]:
    data = path.read_bytes()[:24]
    if len(data) != 24 or data[:8] != b"\x89PNG\r\n\x1a\n" or data[12:16] != b"IHDR":
        raise ValueError(f"not a PNG: {path}")
    return struct.unpack(">II", data[16:24])


def load_probe_output(
    probe: Path,
    library_dir: Path,
    script: Path,
    fonts_dir: Path,
    time_ms: int,
    storage_width: int,
    storage_height: int,
    frame_width: int,
    frame_height: int,
) -> Frame:
    env = os.environ.copy()
    env["LD_LIBRARY_PATH"] = str(library_dir)
    result = subprocess.run(
        [
            str(probe),
            str(script),
            str(fonts_dir),
            str(time_ms),
            str(storage_width),
            str(storage_height),
            str(frame_width),
            str(frame_height),
        ],
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"probe failed ({result.returncode}) for {script} at {time_ms} ms "
            f"with {library_dir}:\n{result.stderr}"
        )

    styles = events = play_x = play_y = active = None
    images: list[Image] = []
    for line in result.stdout.splitlines():
        fields = line.split()
        if not fields:
            continue
        if fields[0] == "TRACK" and len(fields) == 6:
            styles, events, play_x, play_y, active = map(int, fields[1:])
        elif fields[0] == "IMAGE" and len(fields) == 15:
            _, _index, kind, color, x, y, width_s, height_s, _stride, lit, *tail = fields
            min_x, min_y, max_x, max_y, _alpha_sum = map(int, tail)
            images.append(
                Image(
                    kind=int(kind),
                    color=int(color, 16),
                    x=int(x),
                    y=int(y),
                    width=int(width_s),
                    height=int(height_s),
                    lit=int(lit),
                    inner=None if int(lit) == 0 else (min_x, min_y, max_x, max_y),
                )
            )
    if None in (styles, events, play_x, play_y, active):
        raise RuntimeError(f"malformed probe output for {script}:\n{result.stdout}")
    return Frame(styles, events, (play_x, play_y), active, tuple(images))


def union_bounds(images: tuple[Image, ...], kind: int | None = None) -> tuple[int, int, int, int] | None:
    bounds = [image.visible_bounds() for image in images if kind is None or image.kind == kind]
    visible = [item for item in bounds if item is not None]
    if not visible:
        return None
    return (
        min(item[0] for item in visible),
        min(item[1] for item in visible),
        max(item[2] for item in visible),
        max(item[3] for item in visible),
    )


def compact_kind_order(images: tuple[Image, ...]) -> tuple[int, ...]:
    order: list[int] = []
    for image in images:
        if not order or order[-1] != image.kind:
            order.append(image.kind)
    return tuple(order)


def bounds_close(
    left: tuple[int, int, int, int] | None,
    right: tuple[int, int, int, int] | None,
    width: int,
    height: int,
    tolerance_fraction: float = 0.03,
) -> bool:
    if left is None or right is None:
        return left == right
    # Geometry is semantic, but ink extents also contain the rasterizer's
    # outline/blur support. Rassa intentionally does not use libass's raster
    # backend, so allow a caller-selected fraction of each output axis while
    # still comparing timing, visibility, plane kinds/colours/order and
    # per-kind placement. Official PNG snapshots use 3%; dense animation and
    # high-resolution transform probes use 1.5% to catch partial wipes and
    # projective drift hidden by the broader raster-support allowance.
    tolerance_x = max(6, math.ceil(width * tolerance_fraction))
    tolerance_y = max(6, math.ceil(height * tolerance_fraction))
    return (
        abs(left[0] - right[0]) <= tolerance_x
        and abs(left[2] - right[2]) <= tolerance_x
        and abs(left[1] - right[1]) <= tolerance_y
        and abs(left[3] - right[3]) <= tolerance_y
    )


def compare_frame(
    reference: Frame,
    candidate: Frame,
    width: int,
    height: int,
    tolerance_fraction: float = 0.03,
) -> list[str]:
    errors: list[str] = []
    if (reference.styles, reference.events, reference.active) != (
        candidate.styles,
        candidate.events,
        candidate.active,
    ):
        errors.append(
            "track/timing differs: "
            f"libass={(reference.styles, reference.events, reference.active)} "
            f"rassa={(candidate.styles, candidate.events, candidate.active)}"
        )
    if bool(reference.images) != bool(candidate.images):
        errors.append(f"visibility differs: libass={len(reference.images)} rassa={len(candidate.images)}")
        return errors

    reference_colors = {(image.kind, image.color) for image in reference.images if image.lit}
    candidate_colors = {(image.kind, image.color) for image in candidate.images if image.lit}
    if reference_colors != candidate_colors:
        errors.append(f"plane kind/color set differs: libass={reference_colors} rassa={candidate_colors}")
    if compact_kind_order(reference.images) != compact_kind_order(candidate.images):
        errors.append(
            "plane ordering differs: "
            f"libass={compact_kind_order(reference.images)} "
            f"rassa={compact_kind_order(candidate.images)}"
        )

    kinds = {image.kind for image in reference.images} | {image.kind for image in candidate.images}
    for kind in [None, *sorted(kinds)]:
        expected = union_bounds(reference.images, kind)
        actual = union_bounds(candidate.images, kind)
        if not bounds_close(expected, actual, width, height, tolerance_fraction):
            label = "all" if kind is None else str(kind)
            errors.append(f"visible bounds for kind {label} differ: libass={expected} rassa={actual}")
    return errors


@dataclasses.dataclass(frozen=True)
class DenseFrame:
    script: Path
    label: str
    time_ms: int
    storage_width: int
    storage_height: int
    frame_width: int
    frame_height: int
    tolerance_fraction: float


def frame_times(start_ms: int, end_ms: int, fps: int = 24) -> list[int]:
    """Return every CFR sample in [start_ms, end_ms), including the last instant."""
    duration = end_ms - start_ms
    count = math.ceil(duration * fps / 1000)
    times = {
        start_ms + min(duration - 1, frame * 1000 // fps)
        for frame in range(count)
    }
    times.add(end_ms - 1)
    return sorted(times)


def dense_animation_frames(workspace: Path, regression: Path) -> list[DenseFrame]:
    """Animation/high-resolution oracles not represented by upstream PNG snapshots."""
    frames: list[DenseFrame] = []

    # Projective camera distance depends on storage resolution.  The original
    # 220x140 fixture can look close while the same script diverges by hundreds
    # of pixels at 1080p, so exercise its complete one-second animation at the
    # resolution used by the visual comparison.
    vector = workspace / "crates/rassa-test/fixtures/libass/compare/edge/vector_transform.ass"
    for time_ms in frame_times(0, 1000):
        frames.append(
            DenseFrame(
                vector,
                "edge/vector_transform.ass[1920x1080-dense-24fps]",
                time_ms,
                1920,
                1080,
                1920,
                1080,
                0.015,
            )
        )

    decimal_ring = (
        workspace / "crates/rassa-test/fixtures/libass/compare/edge/decimal_thin_ring.ass"
    )
    if decimal_ring.is_file():
        frames.append(
            DenseFrame(
                decimal_ring,
                "edge/decimal_thin_ring.ass[1920x1080]",
                500,
                1920,
                1080,
                1920,
                1080,
                0.015,
            )
        )

    decimal_vector_clip = (
        workspace / "crates/rassa-test/fixtures/libass/compare/edge/decimal_vector_clip.ass"
    )
    if decimal_vector_clip.is_file():
        frames.append(
            DenseFrame(
                decimal_vector_clip,
                "edge/decimal_vector_clip.ass[1920x1080]",
                500,
                1920,
                1080,
                1920,
                1080,
                0.015,
            )
        )

    anisotropic_shear = (
        workspace / "crates/rassa-test/fixtures/libass/compare/edge/anisotropic_shear.ass"
    )
    if anisotropic_shear.is_file():
        for time_ms in range(500, 7000, 1000):
            frames.append(
                DenseFrame(
                    anisotropic_shear,
                    "edge/anisotropic_shear.ass[1920x1080]",
                    time_ms,
                    1920,
                    1080,
                    1920,
                    1080,
                    0.015,
                )
            )

    # The upstream karaoke run-split fixture has PlayRes 640x120. At a
    # 1920x1080 output, font advances use the 9x vertical screen scale while
    # x2scr margin width uses the 3x horizontal scale. Comparing only the
    # fixture's native PNG misses the resulting three-line topology.
    karaoke_runsplits = regression / "karaoke" / "karaoke-and-runsplits.ass"
    if karaoke_runsplits.is_file():
        frames.append(
            DenseFrame(
                karaoke_runsplits,
                "karaoke/karaoke-and-runsplits.ass[1920x1080-wrap]",
                3120,
                1920,
                1080,
                1920,
                1080,
                0.015,
            )
        )

    return frames


def red_character_widths(frame: Frame) -> list[int]:
    """Widths of visible red character planes in libass's display-list order."""
    return [
        image.width
        for image in frame.images
        if image.kind == 0 and image.lit and image.color >> 8 == 0xFF0000
    ]


def correlation(left: list[float], right: list[float]) -> float:
    if len(left) != len(right) or not left:
        return 0.0
    left_mean = sum(left) / len(left)
    right_mean = sum(right) / len(right)
    covariance = sum(
        (left_value - left_mean) * (right_value - right_mean)
        for left_value, right_value in zip(left, right, strict=True)
    )
    left_variance = sum((value - left_mean) ** 2 for value in left)
    right_variance = sum((value - right_mean) ** 2 for value in right)
    if not left_variance or not right_variance:
        return 1.0 if left == right else 0.0
    return covariance / math.sqrt(left_variance * right_variance)


def compare_vertical_karaoke_curve(
    probe: Path,
    libass_lib_dir: Path,
    rassa_lib_dir: Path,
    libass_tests: Path,
) -> tuple[int, int, int]:
    """Compare every encoded-frame sample of upstream's rotated `\\K` fixture.

    Whole-event ink bounds are intentionally not used here: immediately after
    a syllable boundary, one rasterizer can light an antialiased edge while the
    other still has zero coverage, making a union bound jump by a full glyph.
    The observable semantic is the monotonic, progressive red reveal.
    """
    regression = libass_tests / "regression"
    script = regression / "karaoke" / "216-vertical.ass"
    fonts = regression / ".fonts"
    durations = [370, 810, 650, 390, 400, 200, 220, 580, 250, 360, 430, 360]
    starts: list[int] = []
    elapsed = 0
    for duration in durations:
        starts.append(elapsed)
        elapsed += duration

    libraries = {"libass": libass_lib_dir, "rassa": rassa_lib_dir}
    full_widths: dict[str, list[int]] = {name: [] for name in libraries}
    for syllable, (start, duration) in enumerate(zip(starts, durations, strict=True)):
        sample_time = min(4969, 30 + start + duration)
        for name, library in libraries.items():
            frame = load_probe_output(
                probe, library, script, fonts, sample_time, 1920, 1080, 1920, 1080
            )
            widths = red_character_widths(frame)
            if len(widths) <= syllable:
                raise RuntimeError(
                    f"{name} has no completed red plane for syllable {syllable + 1} "
                    f"at {sample_time} ms: {widths}"
                )
            full_widths[name].append(widths[syllable])

    times = [30 + frame * 1000 // 24 for frame in range(119)]
    series: dict[str, list[list[float]]] = {
        name: [[] for _ in durations] for name in libraries
    }
    last_widths: dict[str, dict[int, int]] = {name: {} for name in libraries}
    failed_frames = 0
    for time_ms in times:
        elapsed = time_ms - 30
        current = max(index for index, start in enumerate(starts) if start <= elapsed)
        progress = (elapsed - starts[current]) / durations[current]
        logical_width = 62 * (2 if current == 1 else 1)
        expected_advance = 1 + round(logical_width * progress)
        loaded = {
            name: load_probe_output(
                probe, library, script, fonts, time_ms, 1920, 1080, 1920, 1080
            )
            for name, library in libraries.items()
        }
        widths = {name: red_character_widths(frame) for name, frame in loaded.items()}
        errors: list[str] = []
        libass_track = loaded["libass"]
        rassa_track = loaded["rassa"]
        if (
            libass_track.styles,
            libass_track.events,
            libass_track.active,
        ) != (rassa_track.styles, rassa_track.events, rassa_track.active):
            errors.append("track/timing differs")
        if abs(len(widths["libass"]) - len(widths["rassa"])) > 1:
            errors.append(
                "revealed syllable count differs: "
                f"libass={len(widths['libass'])} rassa={len(widths['rassa'])}"
            )
        for name in libraries:
            width = widths[name][current] if len(widths[name]) > current else 0
            full_width = full_widths[name][current]
            series[name][current].append(width / full_width if full_width else 0.0)
            expected_width = min(full_width, expected_advance)
            if abs(width - expected_width) > 6:
                errors.append(
                    f"{name} progressive width differs: actual={width} expected={expected_width}"
                )
            previous = last_widths[name].get(current)
            if previous is not None and width < previous:
                errors.append(f"{name} reveal is non-monotonic: {previous} -> {width}")
            last_widths[name][current] = width
        if errors:
            failed_frames += 1
            print(f"FAIL karaoke/216-vertical.ass[reveal-curve] at {time_ms} ms")
            for error in errors:
                print(f"  {error}")
        else:
            print(f"PASS karaoke/216-vertical.ass[reveal-curve] at {time_ms} ms")

    curve_failures = 0
    for syllable in range(len(durations)):
        libass_series = series["libass"][syllable]
        rassa_series = series["rassa"][syllable]
        curve_correlation = correlation(libass_series, rassa_series)
        if (
            len(set(libass_series)) < 2
            or len(set(rassa_series)) < 2
            or curve_correlation < 0.95
        ):
            curve_failures += 1
            print(
                f"FAIL karaoke/216-vertical.ass syllable {syllable + 1} curve: "
                f"distinct={len(set(libass_series))}/{len(set(rassa_series))} "
                f"correlation={curve_correlation:.4f}"
            )
    return len(times) - failed_frames, len(times), curve_failures


def regression_scale(script: Path) -> tuple[int, int]:
    scale_file = script.parent / "scale"
    if not scale_file.is_file():
        return 1, 1
    match = re.fullmatch(r"(\d+)x(\d+)", scale_file.read_text().strip())
    if not match or int(match.group(1)) < 1 or int(match.group(2)) < 1:
        raise ValueError(f"invalid libass-tests scale file: {scale_file}")
    return int(match.group(1)), int(match.group(2))


def reference_frames(
    regression_dir: Path, filter_text: str | None
) -> list[tuple[Path, Path, int, int, int]]:
    frames: list[tuple[Path, Path, int, int, int]] = []
    pattern = re.compile(r"^(?P<stem>.+)-(?P<time>\d+)\.png$")
    for png in sorted(regression_dir.glob("*/*.png")):
        match = pattern.match(png.name)
        if not match or png.name.endswith("_diff.png"):
            continue
        script = png.with_name(match.group("stem") + ".ass")
        relative = str(script.relative_to(regression_dir))
        if not script.is_file() or filter_text and filter_text not in relative:
            continue
        scale_x, scale_y = regression_scale(script)
        frames.append((script, png, int(match.group("time")), scale_x, scale_y))
    return frames


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--rassa-lib-dir", type=Path, required=True)
    parser.add_argument("--libass-lib-dir", type=Path, required=True)
    parser.add_argument("--libass-tests", type=Path, required=True)
    parser.add_argument("--filter")
    parser.add_argument("--report-only", action="store_true")
    args = parser.parse_args()

    regression = args.libass_tests / "regression"
    fonts = regression / ".fonts"
    frames = reference_frames(regression, args.filter)
    workspace = Path(__file__).resolve().parent.parent
    dense_frames = [
        frame
        for frame in dense_animation_frames(workspace, regression)
        if not args.filter or args.filter in frame.label
    ]
    vertical_label = "karaoke/216-vertical.ass"
    compare_vertical = not args.filter or args.filter in vertical_label
    if not frames and not dense_frames and not compare_vertical:
        parser.error("no compatible libass-test frames found")

    failures = 0
    for script, png, time_ms, scale_x, scale_y in frames:
        storage_width, storage_height = png_size(png)
        frame_width = storage_width * scale_x
        frame_height = storage_height * scale_y
        reference = load_probe_output(
            args.probe,
            args.libass_lib_dir,
            script,
            fonts,
            time_ms,
            storage_width,
            storage_height,
            frame_width,
            frame_height,
        )
        candidate = load_probe_output(
            args.probe,
            args.rassa_lib_dir,
            script,
            fonts,
            time_ms,
            storage_width,
            storage_height,
            frame_width,
            frame_height,
        )
        errors = compare_frame(reference, candidate, frame_width, frame_height)
        relative = script.relative_to(regression)
        if errors:
            failures += 1
            print(f"FAIL {relative} at {time_ms} ms")
            for error in errors:
                print(f"  {error}")
        else:
            print(f"PASS {relative} at {time_ms} ms")

    for frame in dense_frames:
        reference = load_probe_output(
            args.probe,
            args.libass_lib_dir,
            frame.script,
            fonts,
            frame.time_ms,
            frame.storage_width,
            frame.storage_height,
            frame.frame_width,
            frame.frame_height,
        )
        candidate = load_probe_output(
            args.probe,
            args.rassa_lib_dir,
            frame.script,
            fonts,
            frame.time_ms,
            frame.storage_width,
            frame.storage_height,
            frame.frame_width,
            frame.frame_height,
        )
        errors = compare_frame(
            reference,
            candidate,
            frame.frame_width,
            frame.frame_height,
            frame.tolerance_fraction,
        )
        if errors:
            failures += 1
            print(f"FAIL {frame.label} at {frame.time_ms} ms")
            for error in errors:
                print(f"  {error}")
        else:
            print(f"PASS {frame.label} at {frame.time_ms} ms")

    vertical_passed = vertical_total = vertical_curve_failures = 0
    if compare_vertical:
        vertical_passed, vertical_total, vertical_curve_failures = (
            compare_vertical_karaoke_curve(
                args.probe,
                args.libass_lib_dir,
                args.rassa_lib_dir,
                args.libass_tests,
            )
        )
        failures += vertical_total - vertical_passed

    total = len(frames) + len(dense_frames) + vertical_total
    print(f"semantic differential: {total - failures}/{total} frames passed")
    if vertical_curve_failures:
        print(
            "semantic differential: "
            f"{vertical_curve_failures}/12 vertical karaoke reveal curves failed"
        )
    succeeded = failures == 0 and vertical_curve_failures == 0
    return 0 if succeeded or args.report_only else 1


if __name__ == "__main__":
    sys.exit(main())
