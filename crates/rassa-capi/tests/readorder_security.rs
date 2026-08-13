use std::ffi::c_char;

const CHUNK_TRACK_HEADER: &str = "[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

unsafe fn process_chunk(track: *mut rassa_capi::ASS_Track, packet: &[u8], start: i64) {
    unsafe {
        rassa_capi::ass_process_chunk(
            track,
            packet.as_ptr() as *const c_char,
            packet.len() as i32,
            start,
            100,
        );
    }
}

#[test]
fn negative_matroska_read_order_remains_prune_safe() {
    // CVE-2026-61626: accept negative ReadOrder, never use it as a bitmap index.
    unsafe {
        let library = rassa_capi::ass_library_init();
        assert!(!library.is_null());
        let track = rassa_capi::ass_new_track(library);
        assert!(!track.is_null());
        rassa_capi::ass_process_data(
            track,
            CHUNK_TRACK_HEADER.as_ptr() as *const c_char,
            CHUNK_TRACK_HEADER.len() as i32,
        );

        process_chunk(track, b"-1,0,Default,,0,0,0,,negative", 1_000);
        process_chunk(track, b"7,0,Default,,0,0,0,,valid", 3_000);
        process_chunk(track, b"7,0,Default,,0,0,0,,duplicate", 3_200);
        assert_eq!((*track).n_events, 2);
        assert_eq!((*(*track).events).ReadOrder, -1);
        assert_eq!((*(*track).events.add(1)).ReadOrder, 7);

        rassa_capi::ass_prune_events(track, 2_000);
        assert_eq!((*track).n_events, 1);
        assert_eq!((*(*track).events).ReadOrder, 7);

        process_chunk(track, b"7,0,Default,,0,0,0,,still-duplicate", 3_400);
        assert_eq!((*track).n_events, 1);

        rassa_capi::ass_prune_events(track, 4_000);
        assert_eq!((*track).n_events, 0);
        process_chunk(track, b"7,0,Default,,0,0,0,,reusable", 5_000);
        process_chunk(track, b"-2147483648,0,Default,,0,0,0,,minimum", 5_200);
        assert_eq!((*track).n_events, 2);
        assert_eq!((*(*track).events.add(1)).ReadOrder, i32::MIN);

        rassa_capi::ass_prune_events(track, 6_000);
        assert_eq!((*track).n_events, 0);

        rassa_capi::ass_free_track(track);
        rassa_capi::ass_library_done(library);
    }
}
