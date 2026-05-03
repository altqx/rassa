use std::{env, path::PathBuf, process::ExitCode};

use rassa_check::{DEFAULT_SCRIPT, render_report_to_pgm, render_script, render_script_file_to_pgm};

#[derive(Debug)]
struct Args {
    input: Option<PathBuf>,
    output: PathBuf,
    time_ms: i64,
    width: i32,
    height: i32,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rassa-check: {error}");
            eprintln!(
                "usage: rassa-check [--input file.ass] [--output out.pgm] [--time-ms 500] [--width 640] [--height 360]"
            );
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args(env::args().skip(1))?;
    let report = if let Some(input) = &args.input {
        render_script_file_to_pgm(input, &args.output, args.time_ms, args.width, args.height)
            .map_err(|error| error.to_string())?
    } else {
        let report = render_script(DEFAULT_SCRIPT, args.time_ms, args.width, args.height)
            .map_err(|error| error.to_string())?;
        std::fs::write(&args.output, render_report_to_pgm(&report))
            .map_err(|error| format!("failed to write {}: {error}", args.output.display()))?;
        report
    };

    println!(
        "render ok: planes={} lit_pixels={} bounds={:?} output={}",
        report.plane_count,
        report.lit_pixels,
        report.bounds,
        args.output.display()
    );
    Ok(())
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args {
        input: None,
        output: PathBuf::from("rassa-check.pgm"),
        time_ms: 500,
        width: 640,
        height: 360,
    };

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "-i" => parsed.input = Some(next_path(&mut args, &arg)?),
            "--output" | "-o" => parsed.output = next_path(&mut args, &arg)?,
            "--time-ms" | "-t" => {
                parsed.time_ms = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "invalid --time-ms".to_string())?
            }
            "--width" | "-w" => {
                parsed.width = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "invalid --width".to_string())?
            }
            "--height" | "-h" => {
                parsed.height = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "invalid --height".to_string())?
            }
            "--help" => return Err("help requested".to_string()),
            value if !value.starts_with('-') && parsed.input.is_none() => {
                parsed.input = Some(PathBuf::from(value));
            }
            value if !value.starts_with('-') => {
                parsed.output = PathBuf::from(value);
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }

    Ok(parsed)
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}"))
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    Ok(PathBuf::from(next_value(args, flag)?))
}
