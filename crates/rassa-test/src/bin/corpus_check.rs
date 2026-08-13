//! Crash-corpus runner: parse bytes, render Start/mid/End-1, check structure not pixels.

use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use rassa_core::{ImagePlane, RendererConfig, Size};
use rassa_fonts::FontconfigProvider;
use rassa_parse::{ParsedEvent, parse_script_bytes};
use rassa_render::RenderEngine;

const FRAME_WIDTH: i32 = 854;
const FRAME_HEIGHT: i32 = 480;
const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const PINNED_LIBASS_TESTS_COMMIT: &str = "9498737388cbd78cbab6b703821adc213a335995";

#[derive(Debug)]
struct Arguments {
    paths: Vec<PathBuf>,
    quiet: bool,
}

fn usage() -> &'static str {
    "usage: rassa-corpus-check [--quiet] <file-or-directory> [...]\n\
     Recursively consumes .ass files and arbitrary regular files.\n\
     For the pinned upstream crash corpus:\n\
       rassa-corpus-check /path/to/libass-tests/crash"
}

fn parse_arguments() -> Result<Arguments, String> {
    let mut paths = Vec::new();
    let mut quiet = false;
    for argument in env::args_os().skip(1) {
        if argument == "--quiet" {
            quiet = true;
        } else if argument == "--help" || argument == "-h" {
            println!("{}", usage());
            return Err(String::new());
        } else {
            paths.push(PathBuf::from(argument));
        }
    }
    if paths.is_empty() {
        return Err(usage().to_owned());
    }
    Ok(Arguments { paths, quiet })
}

fn collect_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    fn visit(path: &Path, from_directory: bool, files: &mut Vec<PathBuf>) -> Result<(), String> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| format!("unable to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
        if metadata.is_file() {
            if !from_directory
                || (path.extension().is_some_and(|extension| extension == "ass")
                    && path
                        .file_name()
                        .is_some_and(|name| !name.to_string_lossy().starts_with('.')))
            {
                files.push(path.to_path_buf());
            }
            return Ok(());
        }
        if !metadata.is_dir() {
            return Ok(());
        }
        let mut children = fs::read_dir(path)
            .map_err(|error| format!("unable to read {}: {error}", path.display()))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("unable to enumerate {}: {error}", path.display()))?;
        children.sort();
        for child in children {
            visit(&child, true, files)?;
        }
        Ok(())
    }

    let mut files = Vec::new();
    for path in paths {
        visit(path, false, &mut files)?;
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err("corpus contains no regular files".to_owned());
    }
    Ok(files)
}

fn sample_times(event: &ParsedEvent) -> Vec<i64> {
    let mut times = BTreeSet::from([event.start]);
    if event.duration > 1 {
        times.insert(event.start.saturating_add(event.duration / 2));
        times.insert(event.start.saturating_add(event.duration.saturating_sub(1)));
    }
    times.into_iter().collect()
}

fn validate_plane(
    path: &Path,
    timestamp: i64,
    index: usize,
    plane: &ImagePlane,
) -> Result<(), String> {
    let fail = |message: &str| {
        Err(format!(
            "{} at {timestamp} ms plane {index}: {message}",
            path.display()
        ))
    };
    let width = plane.size.width;
    let height = plane.size.height;
    let stride = plane.stride;
    if width < 0 || height < 0 {
        return fail("negative dimensions");
    }
    if stride < width || stride < 0 {
        return fail("stride is smaller than width or negative");
    }
    let destination_x = i64::from(plane.destination.x);
    let destination_y = i64::from(plane.destination.y);
    let right = destination_x.checked_add(i64::from(width)).ok_or_else(|| {
        format!(
            "{} at {timestamp} ms plane {index}: x bounds overflow",
            path.display()
        )
    })?;
    let bottom = destination_y
        .checked_add(i64::from(height))
        .ok_or_else(|| {
            format!(
                "{} at {timestamp} ms plane {index}: y bounds overflow",
                path.display()
            )
        })?;
    if destination_x < 0
        || destination_y < 0
        || right > i64::from(FRAME_WIDTH)
        || bottom > i64::from(FRAME_HEIGHT)
    {
        return fail("destination lies outside the configured frame");
    }

    let expected = if width == 0 || height == 0 {
        0
    } else {
        usize::try_from(stride)
            .ok()
            .and_then(|stride| stride.checked_mul(usize::try_from(height - 1).ok()?))
            .and_then(|prefix| prefix.checked_add(usize::try_from(width).ok()?))
            .ok_or_else(|| {
                format!(
                    "{} at {timestamp} ms plane {index}: bitmap length overflows",
                    path.display()
                )
            })?
    };
    if plane.bitmap.len() < expected {
        return fail("bitmap allocation is shorter than stride*(height-1)+width");
    }
    if expected > 0 {
        // Probe the final guaranteed byte, mirroring upstream fuzz/fuzz.c.
        std::hint::black_box(plane.bitmap[expected - 1]);
    }
    Ok(())
}

fn consume_file(path: &Path) -> Result<(usize, usize, usize), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("unable to inspect {}: {error}", path.display()))?;
    if metadata.len() > MAX_INPUT_BYTES {
        return Err(format!(
            "{} is {} bytes; maximum corpus input is {MAX_INPUT_BYTES}",
            path.display(),
            metadata.len()
        ));
    }
    let bytes =
        fs::read(path).map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    let track = parse_script_bytes(&bytes)
        .map_err(|error| format!("unable to parse {}: {error}", path.display()))?;
    let config = RendererConfig {
        frame: Size {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
        },
        storage: Size {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
        },
        ..RendererConfig::default()
    };
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let mut frames = 0;
    let mut planes = 0;
    for event in &track.events {
        for timestamp in sample_times(event) {
            let rendered =
                engine.render_frame_with_provider_and_config(&track, &provider, timestamp, &config);
            for (index, plane) in rendered.iter().enumerate() {
                validate_plane(path, timestamp, index, plane)?;
            }
            frames += 1;
            planes += rendered.len();
        }
    }
    Ok((track.events.len(), frames, planes))
}

fn main() -> ExitCode {
    let arguments = match parse_arguments() {
        Ok(arguments) => arguments,
        Err(message) if message.is_empty() => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let files = match collect_files(&arguments.paths) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::from(2);
        }
    };

    let mut total_events = 0;
    let mut total_frames = 0;
    let mut total_planes = 0;
    for path in &files {
        match consume_file(path) {
            Ok((events, frames, planes)) => {
                total_events += events;
                total_frames += frames;
                total_planes += planes;
                if !arguments.quiet {
                    println!(
                        "ok: {} ({events} events, {frames} sampled frames, {planes} planes)",
                        path.display()
                    );
                }
            }
            Err(error) => {
                eprintln!("error: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    println!(
        "corpus conformance ok: {} files, {total_events} events, {total_frames} sampled frames, \
         {total_planes} planes; libass-tests pin {PINNED_LIBASS_TESTS_COMMIT}",
        files.len()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_core::{Point, RgbaColor, ass};

    #[test]
    fn samples_match_upstream_fuzzer_boundaries() {
        let event = ParsedEvent {
            start: 100,
            duration: 9,
            ..ParsedEvent::default()
        };
        assert_eq!(sample_times(&event), [100, 104, 108]);
        assert_eq!(
            sample_times(&ParsedEvent {
                start: i64::MAX,
                duration: i64::MAX,
                ..ParsedEvent::default()
            }),
            [i64::MAX],
        );
    }

    #[test]
    fn plane_invariants_accept_last_row_without_stride_padding() {
        let plane = ImagePlane {
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 4,
            color: RgbaColor(0),
            destination: Point { x: 0, y: 0 },
            kind: ass::ImageType::Character,
            bitmap: vec![0; 6],
        };
        validate_plane(Path::new("fixture.ass"), 0, 0, &plane).unwrap();
    }

    #[test]
    fn plane_invariants_reject_short_or_out_of_frame_planes() {
        let mut plane = ImagePlane {
            size: Size {
                width: 2,
                height: 2,
            },
            stride: 4,
            color: RgbaColor(0),
            destination: Point { x: 0, y: 0 },
            kind: ass::ImageType::Character,
            bitmap: vec![0; 5],
        };
        assert!(validate_plane(Path::new("fixture.ass"), 0, 0, &plane).is_err());
        plane.bitmap.push(0);
        plane.destination.x = FRAME_WIDTH;
        assert!(validate_plane(Path::new("fixture.ass"), 0, 0, &plane).is_err());
    }
}
