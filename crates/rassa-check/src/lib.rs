use std::path::Path;

use rassa_core::{Point, RassaError, RassaResult, Rect, RendererConfig, Size};
use rassa_fonts::FontconfigProvider;
use rassa_parse::parse_script_text;
use rassa_render::RenderEngine;

pub const DEFAULT_SCRIPT: &str = r#"[Script Info]
PlayResX: 640
PlayResY: 360

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,sans,48,&H0000FF00,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,5,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0000,0000,0000,,Rassa render smoke
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderReport {
    pub width: i32,
    pub height: i32,
    pub plane_count: usize,
    pub lit_pixels: usize,
    pub bounds: Option<Rect>,
    pub pixels: Vec<u8>,
}

pub fn render_script(
    script: &str,
    time_ms: i64,
    width: i32,
    height: i32,
) -> RassaResult<RenderReport> {
    if width <= 0 || height <= 0 {
        return Err(RassaError::new("frame width and height must be positive"));
    }

    let track = parse_script_text(script)?;
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let config = RendererConfig {
        frame: Size { width, height },
        storage: Size { width, height },
        pixel_aspect: 1.0,
        font_scale: 1.0,
        line_spacing: 0.0,
        line_position: 0.0,
        hinting: rassa_core::ass::Hinting::None,
        shaping: rassa_core::ass::ShapingLevel::Complex,
        ..Default::default()
    };
    let planes = engine.render_frame_with_provider_and_config(&track, &provider, time_ms, &config);
    let mut report = RenderReport {
        width,
        height,
        plane_count: planes.len(),
        lit_pixels: 0,
        bounds: None,
        pixels: vec![0; width as usize * height as usize],
    };

    for plane in planes {
        let stride = plane.stride.max(0) as usize;
        let plane_width = plane.size.width.max(0) as usize;
        let plane_height = plane.size.height.max(0) as usize;
        if stride == 0 || plane_width == 0 || plane_height == 0 {
            continue;
        }

        for row in 0..plane_height {
            for column in 0..plane_width {
                let source_index = row * stride + column;
                let Some(&coverage) = plane.bitmap.get(source_index) else {
                    continue;
                };
                if coverage == 0 {
                    continue;
                }

                let destination = Point {
                    x: plane.destination.x + column as i32,
                    y: plane.destination.y + row as i32,
                };
                if destination.x < 0
                    || destination.y < 0
                    || destination.x >= width
                    || destination.y >= height
                {
                    continue;
                }

                let target_index = destination.y as usize * width as usize + destination.x as usize;
                report.pixels[target_index] = report.pixels[target_index].max(coverage);
                report.bounds = Some(match report.bounds {
                    Some(bounds) => Rect {
                        x_min: bounds.x_min.min(destination.x),
                        y_min: bounds.y_min.min(destination.y),
                        x_max: bounds.x_max.max(destination.x + 1),
                        y_max: bounds.y_max.max(destination.y + 1),
                    },
                    None => Rect {
                        x_min: destination.x,
                        y_min: destination.y,
                        x_max: destination.x + 1,
                        y_max: destination.y + 1,
                    },
                });
            }
        }
    }

    report.lit_pixels = report.pixels.iter().filter(|pixel| **pixel > 0).count();
    if report.plane_count == 0 || report.lit_pixels == 0 {
        return Err(RassaError::new("render produced no visible pixels"));
    }

    Ok(report)
}

pub fn render_report_to_pgm(report: &RenderReport) -> Vec<u8> {
    let mut output = format!("P5\n{} {}\n255\n", report.width, report.height).into_bytes();
    output.extend_from_slice(&report.pixels);
    output
}

pub fn render_script_file_to_pgm(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    time_ms: i64,
    width: i32,
    height: i32,
) -> RassaResult<RenderReport> {
    let script = std::fs::read_to_string(input.as_ref()).map_err(|error| {
        RassaError::new(format!(
            "failed to read {}: {error}",
            input.as_ref().display()
        ))
    })?;
    let report = render_script(&script, time_ms, width, height)?;
    std::fs::write(output.as_ref(), render_report_to_pgm(&report)).map_err(|error| {
        RassaError::new(format!(
            "failed to write {}: {error}",
            output.as_ref().display()
        ))
    })?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ASS: &str = r#"[Script Info]
PlayResX: 320
PlayResY: 180

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,sans,32,&H0000FF00,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,5,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,Rassa smoke
"#;

    #[test]
    fn render_report_confirms_non_empty_frame() {
        let report = render_script(SAMPLE_ASS, 500, 320, 180).expect("sample renders");
        assert!(report.plane_count > 0);
        assert!(report.lit_pixels > 0);
        assert!(report.bounds.is_some());
    }

    #[test]
    fn pgm_output_has_valid_header_and_pixels() {
        let report = render_script(SAMPLE_ASS, 500, 320, 180).expect("sample renders");
        let pgm = render_report_to_pgm(&report);
        assert!(pgm.starts_with(b"P5\n320 180\n255\n"));
        let header_len = b"P5\n320 180\n255\n".len();
        assert!(pgm[header_len..].iter().any(|byte| *byte > 0));
    }
}
