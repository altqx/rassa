use std::ffi::{CStr, CString, c_char};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CharacterSummary {
    color: u32,
    width: i32,
    coverage: u64,
}

unsafe fn character_summary(mut image: *mut rassa_capi::ASS_Image) -> CharacterSummary {
    let mut color = 0;
    let mut x_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut coverage = 0_u64;
    unsafe {
        while let Some(node) = image.as_ref() {
            if node.type_ == rassa_core::ass::ImageType::Character as i32 && !node.bitmap.is_null()
            {
                color = node.color;
                for y in 0..node.h {
                    for x in 0..node.w {
                        let value = *node.bitmap.add((y * node.stride + x) as usize);
                        if value == 0 {
                            continue;
                        }
                        let screen_x = node.dst_x + x;
                        x_min = x_min.min(screen_x);
                        x_max = x_max.max(screen_x + 1);
                        coverage += u64::from(value);
                    }
                }
            }
            image = node.next;
        }
    }
    CharacterSummary {
        color,
        width: x_max.checked_sub(x_min).unwrap_or(0),
        coverage,
    }
}

#[test]
fn public_event_and_style_mutations_invalidate_render_cache() {
    // ASS_Track exposes writable event/style arrays. libass reads them again
    // on every render, so same-address in-place changes must not be hidden by
    // Rassa's parsed-track/frame caches.
    const SCRIPT: &str = "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,50,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\\an7\\pos(20,20)}A\n";

    unsafe {
        let library = rassa_capi::ass_library_init();
        let renderer = rassa_capi::ass_renderer_init(library);
        rassa_capi::ass_set_frame_size(renderer, 320, 180);
        let font_path = CString::new(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../rassa-test/fixtures/libass/compare/test/font2.otf")
                .to_string_lossy()
                .as_bytes(),
        )
        .expect("font path is a C string");
        let family = CString::new("Aileron").expect("family is a C string");
        rassa_capi::ass_set_fonts(
            renderer,
            font_path.as_ptr(),
            family.as_ptr(),
            rassa_core::ass::DefaultFontProvider::None as i32,
            std::ptr::null(),
            1,
        );
        let track = rassa_capi::ass_read_memory(
            library,
            SCRIPT.as_ptr() as *mut c_char,
            SCRIPT.len(),
            std::ptr::null(),
        );
        assert!(!track.is_null());

        let mut first_change = -1;
        let first = character_summary(rassa_capi::ass_render_frame(
            renderer,
            track,
            500,
            &mut first_change,
        ));
        assert_eq!(first_change, 2);

        let event = (*track).events;
        assert!(!event.is_null());
        let text_len = CStr::from_ptr((*event).Text).to_bytes().len();
        *(*event).Text.add(text_len - 1) = b'W' as c_char;
        let mut text_change = -1;
        let changed_text = character_summary(rassa_capi::ass_render_frame(
            renderer,
            track,
            500,
            &mut text_change,
        ));
        assert_eq!(text_change, 2);
        assert!(changed_text.width > first.width);
        assert_ne!(changed_text.coverage, first.coverage);

        let style_index = usize::try_from((*event).Style).expect("nonnegative style index");
        let style = (*track).styles.add(style_index);
        (*style).PrimaryColour = 0xFF00_0000;
        let mut style_change = -1;
        let changed_style = character_summary(rassa_capi::ass_render_frame(
            renderer,
            track,
            500,
            &mut style_change,
        ));
        assert_eq!(style_change, 2);
        assert_eq!(changed_style.color, 0xFF00_0000);

        rassa_capi::ass_free_track(track);
        rassa_capi::ass_renderer_done(renderer);
        rassa_capi::ass_library_done(library);
    }
}
