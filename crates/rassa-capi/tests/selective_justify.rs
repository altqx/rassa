use rassa_core::ass;
use std::{
    ffi::{CString, c_char},
    ptr,
};

const SCRIPT: &str = "[Script Info]\n\
ScriptType: v4.00+\n\
PlayResX: 800\n\
PlayResY: 600\n\
WrapStyle: 2\n\
\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
Style: Default,Aileron,60,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\
\n\
[Events]\n\
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,AAAAAA\\NAA\n";

#[test]
fn selective_left_justify_aligns_short_line_to_widest_line() {
    unsafe {
        let library = rassa_capi::ass_library_init();
        assert!(!library.is_null());
        let renderer = rassa_capi::ass_renderer_init(library);
        assert!(!renderer.is_null());
        rassa_capi::ass_set_frame_size(renderer, 800, 600);
        rassa_capi::ass_set_storage_size(renderer, 800, 600);

        let font = CString::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rassa-test/fixtures/libass/compare/test/font2.otf"
        ))
        .expect("fixture font path");
        let family = CString::new("Aileron").expect("font family");
        rassa_capi::ass_set_fonts(
            renderer,
            font.as_ptr(),
            family.as_ptr(),
            ass::DefaultFontProvider::None as i32,
            ptr::null(),
            0,
        );

        let track = rassa_capi::ass_read_memory(
            library,
            SCRIPT.as_ptr() as *mut c_char,
            SCRIPT.len(),
            ptr::null(),
        );
        assert!(!track.is_null());

        let mut override_style = rassa_capi::ASS_Style {
            Justify: ass::ASS_JUSTIFY_LEFT,
            ..Default::default()
        };
        rassa_capi::ass_set_selective_style_override(renderer, &mut override_style);
        rassa_capi::ass_set_selective_style_override_enabled(renderer, ass::override_bits::JUSTIFY);

        let mut change = -1;
        let mut image = rassa_capi::ass_render_frame(renderer, track, 1_000, &mut change);
        let mut character_x = Vec::new();
        while let Some(current) = image.as_ref() {
            if current.type_ == ass::ImageType::Character as i32 {
                character_x.push(current.dst_x);
            }
            image = current.next;
        }

        assert_eq!(change, 2);
        assert_eq!(character_x.len(), 2, "one character plane per line");
        assert!(
            (character_x[0] - character_x[1]).abs() <= 1,
            "ASS_OVERRIDE_BIT_JUSTIFY=LEFT must align both line starts: {character_x:?}",
        );

        rassa_capi::ass_free_track(track);
        rassa_capi::ass_renderer_done(renderer);
        rassa_capi::ass_library_done(library);
    }
}
