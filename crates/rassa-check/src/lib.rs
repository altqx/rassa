use std::path::Path;

use image::{ColorType, ImageEncoder, codecs};
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

pub const DEFAULT_BACKGROUND_RGB: [u8; 3] = [0x2B, 0xA4, 0xEF];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderReport {
    pub width: i32,
    pub height: i32,
    pub plane_count: usize,
    pub lit_pixels: usize,
    pub bounds: Option<Rect>,
    pub pixels: Vec<u8>,
    pub rgb_pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Pgm,
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        match path
            .as_ref()
            .extension()?
            .to_str()?
            .to_ascii_lowercase()
            .as_str()
        {
            "pgm" => Some(Self::Pgm),
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "pgm" => Some(Self::Pgm),
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }
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
        rgb_pixels: vec![0; width as usize * height as usize * 3],
    };
    for pixel in report.rgb_pixels.chunks_exact_mut(3) {
        pixel.copy_from_slice(&DEFAULT_BACKGROUND_RGB);
    }

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
                composite_plane_pixel(
                    &mut report.rgb_pixels,
                    target_index,
                    plane.color.0,
                    coverage,
                );
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

fn composite_plane_pixel(rgb_pixels: &mut [u8], target_index: usize, color: u32, coverage: u8) {
    let inverse_alpha = (color & 0xff) as u8;
    if coverage == 0 || inverse_alpha == 255 {
        return;
    }

    let source = [(color >> 24) as u8, (color >> 16) as u8, (color >> 8) as u8];
    let offset = target_index * 3;
    let Some(destination) = rgb_pixels.get_mut(offset..offset + 3) else {
        return;
    };
    for channel in 0..3 {
        destination[channel] = blend_channel(
            source[channel],
            destination[channel],
            coverage,
            inverse_alpha,
        );
    }
}

fn blend_channel(source: u8, destination: u8, coverage: u8, inverse_alpha: u8) -> u8 {
    let k = i32::from(coverage) * 129 * i32::from(255 - inverse_alpha);
    let blended = i32::from(destination)
        - (((i32::from(destination) - i32::from(source)) * k + (1 << 22)) >> 23);
    blended.clamp(0, 255) as u8
}

pub fn render_report_to_pgm(report: &RenderReport) -> Vec<u8> {
    let mut output = format!("P5\n{} {}\n255\n", report.width, report.height).into_bytes();
    output.extend_from_slice(&report.pixels);
    output
}

pub fn render_report_to_image_bytes(
    report: &RenderReport,
    format: ImageFormat,
) -> RassaResult<Vec<u8>> {
    match format {
        ImageFormat::Pgm => Ok(render_report_to_pgm(report)),
        ImageFormat::Png => encode_png(report),
        ImageFormat::Jpeg => encode_jpeg(report),
    }
}

fn encode_png(report: &RenderReport) -> RassaResult<Vec<u8>> {
    let mut output = Vec::new();
    codecs::png::PngEncoder::new(&mut output)
        .write_image(
            &report.rgb_pixels,
            report.width as u32,
            report.height as u32,
            ColorType::Rgb8.into(),
        )
        .map_err(|error| RassaError::new(format!("failed to encode PNG: {error}")))?;
    Ok(output)
}

fn encode_jpeg(report: &RenderReport) -> RassaResult<Vec<u8>> {
    let mut output = Vec::new();
    codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 92)
        .encode(
            &report.rgb_pixels,
            report.width as u32,
            report.height as u32,
            ColorType::Rgb8.into(),
        )
        .map_err(|error| RassaError::new(format!("failed to encode JPEG: {error}")))?;
    Ok(output)
}

pub fn render_script_file_to_pgm(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    time_ms: i64,
    width: i32,
    height: i32,
) -> RassaResult<RenderReport> {
    render_script_file_to_image(input, output, time_ms, width, height, ImageFormat::Pgm)
}

pub fn render_script_file_to_image(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    time_ms: i64,
    width: i32,
    height: i32,
    format: ImageFormat,
) -> RassaResult<RenderReport> {
    let script = std::fs::read_to_string(input.as_ref()).map_err(|error| {
        RassaError::new(format!(
            "failed to read {}: {error}",
            input.as_ref().display()
        ))
    })?;
    let report = render_script(&script, time_ms, width, height)?;
    std::fs::write(
        output.as_ref(),
        render_report_to_image_bytes(&report, format)?,
    )
    .map_err(|error| {
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

    #[test]
    fn png_output_has_valid_signature() {
        let report = render_script(SAMPLE_ASS, 500, 320, 180).expect("sample renders");
        let png = render_report_to_image_bytes(&report, ImageFormat::Png).expect("png encodes");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn render_report_composites_ass_colors_over_libass_blue_background() {
        let report = render_script(SAMPLE_ASS, 500, 320, 180).expect("sample renders");
        assert_eq!(&report.rgb_pixels[..3], &[0x2B, 0xA4, 0xEF]);
        assert!(
            report
                .rgb_pixels
                .chunks_exact(3)
                .any(|pixel| pixel[1] > pixel[0] && pixel[1] > pixel[2])
        );
        assert!(
            report
                .rgb_pixels
                .chunks_exact(3)
                .any(|pixel| pixel[0] < 8 && pixel[1] < 8 && pixel[2] < 8)
        );
    }

    #[test]
    fn blend_channel_matches_libass_compare_c_rounding() {
        assert_eq!(blend_channel(0, 239, 128, 0), 119);
        assert_eq!(blend_channel(0, 164, 128, 0), 82);
        assert_eq!(blend_channel(0, 43, 128, 0), 21);
        assert_eq!(blend_channel(0, 239, 255, 0), 0);
        assert_eq!(blend_channel(0, 239, 128, 128), 179);
    }

    #[test]
    fn jpeg_output_has_valid_signature() {
        let report = render_script(SAMPLE_ASS, 500, 320, 180).expect("sample renders");
        let jpeg = render_report_to_image_bytes(&report, ImageFormat::Jpeg).expect("jpeg encodes");
        assert!(jpeg.starts_with(&[0xFF, 0xD8, 0xFF]));
        assert!(jpeg.ends_with(&[0xFF, 0xD9]));
    }
}
