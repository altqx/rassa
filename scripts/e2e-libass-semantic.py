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
) -> bool:
    if left is None or right is None:
        return left == right
    # Geometry is semantic, but ink extents also contain the rasterizer's
    # outline/blur support. Rassa intentionally does not use libass's raster
    # backend, so allow three percent of the output axis while still comparing
    # timing, visibility, plane kinds/colours/order and per-kind placement.
    # This remains far tighter than the old broad pixel-error thresholds and
    # still catches line placement, collision, scaling and clipping mistakes.
    tolerance_x = max(6, math.ceil(width * 0.03))
    tolerance_y = max(6, math.ceil(height * 0.03))
    return (
        abs(left[0] - right[0]) <= tolerance_x
        and abs(left[2] - right[2]) <= tolerance_x
        and abs(left[1] - right[1]) <= tolerance_y
        and abs(left[3] - right[3]) <= tolerance_y
    )


def compare_frame(reference: Frame, candidate: Frame, width: int, height: int) -> list[str]:
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
        if not bounds_close(expected, actual, width, height):
            label = "all" if kind is None else str(kind)
            errors.append(f"visible bounds for kind {label} differ: libass={expected} rassa={actual}")
    return errors


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
    if not frames:
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

    print(f"semantic differential: {len(frames) - failures}/{len(frames)} frames passed")
    return 0 if failures == 0 or args.report_only else 1


if __name__ == "__main__":
    sys.exit(main())
