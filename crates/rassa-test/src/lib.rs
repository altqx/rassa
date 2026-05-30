#[cfg(test)]
use rassa_core::Size;
use rassa_core::{RendererConfig, ass};
use rassa_fonts::FontconfigProvider;
use rassa_parse::{ParsedTrack, parse_script_text};
use rassa_render::RenderEngine;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaneSummary {
    pub kind: ass::ImageType,
    pub color: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub lit_pixels: usize,
}

pub fn parse_fixture(script: &str) -> ParsedTrack {
    parse_script_text(script).expect("fixture should parse")
}

pub fn render_fixture(script: &str, now_ms: i64) -> Vec<PlaneSummary> {
    let track = parse_fixture(script);
    render_track(&track, now_ms)
}

pub fn render_track(track: &ParsedTrack, now_ms: i64) -> Vec<PlaneSummary> {
    let provider = FontconfigProvider::new();
    let engine = RenderEngine::new();
    summarize_planes(&engine.render_frame_with_provider(track, &provider, now_ms))
}

pub fn render_track_planes<P: rassa_fonts::FontProvider>(
    track: &ParsedTrack,
    provider: &P,
    now_ms: i64,
) -> Vec<rassa_core::ImagePlane> {
    let engine = RenderEngine::new();
    engine.render_frame_with_provider(track, provider, now_ms)
}

pub fn render_track_planes_with_config<P: rassa_fonts::FontProvider>(
    track: &ParsedTrack,
    provider: &P,
    now_ms: i64,
    config: &RendererConfig,
) -> Vec<rassa_core::ImagePlane> {
    let engine = RenderEngine::new();
    engine.render_frame_with_provider_and_config(track, provider, now_ms, config)
}

fn summarize_planes(planes: &[rassa_core::ImagePlane]) -> Vec<PlaneSummary> {
    planes
        .iter()
        .map(|plane| PlaneSummary {
            kind: plane.kind,
            color: plane.color.0,
            x: plane.destination.x,
            y: plane.destination.y,
            width: plane.size.width,
            height: plane.size.height,
            lit_pixels: plane.bitmap.iter().filter(|value| **value > 0).count(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_fonts::{AttachedFontProvider, FontAttachment, FontProvider};
    use std::{
        env,
        ffi::{CStr, CString, c_char},
        fs,
        path::PathBuf,
        process::Command,
        ptr,
        time::{SystemTime, UNIX_EPOCH},
    };

    const INLINE_OVERRIDE_FIXTURE: &str = "[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,36,&H00112233,&H00445566,&H000A0B0C,&H00101010,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\an7\\pos(20,20)\\t(0,1000,\\1c&H00223344&)}{\\K100}Test";
    const STYLE_ONLY_FIXTURE: &str = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Alt,sans,18,&H00ABCDEF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,11,12,13,1";

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("rassa-test should live under crates/rassa-test")
            .to_path_buf()
    }

    fn write_temp_fixture(name: &str, content: &str) -> PathBuf {
        let mut path = env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should work")
            .as_nanos();
        path.push(format!("rassa-test-{name}-{stamp}.ass"));
        fs::write(&path, content).expect("fixture file should be written");
        path
    }

    fn style_override_list(values: &[&str]) -> (Vec<CString>, Vec<*mut c_char>) {
        let storage = values
            .iter()
            .map(|value| CString::new(*value).expect("override cstring"))
            .collect::<Vec<_>>();
        let mut raw = storage
            .iter()
            .map(|value| value.as_ptr() as *mut c_char)
            .collect::<Vec<_>>();
        raw.push(ptr::null_mut());
        (storage, raw)
    }

    fn total_image_area(mut image: *mut rassa_capi::ASS_Image) -> i32 {
        let mut total = 0;
        unsafe {
            while !image.is_null() {
                total += (*image).w * (*image).h;
                image = (*image).next;
            }
        }
        total
    }

    fn image_vertical_span(mut image: *mut rassa_capi::ASS_Image) -> i32 {
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        unsafe {
            while !image.is_null() {
                min_y = min_y.min((*image).dst_y);
                max_y = max_y.max((*image).dst_y + (*image).h);
                image = (*image).next;
            }
        }
        if min_y == i32::MAX { 0 } else { max_y - min_y }
    }

    fn image_bounds(mut image: *mut rassa_capi::ASS_Image) -> Option<(i32, i32, i32, i32)> {
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        unsafe {
            while !image.is_null() {
                min_x = min_x.min((*image).dst_x);
                min_y = min_y.min((*image).dst_y);
                max_x = max_x.max((*image).dst_x + (*image).w);
                max_y = max_y.max((*image).dst_y + (*image).h);
                image = (*image).next;
            }
        }
        (min_x != i32::MAX).then_some((min_x, min_y, max_x, max_y))
    }

    fn image_colors(mut image: *mut rassa_capi::ASS_Image) -> Vec<u32> {
        let mut colors = Vec::new();
        unsafe {
            while !image.is_null() {
                colors.push((*image).color);
                image = (*image).next;
            }
        }
        colors
    }

    fn image_types(mut image: *mut rassa_capi::ASS_Image) -> Vec<i32> {
        let mut types = Vec::new();
        unsafe {
            while !image.is_null() {
                types.push((*image).type_);
                image = (*image).next;
            }
        }
        types
    }

    fn image_min_y_for_color(mut image: *mut rassa_capi::ASS_Image, color: u32) -> Option<i32> {
        let mut min_y = i32::MAX;
        unsafe {
            while !image.is_null() {
                if (*image).type_ == ass::ImageType::Character as i32 && (*image).color == color {
                    min_y = min_y.min((*image).dst_y);
                }
                image = (*image).next;
            }
        }
        (min_y != i32::MAX).then_some(min_y)
    }

    type ImageSignature = (u32, i32, i32, i32, i32, Vec<u8>);

    fn image_signatures(mut image: *mut rassa_capi::ASS_Image) -> Vec<ImageSignature> {
        let mut signatures = Vec::new();
        unsafe {
            while !image.is_null() {
                let width = (*image).w.max(0) as usize;
                let height = (*image).h.max(0) as usize;
                let stride = (*image).stride.max(0) as usize;
                let mut bitmap = Vec::with_capacity(width.saturating_mul(height));
                if !(*image).bitmap.is_null() && width > 0 && height > 0 && stride >= width {
                    for row in 0..height {
                        let row_ptr = (*image).bitmap.add(row * stride);
                        let row_slice = std::slice::from_raw_parts(row_ptr, width);
                        bitmap.extend_from_slice(row_slice);
                    }
                }
                signatures.push((
                    (*image).color,
                    (*image).dst_x,
                    (*image).dst_y,
                    (*image).w,
                    (*image).h,
                    bitmap,
                ));
                image = (*image).next;
            }
        }
        signatures
    }

    fn has_large_solid_bitmap(signatures: &[ImageSignature]) -> bool {
        signatures.iter().any(|(_, _, _, width, height, bitmap)| {
            *width >= 20
                && *height >= 10
                && !bitmap.is_empty()
                && bitmap.iter().all(|value| *value == bitmap[0])
                && bitmap[0] > 0
        })
    }

    fn png_dimensions(bytes: &[u8]) -> Option<(i32, i32)> {
        let header = bytes.get(0..24)?;
        (header.get(0..8)? == b"\x89PNG\r\n\x1a\n" && header.get(12..16)? == b"IHDR")
            .then_some(())?;
        let width = u32::from_be_bytes(header.get(16..20)?.try_into().ok()?) as i32;
        let height = u32::from_be_bytes(header.get(20..24)?.try_into().ok()?) as i32;
        Some((width, height))
    }

    fn render_compare_reference(
        script: &str,
        now_ms: i64,
        reference_png: &[u8],
    ) -> Vec<PlaneSummary> {
        let (width, height) =
            png_dimensions(reference_png).expect("reference PNG should have dimensions");
        let track = parse_fixture(script);
        assert_eq!(
            track.play_res_x, width,
            "compare fixture PlayResX should match reference PNG width"
        );
        assert_eq!(
            track.play_res_y, height,
            "compare fixture PlayResY should match reference PNG height"
        );

        let summary = render_track(&track, now_ms);
        assert!(
            !summary.is_empty(),
            "compare fixture should render images at {now_ms} ms"
        );
        assert!(
            summary
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Character)
        );
        assert!(summary.iter().all(|plane| plane.lit_pixels > 0));
        assert!(
            summary
                .iter()
                .all(|plane| plane.width > 0 && plane.height > 0)
        );
        assert!(
            summary
                .iter()
                .all(|plane| plane.x + plane.width > 0 && plane.x < width)
        );
        assert!(
            summary
                .iter()
                .all(|plane| plane.y + plane.height > 0 && plane.y < height)
        );
        summary
    }

    fn compare_fixture_font_provider() -> AttachedFontProvider {
        AttachedFontProvider::from_attachments_in_dir(
            &[
                FontAttachment {
                    name: "font1.ttf".to_string(),
                    data: include_bytes!("../fixtures/libass/compare/test/font1.ttf").to_vec(),
                },
                FontAttachment {
                    name: "font2.otf".to_string(),
                    data: include_bytes!("../fixtures/libass/compare/test/font2.otf").to_vec(),
                },
            ],
            Some(env::temp_dir().join("rassa-compare-fonts")),
        )
    }

    #[derive(Debug, PartialEq, Eq)]
    struct PlaneBitmapStats {
        lit_pixels: usize,
        alpha_sum: u64,
        partial_pixels: usize,
        inner_bbox: Option<(usize, usize, usize, usize)>,
    }

    fn plane_bitmap_stats(plane: &rassa_core::ImagePlane) -> PlaneBitmapStats {
        let width = plane.size.width.max(0) as usize;
        let height = plane.size.height.max(0) as usize;
        let stride = plane.stride.max(0) as usize;
        let mut lit_pixels = 0_usize;
        let mut alpha_sum = 0_u64;
        let mut partial_pixels = 0_usize;
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        let mut max_x = 0_usize;
        let mut max_y = 0_usize;

        for y in 0..height {
            for x in 0..width {
                let value = plane.bitmap[y * stride + x];
                if value > 0 {
                    lit_pixels += 1;
                    alpha_sum += u64::from(value);
                    if value < 255 {
                        partial_pixels += 1;
                    }
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }

        PlaneBitmapStats {
            lit_pixels,
            alpha_sum,
            partial_pixels,
            inner_bbox: (lit_pixels > 0).then_some((min_x, min_y, max_x, max_y)),
        }
    }

    fn upstream_compare_raw_url(name: &str) -> String {
        assert!(
            name.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
            "compare fixture names must be plain file names"
        );
        format!("https://raw.githubusercontent.com/libass/libass/master/compare/test/{name}")
    }

    fn fetch_upstream_compare_file(name: &str) -> Vec<u8> {
        let url = upstream_compare_raw_url(name);
        let output = Command::new("curl")
            .args(["-fsSL", "--max-time", "20", "--retry", "2", &url])
            .output()
            .expect("curl should be available for live upstream fixture fetches");
        assert!(
            output.status.success(),
            "failed to fetch {url}: status={:?}, stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.stdout.is_empty(),
            "server file {url} should not be empty"
        );
        output.stdout
    }

    fn upstream_compare_font_provider_from_server() -> AttachedFontProvider {
        AttachedFontProvider::from_attachments_in_dir(
            &[
                FontAttachment {
                    name: "font1.ttf".to_string(),
                    data: fetch_upstream_compare_file("font1.ttf"),
                },
                FontAttachment {
                    name: "font2.otf".to_string(),
                    data: fetch_upstream_compare_file("font2.otf"),
                },
            ],
            Some(env::temp_dir().join("rassa-compare-fonts-server")),
        )
    }

    #[test]
    #[ignore = "live network check for pulling encoded upstream files directly from the server"]
    fn upstream_compare_reference_can_be_loaded_directly_from_server() {
        let script = String::from_utf8(fetch_upstream_compare_file("sub2.ass"))
            .expect("upstream ASS fixture should be utf-8 text");
        let reference_png = fetch_upstream_compare_file("sub2-153000.png");
        let font_provider = upstream_compare_font_provider_from_server();

        let (width, height) = png_dimensions(&reference_png)
            .expect("server PNG bytes should still be encoded PNG data");
        assert_eq!((width, height), (320, 180));

        let track = parse_fixture(&script);
        let planes = render_track_planes_with_config(
            &track,
            &font_provider,
            153000,
            &RendererConfig {
                frame: Size { width, height },
                storage: Size { width, height },
                ..RendererConfig::default()
            },
        );

        assert!(
            !planes.is_empty(),
            "server-loaded compare fixture should render with server-loaded raw font bytes"
        );
    }

    fn decode_png_compare_target(bytes: &[u8]) -> (usize, usize, Vec<u16>) {
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().expect("reference PNG should decode");
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).expect("reference PNG frame");
        let data = &buffer[..info.buffer_size()];
        let target = match (info.color_type, info.bit_depth) {
            (png::ColorType::Rgba, png::BitDepth::Eight) => data
                .chunks_exact(4)
                .flat_map(|px| {
                    let a = px[3];
                    let premul = |c: u8| {
                        let ca = u16::from(c) * u16::from(a);
                        let value = (ca + (ca >> 8) + 128) >> 8;
                        257_u16 * value
                    };
                    [
                        premul(px[0]),
                        premul(px[1]),
                        premul(px[2]),
                        257_u16 * u16::from(!a),
                    ]
                })
                .collect(),
            (png::ColorType::Rgba, png::BitDepth::Sixteen) => data
                .chunks_exact(8)
                .flat_map(|px| {
                    let r = u16::from_be_bytes([px[0], px[1]]);
                    let g = u16::from_be_bytes([px[2], px[3]]);
                    let b = u16::from_be_bytes([px[4], px[5]]);
                    let a = u16::from_be_bytes([px[6], px[7]]);
                    let premul = |c: u16| {
                        let ca = u32::from(c) * u32::from(a);
                        ((ca + (ca >> 16) + (1 << 15)) >> 16) as u16
                    };
                    [premul(r), premul(g), premul(b), !a]
                })
                .collect(),
            other => panic!("unsupported reference PNG format: {other:?}"),
        };
        (info.width as usize, info.height as usize, target)
    }

    fn blend_planes_to_compare_frame(
        width: usize,
        height: usize,
        planes: &[rassa_core::ImagePlane],
    ) -> Vec<u8> {
        let mut frame = vec![0_u8; width * height * 4];
        for px in frame.chunks_exact_mut(4) {
            px[3] = 255;
        }

        for plane in planes {
            let r = (plane.color.0 >> 24) as u8;
            let g = (plane.color.0 >> 16) as u8;
            let b = (plane.color.0 >> 8) as u8;
            let a = plane.color.0 as u8;
            let mul = 129_i32 * i32::from(255_u8.saturating_sub(a));
            let offs = 1_i32 << 22;
            let x_min = plane.destination.x.max(0) as usize;
            let y_min = plane.destination.y.max(0) as usize;
            let x_max = (plane.destination.x + plane.size.width)
                .min(width as i32)
                .max(0) as usize;
            let y_max = (plane.destination.y + plane.size.height)
                .min(height as i32)
                .max(0) as usize;
            if x_min >= x_max || y_min >= y_max || plane.stride <= 0 {
                continue;
            }
            let stride = plane.stride as usize;
            for y in y_min..y_max {
                for x in x_min..x_max {
                    let src_x = (x as i32 - plane.destination.x) as usize;
                    let src_y = (y as i32 - plane.destination.y) as usize;
                    let src = i32::from(plane.bitmap[src_y * stride + src_x]);
                    let k = src * mul;
                    let dst = &mut frame[(y * width + x) * 4..][..4];
                    for (channel, target) in [r, g, b, 0].into_iter().enumerate() {
                        let current = i32::from(dst[channel]);
                        dst[channel] =
                            (current - (((current - i32::from(target)) * k + offs) >> 23)) as u8;
                    }
                }
            }
        }
        frame
    }

    fn compare_bbox(buffer: &[u16], width: usize) -> Option<(usize, usize, usize, usize)> {
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        let mut max_x = 0_usize;
        let mut max_y = 0_usize;
        for (idx, px) in buffer.chunks_exact(4).enumerate() {
            if px[3] == 65535 {
                continue;
            }
            let x = idx % width;
            let y = idx / width;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + 1);
            max_y = max_y.max(y + 1);
        }
        (min_x != usize::MAX).then_some((min_x, min_y, max_x, max_y))
    }

    fn downsample_compare_frame(
        temp: &[u8],
        width: usize,
        height: usize,
        scale_x: usize,
        scale_y: usize,
    ) -> Vec<u16> {
        let scale_area = scale_x * scale_y;
        let mul = (257_u64 << 20) / scale_area as u64;
        let offs = (1_u64 << 19) - 1;
        let temp_width = width * scale_x;
        let mut frame = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            for x in 0..width {
                let mut sums = [0_u16; 4];
                for sy in 0..scale_y {
                    let row_start = ((y * scale_y + sy) * temp_width + x * scale_x) * 4;
                    for sx in 0..scale_x {
                        let offset = row_start + sx * 4;
                        for channel in 0..4 {
                            sums[channel] += u16::from(temp[offset + channel]);
                        }
                    }
                }
                let mut values = [0_u16; 4];
                for (channel, sum) in sums.into_iter().enumerate() {
                    values[channel] = ((u64::from(sum) * mul + offs) >> 20) as u16;
                }
                if values[3] == u16::MAX {
                    values[0] = 0;
                    values[1] = 0;
                    values[2] = 0;
                }
                frame.extend_from_slice(&values);
            }
        }
        frame
    }

    fn write_compare_debug_png(
        path: &std::path::Path,
        buffer: &[u16],
        width: usize,
        height: usize,
    ) {
        let file = fs::File::create(path).expect("debug PNG should be creatable");
        let mut encoder = png::Encoder::new(file, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Sixteen);
        let mut writer = encoder.write_header().expect("debug PNG header");
        let mut rgba = Vec::with_capacity(width * height * 8);
        for px in buffer.chunks_exact(4) {
            let inv_alpha = px[3];
            let alpha = 65535_u16.saturating_sub(inv_alpha);
            let unpremul = |channel: u16| -> u16 {
                if alpha == 0 {
                    0
                } else {
                    ((u32::from(channel) * 65535 + u32::from(alpha) / 2) / u32::from(alpha))
                        .min(65535) as u16
                }
            };
            for value in [unpremul(px[0]), unpremul(px[1]), unpremul(px[2]), alpha] {
                rgba.extend_from_slice(&value.to_be_bytes());
            }
        }
        writer.write_image_data(&rgba).expect("debug PNG data");
    }

    fn compare_dump_name(script: &str) -> String {
        script
            .lines()
            .find_map(|line| line.strip_prefix("Dialogue:"))
            .and_then(|line| line.rsplit_once(',').map(|(_, text)| text))
            .or_else(|| {
                script
                    .lines()
                    .find_map(|line| line.strip_prefix("Title:"))
                    .map(str::trim)
            })
            .filter(|value| !value.is_empty())
            .unwrap_or("compare")
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect::<String>()
    }

    fn maybe_dump_compare_debug(
        script: &str,
        now_ms: i64,
        width: usize,
        height: usize,
        actual: &[u16],
        target: &[u16],
    ) {
        let Ok(dir) = env::var("RASSA_DUMP_COMPARE") else {
            return;
        };
        let name = compare_dump_name(script);
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).expect("debug dump directory should be creatable");
        write_compare_debug_png(
            &dir.join(format!("{name}-{now_ms:04}-actual.png")),
            actual,
            width,
            height,
        );
        write_compare_debug_png(
            &dir.join(format!("{name}-{now_ms:04}-target.png")),
            target,
            width,
            height,
        );
    }

    fn maybe_dump_compare_planes(script: &str, now_ms: i64, planes: &[rassa_core::ImagePlane]) {
        let Ok(dir) = env::var("RASSA_DUMP_COMPARE_PLANES") else {
            return;
        };
        let name = compare_dump_name(script);
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).expect("plane dump directory should be creatable");
        for (idx, plane) in planes.iter().enumerate() {
            let path = dir.join(format!(
                "{name}-{now_ms:04}-{idx:02}-{:?}-x{}-y{}-w{}-h{}-s{}.pgm",
                plane.kind,
                plane.destination.x,
                plane.destination.y,
                plane.size.width,
                plane.size.height,
                plane.stride,
            ));
            let mut data =
                format!("P5\n{} {}\n255\n", plane.size.width, plane.size.height).into_bytes();
            let stride = plane.stride as usize;
            let width = plane.size.width as usize;
            let height = plane.size.height as usize;
            for y in 0..height {
                data.extend_from_slice(&plane.bitmap[y * stride..y * stride + width]);
            }
            fs::write(path, data).expect("plane dump should be written");
        }
    }

    const COMPARE_SCALE: usize = 8;

    fn compare_fixture_planes(
        script: &str,
        now_ms: i64,
        reference_png: &[u8],
    ) -> Vec<rassa_core::ImagePlane> {
        let (width, height, _) = decode_png_compare_target(reference_png);
        let track = parse_fixture(script);
        let provider = compare_fixture_font_provider();
        let config = RendererConfig {
            frame: Size {
                width: (width * COMPARE_SCALE) as i32,
                height: (height * COMPARE_SCALE) as i32,
            },
            storage: Size {
                width: width as i32,
                height: height as i32,
            },
            ..RendererConfig::default()
        };
        render_track_planes_with_config(&track, &provider, now_ms, &config)
    }

    fn plane_geometries(
        planes: &[rassa_core::ImagePlane],
    ) -> Vec<(ass::ImageType, i32, i32, i32, i32)> {
        planes
            .iter()
            .map(|plane| {
                (
                    plane.kind,
                    plane.destination.x,
                    plane.destination.y,
                    plane.size.width,
                    plane.size.height,
                )
            })
            .collect()
    }

    fn assert_pixel_perfect_compare_fixture(script: &str, now_ms: i64, reference_png: &[u8]) {
        let (width, height, target) = decode_png_compare_target(reference_png);
        let planes = compare_fixture_planes(script, now_ms, reference_png);
        maybe_dump_compare_planes(script, now_ms, &planes);
        let actual = downsample_compare_frame(
            &blend_planes_to_compare_frame(width * COMPARE_SCALE, height * COMPARE_SCALE, &planes),
            width,
            height,
            COMPARE_SCALE,
            COMPARE_SCALE,
        );
        assert_eq!(actual.len(), target.len());
        let mut different_pixels = 0_usize;
        let mut first_diff = None;
        let mut actual_non_transparent = 0_usize;
        let mut target_non_transparent = 0_usize;
        let mut actual_alpha_sum = 0_u64;
        let mut target_alpha_sum = 0_u64;
        for (idx, (a, t)) in actual
            .chunks_exact(4)
            .zip(target.chunks_exact(4))
            .enumerate()
        {
            if a != t {
                different_pixels += 1;
                first_diff.get_or_insert((idx, [a[0], a[1], a[2], a[3]], [t[0], t[1], t[2], t[3]]));
            }
            if a[3] != 65535 {
                actual_non_transparent += 1;
                actual_alpha_sum += u64::from(65535 - a[3]);
            }
            if t[3] != 65535 {
                target_non_transparent += 1;
                target_alpha_sum += u64::from(65535 - t[3]);
            }
        }
        if different_pixels > 0 {
            maybe_dump_compare_debug(script, now_ms, width, height, &actual, &target);
            if env::var_os("RASSA_DEBUG_COMPARE_PIXELS").is_some() {
                for y in 60..height.min(125) {
                    let mut row = Vec::new();
                    for x in 115..width.min(205) {
                        let idx = (y * width + x) * 4;
                        let a = &actual[idx..idx + 4];
                        let t = &target[idx..idx + 4];
                        if a != t {
                            row.push((x, [a[0], a[1], a[2], a[3]], [t[0], t[1], t[2], t[3]]));
                        }
                    }
                    if !row.is_empty() {
                        eprintln!("u16diff row {y}: {row:?}");
                    }
                }
            }
        }
        let actual_bbox = compare_bbox(&actual, width);
        let target_bbox = compare_bbox(&target, width);
        let row_summary = |buffer: &[u16]| -> Vec<(usize, usize, u64)> {
            buffer
                .chunks_exact(width * 4)
                .enumerate()
                .filter_map(|(row, pixels)| {
                    let mut count = 0_usize;
                    let mut alpha_sum = 0_u64;
                    for px in pixels.chunks_exact(4) {
                        if px[3] != 65535 {
                            count += 1;
                            alpha_sum += u64::from(65535 - px[3]);
                        }
                    }
                    (count > 0).then_some((row, count, alpha_sum))
                })
                .collect()
        };
        assert_eq!(
            different_pixels,
            0,
            "rassa rendered frame differs from upstream libass compare reference at {now_ms} ms; planes={:?}, actual_nontransparent={}, target_nontransparent={}, actual_alpha_sum={}, target_alpha_sum={}, actual_bbox={actual_bbox:?}, target_bbox={target_bbox:?}, actual_rows={:?}, target_rows={:?}, first_diff={first_diff:?}",
            summarize_planes(&planes),
            actual_non_transparent,
            target_non_transparent,
            actual_alpha_sum,
            target_alpha_sum,
            row_summary(&actual),
            row_summary(&target),
        );
    }

    #[test]
    fn render_summary_is_deterministic_for_inline_fixture() {
        let first = render_fixture(INLINE_OVERRIDE_FIXTURE, 500);
        let second = render_fixture(INLINE_OVERRIDE_FIXTURE, 500);

        assert_eq!(first, second);
        assert!(
            first
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Character)
        );
    }

    #[test]
    fn renders_upstream_compare_sample_sub1() {
        let script = include_str!("../fixtures/libass/compare/test/sub1.ass");
        let summary = render_fixture(script, 2000);

        assert!(!summary.is_empty());
        assert!(
            summary
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Character)
        );
        assert!(summary.iter().all(|plane| plane.lit_pixels > 0));
    }

    #[test]
    fn renders_upstream_compare_sample_sub2() {
        let script = include_str!("../fixtures/libass/compare/test/sub2.ass");
        let summary = render_fixture(script, 152000);

        assert!(!summary.is_empty());
        assert!(
            summary
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Character)
        );
        assert!(
            summary
                .iter()
                .all(|plane| plane.width >= 0 && plane.height >= 0)
        );
    }

    #[test]
    #[ignore = "focused libass fill-raster source coverage parity guard"]
    fn upstream_compare_sub2_no_blur_source_plane_matches_libass_edge_coverage() {
        let script =
            include_str!("../fixtures/libass/compare/test/sub2.ass").replace("{\\blur1}", "");
        let track = parse_fixture(&script);
        let provider = compare_fixture_font_provider();
        let planes = render_track_planes_with_config(
            &track,
            &provider,
            153000,
            &RendererConfig {
                frame: Size {
                    width: 2560,
                    height: 1440,
                },
                storage: Size {
                    width: 320,
                    height: 180,
                },
                ..RendererConfig::default()
            },
        );
        let character_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .collect::<Vec<_>>();
        assert_eq!(
            character_planes.len(),
            1,
            "no-blur sub2 fixture should produce one fill source plane; planes={:?}",
            summarize_planes(&planes)
        );
        let plane = character_planes[0];
        let stats = plane_bitmap_stats(plane);

        assert_eq!(plane.destination.x, 329);
        assert_eq!(plane.destination.y, 519);
        assert_eq!(plane.size.width, 1966);
        assert_eq!(plane.size.height, 564);
        assert_eq!(
            plane.stride, plane.size.width,
            "rassa stores compact owned plane rows; libass probe stride is 1984"
        );
        assert_eq!(
            stats,
            PlaneBitmapStats {
                lit_pixels: 261_814,
                alpha_sum: 65_024_894,
                partial_pixels: 13_548,
                inner_bbox: Some((0, 1, 1951, 551)),
            }
        );
    }

    #[test]
    fn upstream_compare_reference_png_matrix_renders_within_frame() {
        let sub1 = include_str!("../fixtures/libass/compare/test/sub1.ass");
        let sub1_0500 = render_compare_reference(
            sub1,
            500,
            include_bytes!("../fixtures/libass/compare/test/sub1-0500.png"),
        );
        let sub1_1500 = render_compare_reference(
            sub1,
            1500,
            include_bytes!("../fixtures/libass/compare/test/sub1-1500.png"),
        );
        let sub1_2500 = render_compare_reference(
            sub1,
            2500,
            include_bytes!("../fixtures/libass/compare/test/sub1-2500.png"),
        );
        assert_ne!(
            sub1_0500, sub1_1500,
            "sub1 compare frames should exercise time-varying rendering"
        );
        assert_ne!(
            sub1_1500, sub1_2500,
            "sub1 compare frames should exercise time-varying rendering"
        );

        let sub2 = include_str!("../fixtures/libass/compare/test/sub2.ass");
        render_compare_reference(
            sub2,
            153000,
            include_bytes!("../fixtures/libass/compare/test/sub2-153000.png"),
        );
    }

    #[test]
    #[ignore = "focused libass pixel guard for ASS Effect Banner/Scroll and drawing \\pbo edge cases"]
    fn upstream_compare_effect_and_drawing_edge_cases_match_libass() {
        let script = include_str!("../fixtures/libass/compare/edge/effect_drawing.ass");
        assert_pixel_perfect_compare_fixture(
            script,
            500,
            include_bytes!("../fixtures/libass/compare/edge/effect_drawing-500.png"),
        );
        assert_pixel_perfect_compare_fixture(
            script,
            1500,
            include_bytes!("../fixtures/libass/compare/edge/effect_drawing-1500.png"),
        );
    }

    #[test]
    #[ignore = "focused libass pixel guard for vector drawing combined with inline transforms, \\org, shear and clip"]
    fn upstream_compare_vector_transform_edge_cases_match_libass() {
        let script = include_str!("../fixtures/libass/compare/edge/vector_transform.ass");
        assert_pixel_perfect_compare_fixture(
            script,
            500,
            include_bytes!("../fixtures/libass/compare/edge/vector_transform-500.png"),
        );
    }

    #[test]
    fn capi_smoke_parses_and_renders_fixture() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            assert!(!track.is_null());
            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);

            assert!(!images.is_null());
            assert!(detect_change > 0);
            assert!((*images).w >= 0);
            assert!((*images).h >= 0);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_process_data_populates_track_fields() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);

            rassa_capi::ass_process_data(
                track,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *const c_char,
                INLINE_OVERRIDE_FIXTURE.len() as i32,
            );

            assert!(!track.is_null());
            assert_eq!((*track).n_styles, 1);
            assert_eq!((*track).n_events, 1);
            assert_eq!((*track).PlayResX, 320);
            assert_eq!((*track).PlayResY, 180);
            assert!(!(*track).styles.is_null());
            assert!(!(*track).events.is_null());
            assert_eq!((*(*track).events).Duration, 2000);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_chunk_and_prune_manage_event_timeline() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);

            let first = b"first";
            let second = b"second";
            rassa_capi::ass_process_chunk(
                track,
                first.as_ptr() as *const c_char,
                first.len() as i32,
                1000,
                500,
            );
            rassa_capi::ass_process_chunk(
                track,
                second.as_ptr() as *const c_char,
                second.len() as i32,
                3000,
                500,
            );

            assert_eq!((*track).n_events, 2);
            assert_eq!(rassa_capi::ass_step_sub(track, 1200, 1), 1800);
            assert_eq!(rassa_capi::ass_step_sub(track, 3200, -1), -2200);
            assert_eq!(rassa_capi::ass_step_sub(track, 3200, 0), -200);
            assert_eq!(rassa_capi::ass_step_sub(track, 3600, -2), -2600);

            rassa_capi::ass_prune_events(track, 2000);

            assert_eq!((*track).n_events, 1);
            assert_eq!((*(*track).events).Start, 3000);
            assert_eq!((*(*track).events).ReadOrder, 0);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_render_frame_reports_detect_change_states() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut first_change = 0;
            let first = rassa_capi::ass_render_frame(renderer, track, 500, &mut first_change);
            let mut second_change = 0;
            let second = rassa_capi::ass_render_frame(renderer, track, 500, &mut second_change);
            let mut third_change = 0;
            let third = rassa_capi::ass_render_frame(renderer, track, 900, &mut third_change);

            assert!(!first.is_null());
            assert!(!second.is_null());
            assert!(!third.is_null());
            assert_eq!(first_change, 2);
            assert_eq!(second_change, 0);
            assert!(third_change >= 1);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_normal_multiline_renders_glyph_masks_not_solid_boxes() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,First normal line\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Second normal line\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Third normal line\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Fourth normal line";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let signatures = image_signatures(images);

            assert!(!signatures.is_empty());
            assert!(
                !has_large_solid_bitmap(&signatures),
                "normal C API renders must not replace glyphs with large filled rectangles"
            );

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_renderer_frame_size_clips_output() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            rassa_capi::ass_set_frame_size(renderer, 48, 48);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let mut image = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert!(!image.is_null());
            while !image.is_null() {
                assert!((*image).dst_x >= 0);
                assert!((*image).dst_y >= 0);
                assert!((*image).dst_x + (*image).w <= 48);
                assert!((*image).dst_y + (*image).h <= 48);
                image = (*image).next;
            }

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_render_frame_orders_images_by_layer_then_read_order() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 5,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\1c&H0000FF&}High\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,40)\\1c&H00FF00&}Low";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let colors = image_colors(images);

            assert_eq!(colors.first().copied(), Some(0x00FF_0000));

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_render_frame_orders_shadow_outline_before_character() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00111111,&H0000FFFF,&H00222222,&H00333333,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Hi";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let types = image_types(images);
            let first_shadow = types
                .iter()
                .position(|type_| *type_ == ass::ImageType::Shadow as i32)
                .expect("shadow image");
            let first_outline = types
                .iter()
                .position(|type_| *type_ == ass::ImageType::Outline as i32)
                .expect("outline image");
            let first_character = types
                .iter()
                .position(|type_| *type_ == ass::ImageType::Character as i32)
                .expect("character image");

            assert!(first_shadow < first_outline);
            assert!(first_outline < first_character);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_render_frame_allows_collision_across_different_layers() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 1,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H00FF00&}Second";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);

            assert_eq!(
                image_min_y_for_color(images, 0x0000_00FF),
                image_min_y_for_color(images, 0x0000_FF00)
            );

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_font_scale_changes_rendered_image_size() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_area = total_image_area(baseline);

            rassa_capi::ass_set_font_scale(renderer, 2.0);
            let scaled = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let scaled_area = total_image_area(scaled);

            assert!(baseline_area > 0);
            assert!(scaled_area > baseline_area);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_text_scale_overrides_change_rendered_bounds() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let baseline_fixture = "[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Scale";
            let scaled_fixture = "[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50}Scale";
            let baseline_track = rassa_capi::ass_read_memory(
                library,
                baseline_fixture.as_ptr() as *mut c_char,
                baseline_fixture.len(),
                ptr::null(),
            );
            let scaled_track = rassa_capi::ass_read_memory(
                library,
                scaled_fixture.as_ptr() as *mut c_char,
                scaled_fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline =
                rassa_capi::ass_render_frame(renderer, baseline_track, 500, &mut detect_change);
            let baseline_bounds = image_bounds(baseline).expect("baseline bounds");
            let scaled =
                rassa_capi::ass_render_frame(renderer, scaled_track, 500, &mut detect_change);
            let scaled_bounds = image_bounds(scaled).expect("scaled bounds");

            assert!(scaled_bounds.2 - scaled_bounds.0 > baseline_bounds.2 - baseline_bounds.0);
            assert!(scaled_bounds.3 - scaled_bounds.1 < baseline_bounds.3 - baseline_bounds.1);

            rassa_capi::ass_free_track(baseline_track);
            rassa_capi::ass_free_track(scaled_track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_drawing_scale_overrides_change_rendered_bounds() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let baseline_fixture = "[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 10 0 10 10 0 10";
            let scaled_fixture = "[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50\\p1}m 0 0 l 10 0 10 10 0 10";
            let baseline_track = rassa_capi::ass_read_memory(
                library,
                baseline_fixture.as_ptr() as *mut c_char,
                baseline_fixture.len(),
                ptr::null(),
            );
            let scaled_track = rassa_capi::ass_read_memory(
                library,
                scaled_fixture.as_ptr() as *mut c_char,
                scaled_fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline =
                rassa_capi::ass_render_frame(renderer, baseline_track, 500, &mut detect_change);
            let baseline_bounds = image_bounds(baseline).expect("baseline bounds");
            let scaled =
                rassa_capi::ass_render_frame(renderer, scaled_track, 500, &mut detect_change);
            let scaled_bounds = image_bounds(scaled).expect("scaled bounds");

            assert!(scaled_bounds.2 - scaled_bounds.0 > baseline_bounds.2 - baseline_bounds.0);
            assert!(scaled_bounds.3 - scaled_bounds.1 < baseline_bounds.3 - baseline_bounds.1);

            rassa_capi::ass_free_track(baseline_track);
            rassa_capi::ass_free_track(scaled_track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_text_spacing_override_changes_rendered_width() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let baseline_fixture = "[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}IIII";
            let spaced_fixture = "[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fsp8}IIII";
            let baseline_track = rassa_capi::ass_read_memory(
                library,
                baseline_fixture.as_ptr() as *mut c_char,
                baseline_fixture.len(),
                ptr::null(),
            );
            let spaced_track = rassa_capi::ass_read_memory(
                library,
                spaced_fixture.as_ptr() as *mut c_char,
                spaced_fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline =
                rassa_capi::ass_render_frame(renderer, baseline_track, 500, &mut detect_change);
            let baseline_bounds = image_bounds(baseline).expect("baseline bounds");
            let spaced =
                rassa_capi::ass_render_frame(renderer, spaced_track, 500, &mut detect_change);
            let spaced_bounds = image_bounds(spaced).expect("spaced bounds");

            assert!(spaced_bounds.2 - spaced_bounds.0 > baseline_bounds.2 - baseline_bounds.0);

            rassa_capi::ass_free_track(baseline_track);
            rassa_capi::ass_free_track(spaced_track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_frame_size_scales_rendered_image_size() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_area = total_image_area(baseline);

            rassa_capi::ass_set_frame_size(renderer, 640, 360);
            let scaled = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let scaled_area = total_image_area(scaled);

            assert!(baseline_area > 0);
            assert!(scaled_area > baseline_area);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_invalid_frame_size_resets_both_dimensions() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_signature = image_signatures(baseline);

            rassa_capi::ass_set_frame_size(renderer, 640, 360);
            let scaled = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert_ne!(image_signatures(scaled), baseline_signature);

            rassa_capi::ass_set_frame_size(renderer, -1, 360);
            let reset = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert_eq!(image_signatures(reset), baseline_signature);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_pixel_aspect_widens_rendered_output() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            rassa_capi::ass_set_frame_size(renderer, 640, 180);
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_area = total_image_area(baseline);

            rassa_capi::ass_set_pixel_aspect(renderer, 2.0);
            let widened = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let widened_area = total_image_area(widened);

            assert!(baseline_area > 0);
            assert!(widened_area > baseline_area);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_negative_pixel_aspect_resets_to_default() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            rassa_capi::ass_set_frame_size(renderer, 640, 180);
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_signature = image_signatures(baseline);

            rassa_capi::ass_set_pixel_aspect(renderer, 2.0);
            let widened = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert_ne!(image_signatures(widened), baseline_signature);

            rassa_capi::ass_set_pixel_aspect(renderer, -2.0);
            let reset = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert_eq!(image_signatures(reset), baseline_signature);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_storage_size_affects_default_aspect_mapping() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Storage";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            rassa_capi::ass_set_frame_size(renderer, 400, 240);
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_area = total_image_area(baseline);
            let baseline_signature = image_signatures(baseline);

            rassa_capi::ass_set_storage_size(renderer, 400, 120);
            let storage_adjusted =
                rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let storage_adjusted_area = total_image_area(storage_adjusted);

            assert!(baseline_area > 0);
            assert!(storage_adjusted_area < baseline_area);

            rassa_capi::ass_set_storage_size(renderer, 400, -1);
            let reset = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert_eq!(image_signatures(reset), baseline_signature);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_margins_map_output_into_content_area_by_default() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            rassa_capi::ass_set_frame_size(renderer, 120, 120);
            rassa_capi::ass_set_margins(renderer, 10, 10, 10, 10);
            rassa_capi::ass_set_use_margins(renderer, 0);
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let bounds = image_bounds(images).expect("rendered image bounds");

            assert!(bounds.0 >= 10);
            assert!(bounds.1 >= 9);
            assert!(bounds.2 <= 110);
            assert!(bounds.3 <= 110);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_line_position_moves_subtitles_upward() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Shift";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_y = (*baseline).dst_y;

            rassa_capi::ass_set_line_position(renderer, 50.0);
            let shifted = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let shifted_y = (*shifted).dst_y;

            assert!(shifted_y < baseline_y);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_line_spacing_expands_multiline_subtitle_height() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 200\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,One\\NTwo";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_span = image_vertical_span(baseline);

            rassa_capi::ass_set_line_spacing(renderer, 20.0);
            let spaced = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let spaced_span = image_vertical_span(spaced);

            assert!(spaced_span > baseline_span);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_selective_style_override_changes_render_output() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let fixture = "[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Override";
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_ptr() as *mut c_char,
                fixture.len(),
                ptr::null(),
            );

            let mut detect_change = 0;
            let baseline = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let baseline_area = total_image_area(baseline);
            let baseline_colors = image_colors(baseline);

            let mut override_style = rassa_capi::ASS_Style {
                PrimaryColour: 0x000A0B0C,
                SecondaryColour: 0x000A0B0C,
                FontSize: 48.0,
                ..Default::default()
            };
            rassa_capi::ass_set_selective_style_override_enabled(
                renderer,
                ass::override_bits::STYLE,
            );
            rassa_capi::ass_set_selective_style_override(renderer, &mut override_style);

            let overridden = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let overridden_area = total_image_area(overridden);
            let overridden_colors = image_colors(overridden);

            assert!(overridden_area > baseline_area);
            assert!(overridden_colors.contains(&0x0C0B_0A00));
            assert!(!baseline_colors.contains(&0x0C0B_0A00));

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_read_file_loads_track_from_disk() {
        let path = write_temp_fixture("read-file", INLINE_OVERRIDE_FIXTURE);
        unsafe {
            let library = rassa_capi::ass_library_init();
            let c_path = CString::new(path.to_string_lossy().as_bytes()).expect("path cstring");
            let track = rassa_capi::ass_read_file(library, c_path.as_ptr(), ptr::null());

            assert!(!track.is_null());
            assert_eq!((*track).n_events, 1);
            assert_eq!((*track).PlayResX, 320);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn capi_read_memory_reencodes_legacy_codepage() {
        let mut fixture = b"[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n".to_vec();
        fixture.extend_from_slice(&[
            68, 105, 97, 108, 111, 103, 117, 101, 58, 32, 48, 44, 48, 58, 48, 48, 58, 48, 48, 46,
            48, 48, 44, 48, 58, 48, 48, 58, 48, 49, 46, 48, 48, 44, 68, 101, 102, 97, 117, 108,
            116, 44, 44, 48, 44, 48, 44, 48, 44, 44, 147, 250, 150, 123, 140, 234,
        ]);
        let mut fixture: Vec<c_char> = fixture.into_iter().map(|byte| byte as c_char).collect();
        let codepage = CString::new("SHIFT_JIS").expect("codepage cstring");

        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_read_memory(
                library,
                fixture.as_mut_ptr(),
                fixture.len(),
                codepage.as_ptr(),
            );

            assert!(!track.is_null());
            assert_eq!((*track).n_events, 1);
            let text = CStr::from_ptr((*(*track).events).Text).to_string_lossy();
            assert_eq!(text, "日本語");

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_read_styles_replaces_track_style_table() {
        let path = write_temp_fixture("read-styles", STYLE_ONLY_FIXTURE);
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);
            let initial_style = rassa_capi::ass_alloc_style(track);
            assert_eq!(initial_style, 0);
            assert_eq!((*track).n_styles, 1);

            let c_path = CString::new(path.to_string_lossy().as_bytes()).expect("path cstring");
            let result = rassa_capi::ass_read_styles(track, c_path.as_ptr(), ptr::null());

            assert_eq!(result, 0);
            assert_eq!((*track).n_styles, 1);
            assert!(!(*track).styles.is_null());
            assert_eq!((*(*track).styles).FontSize as i32, 18);
            assert_eq!((*(*track).styles).MarginL, 11);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
        let _ = fs::remove_file(path);
    }

    #[test]
    fn libass_drop_in_pkg_config_metadata_is_present() {
        let root = workspace_root();
        let libass_pc = fs::read_to_string(root.join("pkgconfig/libass.pc"))
            .expect("drop-in builds should provide pkgconfig/libass.pc");

        assert!(
            libass_pc.contains("Name: libass"),
            "libass.pc should identify itself as libass for pkg-config consumers"
        );
        assert!(
            libass_pc.contains("Libs: -L${libdir} -lass"),
            "libass.pc should make pkg-config libass link with -lass"
        );
        assert!(
            libass_pc.contains("Cflags: -I${includedir}"),
            "libass.pc should expose include/ass/ass.h"
        );
    }

    #[test]
    fn workspace_builds_a_libass_named_capi_cdylib() {
        let root = workspace_root();
        let workspace_toml = fs::read_to_string(root.join("Cargo.toml"))
            .expect("workspace Cargo.toml should be readable");
        let rassa_capi_toml = fs::read_to_string(root.join("crates/rassa-capi/Cargo.toml"))
            .expect("internal C API implementation crate should exist");
        let libass_capi_toml = fs::read_to_string(root.join("crates/rassa-libass-capi/Cargo.toml"))
            .expect("drop-in libass C API crate should exist");

        assert!(
            workspace_toml.contains("\"crates/rassa-libass-capi\""),
            "workspace should include the libass-named C API crate"
        );
        assert!(
            libass_capi_toml.contains("name = \"ass\""),
            "drop-in C API cdylib should build target/release/libass.so"
        );
        assert!(
            libass_capi_toml.contains("crate-type = [\"rlib\", \"cdylib\"]"),
            "drop-in C API crate should expose a cdylib"
        );
        assert!(
            rassa_capi_toml.contains("crate-type = [\"rlib\"]"),
            "rassa-capi should remain an internal Rust rlib implementation, not a second public C cdylib"
        );
        assert!(
            root.join("pkgconfig/rassa.pc").exists(),
            "new applications should be able to discover the native rassa shared library through pkg-config"
        );
    }

    #[test]
    fn workspace_exposes_rassa_rust_abi_facade() {
        let root = workspace_root();
        let workspace_toml = fs::read_to_string(root.join("Cargo.toml"))
            .expect("workspace Cargo.toml should be readable");
        let rassa_toml = fs::read_to_string(root.join("crates/rassa/Cargo.toml"))
            .expect("rassa Rust API crate should exist");

        assert!(
            workspace_toml.contains("\"crates/rassa\""),
            "workspace should include the rassa Rust API facade crate"
        );
        assert!(
            rassa_toml.contains("name = \"rassa\""),
            "public Rust API crate should be the cargo package named rassa"
        );
        assert!(
            rassa_toml.contains("path = \"src/lib.rs\""),
            "rassa Rust API should build as a normal Rust library"
        );
        assert!(
            rassa_toml.contains("crate-type = [\"rlib\", \"cdylib\"]"),
            "rassa should build both the Rust rlib and native librassa.so for new applications"
        );
    }

    #[test]
    fn rassa_rust_abi_parses_and_renders_without_c_pointers() {
        let script = rassa::Script::parse(INLINE_OVERRIDE_FIXTURE).expect("script should parse");
        let renderer = rassa::Renderer::new();
        let frame = renderer
            .render_frame(&script, 500)
            .expect("safe Rust render API should return a frame");

        assert_eq!(
            script.play_res(),
            Size {
                width: 320,
                height: 180
            }
        );
        assert_eq!(frame.now_ms, 500);
        assert!(frame.planes.iter().any(|plane| !plane.bitmap.is_empty()));
    }

    #[test]
    fn capi_track_feature_and_allocator_helpers_behave() {
        unsafe {
            assert!(rassa_capi::ass_library_version() > 0);

            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);
            assert_eq!(rassa_capi::ass_track_set_feature(track, 0, 1), 0);
            assert_eq!(rassa_capi::ass_track_set_feature(track, 99, 1), -1);

            let allocation = rassa_capi::ass_malloc(64);
            assert!(!allocation.is_null());
            rassa_capi::ass_free(allocation);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_available_font_providers_match_libass_order() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let mut providers = ptr::null_mut();
            let mut size = 0usize;

            rassa_capi::ass_get_available_font_providers(library, &mut providers, &mut size);

            assert!(!providers.is_null());
            assert_eq!(size, 3);
            let values = std::slice::from_raw_parts(providers, size);
            assert_eq!(
                values,
                &[
                    ass::DefaultFontProvider::None as i32,
                    ass::DefaultFontProvider::Autodetect as i32,
                    ass::DefaultFontProvider::Fontconfig as i32,
                ]
            );

            rassa_capi::ass_free(providers.cast());
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_invalid_font_provider_behaves_like_none() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );
            let none_renderer = rassa_capi::ass_renderer_init(library);
            let invalid_renderer = rassa_capi::ass_renderer_init(library);

            rassa_capi::ass_set_fonts(
                none_renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as i32,
                ptr::null(),
                0,
            );
            rassa_capi::ass_set_fonts(
                invalid_renderer,
                ptr::null(),
                ptr::null(),
                99,
                ptr::null(),
                0,
            );

            let mut none_change = 0;
            let none_images =
                rassa_capi::ass_render_frame(none_renderer, track, 500, &mut none_change);
            let none_signature = image_signatures(none_images);
            let mut invalid_change = 0;
            let invalid_images =
                rassa_capi::ass_render_frame(invalid_renderer, track, 500, &mut invalid_change);

            assert!(none_change > 0);
            assert!(invalid_change > 0);
            assert_eq!(image_signatures(invalid_images), none_signature);

            rassa_capi::ass_renderer_done(none_renderer);
            rassa_capi::ass_renderer_done(invalid_renderer);
            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_default_font_path_renders_when_system_providers_are_disabled() {
        let system_font = FontconfigProvider::new()
            .resolve_family("sans")
            .path
            .expect("system font path should exist");
        let default_font =
            CString::new(system_font.to_string_lossy().as_bytes()).expect("font path cstring");

        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );
            let no_default_renderer = rassa_capi::ass_renderer_init(library);
            let default_renderer = rassa_capi::ass_renderer_init(library);

            rassa_capi::ass_set_fonts(
                no_default_renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as i32,
                ptr::null(),
                0,
            );
            rassa_capi::ass_set_fonts(
                default_renderer,
                default_font.as_ptr(),
                ptr::null(),
                ass::DefaultFontProvider::None as i32,
                ptr::null(),
                0,
            );

            let mut no_default_change = 0;
            let no_default_images = rassa_capi::ass_render_frame(
                no_default_renderer,
                track,
                500,
                &mut no_default_change,
            );
            let mut default_change = 0;
            let default_images =
                rassa_capi::ass_render_frame(default_renderer, track, 500, &mut default_change);

            assert!(no_default_change > 0);
            assert!(default_change > 0);
            assert!(image_signatures(no_default_images).is_empty());
            assert!(!image_signatures(default_images).is_empty());

            rassa_capi::ass_renderer_done(no_default_renderer);
            rassa_capi::ass_renderer_done(default_renderer);
            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_track_features_are_locked_after_rendering() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            assert_eq!(rassa_capi::ass_track_set_feature(track, 0, 1), 0);

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            assert!(!images.is_null());
            assert_eq!(rassa_capi::ass_track_set_feature(track, 0, 0), -1);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_alloc_and_free_style_event_slots_reset_state() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);

            rassa_capi::ass_process_data(
                track,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *const c_char,
                INLINE_OVERRIDE_FIXTURE.len() as i32,
            );
            assert_eq!((*track).n_styles, 1);
            assert_eq!((*track).n_events, 1);

            let style_index = rassa_capi::ass_alloc_style(track);
            let event_index = rassa_capi::ass_alloc_event(track);
            assert_eq!(style_index, 1);
            assert_eq!(event_index, 1);
            assert_eq!((*track).n_styles, 2);
            assert_eq!((*track).n_events, 2);

            rassa_capi::ass_free_style(track, 0);
            rassa_capi::ass_free_event(track, 0);

            assert!((*(*track).styles.add(0)).Name.is_null());
            assert!((*(*track).styles.add(0)).FontName.is_null());
            assert_eq!((*(*track).styles.add(0)).FontSize as i32, 20);
            assert!((*(*track).events.add(0)).Text.is_null());
            assert_eq!((*(*track).events.add(0)).Start, 0);
            assert_eq!((*(*track).events.add(0)).Duration, 0);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_check_readorder_affects_chunk_insertions() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);

            rassa_capi::ass_set_check_readorder(track, 0);
            rassa_capi::ass_process_chunk(track, b"zero".as_ptr() as *const c_char, 4, 1000, 100);
            rassa_capi::ass_process_chunk(track, b"zero2".as_ptr() as *const c_char, 5, 1200, 100);
            assert_eq!((*(*track).events.add(0)).ReadOrder, 0);
            assert_eq!((*(*track).events.add(1)).ReadOrder, 0);

            rassa_capi::ass_set_check_readorder(track, 1);
            rassa_capi::ass_process_chunk(track, b"one".as_ptr() as *const c_char, 3, 1400, 100);
            assert_eq!((*(*track).events.add(2)).ReadOrder, 2);

            rassa_capi::ass_set_check_readorder(track, 2);
            rassa_capi::ass_process_chunk(track, b"two".as_ptr() as *const c_char, 3, 1600, 100);
            assert_eq!((*(*track).events.add(3)).ReadOrder, 0);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_flush_events_clears_track_buffer() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);

            rassa_capi::ass_process_chunk(track, b"first".as_ptr() as *const c_char, 5, 1000, 100);
            rassa_capi::ass_process_chunk(track, b"second".as_ptr() as *const c_char, 6, 2000, 100);
            assert_eq!((*track).n_events, 2);

            rassa_capi::ass_flush_events(track);

            assert_eq!((*track).n_events, 0);
            assert!(rassa_capi::ass_step_sub(track, 1500, 1) == 0);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_configured_prune_runs_during_render() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let renderer = rassa_capi::ass_renderer_init(library);
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            assert_eq!((*track).n_events, 1);
            rassa_capi::ass_configure_prune(track, 500);

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 2600, &mut detect_change);

            assert!(images.is_null());
            assert_eq!((*track).n_events, 0);
            assert!(detect_change >= 1);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_style_overrides_apply_on_load() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let (_storage, mut overrides) =
                style_override_list(&["PlayResX=640", "Default.FontSize=48", "Default.MarginL=33"]);
            rassa_capi::ass_set_style_overrides(library, overrides.as_mut_ptr());

            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );

            assert!(!track.is_null());
            assert_eq!((*track).PlayResX, 640);
            assert_eq!((*(*track).styles).FontSize as i32, 48);
            assert_eq!((*(*track).styles).MarginL, 33);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_process_force_style_mutates_existing_track() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_new_track(library);
            rassa_capi::ass_process_data(
                track,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *const c_char,
                INLINE_OVERRIDE_FIXTURE.len() as i32,
            );

            let (_storage, mut overrides) = style_override_list(&[
                "Timer=120.5",
                "Default.PrimaryColour=&H00010203&",
                "Default.Bold=1",
                "Default.Blur=4.5",
            ]);
            rassa_capi::ass_set_style_overrides(library, overrides.as_mut_ptr());
            rassa_capi::ass_process_force_style(track);

            assert_eq!((*track).Timer, 120.5);
            assert_eq!((*(*track).styles).PrimaryColour, 0x00010203);
            assert_eq!((*(*track).styles).Bold, 1);
            assert_eq!((*(*track).styles).Blur, 4.5);

            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_set_shaper_allows_rendering() {
        const SHAPING_FIXTURE: &str = "[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,48,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,office";

        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_read_memory(
                library,
                SHAPING_FIXTURE.as_ptr() as *mut c_char,
                SHAPING_FIXTURE.len(),
                ptr::null(),
            );
            let simple_renderer = rassa_capi::ass_renderer_init(library);
            let complex_renderer = rassa_capi::ass_renderer_init(library);
            let invalid_renderer = rassa_capi::ass_renderer_init(library);

            rassa_capi::ass_set_shaper(simple_renderer, ass::ShapingLevel::Simple as i32);
            rassa_capi::ass_set_shaper(complex_renderer, ass::ShapingLevel::Complex as i32);
            rassa_capi::ass_set_shaper(invalid_renderer, 99);

            let mut simple_change = 0;
            let simple =
                rassa_capi::ass_render_frame(simple_renderer, track, 500, &mut simple_change);
            let mut complex_change = 0;
            let complex =
                rassa_capi::ass_render_frame(complex_renderer, track, 500, &mut complex_change);
            let complex_signature = image_signatures(complex);
            let mut invalid_change = 0;
            let invalid =
                rassa_capi::ass_render_frame(invalid_renderer, track, 500, &mut invalid_change);
            let invalid_signature = image_signatures(invalid);

            assert!(!simple.is_null());
            assert!(!complex.is_null());
            assert!(!invalid.is_null());
            assert!(simple_change > 0);
            assert!(complex_change > 0);
            assert!(invalid_change > 0);
            assert!(!image_signatures(simple).is_empty());
            assert!(!complex_signature.is_empty());
            assert_eq!(invalid_signature, complex_signature);

            rassa_capi::ass_renderer_done(simple_renderer);
            rassa_capi::ass_renderer_done(complex_renderer);
            rassa_capi::ass_renderer_done(invalid_renderer);
            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }

    #[test]
    fn capi_set_hinting_allows_rendering() {
        unsafe {
            let library = rassa_capi::ass_library_init();
            let track = rassa_capi::ass_read_memory(
                library,
                INLINE_OVERRIDE_FIXTURE.as_ptr() as *mut c_char,
                INLINE_OVERRIDE_FIXTURE.len(),
                ptr::null(),
            );
            let renderer = rassa_capi::ass_renderer_init(library);

            rassa_capi::ass_set_hinting(renderer, ass::Hinting::Normal as i32);

            let mut detect_change = 0;
            let images = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);

            assert!(!images.is_null());
            assert!(detect_change > 0);
            assert!(!image_signatures(images).is_empty());

            rassa_capi::ass_renderer_done(renderer);
            rassa_capi::ass_free_track(track);
            rassa_capi::ass_library_done(library);
        }
    }
}
