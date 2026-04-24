use rassa_core::ass;
use rassa_fonts::FontconfigProvider;
use rassa_parse::{parse_script_text, ParsedTrack};
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
    use std::{env, ffi::{c_char, CString}, fs, path::PathBuf, ptr, time::{SystemTime, UNIX_EPOCH}};

    const INLINE_OVERRIDE_FIXTURE: &str = "[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,36,&H00112233,&H00445566,&H000A0B0C,&H00101010,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\an7\\pos(20,20)\\t(0,1000,\\1c&H00223344&)}{\\K100}Test";
    const STYLE_ONLY_FIXTURE: &str = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Alt,sans,18,&H00ABCDEF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,11,12,13,1";

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
    
    fn image_signatures(mut image: *mut rassa_capi::ASS_Image) -> Vec<(u32, i32, i32, i32, i32, Vec<u8>)> {
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
                signatures.push(((*image).color, (*image).dst_x, (*image).dst_y, (*image).w, (*image).h, bitmap));
                image = (*image).next;
            }
        }
        signatures
    }

    #[test]
    fn parses_inline_regression_fixture() {
        let track = parse_fixture(INLINE_OVERRIDE_FIXTURE);

        assert_eq!(track.events.len(), 1);
        assert_eq!(track.styles.len(), 1);
        assert_eq!(track.play_res_x, 320);
        assert_eq!(track.play_res_y, 180);
    }

    #[test]
    fn render_summary_is_deterministic_for_inline_fixture() {
        let first = render_fixture(INLINE_OVERRIDE_FIXTURE, 500);
        let second = render_fixture(INLINE_OVERRIDE_FIXTURE, 500);

        assert_eq!(first, second);
        assert!(first.iter().any(|plane| plane.kind == ass::ImageType::Character));
    }

    #[test]
    fn renders_upstream_compare_sample_sub1() {
        let script = include_str!("../../../libass/compare/test/sub1.ass");
        let summary = render_fixture(script, 2000);

        assert!(!summary.is_empty());
        assert!(summary.iter().any(|plane| plane.kind == ass::ImageType::Character));
        assert!(summary.iter().all(|plane| plane.lit_pixels > 0));
    }

    #[test]
    fn renders_upstream_compare_sample_sub2() {
        let script = include_str!("../../../libass/compare/test/sub2.ass");
        let summary = render_fixture(script, 152000);

        assert!(!summary.is_empty());
        assert!(summary.iter().any(|plane| plane.kind == ass::ImageType::Character));
        assert!(summary.iter().all(|plane| plane.width >= 0 && plane.height >= 0));
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
            rassa_capi::ass_process_chunk(track, first.as_ptr() as *const c_char, first.len() as i32, 1000, 500);
            rassa_capi::ass_process_chunk(track, second.as_ptr() as *const c_char, second.len() as i32, 3000, 500);

            assert_eq!((*track).n_events, 2);
            assert_eq!(rassa_capi::ass_step_sub(track, 1200, 1), 1800);
            assert_eq!(rassa_capi::ass_step_sub(track, 3200, -1), -200);

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

            let mut override_style = rassa_capi::ASS_Style::default();
            override_style.PrimaryColour = 0x000A0B0C;
            override_style.SecondaryColour = 0x000A0B0C;
            override_style.FontSize = 48.0;
            rassa_capi::ass_set_selective_style_override_enabled(renderer, ass::override_bits::STYLE);
            rassa_capi::ass_set_selective_style_override(renderer, &mut override_style);

            let overridden = rassa_capi::ass_render_frame(renderer, track, 500, &mut detect_change);
            let overridden_area = total_image_area(overridden);
            let overridden_colors = image_colors(overridden);

            assert!(overridden_area > baseline_area);
            assert!(overridden_colors.contains(&0x000A0B0C));
            assert!(!baseline_colors.contains(&0x000A0B0C));

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
            let (_storage, mut overrides) = style_override_list(&[
                "PlayResX=640",
                "Default.FontSize=48",
                "Default.MarginL=33",
            ]);
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

            rassa_capi::ass_set_shaper(simple_renderer, ass::ShapingLevel::Simple as i32);
            rassa_capi::ass_set_shaper(complex_renderer, ass::ShapingLevel::Complex as i32);

            let mut simple_change = 0;
            let simple = rassa_capi::ass_render_frame(simple_renderer, track, 500, &mut simple_change);
            let mut complex_change = 0;
            let complex = rassa_capi::ass_render_frame(complex_renderer, track, 500, &mut complex_change);

            assert!(!simple.is_null());
            assert!(!complex.is_null());
            assert!(simple_change > 0);
            assert!(complex_change > 0);
            assert!(!image_signatures(simple).is_empty());
            assert!(!image_signatures(complex).is_empty());

            rassa_capi::ass_renderer_done(simple_renderer);
            rassa_capi::ass_renderer_done(complex_renderer);
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