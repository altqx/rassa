use super::*;
use rassa_fonts::{FontProvider, FontQuery, FontconfigProvider, NullFontProvider};
use rassa_parse::parse_script_text;

fn config(
    frame_width: i32,
    frame_height: i32,
    margins: rassa_core::Margins,
    use_margins: bool,
) -> RendererConfig {
    RendererConfig {
        frame: Size {
            width: frame_width,
            height: frame_height,
        },
        margins,
        use_margins,
        ..RendererConfig::default()
    }
}

fn total_plane_area(planes: &[ImagePlane]) -> i32 {
    planes
        .iter()
        .map(|plane| plane.size.width * plane.size.height)
        .sum()
}

#[test]
fn fad_uses_libass_truncating_alpha_interpolation() {
    let event = ParsedEvent {
        start: 0,
        duration: 4000,
        ..ParsedEvent::default()
    };

    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 1000,
                fade_out_ms: 1000,
            },
            Some(&event),
            500,
        ),
        127
    );
    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 1000,
                fade_out_ms: 1000,
            },
            Some(&event),
            3500,
        ),
        127
    );
}

#[test]
fn fad_uses_libass_wrapping_out_start_when_fade_out_exceeds_duration() {
    let event = ParsedEvent {
        start: 0,
        duration: 800,
        ..ParsedEvent::default()
    };

    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 100,
                fade_out_ms: 1000,
            },
            Some(&event),
            100,
        ),
        76
    );
    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 100,
                fade_out_ms: 1000,
            },
            Some(&event),
            400,
        ),
        153
    );
}

#[test]
fn fade_alpha_combines_with_existing_colour_alpha() {
    assert_eq!(with_fade_alpha(0xFF00_0080, 0), 0xFF00_0080);
    assert_eq!(with_fade_alpha(0xFF00_0000, 127), 0xFF00_007F);
    assert_eq!(with_fade_alpha(0xFF00_0080, 127), 0xFF00_00BF);
}

fn vertical_span(planes: &[ImagePlane]) -> i32 {
    let min_y = planes
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("plane");
    let max_y = planes
        .iter()
        .map(|plane| plane.destination.y + plane.size.height)
        .max()
        .expect("plane");
    max_y - min_y
}

fn kind_bounds(planes: &[ImagePlane], kind: ass::ImageType) -> Option<Rect> {
    let mut matching_planes = planes.iter().filter(|plane| plane.kind == kind);
    let first = matching_planes.next()?;
    let mut bounds = Rect {
        x_min: first.destination.x,
        y_min: first.destination.y,
        x_max: first.destination.x + first.size.width,
        y_max: first.destination.y + first.size.height,
    };
    for plane in matching_planes {
        bounds.x_min = bounds.x_min.min(plane.destination.x);
        bounds.y_min = bounds.y_min.min(plane.destination.y);
        bounds.x_max = bounds.x_max.max(plane.destination.x + plane.size.width);
        bounds.y_max = bounds.y_max.max(plane.destination.y + plane.size.height);
    }
    Some(bounds)
}

fn character_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    kind_bounds(planes, ass::ImageType::Character)
}

fn kind_visible_bounds(planes: &[ImagePlane], kind: ass::ImageType) -> Option<Rect> {
    let matching: Vec<ImagePlane> = planes
        .iter()
        .filter(|plane| plane.kind == kind)
        .cloned()
        .collect();
    visible_bounds(&matching)
}

fn visible_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for plane in planes {
        let stride = plane.stride.max(0) as usize;
        if stride == 0 {
            continue;
        }
        for y in 0..plane.size.height.max(0) as usize {
            for x in 0..plane.size.width.max(0) as usize {
                if plane.bitmap[y * stride + x] == 0 {
                    continue;
                }
                let px = plane.destination.x + x as i32;
                let py = plane.destination.y + y as i32;
                match &mut bounds {
                    Some(rect) => {
                        rect.x_min = rect.x_min.min(px);
                        rect.y_min = rect.y_min.min(py);
                        rect.x_max = rect.x_max.max(px + 1);
                        rect.y_max = rect.y_max.max(py + 1);
                    }
                    None => {
                        bounds = Some(Rect {
                            x_min: px,
                            y_min: py,
                            x_max: px + 1,
                            y_max: py + 1,
                        });
                    }
                }
            }
        }
    }
    bounds
}

fn drawing_alignment_script(alignment: i32, override_tags: &str, event_margins: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,32,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,{alignment},30,50,15,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,{event_margins},,{{{override_tags}\\p1}}m 0 0 l 40 0 40 20 0 20\n"
    )
}

fn render_drawing_bounds(script: &str) -> Rect {
    let track = parse_script_text(script).expect("alignment probe script should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    visible_bounds(&planes).expect("drawing probe should produce visible pixels")
}

fn text_alignment_script(alignment: i32, event_margins: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,32,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,{alignment},30,50,15,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,{event_margins},,Margin\n"
    )
}

fn render_text_bounds(script: &str) -> Option<Rect> {
    let track = parse_script_text(script).expect("text alignment probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    visible_bounds(&planes)
}

fn render_text_bounds_with_config(script: &str, config: &RendererConfig) -> Option<Rect> {
    let track = parse_script_text(script).expect("text alignment probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(&track, &provider, 500, config);
    visible_bounds(&planes)
}

fn baseline_fontconfig_matches_dejavu_fallback(family: &str) -> bool {
    baseline_fontconfig_family_contains(family, "DejaVu")
}

fn baseline_fontconfig_family_contains(family: &str, expected: &str) -> bool {
    let provider = FontconfigProvider::new();
    provider
        .resolve(&FontQuery::new(family))
        .family
        .contains(expected)
}

fn render_text_plane_bounds(script: &str) -> Option<Rect> {
    render_text_plane_bounds_at(script, 500)
}

fn render_text_plane_bounds_at(script: &str, now_ms: i64) -> Option<Rect> {
    render_text_kind_bounds_at(script, now_ms, ass::ImageType::Character)
}

fn render_text_visible_bounds_at(script: &str, now_ms: i64) -> Option<Rect> {
    let track = parse_script_text(script).expect("text visible probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, now_ms);
    visible_bounds(&planes)
}

fn render_text_kind_bounds_at(script: &str, now_ms: i64, kind: ass::ImageType) -> Option<Rect> {
    let track = parse_script_text(script).expect("text plane probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, now_ms);
    kind_bounds(&planes, kind)
}

fn assert_rect_near(actual: Option<Rect>, expected: Rect, tolerance: i32, context: &str) {
    let actual = actual.unwrap_or_else(|| panic!("{context}: expected {expected:?}, got None"));
    assert!(
        (actual.x_min - expected.x_min).abs() <= tolerance
            && (actual.y_min - expected.y_min).abs() <= tolerance
            && (actual.x_max - expected.x_max).abs() <= tolerance
            && (actual.y_max - expected.y_max).abs() <= tolerance,
        "{context}: actual={actual:?} expected={expected:?} tolerance={tolerance}"
    );
}

#[test]
fn decimal_positioned_drawing_uses_exact_coordinates() {
    let decimal = drawing_alignment_script(7, "\\pos(100.6,50.6)", "0,0,0");
    let integer = drawing_alignment_script(7, "\\pos(101,51)", "0,0,0");

    assert_eq!(
        render_drawing_bounds(&decimal),
        render_drawing_bounds(&integer)
    );
}

#[test]
fn decimal_move_interpolates_from_exact_coordinates() {
    let decimal = drawing_alignment_script(7, "\\move(10.5,20.5,110.5,120.5,0,1000)", "0,0,0");
    let integer = drawing_alignment_script(7, "\\move(61,71,61,71)", "0,0,0");

    assert_eq!(
        render_drawing_bounds(&decimal),
        render_drawing_bounds(&integer)
    );
}

#[test]
fn downscaled_positioned_text_scales_font_and_anchor_like_libass() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\an5\\pos(320,180)}POS\n";
    let config = RendererConfig {
        frame: Size {
            width: 320,
            height: 180,
        },
        storage: Size {
            width: 320,
            height: 180,
        },
        pixel_aspect: 1.0,
        shaping: ass::ShapingLevel::Complex,
        ..Default::default()
    };
    let actual = render_text_bounds_with_config(script, &config)
        .expect("positioned text should render in downscaled frame");
    let expected = Rect {
        x_min: 141,
        y_min: 83,
        x_max: 179,
        y_max: 97,
    };

    assert!(
        (actual.x_min - expected.x_min).abs() <= 2 && (actual.y_min - expected.y_min).abs() <= 1,
        "downscaled \\pos anchor should stay in libass position: actual={actual:?} expected={expected:?}"
    );
    assert!(
        (actual.width() - expected.width()).abs() <= 2
            && (actual.height() - expected.height()).abs() <= 2,
        "downscaled \\pos text must scale glyph dimensions like libass: actual={actual:?} expected={expected:?}"
    );
}

#[test]
fn positioned_center_text_anchors_visible_ink_not_layout_advance() {
    if !baseline_fontconfig_matches_dejavu_fallback("Againts") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs140\\bord0\\blur1\\fnAgaints\\pos(947.46,191.6)}ท่านคาชิวากิ อาซาฮิ\n";
    let actual = render_text_bounds(script).expect("baseline positioned text should render");
    let center_x = (actual.x_min + actual.x_max) / 2;

    assert!(
        (center_x - 947).abs() <= 8,
        "\\pos center anchor must use visible rendered text width, not stale layout advance: bounds={actual:?} center_x={center_x}"
    );
    assert!(
        (actual.y_min - 80).abs() <= 4,
        "bottom-aligned \\pos text must reserve libass-like descender space below visible glyphs: bounds={actual:?}"
    );
}

#[test]
fn positioned_multiline_text_uses_libass_like_line_gap_and_descender_space() {
    if !baseline_fontconfig_matches_dejavu_fallback("Raphtalia") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs100\\bord0\\blur1\\fnRaphtalia\\b1\\pos(944.4,752.8)}จงเตรียมตัว\\Nให้พร้อมสรรพก่อนมา\n";
    let actual =
        render_text_bounds(script).expect("baseline multiline positioned text should render");

    assert!(
        (actual.y_min - 570).abs() <= 6,
        "multiline bottom-aligned \\pos text should use libass-like vertical block metrics: bounds={actual:?}"
    );
    assert!(
        (actual.height() - 158).abs() <= 8,
        "multiline positioned text should keep libass-like line gap: bounds={actual:?}"
    );
}

#[test]
fn positioned_multiline_text_aligns_deep_glyph_bottoms_like_libass() {
    if !baseline_fontconfig_matches_dejavu_fallback("Raphtalia") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs100\\bord0\\blur1\\fnRaphtalia\\b1\\pos(928,992)}ห้ามท่านหนี\\Nจากคำขอนี้เป็นอันขาด\n";
    let actual =
        render_text_bounds(script).expect("baseline multiline positioned text should render");

    assert!(
        (actual.y_min - 808).abs() <= 4,
        "top of deep-glyph multiline block should match libass baseline line 1270: bounds={actual:?}"
    );
    assert!(
        (actual.y_max - 968).abs() <= 4,
        "bottom-aligned \\pos should keep deep Thai glyphs above the libass descender gap: bounds={actual:?}"
    );
    assert!(
        (actual.height() - 160).abs() <= 6,
        "deep-glyph multiline block should keep libass-like visible-bottom line spacing: bounds={actual:?}"
    );
}

#[test]
fn positioned_thai_deep_glyphs_keep_libass_like_bottom_anchor() {
    if !baseline_fontconfig_family_contains("K2D ExtraBold", "K2D") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 400\nPlayResY: 240\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED TH2,K2D ExtraBold,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0,0,2,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,ED TH2,,0,0,0,,{\\an2\\pos(200,180)\\bord0\\shad0\\blur0}อุ อู ญ ฐ ฏ ฎ\n";
    let actual = render_text_bounds(script).expect("Thai positioned text should render");

    assert!(
        (actual.y_min - 132).abs() <= 4,
        "Thai lower vowels and descender glyphs should not be raised above libass-like bottom anchor: bounds={actual:?}"
    );
    assert!(
        (actual.y_max - 173).abs() <= 4,
        "Thai deep glyph bottom should stay near libass-like descender plane: bounds={actual:?}"
    );
}

#[test]
fn bottom_positioned_latin_glyph_uses_libass_like_sub_anchor() {
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nWrapStyle: 0\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED TH2,Arial,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0.7,3,2,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,ED TH2,,0,0,0,fx,{\\an2\\pos(1167.9,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&}A\n";

    assert_rect_near(
        render_text_kind_bounds_at(script, 500, ass::ImageType::Character),
        Rect {
            x_min: 1145,
            y_min: 989,
            x_max: 1193,
            y_max: 1037,
        },
        1,
        "02.ass lower Latin bottom-positioned glyphs should use libass-like sub-anchor placement",
    );
}

#[test]
fn top_center_latin_single_glyph_uses_libass_bbox_anchor() {
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(727.1,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}I\n";

    assert_eq!(
        render_text_kind_bounds_at(script, 1_308_405, ass::ImageType::Character),
        Some(Rect {
            x_min: 720,
            y_min: 39,
            x_max: 744,
            y_max: 95,
        }),
        "top-center Latin single-glyph \\pos should use libass bbox base point; this guards 02.ass line 113 plane geometry"
    );
}

#[test]
fn top_center_latin_varied_glyphs_use_libass_metric_anchor() {
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(727.1,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}I\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(768.4,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}m\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(848.2,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}i\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(894.2,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}y\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(984.1,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}g\nDialogue: 4,0:21:46.23,0:21:50.58,ED2,,0,0,0,fx,{\\pos(1035.1,65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}l\n";
    let track = parse_script_text(script).expect("top Latin regression script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 1_308_405);
    let actual = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(plane_rect)
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            Rect {
                x_min: 720,
                y_min: 39,
                x_max: 744,
                y_max: 95
            },
            Rect {
                x_min: 742,
                y_min: 49,
                x_max: 798,
                y_max: 105
            },
            Rect {
                x_min: 841,
                y_min: 37,
                x_max: 865,
                y_max: 93
            },
            Rect {
                x_min: 874,
                y_min: 49,
                x_max: 930,
                y_max: 105
            },
            Rect {
                x_min: 965,
                y_min: 49,
                x_max: 1005,
                y_max: 105
            },
            Rect {
                x_min: 1028,
                y_min: 37,
                x_max: 1052,
                y_max: 93
            },
        ],
        "positioned \\an5 Latin glyphs must share libass's font-metric anchor instead of using each glyph bitmap top as the line ascender"
    );
}

#[test]
fn lower_ed_th2_positioned_per_glyph_line_matches_libass_bounds() {
    if !baseline_fontconfig_family_contains("K2D ExtraBold", "K2D") {
        return;
    }
    let provider = FontconfigProvider::new();
    let script = r#"[Script Info]
ScriptType: v4.00+
WrapStyle: 0
PlayResX: 1920
PlayResY: 1080
ScaledBorderAndShadow: yes
YCbCr Matrix: TV.709

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED TH2,K2D ExtraBold,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0.7,3,2,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(677.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(0,160,\alpha&H00&)\t(4790,\alpha&HFF&)}ฉั
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(703.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(20,180,\alpha&H00&)\t(4810,\alpha&HFF&)}น
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(728.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(40,200,\alpha&H00&)\t(4830,\alpha&HFF&)}คื
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(752.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(60,220,\alpha&H00&)\t(4850,\alpha&HFF&)}อ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(775.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(80,240,\alpha&H00&)\t(4870,\alpha&HFF&)}ส
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(797.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(100,260,\alpha&H00&)\t(4890,\alpha&HFF&)}า
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(818.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(120,280,\alpha&H00&)\t(4910,\alpha&HFF&)}ว
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(840.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(140,300,\alpha&H00&)\t(4930,\alpha&HFF&)}แ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(863.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(160,320,\alpha&H00&)\t(4950,\alpha&HFF&)}ก
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(887.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(180,340,\alpha&H00&)\t(4970,\alpha&HFF&)}ร่
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(909.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(200,360,\alpha&H00&)\t(4990,\alpha&HFF&)}ง
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(931.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(220,380,\alpha&H00&)\t(5010,\alpha&HFF&)}ผู้
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(952.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(240,400,\alpha&H00&)\t(5030,\alpha&HFF&)}ไ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(972.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(260,420,\alpha&H00&)\t(5050,\alpha&HFF&)}ร้
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(990.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(280,440,\alpha&H00&)\t(5070,\alpha&HFF&)}เ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1010,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(300,460,\alpha&H00&)\t(5090,\alpha&HFF&)}ที
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1034.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(320,480,\alpha&H00&)\t(5110,\alpha&HFF&)}ย
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1059.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(340,500,\alpha&H00&)\t(5130,\alpha&HFF&)}ม
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1085.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(360,520,\alpha&H00&)\t(5150,\alpha&HFF&)}ท
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1108.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(380,540,\alpha&H00&)\t(5170,\alpha&HFF&)}า
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1131.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(400,560,\alpha&H00&)\t(5190,\alpha&HFF&)}น
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1149.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(420,580,\alpha&H00&)\t(5210,\alpha&HFF&)}
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1167.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(440,600,\alpha&H00&)\t(5230,\alpha&HFF&)}A
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1192.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(460,620,\alpha&H00&)\t(5250,\alpha&HFF&)}h
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1208.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(480,640,\alpha&H00&)\t(5270,\alpha&HFF&)}
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1224.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(500,660,\alpha&H00&)\t(5290,\alpha&HFF&)}a
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1246.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(520,680,\alpha&H00&)\t(5310,\alpha&HFF&)}h
"#;
    let track = parse_script_text(script).expect("lower ED TH2 regression script should parse");
    let engine = RenderEngine::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 1_308_800);
    assert_eq!(
        planes.len(),
        75,
        "lower ED TH2 fixture should emit one shadow, outline, and character plane per visible glyph"
    );
    let actual =
        visible_bounds(&planes).expect("lower ED TH2 fixture should render visible pixels");
    let expected = Rect {
        x_min: 663,
        y_min: 986,
        x_max: 1267,
        y_max: 1045,
    };

    assert_rect_near(
        Some(actual),
        expected,
        5,
        "lower ED TH2 logic should keep glyph count and visible bounds near libass while rasterizer parity is out of scope",
    );
}

fn assert_lower_ed_th2_single_event_planes(
    name: &str,
    override_tags: &str,
    text: &str,
    shadow: Rect,
    outline: Rect,
    character: Rect,
) {
    if !baseline_fontconfig_family_contains("K2D ExtraBold", "Liberation") {
        return;
    }
    let script = format!(
        "{}Dialogue: 0,0:22:56.14,0:23:00.72,ED TH2,,0,0,0,fx,{{{override_tags}}}{text}\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script).expect("lower ED TH2 single-event probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 1_380_000);

    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        shadow,
        0,
        &format!("02.ass {name} ED TH2 shadow allocation should match libass"),
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        outline,
        0,
        &format!("02.ass {name} ED TH2 outline allocation should match libass"),
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        character,
        0,
        &format!("02.ass {name} ED TH2 character allocation should match libass"),
    );
}

#[test]
fn current_02ass_lower_thai_late_fade_visible_bounds_match_libass() {
    if !baseline_fontconfig_family_contains("K2D ExtraBold", "Liberation") {
        return;
    }

    struct Case {
        name: &'static str,
        tags: &'static str,
        text: &'static str,
        shadow: Rect,
        outline: Rect,
        character: Rect,
    }

    let cases = [
        Case {
            name: "line 22116",
            tags: r"\an2\pos(1014.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(100,260,\alpha&H00&)\t(1300,\alpha&HFF&)",
            text: "ะ",
            shadow: rect_xywh(1008, 1007, 19, 25),
            outline: rect_xywh(1005, 1004, 19, 25),
            character: rect_xywh(1005, 1005, 19, 23),
        },
        Case {
            name: "line 22115",
            tags: r"\an2\pos(992.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(80,240,\alpha&H00&)\t(1280,\alpha&HFF&)",
            text: "อ",
            shadow: rect_xywh(983, 1005, 24, 28),
            outline: rect_xywh(980, 1002, 24, 28),
            character: rect_xywh(981, 1002, 22, 28),
        },
        Case {
            name: "line 22111",
            tags: r"\an2\pos(906.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(0,160,\alpha&H00&)\t(1200,\alpha&HFF&)",
            text: "กั",
            shadow: rect_xywh(897, 993, 29, 40),
            outline: rect_xywh(894, 990, 29, 40),
            character: rect_xywh(895, 991, 27, 39),
        },
    ];

    for case in cases {
        let script = format!(
            "{}Dialogue: 0,0:00:00.00,0:00:01.70,ED TH2,,0,0,0,fx,{{{}}}{}\n",
            current_02ass_ed2_header(),
            case.tags,
            case.text
        );
        let track =
            parse_script_text(&script).expect("lower ED TH2 late-fade visible probe should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 1_390);

        assert_rect_near(
            kind_visible_bounds(&planes, ass::ImageType::Shadow),
            case.shadow,
            0,
            &format!(
                "02.ass {} ED TH2 shadow visible ink should match libass",
                case.name
            ),
        );
        assert_rect_near(
            kind_visible_bounds(&planes, ass::ImageType::Outline),
            case.outline,
            0,
            &format!(
                "02.ass {} ED TH2 outline visible ink should match libass",
                case.name
            ),
        );
        assert_rect_near(
            kind_visible_bounds(&planes, ass::ImageType::Character),
            case.character,
            0,
            &format!(
                "02.ass {} ED TH2 character visible ink should match libass",
                case.name
            ),
        );
    }
}

#[test]
fn current_02ass_lower_thai_fallback_glyphs_match_libass_allocation() {
    assert_lower_ed_th2_single_event_planes(
        "line 22008",
        "\\an2\\pos(914.2,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(280,440,\\alpha&H00&)\\t(4360,\\alpha&HFF&)",
        "ะ",
        Rect {
            x_min: 907,
            y_min: 1007,
            x_max: 939,
            y_max: 1039,
        },
        Rect {
            x_min: 904,
            y_min: 1004,
            x_max: 936,
            y_max: 1036,
        },
        Rect {
            x_min: 905,
            y_min: 1005,
            x_max: 937,
            y_max: 1037,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22014",
        "\\an2\\pos(1044.2,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(400,560,\\alpha&H00&)\\t(4480,\\alpha&HFF&)",
        "ฟ",
        Rect {
            x_min: 1032,
            y_min: 997,
            x_max: 1064,
            y_max: 1045,
        },
        Rect {
            x_min: 1029,
            y_min: 994,
            x_max: 1061,
            y_max: 1042,
        },
        Rect {
            x_min: 1030,
            y_min: 995,
            x_max: 1062,
            y_max: 1043,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22018",
        "\\an2\\pos(1140.4,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(480,640,\\alpha&H00&)\\t(4560,\\alpha&HFF&)",
        "า",
        Rect {
            x_min: 1132,
            y_min: 1005,
            x_max: 1164,
            y_max: 1037,
        },
        Rect {
            x_min: 1129,
            y_min: 1002,
            x_max: 1161,
            y_max: 1034,
        },
        Rect {
            x_min: 1130,
            y_min: 1002,
            x_max: 1162,
            y_max: 1034,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22005",
        "\\an2\\pos(847.5,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(220,380,\\alpha&H00&)\\t(4300,\\alpha&HFF&)",
        "ว",
        Rect {
            x_min: 838,
            y_min: 1005,
            x_max: 870,
            y_max: 1037,
        },
        Rect {
            x_min: 835,
            y_min: 1002,
            x_max: 867,
            y_max: 1034,
        },
        Rect {
            x_min: 835,
            y_min: 1002,
            x_max: 867,
            y_max: 1034,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22017",
        "\\an2\\pos(1116.2,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(460,620,\\alpha&H00&)\\t(4540,\\alpha&HFF&)",
        "ฟ้",
        Rect {
            x_min: 1104,
            y_min: 992,
            x_max: 1142,
            y_max: 1045,
        },
        Rect {
            x_min: 1101,
            y_min: 989,
            x_max: 1139,
            y_max: 1042,
        },
        Rect {
            x_min: 1102,
            y_min: 989,
            x_max: 1140,
            y_max: 1043,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 21998",
        "\\an2\\pos(689.9,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(80,240,\\alpha&H00&)\\t(4160,\\alpha&HFF&)",
        "ว่",
        Rect {
            x_min: 680,
            y_min: 994,
            x_max: 713,
            y_max: 1037,
        },
        Rect {
            x_min: 677,
            y_min: 991,
            x_max: 710,
            y_max: 1034,
        },
        Rect {
            x_min: 678,
            y_min: 992,
            x_max: 710,
            y_max: 1034,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22011",
        "\\an2\\pos(972,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(340,500,\\alpha&H00&)\\t(4420,\\alpha&HFF&)",
        "ลึ",
        Rect {
            x_min: 962,
            y_min: 992,
            x_max: 994,
            y_max: 1037,
        },
        Rect {
            x_min: 959,
            y_min: 989,
            x_max: 991,
            y_max: 1034,
        },
        Rect {
            x_min: 960,
            y_min: 990,
            x_max: 992,
            y_max: 1034,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22013",
        "\\an2\\pos(1018.8,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(380,540,\\alpha&H00&)\\t(4460,\\alpha&HFF&)",
        "สู่",
        Rect {
            x_min: 1009,
            y_min: 994,
            x_max: 1047,
            y_max: 1049,
        },
        Rect {
            x_min: 1006,
            y_min: 991,
            x_max: 1044,
            y_max: 1046,
        },
        Rect {
            x_min: 1007,
            y_min: 992,
            x_max: 1045,
            y_max: 1046,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22009",
        "\\an2\\pos(931,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(300,460,\\alpha&H00&)\\t(4380,\\alpha&HFF&)",
        "เ",
        Rect {
            x_min: 928,
            y_min: 1005,
            x_max: 944,
            y_max: 1037,
        },
        Rect {
            x_min: 925,
            y_min: 1002,
            x_max: 941,
            y_max: 1034,
        },
        Rect {
            x_min: 926,
            y_min: 1002,
            x_max: 942,
            y_max: 1034,
        },
    );
    assert_lower_ed_th2_single_event_planes(
        "line 22004",
        "\\an2\\pos(824.5,1050)\\bord0.7\\shad3\\blur0\\c&HFFFFFF&\\3c&H000000&\\4c&HB5B7B7&\\fad(200,400)\\alpha&HFF&\\t(200,360,\\alpha&H00&)\\t(4280,\\alpha&HFF&)",
        "ห้",
        Rect {
            x_min: 813,
            y_min: 992,
            x_max: 855,
            y_max: 1037,
        },
        Rect {
            x_min: 810,
            y_min: 989,
            x_max: 852,
            y_max: 1034,
        },
        Rect {
            x_min: 811,
            y_min: 989,
            x_max: 853,
            y_max: 1034,
        },
    );
}

#[test]
fn rotated_positioned_text_keeps_libass_like_transparent_frz_plane() {
    if !baseline_fontconfig_family_contains("Raphtalia", "Raphtalia") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs66\\shad\\bord0\\blur1\\fnRaphtalia\\c&H070707&\\b0\\fscx99\\fscy107\\frz345.2\\pos(1258.48,593.06)}หลังเลิกเรียน จะรอที่\n";
    let actual = render_text_plane_bounds(script).expect("rotated positioned text should render");
    let expected = Rect {
        x_min: 1091,
        y_min: 499,
        x_max: 1461,
        y_max: 626,
    };

    assert!(
        (actual.x_min - expected.x_min).abs() <= 3
            && (actual.y_min - expected.y_min).abs() <= 3
            && (actual.x_max - expected.x_max).abs() <= 3
            && (actual.y_max - expected.y_max).abs() <= 3,
        "rotated positioned text should keep libass-like transparent \\frz plane: actual={actual:?} expected={expected:?}"
    );
}

#[test]
fn decimal_clipped_transformed_single_char_keeps_libass_like_plane() {
    if !baseline_fontconfig_matches_dejavu_fallback("OFL Sorts Mill Goudy TT") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 8,0:00:00.00,0:00:01.00,ED2,,0,0,0,fx,{\\move(727.1,73,727.1,65)\\org(637.1,-25)\\t(53.571428571429,107.14285714286,\\frz4)\\t(107.14285714286,160.71428571429,\\frz-4)\\t(160.71428571429,214.28571428571,\\frz4\\t(214.28571428571,267.85714285714,\\frz-4\\t(267.85714285714,321.42857142857,\\frz4\\t(321.42857142857,375,\\frz-4\\t(375,428.57142857143,\\frz4\\t(857.14285714286,482.14285714286,\\frz-4\\t(482.14285714286,535.71428571429,\\frz4\\t(535.71428571429,589.28571428571,\\frz-4\\t(589.28571428571,642.85714285714,\\frz4\\t(642.85714285714,696.42857142857,\\frz-4\\t(696.42857142857,750,\\frz0)))))))))))\\b0\\bord0\\blur0.2\\shad0\\an5\\fs80\\t(0,750,\\fs70\\frz0)\\clip(659.3,63.6,1260.8,77.4)\\c&H9DD9FC&}I\n";
    let actual = render_text_plane_bounds(script)
        .expect("02.ass-style decimal clipped transformed glyph should emit a plane");

    assert_eq!(
        actual,
        Rect {
            x_min: 721,
            y_min: 63,
            x_max: 745,
            y_max: 77,
        },
        "decimal rectangular clip over transformed one-char text should keep the current libass ASS_Image plane geometry"
    );
}

#[test]
fn clipped_org_move_single_char_slice_keeps_libass_like_plane() {
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,Arial,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.93,ED2,,0,0,0,fx,{\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714285714,\frz4)\t(132.85714285714,199.28571428571,\frz-4)\t(199.28571428571,265.71428571429,\frz4\t(265.71428571429,332.14285714286,\frz-4\t(332.14285714286,398.57142857143,\frz4\t(398.57142857143,465,\frz-4\t(465,531.42857142857,\frz4\t(1062.8571428571,597.85714285714,\frz-4\t(597.85714285714,664.28571428571,\frz4\t(664.28571428571,730.71428571429,\frz-4\t(730.71428571429,797.14285714286,\frz4\t(797.14285714286,863.57142857143,\frz-4\t(863.57142857143,930,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,930,\fs70\frz0)\clip(659.3,32.4,1260.8,45.8)\c&HDEF2FE&}A
"#;
    assert_rect_near(
        render_text_plane_bounds_at(script, 870),
        Rect {
            x_min: 1046,
            y_min: 39,
            x_max: 1102,
            y_max: 45,
        },
        2,
        "02.ass line 577-style clipped org/move transformed glyph should retain libass plane geometry",
    );
}

#[test]
fn clipped_org_move_empty_edge_slices_keep_libass_like_planes() {
    if !baseline_fontconfig_matches_dejavu_fallback("OFL Sorts Mill Goudy TT") {
        return;
    }

    let script = |clip: &str, text: &str, move_x: &str, move_y: &str, org_x: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.93,ED2,,0,0,0,fx,{{\move({move_x},{move_y},{move_x},65)\org({org_x},-25)\t(66.428571428571,132.85714285714,\frz4)\t(132.85714285714,199.28571428571,\frz-4)\t(199.28571428571,265.71428571429,\frz4\t(265.71428571429,332.14285714286,\frz-4\t(332.14285714286,398.57142857143,\frz4\t(398.57142857143,465,\frz-4\t(465,531.42857142857,\frz4\t(1062.8571428571,597.85714285714,\frz-4\t(597.85714285714,664.28571428571,\frz4\t(664.28571428571,730.71428571429,\frz-4\t(730.71428571429,797.14285714286,\frz4\t(797.14285714286,863.57142857143,\frz-4\t(863.57142857143,930,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,930,\fs70\frz0){clip}\c&H62C3FA&}}{text}
"#
        )
    };

    assert_rect_near(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,92.2,1260.8,106.36666666667)",
                "A",
                "1072.3",
                "57",
                "982.3",
            ),
            870,
        ),
        Rect {
            x_min: 1047,
            y_min: 92,
            x_max: 1103,
            y_max: 93,
        },
        1,
        "02.ass lower empty clipped A slice should keep libass transparent ASS_Image plane geometry",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,27.2,1260.8,40.533333333333)",
                "h",
                "1106.8",
                "73",
                "1016.8",
            ),
            870,
        ),
        Rect {
            x_min: 1088,
            y_min: 36,
            x_max: 1128,
            y_max: 40,
        },
        1,
        "02.ass upper empty clipped h slice should keep libass transparent ASS_Image plane geometry",
    );
}

#[test]
fn current_02ass_early_active_projective_thin_clip_edges_match_libass_allocation() {
    if !baseline_fontconfig_family_contains("OFL Sorts Mill Goudy TT", "Liberation") {
        return;
    }

    let script = |clip: &str, text: &str, move_x: &str, move_y: &str, org_x: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.93,ED2,,0,0,0,fx,{{\move({move_x},{move_y},{move_x},65)\org({org_x},-25)\t(66.428571428571,132.85714285714,\frz4)\t(132.85714285714,199.28571428571,\frz-4)\t(199.28571428571,265.71428571429,\frz4\t(265.71428571429,332.14285714286,\frz-4\t(332.14285714286,398.57142857143,\frz4\t(398.57142857143,465,\frz-4\t(465,531.42857142857,\frz4\t(1062.8571428571,597.85714285714,\frz-4\t(597.85714285714,664.28571428571,\frz4\t(664.28571428571,730.71428571429,\frz-4\t(730.71428571429,797.14285714286,\frz4\t(797.14285714286,863.57142857143,\frz-4\t(863.57142857143,930,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,930,\fs70\frz0){clip}\c&H5DC1FA&}}{text}
"#
        )
    };

    assert_rect_near(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,24.6,1260.8,37.9)",
                "A",
                "1072.3",
                "57",
                "982.3",
            ),
            475,
        ),
        rect_xywh(1043, 36, 56, 1),
        0,
        "02.ass @ 21:48.405 line 574 upper A thin-clip slice should retain libass transparent ASS_Image allocation",
    );
    assert_eq!(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,92.2,1260.8,106.36666666667)",
                "A",
                "1072.3",
                "57",
                "982.3",
            ),
            475,
        ),
        None,
        "02.ass @ 21:48.405 line 600 lower A thin-clip slice should be dropped like libass",
    );
    assert_eq!(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,94.8,1260.8,109)",
                "A",
                "1072.3",
                "57",
                "982.3",
            ),
            475,
        ),
        None,
        "02.ass @ 21:48.405 line 601 lower A thin-clip slice should be dropped like libass",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,94.8,1260.8,109)",
                "h",
                "1106.8",
                "73",
                "1016.8",
            ),
            475,
        ),
        rect_xywh(1086, 94, 40, 15),
        0,
        "02.ass @ 21:48.405 line 636 lower h thin-clip slice should retain libass ASS_Image allocation",
    );
}

#[test]
fn current_02ass_late_y_thin_clip_slices_match_libass_allocation() {
    let script = |clip: &str, color_tag: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\move(1036.5,57,1036.5,65)\org(946.5,-25)\t(29.285714285714,58.571428571429,\frz4)\t(58.571428571429,87.857142857143,\frz-4)\t(87.857142857143,117.14285714286,\frz4\t(117.14285714286,146.42857142857,\frz-4\t(146.42857142857,175.71428571429,\frz4\t(175.71428571429,205,\frz-4\t(205,234.28571428571,\frz4\t(468.57142857143,263.57142857143,\frz-4\t(263.57142857143,292.85714285714,\frz4\t(292.85714285714,322.14285714286,\frz-4\t(322.14285714286,351.42857142857,\frz4\t(351.42857142857,380.71428571429,\frz-4\t(380.71428571429,410,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,410,\fs70\frz0){clip}{color_tag}}}y
"#
        )
    };

    assert_eq!(
        render_text_plane_bounds_at(
            &script("\\clip(784.3,22,1135.7,35.266666666667)", "\\c&HF4FAFE&"),
            50,
        ),
        None,
        "02.ass @ 23:11.950 line 21350 upper y slice should be dropped like libass when the clip misses the libass allocation",
    );
    assert_eq!(
        render_text_plane_bounds_at(
            &script("\\clip(784.3,27.2,1135.7,40.533333333333)", "\\c&HE9F6FE&"),
            0,
        ),
        None,
        "02.ass @ 23:11.950 line 21352 start-frame upper y slice should be dropped like libass even though rassa's tight transformed bitmap intersects the clip",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &script("\\clip(784.3,29.8,1135.7,43.166666666667)", "\\c&HE4F4FE&"),
            0,
        ),
        rect_xywh(1014, 40, 56, 3),
        0,
        "02.ass @ 23:11.950 line 21353 start-frame upper y slice should use libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &script("\\clip(784.3,29.8,1135.7,43.166666666667)", "\\c&HE4F4FE&"),
            100,
        ),
        rect_xywh(1014, 42, 56, 1),
        0,
        "02.ass @ 23:12.050 line 21353 upper y slice should retain the one-row libass ASS_Image allocation",
    );
    assert_eq!(
        render_text_visible_bounds_at(
            &script("\\clip(784.3,29.8,1135.7,43.166666666667)", "\\c&HE4F4FE&"),
            100,
        ),
        None,
        "02.ass @ 23:12.050 line 21353 upper y slice should remain transparent like libass",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &script("\\clip(784.3,32.4,1135.7,45.8)", "\\c&HDEF2FE&"),
            100,
        ),
        rect_xywh(1014, 42, 56, 3),
        0,
        "02.ass @ 23:12.050 line 21354 upper y slice should retain libass ASS_Image allocation",
    );
    assert_eq!(
        render_text_visible_bounds_at(
            &script("\\clip(784.3,32.4,1135.7,45.8)", "\\c&HDEF2FE&"),
            100,
        ),
        None,
        "02.ass @ 23:12.050 line 21354 upper y slice should remain transparent like libass",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(784.3,94.8,1135.7,109)", "\\c&H5DC1FA&"), 0),
        rect_xywh(1014, 94, 56, 15),
        0,
        "02.ass @ 23:11.950 line 21378 start-frame lower y slice should use libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(784.3,94.8,1135.7,109)", "\\c&H5DC1FA&"), 50),
        Rect {
            x_min: 1018,
            y_min: 94,
            x_max: 1074,
            y_max: 108,
        },
        0,
        "02.ass @ 23:11.950 line 21378 lower y slice should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_visible_bounds_at(
            &script("\\clip(784.3,92.2,1135.7,106.36666666667)", "\\c&H62C3FA&"),
            100,
        ),
        Rect {
            x_min: 1019,
            y_min: 92,
            x_max: 1036,
            y_max: 99,
        },
        0,
        "02.ass @ 23:12.050 line 21377 lower y slice should preserve libass visible ink",
    );
    assert_rect_near(
        render_text_visible_bounds_at(
            &script("\\clip(784.3,94.8,1135.7,109)", "\\c&H5DC1FA&"),
            100,
        ),
        Rect {
            x_min: 1019,
            y_min: 94,
            x_max: 1034,
            y_max: 99,
        },
        0,
        "02.ass @ 23:12.050 line 21378 lower y slice should preserve libass visible ink",
    );
}

#[test]
fn current_02ass_late_y_active_projective_visible_bounds_match_libass_scanline_stack() {
    let script = |clip: &str, color_tag: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\move(1036.5,57,1036.5,65)\org(946.5,-25)\t(29.285714285714,58.571428571429,\frz4)\t(58.571428571429,87.857142857143,\frz-4)\t(87.857142857143,117.14285714286,\frz4\t(117.14285714286,146.42857142857,\frz-4\t(146.42857142857,175.71428571429,\frz4\t(175.71428571429,205,\frz-4\t(205,234.28571428571,\frz4\t(468.57142857143,263.57142857143,\frz-4\t(263.57142857143,292.85714285714,\frz4\t(292.85714285714,322.14285714286,\frz-4\t(322.14285714286,351.42857142857,\frz4\t(351.42857142857,380.71428571429,\frz-4\t(380.71428571429,410,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,410,\fs70\frz0){clip}{color_tag}}}y
"#
        )
    };

    let cases = [
        (
            21355,
            "\\clip(784.3,35,1135.7,48.433333333333)",
            "\\c&HD9F0FD&",
            rect_xywh(1018, 45, 36, 3),
        ),
        (
            21356,
            "\\clip(784.3,37.6,1135.7,51.066666666667)",
            "\\c&HD3EEFD&",
            rect_xywh(1018, 45, 36, 6),
        ),
        (
            21357,
            "\\clip(784.3,40.2,1135.7,53.7)",
            "\\c&HCEECFD&",
            rect_xywh(1018, 45, 36, 8),
        ),
        (
            21358,
            "\\clip(784.3,42.8,1135.7,56.333333333333)",
            "\\c&HC9EAFD&",
            rect_xywh(1018, 45, 36, 11),
        ),
        (
            21359,
            "\\clip(784.3,45.4,1135.7,58.966666666667)",
            "\\c&HC3E8FD&",
            rect_xywh(1018, 45, 36, 13),
        ),
        (
            21360,
            "\\clip(784.3,48,1135.7,61.6)",
            "\\c&HBEE6FD&",
            rect_xywh(1018, 48, 36, 13),
        ),
        (
            21361,
            "\\clip(784.3,50.6,1135.7,64.233333333333)",
            "\\c&HB8E4FC&",
            rect_xywh(1019, 50, 34, 14),
        ),
        (
            21362,
            "\\clip(784.3,53.2,1135.7,66.866666666667)",
            "\\c&HB3E2FC&",
            rect_xywh(1020, 53, 32, 13),
        ),
        (
            21363,
            "\\clip(784.3,55.8,1135.7,69.5)",
            "\\c&HAEE0FC&",
            rect_xywh(1021, 55, 30, 14),
        ),
        (
            21364,
            "\\clip(784.3,58.4,1135.7,72.133333333333)",
            "\\c&HA8DDFC&",
            rect_xywh(1022, 58, 28, 14),
        ),
        (
            21365,
            "\\clip(784.3,61,1135.7,74.766666666667)",
            "\\c&HA3DBFC&",
            rect_xywh(1023, 61, 26, 13),
        ),
        (
            21366,
            "\\clip(784.3,63.6,1135.7,77.4)",
            "\\c&H9DD9FC&",
            rect_xywh(1024, 63, 24, 14),
        ),
        (
            21367,
            "\\clip(784.3,66.2,1135.7,80.033333333333)",
            "\\c&H98D7FB&",
            rect_xywh(1025, 66, 22, 14),
        ),
        (
            21368,
            "\\clip(784.3,68.8,1135.7,82.666666666667)",
            "\\c&H93D5FB&",
            rect_xywh(1026, 68, 20, 14),
        ),
        (
            21369,
            "\\clip(784.3,71.4,1135.7,85.3)",
            "\\c&H8DD3FB&",
            rect_xywh(1027, 71, 18, 14),
        ),
        (
            21370,
            "\\clip(784.3,74,1135.7,87.933333333333)",
            "\\c&H88D1FB&",
            rect_xywh(1028, 74, 16, 13),
        ),
        (
            21371,
            "\\clip(784.3,76.6,1135.7,90.566666666667)",
            "\\c&H82CFFB&",
            rect_xywh(1028, 76, 15, 14),
        ),
        (
            21372,
            "\\clip(784.3,79.2,1135.7,93.2)",
            "\\c&H7DCDFB&",
            rect_xywh(1020, 79, 22, 14),
        ),
        (
            21373,
            "\\clip(784.3,81.8,1135.7,95.833333333333)",
            "\\c&H78CBFA&",
            rect_xywh(1019, 81, 22, 14),
        ),
        (
            21374,
            "\\clip(784.3,84.4,1135.7,98.466666666667)",
            "\\c&H72C9FA&",
            rect_xywh(1019, 84, 21, 14),
        ),
        (
            21375,
            "\\clip(784.3,87,1135.7,101.1)",
            "\\c&H6DC7FA&",
            rect_xywh(1019, 87, 20, 12),
        ),
        (
            21376,
            "\\clip(784.3,89.6,1135.7,103.73333333333)",
            "\\c&H67C5FA&",
            rect_xywh(1019, 89, 18, 10),
        ),
    ];

    for (line, clip, color_tag, expected) in cases {
        let context = format!(
            "02.ass @ 23:12.050 line {line} active-projective y visible bounds should match libass"
        );
        assert_rect_near(
            render_text_visible_bounds_at(&script(clip, color_tag), 100),
            expected,
            0,
            &context,
        );
    }
}

#[test]
fn current_02ass_late_o_active_projective_visible_bounds_match_libass_scanline_stack() {
    let script = |clip: &str, color_tag: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\move(1062.2,73,1062.2,65)\org(972.2,-25)\t(29.285714285714,58.571428571429,\frz4)\t(58.571428571429,87.857142857143,\frz-4)\t(87.857142857143,117.14285714286,\frz4\t(117.14285714286,146.42857142857,\frz-4\t(146.42857142857,175.71428571429,\frz4\t(175.71428571429,205,\frz-4\t(205,234.28571428571,\frz4\t(468.57142857143,263.57142857143,\frz-4\t(263.57142857143,292.85714285714,\frz4\t(292.85714285714,322.14285714286,\frz-4\t(322.14285714286,351.42857142857,\frz4\t(351.42857142857,380.71428571429,\frz-4\t(380.71428571429,410,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,410,\fs70\frz0){clip}{color_tag}}}o
"#
        )
    };

    let cases = [
        (
            21395,
            "\\clip(784.3,48,1135.7,61.6)",
            "\\c&HBEE6FD&",
            rect_xywh(1050, 57, 23, 4),
        ),
        (
            21396,
            "\\clip(784.3,50.6,1135.7,64.233333333333)",
            "\\c&HB8E4FC&",
            rect_xywh(1047, 57, 29, 7),
        ),
        (
            21397,
            "\\clip(784.3,53.2,1135.7,66.866666666667)",
            "\\c&HB3E2FC&",
            rect_xywh(1046, 57, 31, 9),
        ),
        (
            21398,
            "\\clip(784.3,55.8,1135.7,69.5)",
            "\\c&HAEE0FC&",
            rect_xywh(1045, 57, 33, 12),
        ),
        (
            21399,
            "\\clip(784.3,58.4,1135.7,72.133333333333)",
            "\\c&HA8DDFC&",
            rect_xywh(1044, 58, 35, 14),
        ),
        (
            21400,
            "\\clip(784.3,61,1135.7,74.766666666667)",
            "\\c&HA3DBFC&",
            rect_xywh(1044, 61, 35, 13),
        ),
        (
            21401,
            "\\clip(784.3,63.6,1135.7,77.4)",
            "\\c&H9DD9FC&",
            rect_xywh(1044, 63, 35, 14),
        ),
        (
            21402,
            "\\clip(784.3,66.2,1135.7,80.033333333333)",
            "\\c&H98D7FB&",
            rect_xywh(1044, 66, 35, 14),
        ),
        (
            21403,
            "\\clip(784.3,68.8,1135.7,82.666666666667)",
            "\\c&H93D5FB&",
            rect_xywh(1044, 68, 35, 14),
        ),
        (
            21404,
            "\\clip(784.3,71.4,1135.7,85.3)",
            "\\c&H8DD3FB&",
            rect_xywh(1044, 71, 35, 14),
        ),
        (
            21405,
            "\\clip(784.3,74,1135.7,87.933333333333)",
            "\\c&H88D1FB&",
            rect_xywh(1044, 74, 35, 13),
        ),
        (
            21406,
            "\\clip(784.3,76.6,1135.7,90.566666666667)",
            "\\c&H82CFFB&",
            rect_xywh(1044, 76, 35, 14),
        ),
        (
            21407,
            "\\clip(784.3,79.2,1135.7,93.2)",
            "\\c&H7DCDFB&",
            rect_xywh(1044, 79, 35, 14),
        ),
        (
            21408,
            "\\clip(784.3,81.8,1135.7,95.833333333333)",
            "\\c&H78CBFA&",
            rect_xywh(1044, 81, 35, 14),
        ),
        (
            21409,
            "\\clip(784.3,84.4,1135.7,98.466666666667)",
            "\\c&H72C9FA&",
            rect_xywh(1045, 84, 33, 14),
        ),
        (
            21410,
            "\\clip(784.3,87,1135.7,101.1)",
            "\\c&H6DC7FA&",
            rect_xywh(1045, 87, 33, 11),
        ),
        (
            21411,
            "\\clip(784.3,89.6,1135.7,103.73333333333)",
            "\\c&H67C5FA&",
            rect_xywh(1047, 89, 29, 9),
        ),
        (
            21412,
            "\\clip(784.3,92.2,1135.7,106.36666666667)",
            "\\c&H62C3FA&",
            rect_xywh(1049, 92, 25, 6),
        ),
        (
            21413,
            "\\clip(784.3,94.8,1135.7,109)",
            "\\c&H5DC1FA&",
            rect_xywh(1051, 94, 21, 4),
        ),
    ];

    for (line, clip, color_tag, expected) in cases {
        let context = format!(
            "02.ass @ 23:12.050 line {line} active-projective o visible bounds should match libass"
        );
        assert_rect_near(
            render_text_visible_bounds_at(&script(clip, color_tag), 100),
            expected,
            0,
            &context,
        );
    }
}

#[test]
fn current_02ass_late_o_thin_clip_lower_slices_preserve_libass_ink() {
    let script = |clip: &str, color_tag: &str| {
        format!(
            "{}Dialogue: 8,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move(1062.2,73,1062.2,65)\\org(972.2,-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord0\\blur0.2\\shad0\\an5\\fs80\\t(0,410,\\fs70\\frz0){clip}{color_tag}}}o\n",
            current_02ass_ed2_header()
        )
    };

    let lower = script("\\clip(784.3,92.2,1135.7,106.36666666667)", "\\c&H62C3FA&");
    assert_rect_near(
        render_text_plane_bounds_at(&lower, 0),
        rect_xywh(1041, 92, 56, 14),
        0,
        "02.ass @ 23:11.950 line 21412 lower o slice should keep libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_visible_bounds_at(&lower, 0),
        Rect {
            x_min: 1047,
            y_min: 92,
            x_max: 1077,
            y_max: 100,
        },
        0,
        "02.ass @ 23:11.950 line 21412 lower o slice should preserve visible ink inside the libass allocation",
    );

    let bottom = script("\\clip(784.3,94.8,1135.7,109)", "\\c&H5DC1FA&");
    assert_rect_near(
        render_text_plane_bounds_at(&bottom, 0),
        rect_xywh(1041, 94, 56, 15),
        0,
        "02.ass @ 23:11.950 line 21413 bottom o slice should keep libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_visible_bounds_at(&bottom, 0),
        Rect {
            x_min: 1049,
            y_min: 94,
            x_max: 1076,
            y_max: 100,
        },
        0,
        "02.ass @ 23:11.950 line 21413 bottom o slice should preserve visible ink inside the libass allocation",
    );

    assert_rect_near(
        render_text_plane_bounds_at(&lower, 50),
        rect_xywh(1046, 92, 56, 14),
        0,
        "02.ass @ 23:12.000 line 21412 lower o slice should keep libass ASS_Image allocation after the first transform step",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&bottom, 50),
        rect_xywh(1046, 94, 56, 12),
        0,
        "02.ass @ 23:12.000 line 21413 bottom o slice should keep libass ASS_Image allocation after the first transform step",
    );
}

#[test]
fn current_02ass_static_top_fade_glyph_visible_bounds_match_libass_absink() {
    struct Case {
        name: &'static str,
        tags: &'static str,
        text: &'static str,
        shadow: Option<Rect>,
        outline: Option<Rect>,
        character: Rect,
    }

    let cases = [
        Case {
            name: "line 21344 outlined u",
            tags: r"\pos(999.7,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)",
            text: "u",
            shadow: Some(rect_xywh(983, 50, 40, 47)),
            outline: Some(rect_xywh(980, 47, 40, 47)),
            character: rect_xywh(986, 53, 28, 35),
        },
        Case {
            name: "line 21309 outlined o",
            tags: r"\pos(972,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)",
            text: "o",
            shadow: Some(rect_xywh(954, 49, 43, 48)),
            outline: Some(rect_xywh(951, 46, 43, 48)),
            character: rect_xywh(957, 53, 30, 35),
        },
        Case {
            name: "line 21239 outlined o",
            tags: r"\pos(926.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)",
            text: "o",
            shadow: Some(rect_xywh(908, 49, 43, 48)),
            outline: Some(rect_xywh(905, 46, 43, 48)),
            character: rect_xywh(912, 53, 30, 35),
        },
        Case {
            name: "line 21204 outlined d",
            tags: r"\pos(899.6,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)",
            text: "d",
            shadow: Some(rect_xywh(881, 37, 42, 60)),
            outline: Some(rect_xywh(878, 34, 42, 60)),
            character: rect_xywh(884, 41, 30, 47),
        },
        Case {
            name: "line 21274 outlined r",
            tags: r"\pos(949.4,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)",
            text: "r",
            shadow: Some(rect_xywh(940, 49, 29, 47)),
            outline: Some(rect_xywh(937, 46, 29, 47)),
            character: rect_xywh(943, 53, 16, 34),
        },
        Case {
            name: "line 21169 outlined O",
            tags: r"\pos(865.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)",
            text: "O",
            shadow: Some(rect_xywh(840, 39, 56, 58)),
            outline: Some(rect_xywh(837, 36, 56, 58)),
            character: rect_xywh(843, 42, 44, 46),
        },
        Case {
            name: "line 21345 fill-only u",
            tags: r"\pos(999.7,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0",
            text: "u",
            shadow: None,
            outline: None,
            character: rect_xywh(984, 52, 31, 37),
        },
        Case {
            name: "line 21310 fill-only o",
            tags: r"\pos(972,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0",
            text: "o",
            shadow: None,
            outline: None,
            character: rect_xywh(955, 52, 33, 37),
        },
        Case {
            name: "line 21240 fill-only o",
            tags: r"\pos(926.8,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0",
            text: "o",
            shadow: None,
            outline: None,
            character: rect_xywh(910, 52, 33, 37),
        },
        Case {
            name: "line 21170 fill-only O",
            tags: r"\pos(865.1,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0",
            text: "O",
            shadow: None,
            outline: None,
            character: rect_xywh(842, 41, 46, 48),
        },
        Case {
            name: "line 21205 fill-only d",
            tags: r"\pos(899.6,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0",
            text: "d",
            shadow: None,
            outline: None,
            character: rect_xywh(883, 40, 32, 49),
        },
        Case {
            name: "line 21275 fill-only r",
            tags: r"\pos(949.4,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0",
            text: "r",
            shadow: None,
            outline: None,
            character: rect_xywh(941, 52, 19, 37),
        },
    ];

    for case in cases {
        let script = format!(
            "{}Dialogue: 3,0:00:00.00,0:00:00.48,ED2,,0,0,0,fx,{{{}}}{}\n",
            current_02ass_ed2_header(),
            case.tags,
            case.text
        );
        let track =
            parse_script_text(&script).expect("02.ass static top visible probe should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 100);

        if let Some(shadow) = case.shadow {
            assert_rect_near(
                kind_visible_bounds(&planes, ass::ImageType::Shadow),
                shadow,
                0,
                &format!(
                    "02.ass {} shadow visible ink should match libass",
                    case.name
                ),
            );
        }
        if let Some(outline) = case.outline {
            assert_rect_near(
                kind_visible_bounds(&planes, ass::ImageType::Outline),
                outline,
                0,
                &format!(
                    "02.ass {} outline visible ink should match libass",
                    case.name
                ),
            );
        }
        assert_rect_near(
            kind_visible_bounds(&planes, ass::ImageType::Character),
            case.character,
            0,
            &format!(
                "02.ass {} character visible ink should match libass",
                case.name
            ),
        );
    }
}

#[test]
fn current_02ass_late_o_unclipped_mid_frz_visible_bounds_match_libass() {
    let script = format!(
        "{}Dialogue: 7,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move(1062.2,73,1062.2,65)\\org(972.2,-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,410,\\fs70\\frz0)\\1a&H70&}}o\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script).expect("02.ass late full-o visible probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 100);

    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1040, 54, 49, 53),
        0,
        "02.ass @ 23:12.050 line 21383 o shadow visible ink should match libass",
    );
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1037, 51, 49, 53),
        0,
        "02.ass @ 23:12.050 line 21383 o outline visible ink should match libass",
    );
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1045, 58, 33, 39),
        0,
        "02.ass @ 23:12.050 line 21383 o character visible ink should match libass",
    );
}

#[test]
fn current_02ass_late_o_thin_clip_mid_frz_slices_match_libass_allocation() {
    let script = |clip: &str, color_tag: &str| {
        format!(
            "{}Dialogue: 8,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move(1062.2,73,1062.2,65)\\org(972.2,-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord0\\blur0.2\\shad0\\an5\\fs80\\t(0,410,\\fs70\\frz0){clip}{color_tag}}}o\n",
            current_02ass_ed2_header()
        )
    };

    let upper = script("\\clip(784.3,45.4,1135.7,58.966666666667)", "\\c&HC3E8FD&");
    assert_rect_near(
        render_text_plane_bounds_at(&upper, 100),
        rect_xywh(1041, 54, 56, 4),
        0,
        "02.ass @ 23:12.050 line 21394 upper o slice should keep libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_visible_bounds_at(&upper, 100),
        rect_xywh(1057, 57, 9, 1),
        0,
        "02.ass @ 23:12.050 line 21394 upper o slice should preserve the one-row libass visible ink",
    );

    let mid = script("\\clip(784.3,55.8,1135.7,69.5)", "\\c&HAEE0FC&");
    assert_rect_near(
        render_text_plane_bounds_at(&mid, 100),
        rect_xywh(1041, 55, 56, 14),
        0,
        "02.ass @ 23:12.050 line 21398 mid o slice should keep libass ASS_Image allocation",
    );

    let bottom = script("\\clip(784.3,94.8,1135.7,109)", "\\c&H5DC1FA&");
    assert_rect_near(
        render_text_plane_bounds_at(&bottom, 100),
        rect_xywh(1041, 94, 56, 15),
        0,
        "02.ass @ 23:12.050 line 21413 bottom o slice should keep libass ASS_Image allocation",
    );
}

#[test]
fn current_02ass_late_move_org_frz_blurred_glyphs_start_frame_match_libass_allocation() {
    struct Case {
        text: char,
        move_x: f64,
        move_y: f64,
        org_x: f64,
        shadow: Rect,
        outline: Rect,
        character: Rect,
    }

    let cases = [
        Case {
            text: 'y',
            move_x: 1036.5,
            move_y: 57.0,
            org_x: 946.5,
            shadow: rect_xywh(1014, 39, 56, 72),
            outline: rect_xywh(1011, 36, 56, 72),
            character: rect_xywh(1018, 44, 48, 64),
        },
        Case {
            text: 'o',
            move_x: 1062.2,
            move_y: 73.0,
            org_x: 972.2,
            shadow: rect_xywh(1040, 54, 56, 72),
            outline: rect_xywh(1037, 51, 56, 72),
            character: rect_xywh(1045, 59, 48, 48),
        },
    ];

    for case in cases {
        let script = format!(
            "{}Dialogue: 7,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move({:.1},{:.0},{:.1},65)\\org({:.1},-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,410,\\fs70\\frz0)\\1a&H70&}}{}\n",
            current_02ass_ed2_header(),
            case.move_x,
            case.move_y,
            case.move_x,
            case.org_x,
            case.text
        );
        let track = parse_script_text(&script)
            .expect("02.ass start-frame transformed glyph probe should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 0);

        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Shadow),
            case.shadow,
            0,
            &format!(
                "02.ass @ 23:11.950 start-frame transformed {} shadow allocation should match libass",
                case.text
            ),
        );
        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Outline),
            case.outline,
            0,
            &format!(
                "02.ass @ 23:11.950 start-frame transformed {} outline allocation should match libass",
                case.text
            ),
        );
        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Character),
            case.character,
            0,
            &format!(
                "02.ass @ 23:11.950 start-frame transformed {} character allocation should match libass",
                case.text
            ),
        );
    }
}

#[test]
fn current_02ass_late_y_unclipped_mid_frz_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 7,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move(1036.5,57,1036.5,65)\\org(946.5,-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,410,\\fs70\\frz0)\\1a&H70&}}y\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script)
        .expect("02.ass late y mid-frz transformed glyph probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 100);

    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1014, 42, 56, 72),
        0,
        "02.ass @ 23:12.050 line 21348 y shadow allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1011, 39, 56, 72),
        0,
        "02.ass @ 23:12.050 line 21348 y outline allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1018, 46, 48, 64),
        0,
        "02.ass @ 23:12.050 line 21348 y character allocation should match libass",
    );

    // Geometry is already clean for this frame; the remaining scanner residual is
    // ABSINK.  These are the libass visible-ink bounds from fresh event scan
    // 1392050 line 21348, and they intentionally fail before the coverage fix.
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1014, 42, 49, 66),
        0,
        "02.ass @ 23:12.050 line 21348 y shadow visible ink should match libass",
    );
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1011, 39, 49, 66),
        0,
        "02.ass @ 23:12.050 line 21348 y outline visible ink should match libass",
    );
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1018, 46, 36, 52),
        0,
        "02.ass @ 23:12.050 line 21348 y character visible ink should match libass",
    );
}

#[test]
fn current_02ass_late_o_unclipped_mid_frz_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 7,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move(1062.2,73,1062.2,65)\\org(972.2,-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,410,\\fs70\\frz0)\\1a&H70&}}o\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script)
        .expect("02.ass late o mid-frz transformed glyph probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 100);

    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1040, 53, 56, 56),
        0,
        "02.ass @ 23:12.050 line 21383 o shadow allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1037, 50, 56, 56),
        0,
        "02.ass @ 23:12.050 line 21383 o outline allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1045, 58, 48, 48),
        0,
        "02.ass @ 23:12.050 line 21383 o character allocation should match libass",
    );
}

#[test]
fn current_02ass_late_move_org_frz_blurred_glyphs_match_libass_allocation() {
    struct Case {
        text: char,
        move_x: f64,
        move_y: f64,
        org_x: f64,
        shadow: Rect,
        outline: Rect,
        character: Rect,
    }

    let cases = [
        Case {
            text: 'y',
            move_x: 1036.5,
            move_y: 57.0,
            org_x: 946.5,
            shadow: rect_xywh(1017, 35, 56, 72),
            outline: rect_xywh(1014, 32, 56, 72),
            character: rect_xywh(1022, 40, 48, 64),
        },
        Case {
            text: 'o',
            move_x: 1062.2,
            move_y: 73.0,
            org_x: 972.2,
            shadow: rect_xywh(1045, 49, 56, 56),
            outline: rect_xywh(1042, 46, 56, 56),
            character: rect_xywh(1050, 54, 48, 48),
        },
    ];

    for case in cases {
        let Case {
            text,
            move_x,
            move_y,
            org_x,
            shadow,
            outline,
            character,
        } = case;
        let script = format!(
            "{}Dialogue: 7,0:00:00.00,0:00:00.41,ED2,,0,0,0,fx,{{\\move({move_x:.1},{move_y:.0},{move_x:.1},65)\\org({org_x:.1},-25)\\t(29.285714285714,58.571428571429,\\frz4)\\t(58.571428571429,87.857142857143,\\frz-4)\\t(87.857142857143,117.14285714286,\\frz4\\t(117.14285714286,146.42857142857,\\frz-4\\t(146.42857142857,175.71428571429,\\frz4\\t(175.71428571429,205,\\frz-4\\t(205,234.28571428571,\\frz4\\t(468.57142857143,263.57142857143,\\frz-4\\t(263.57142857143,292.85714285714,\\frz4\\t(292.85714285714,322.14285714286,\\frz-4\\t(322.14285714286,351.42857142857,\\frz4\\t(351.42857142857,380.71428571429,\\frz-4\\t(380.71428571429,410,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,410,\\fs70\\frz0)\\1a&H70&}}{text}\n",
            current_02ass_ed2_header()
        );
        let track =
            parse_script_text(&script).expect("02.ass late transformed glyph probe should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 50);

        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Shadow),
            shadow,
            0,
            &format!(
                "02.ass @ 23:11.950 late transformed {text} shadow allocation should match libass"
            ),
        );
        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Outline),
            outline,
            0,
            &format!(
                "02.ass @ 23:11.950 late transformed {text} outline allocation should match libass"
            ),
        );
        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Character),
            character,
            0,
            &format!(
                "02.ass @ 23:11.950 late transformed {text} character allocation should match libass"
            ),
        );
    }
}

#[test]
fn current_02ass_225634_move_org_frz_blurred_latin_glyphs_match_libass_allocation() {
    struct Case {
        text: char,
        move_x: f64,
        move_y: f64,
        org_x: f64,
        shadow: Rect,
        outline: Rect,
        character: Rect,
    }

    let cases = [
        Case {
            text: 'h',
            move_x: 643.3,
            move_y: 57.0,
            org_x: 553.3,
            shadow: rect_xywh(626, 25, 56, 72),
            outline: rect_xywh(623, 22, 56, 72),
            character: rect_xywh(630, 30, 48, 64),
        },
        Case {
            text: 'i',
            move_x: 664.8,
            move_y: 73.0,
            org_x: 574.8,
            shadow: rect_xywh(659, 36, 40, 72),
            outline: rect_xywh(656, 33, 40, 72),
            character: rect_xywh(664, 41, 16, 64),
        },
        Case {
            text: 'n',
            move_x: 686.4,
            move_y: 57.0,
            org_x: 596.4,
            shadow: rect_xywh(669, 38, 56, 56),
            outline: rect_xywh(666, 35, 56, 56),
            character: rect_xywh(674, 42, 32, 48),
        },
    ];

    for case in cases {
        let script = format!(
            "{}Dialogue: 7,0:00:00.00,0:00:00.54,ED2,,0,0,0,fx,{{\\move({:.1},{:.0},{:.1},65)\\org({:.1},-25)\\t(38.571428571429,77.142857142857,\\frz4)\\t(77.142857142857,115.71428571429,\\frz-4)\\t(115.71428571429,154.28571428571,\\frz4\\t(154.28571428571,192.85714285714,\\frz-4\\t(192.85714285714,231.42857142857,\\frz4\\t(231.42857142857,270,\\frz-4\\t(270,308.57142857143,\\frz4\\t(617.14285714286,347.14285714286,\\frz-4\\t(347.14285714286,385.71428571429,\\frz4\\t(385.71428571429,424.28571428571,\\frz-4\\t(424.28571428571,462.85714285714,\\frz4\\t(462.85714285714,501.42857142857,\\frz-4\\t(501.42857142857,540,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,540,\\fs70\\frz0)\\1a&H70&}}{}\n",
            current_02ass_ed2_header(),
            case.move_x,
            case.move_y,
            case.move_x,
            case.org_x,
            case.text,
        );
        let track = parse_script_text(&script)
            .expect("02.ass 22:56.34 transformed glyph probe should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 160);

        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Shadow),
            case.shadow,
            0,
            &format!(
                "02.ass @ 22:56.500 transformed {} shadow allocation should match libass",
                case.text
            ),
        );
        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Outline),
            case.outline,
            0,
            &format!(
                "02.ass @ 22:56.500 transformed {} outline allocation should match libass",
                case.text
            ),
        );
        assert_rect_near(
            kind_bounds(&planes, ass::ImageType::Character),
            case.character,
            0,
            &format!(
                "02.ass @ 22:56.500 transformed {} character allocation should match libass",
                case.text
            ),
        );
    }
}

#[test]
fn current_02ass_active_move_fs_blur_single_t_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 6,0:00:00.00,0:00:02.57,ED2,,0,0,0,fx,{{\\move(1060.6,32,1040.6,65,0,200)\\b0\\bord0\\shad0\\blur2\\fs50\\t(0,400,\\fs70\\blur0.6)\\an5\\fad(200,0)}}t\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script).expect("02.ass active fs/blur t probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 40);

    assert_eq!(
        planes.len(),
        1,
        "02.ass line 18512 single t fixture should emit exactly one character plane"
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1045, 19, 26, 58),
        0,
        "02.ass @ 22:56.500 line 18512 active fs/blur t character allocation should match libass",
    );
}

#[test]
fn current_02ass_active_move_fs_blur_upper_a_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:03.12,ED2,,0,0,0,fx,{{\\move(1171.7,98,1151.7,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}A\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script).expect("02.ass active fs/blur A probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 2665);

    assert_eq!(
        planes.len(),
        3,
        "02.ass line 639 single A fixture should emit shadow, outline, and character planes"
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1126, 39, 72, 72),
        0,
        "02.ass @ 1308405 line 639 active fs/blur A shadow allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1123, 36, 72, 72),
        0,
        "02.ass @ 1308405 line 639 active fs/blur A outline allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1130, 43, 48, 48),
        0,
        "02.ass @ 1308405 line 639 active fs/blur A character allocation should match libass",
    );
}

#[test]
fn current_02ass_active_move_fs_blur_upper_h_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:03.12,ED2,,0,0,0,fx,{{\\move(1206.2,32,1186.2,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}h\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script).expect("02.ass active fs/blur h probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 2665);

    assert_eq!(
        planes.len(),
        3,
        "02.ass line 674 single h fixture should emit shadow, outline, and character planes"
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1168, 36, 56, 72),
        0,
        "02.ass @ 1308405 line 674 active fs/blur h shadow allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1165, 33, 56, 72),
        0,
        "02.ass @ 1308405 line 674 active fs/blur h outline allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1173, 41, 32, 48),
        0,
        "02.ass @ 1308405 line 674 active fs/blur h character allocation should match libass",
    );
}

#[test]
fn current_02ass_h_thin_clip_slices_keep_libass_allocation() {
    let script = |clip: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.54,ED2,,0,0,0,fx,{{\move(643.3,57,643.3,65)\org(553.3,-25)\t(38.571428571429,77.142857142857,\frz4)\t(77.142857142857,115.71428571429,\frz-4)\t(115.71428571429,154.28571428571,\frz4\t(154.28571428571,192.85714285714,\frz-4\t(192.85714285714,231.42857142857,\frz4\t(231.42857142857,270,\frz-4\t(270,308.57142857143,\frz4\t(617.14285714286,347.14285714286,\frz-4\t(347.14285714286,385.71428571429,\frz4\t(385.71428571429,424.28571428571,\frz-4\t(424.28571428571,462.85714285714,\frz4\t(462.85714285714,501.42857142857,\frz-4\t(501.42857142857,540,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,540,\fs70\frz0){clip}\c&HF9FCFE&}}h
"#
        )
    };

    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(539.1,22,1380.9,32.633333333333)"), 160),
        Rect {
            x_min: 626,
            y_min: 26,
            x_max: 682,
            y_max: 32,
        },
        0,
        "02.ass @ 22:56.500 line 17954 upper h slice should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(539.1,22,1380.9,35.266666666667)"), 160),
        Rect {
            x_min: 626,
            y_min: 26,
            x_max: 682,
            y_max: 35,
        },
        0,
        "02.ass @ 22:56.500 line 17955 upper h slice should crop to libass allocation top",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(539.1,24.6,1380.9,37.9)"), 160),
        Rect {
            x_min: 626,
            y_min: 26,
            x_max: 682,
            y_max: 37,
        },
        0,
        "02.ass @ 22:56.500 line 17956 upper h slice should crop to libass allocation top",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(539.1,76.6,1380.9,90.566666666667)"), 160),
        Rect {
            x_min: 626,
            y_min: 76,
            x_max: 682,
            y_max: 90,
        },
        0,
        "02.ass @ 22:56.500 line 17976 lower h slice should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(&script("\\clip(539.1,94.8,1380.9,109)"), 160),
        Rect {
            x_min: 626,
            y_min: 94,
            x_max: 682,
            y_max: 98,
        },
        0,
        "02.ass @ 22:56.500 line 17983 lower h tail should retain libass transparent row",
    );

    let variant = |clip: &str, move_tag: &str, org_tag: &str, color_tag: &str, glyph: &str| {
        script(clip)
            .replace("\\move(643.3,57,643.3,65)", move_tag)
            .replace("\\org(553.3,-25)", org_tag)
            .replace("\\c&HF9FCFE&", color_tag)
            .replace("}h\n", &format!("}}{glyph}\n"))
    };
    assert_eq!(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,24.6,1380.9,37.9)",
                "\\move(664.8,73,664.8,65)",
                "\\org(574.8,-25)",
                "\\c&HEEF8FE&",
                "i",
            ),
            160,
        ),
        None,
        "02.ass @ 22:56.500 line 17991 upper i edge should be dropped like libass when the clip misses ink",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,27.2,1380.9,40.533333333333)",
                "\\move(664.8,73,664.8,65)",
                "\\org(574.8,-25)",
                "\\c&HE9F6FE&",
                "i",
            ),
            160,
        ),
        Rect {
            x_min: 659,
            y_min: 37,
            x_max: 683,
            y_max: 40,
        },
        0,
        "02.ass @ 22:56.500 line 17992 upper i slice should retain libass 24px allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,37.6,1380.9,51.066666666667)",
                "\\move(664.8,73,664.8,65)",
                "\\org(574.8,-25)",
                "\\c&HD3EEFD&",
                "i",
            ),
            160,
        ),
        Rect {
            x_min: 659,
            y_min: 37,
            x_max: 683,
            y_max: 51,
        },
        0,
        "02.ass @ 22:56.500 line 17996 middle i slice should use libass left edge",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,94.8,1380.9,109)",
                "\\move(664.8,73,664.8,65)",
                "\\org(574.8,-25)",
                "\\c&H5DC1FA&",
                "i",
            ),
            160,
        ),
        Rect {
            x_min: 659,
            y_min: 94,
            x_max: 683,
            y_max: 109,
        },
        0,
        "02.ass @ 22:56.500 line 18018 lower i edge should retain libass transparent allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,27.2,1380.9,40.533333333333)",
                "\\move(686.4,57,686.4,65)",
                "\\org(596.4,-25)",
                "\\c&HE9F6FE&",
                "n",
            ),
            160,
        ),
        Rect {
            x_min: 670,
            y_min: 38,
            x_max: 710,
            y_max: 40,
        },
        0,
        "02.ass @ 22:56.500 line 18027 upper n slice should crop to libass allocation top",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,37.6,1380.9,51.066666666667)",
                "\\move(686.4,57,686.4,65)",
                "\\org(596.4,-25)",
                "\\c&HD3EEFD&",
                "n",
            ),
            160,
        ),
        Rect {
            x_min: 670,
            y_min: 38,
            x_max: 710,
            y_max: 51,
        },
        0,
        "02.ass @ 22:56.500 line 18031 upper n slice should crop to libass allocation top",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,53.2,1380.9,66.866666666667)",
                "\\move(686.4,57,686.4,65)",
                "\\org(596.4,-25)",
                "\\c&HB3E2FC&",
                "n",
            ),
            160,
        ),
        Rect {
            x_min: 670,
            y_min: 53,
            x_max: 710,
            y_max: 66,
        },
        0,
        "02.ass @ 22:56.500 line 18037 middle n slice should use libass 40px allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,92.2,1380.9,106.36666666667)",
                "\\move(686.4,57,686.4,65)",
                "\\org(596.4,-25)",
                "\\c&H62C3FA&",
                "n",
            ),
            160,
        ),
        Rect {
            x_min: 670,
            y_min: 92,
            x_max: 710,
            y_max: 94,
        },
        0,
        "02.ass @ 22:56.500 line 18052 lower n edge should clamp to libass allocation bottom",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,27.2,1380.9,40.533333333333)",
                "\\move(613.9,73,613.9,65)",
                "\\org(523.9,-25)",
                "\\c&HE9F6FE&",
                "S",
            ),
            160,
        ),
        Rect {
            x_min: 593,
            y_min: 39,
            x_max: 649,
            y_max: 40,
        },
        0,
        "02.ass @ 22:56.500 line 17922 upper S edge should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,40.2,1380.9,53.7)",
                "\\move(613.9,73,613.9,65)",
                "\\org(523.9,-25)",
                "\\c&HCEECFD&",
                "S",
            ),
            160,
        ),
        Rect {
            x_min: 593,
            y_min: 40,
            x_max: 649,
            y_max: 53,
        },
        0,
        "02.ass @ 22:56.500 line 17927 middle S slice should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,92.2,1380.9,106.36666666667)",
                "\\move(613.9,73,613.9,65)",
                "\\org(523.9,-25)",
                "\\c&H62C3FA&",
                "S",
            ),
            160,
        ),
        Rect {
            x_min: 593,
            y_min: 92,
            x_max: 649,
            y_max: 106,
        },
        0,
        "02.ass @ 22:56.500 line 17947 lower S slice should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,94.8,1380.9,109)",
                "\\move(613.9,73,613.9,65)",
                "\\org(523.9,-25)",
                "\\c&H5DC1FA&",
                "S",
            ),
            160,
        ),
        Rect {
            x_min: 593,
            y_min: 94,
            x_max: 649,
            y_max: 109,
        },
        0,
        "02.ass @ 22:56.500 line 17948 lower S edge should retain libass transparent allocation",
    );
    assert_eq!(
        render_text_plane_bounds_at(
            &variant(
                "\\clip(539.1,94.8,1380.9,109)",
                "\\move(686.4,57,686.4,65)",
                "\\org(596.4,-25)",
                "\\c&H5DC1FA&",
                "n",
            ),
            160,
        ),
        None,
        "02.ass @ 22:56.500 line 18053 below the n allocation should be dropped like libass",
    );

    let z_slice_script = |clip: &str, color_tag: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.16,ED2,,0,0,0,fx,{{\move(1210.1,57,1210.1,65)\org(1120.1,-25)\t(11.428571428571,22.857142857143,\frz4)\t(22.857142857143,34.285714285714,\frz-4)\t(34.285714285714,45.714285714286,\frz4\t(45.714285714286,57.142857142857,\frz-4\t(57.142857142857,68.571428571429,\frz4\t(68.571428571429,80,\frz-4\t(80,91.428571428571,\frz4\t(182.85714285714,102.85714285714,\frz-4\t(102.85714285714,114.28571428571,\frz4\t(114.28571428571,125.71428571429,\frz-4\t(125.71428571429,137.14285714286,\frz4\t(137.14285714286,148.57142857143,\frz-4\t(148.57142857143,160,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,160,\fs70\frz0){clip}{color_tag}}}z
"#
        )
    };
    assert_eq!(
        render_text_plane_bounds_at(
            &z_slice_script("\\clip(539.1,24.6,1380.9,37.9)", "\\c&HEEF8FE&"),
            100,
        ),
        None,
        "02.ass @ 23:00.000 line 18761 upper z slice should be dropped when the libass allocation is outside the clip",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &z_slice_script("\\clip(539.1,66.2,1380.9,80.033333333333)", "\\c&H98D7FB&"),
            100,
        ),
        Rect {
            x_min: 1195,
            y_min: 66,
            x_max: 1235,
            y_max: 80,
        },
        0,
        "02.ass @ 23:00.000 line 18777 lower z slice should retain libass ASS_Image allocation",
    );
    assert_rect_near(
        render_text_visible_bounds_at(
            &z_slice_script("\\clip(539.1,66.2,1380.9,80.033333333333)", "\\c&H98D7FB&"),
            100,
        ),
        Rect {
            x_min: 1198,
            y_min: 66,
            x_max: 1227,
            y_max: 80,
        },
        0,
        "02.ass @ 23:00.000 line 18777 lower z slice should preserve visible ink inside the libass allocation",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &z_slice_script("\\clip(539.1,94.8,1380.9,109)", "\\c&HFAC15D&"),
            100,
        ),
        Rect {
            x_min: 1195,
            y_min: 94,
            x_max: 1235,
            y_max: 99,
        },
        0,
        "02.ass @ 23:00.000 line 18788 transparent z tail should keep libass ASS_Image allocation",
    );

    let o_slice_script = |clip: &str, color_tag: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.16,ED2,,0,0,0,fx,{{\move(1232.9,73,1232.9,65)\org(1142.9,-25)\t(11.428571428571,22.857142857143,\frz4)\t(22.857142857143,34.285714285714,\frz-4)\t(34.285714285714,45.714285714286,\frz4\t(45.714285714286,57.142857142857,\frz-4\t(57.142857142857,68.571428571429,\frz4\t(68.571428571429,80,\frz-4\t(80,91.428571428571,\frz4\t(182.85714285714,102.85714285714,\frz-4\t(102.85714285714,114.28571428571,\frz4\t(114.28571428571,125.71428571429,\frz-4\t(125.71428571429,137.14285714286,\frz4\t(137.14285714286,148.57142857143,\frz-4\t(148.57142857143,160,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,160,\fs70\frz0){clip}{color_tag}}}o
"#
        )
    };
    assert_eq!(
        render_text_plane_bounds_at(
            &o_slice_script("\\clip(539.1,35,1380.9,48.433333333333)", "\\c&Hd9f0fd&"),
            100,
        ),
        None,
        "02.ass @ 23:00.000 line 18800 upper o slice should be dropped when the libass allocation is outside the clip",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &o_slice_script("\\clip(539.1,94.8,1380.9,109)", "\\c&HFAC15D&"),
            100,
        ),
        Rect {
            x_min: 1215,
            y_min: 94,
            x_max: 1271,
            y_max: 104,
        },
        0,
        "02.ass @ 23:00.000 line 18823 transparent o tail should keep libass ASS_Image allocation",
    );
}

fn current_02ass_ed2_header() -> &'static str {
    r#"[Script Info]
ScriptType: v4.00+
WrapStyle: 0
PlayResX: 1920
PlayResY: 1080
ScaledBorderAndShadow: yes
YCbCr Matrix: TV.709

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2-furigana,OFL Sorts Mill Goudy TT,35,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,1.5,1.5,8,30,30,30,1
Style: ED TH2-furigana,K2D ExtraBold,37.5,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0.35,1.5,2,30,30,30,1
Style: Default-furigana,Arial,24,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,1,2,10,10,10,1
Style: Default,Arial,48,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1
Style: ED TH2,K2D ExtraBold,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0.7,3,2,30,30,30,1
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"#
}

fn assert_current_02ass_static_top_center_blurred_glyph(
    text: char,
    x: f64,
    shadow: Rect,
    outline: Rect,
    character: Rect,
) {
    let script = format!(
        "{}Dialogue: 3,0:00:00.00,0:00:04.21,ED2,,0,0,0,fx,{{\\pos({x:.1},65)\\b0\\bord3.5\\blur1.2\\fs70\\an5\\fsp0\\fad(0,400)}}{text}\n",
        current_02ass_ed2_header()
    );
    let track = parse_script_text(&script).expect("02.ass static glyph probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 4050);
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        shadow,
        0,
        &format!(
            "02.ass @ 22:56.500 static top-center blurred {text} shadow allocation should match libass"
        ),
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        outline,
        0,
        &format!(
            "02.ass @ 22:56.500 static top-center blurred {text} outline allocation should match libass"
        ),
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        character,
        0,
        &format!(
            "02.ass @ 22:56.500 static top-center blurred {text} character allocation should match libass"
        ),
    );
}

fn rect_xywh(x: i32, y: i32, width: i32, height: i32) -> Rect {
    Rect {
        x_min: x,
        y_min: y,
        x_max: x + width,
        y_max: y + height,
    }
}

fn current_02ass_spark_drawing_path() -> &'static str {
    "m 41.909 83.818 b 65.378 83.818 83.818 65.378 83.818 41.909 b 83.818 18.44 65.378 0 41.909 0 b 18.44 0 0 18.44 0 41.909 b 0 65.378 18.44 83.818 41.909 83.818 m 41.909 0.838 b 66.216 0.838 82.979 17.602 82.979 41.909 b 82.979 63.701 67.054 77.95 56.996 80.465 b 51.967 82.141 55.32 78.789 41.909 78.789 b 28.498 78.789 31.851 82.141 26.822 80.465 b 16.764 77.95 0.838 65.378 0.838 41.909 b 0.838 18.44 18.44 0.838 41.909 0.838 m 73.76 18.44 b 66.216 9.22 62.863 11.734 71.245 20.116 b 77.112 31.851 78.789 27.66 73.76 18.44 m 10.058 12.573 b 10.058 15.925 15.087 15.925 15.087 12.573 b 15.087 9.22 10.058 9.22 10.058 12.573 m 11.734 13.411 l 12.573 25.145 l 13.411 13.411 l 25.145 12.573 l 13.411 11.734 l 12.573 0 l 11.734 11.734 l 0 12.573 m 41.909 78.789 b 52.805 78.789 51.129 83.818 43.585 83.818 b 35.203 83.818 31.851 78.789 41.909 78.789"
}

struct Current02AssP1DrawingCase {
    name: &'static str,
    duration_cs: &'static str,
    override_prefix: &'static str,
    now_ms: i64,
    shadow: Rect,
    outline: Rect,
    character: Rect,
}

fn assert_current_02ass_p1_drawing_case(case: Current02AssP1DrawingCase) {
    assert_current_02ass_p1_drawing_case_with_suffix(case, "");
}

fn assert_current_02ass_p1_drawing_case_with_suffix(
    case: Current02AssP1DrawingCase,
    drawing_suffix: &str,
) {
    let script = format!(
        "{}Dialogue: 9,0:00:00.00,0:00:{},ED2,,0,0,0,fx,{{{}}}{}{}\n",
        current_02ass_ed2_header(),
        case.duration_cs,
        case.override_prefix,
        current_02ass_spark_drawing_path(),
        drawing_suffix
    );
    let track = parse_script_text(&script).expect("02.ass p1 drawing probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, case.now_ms);

    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        case.shadow,
        0,
        &format!(
            "02.ass @ 22:56.500 {} p1 drawing shadow allocation should match libass",
            case.name
        ),
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        case.outline,
        0,
        &format!(
            "02.ass @ 22:56.500 {} p1 drawing outline allocation should match libass",
            case.name
        ),
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        case.character,
        0,
        &format!(
            "02.ass @ 22:56.500 {} p1 drawing character allocation should match libass",
            case.name
        ),
    );
}

struct Current02AssP1DrawingVisibleCase {
    name: &'static str,
    duration_cs: &'static str,
    override_prefix: &'static str,
    now_ms: i64,
    suffix: &'static str,
    shadow: Rect,
    outline: Rect,
    character: Rect,
}

fn assert_current_02ass_p1_drawing_visible_case(case: Current02AssP1DrawingVisibleCase) {
    let script = format!(
        "{}Dialogue: 9,0:00:00.00,0:00:{},ED2,,0,0,0,fx,{{{}}}{}{}\n",
        current_02ass_ed2_header(),
        case.duration_cs,
        case.override_prefix,
        current_02ass_spark_drawing_path(),
        case.suffix
    );
    let track = parse_script_text(&script).expect("02.ass p1 drawing visible probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, case.now_ms);

    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Shadow),
        case.shadow,
        0,
        &format!(
            "02.ass @ 23:12.050 {} p1 drawing shadow visible ink should match libass",
            case.name
        ),
    );
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Outline),
        case.outline,
        0,
        &format!(
            "02.ass @ 23:12.050 {} p1 drawing outline visible ink should match libass",
            case.name
        ),
    );
    assert_rect_near(
        kind_visible_bounds(&planes, ass::ImageType::Character),
        case.character,
        0,
        &format!(
            "02.ass @ 23:12.050 {} p1 drawing character visible ink should match libass",
            case.name
        ),
    );
}

#[test]
fn current_02ass_line_16239_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 16239",
        duration_cs: "01.13",
        override_prefix: "\\c&H42E6FF&\\move(1550.9,85,1565.9,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
        now_ms: 730,
        shadow: rect_xywh(1541, 29, 40, 40),
        outline: rect_xywh(1540, 28, 40, 40),
        character: rect_xywh(1545, 33, 32, 32),
    });
}

#[test]
fn current_02ass_line_16241_negative_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 16241",
        duration_cs: "01.23",
        override_prefix: "\\c&H42E6FF&\\move(1606.3,85,1571.3,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
        now_ms: 500,
        shadow: rect_xywh(1573, 43, 40, 40),
        outline: rect_xywh(1572, 42, 40, 40),
        character: rect_xywh(1577, 47, 32, 32),
    });
}

#[test]
fn current_02ass_line_16237_late_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 16237",
        duration_cs: "01.38",
        override_prefix: "\\c&H42E6FF&\\move(1476.6,85,1492.6,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
        now_ms: 1210,
        shadow: rect_xywh(1474, 15, 40, 40),
        outline: rect_xywh(1473, 14, 40, 40),
        character: rect_xywh(1478, 19, 32, 32),
    });
}

#[test]
fn current_02ass_line_16238_late_descending_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 16238",
        duration_cs: "01.38",
        override_prefix: "\\c&HAA58FF&\\move(1436.6,85,1359.6,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
        now_ms: 1210,
        shadow: rect_xywh(1352, 32, 40, 40),
        outline: rect_xywh(1351, 31, 40, 40),
        character: rect_xywh(1356, 36, 32, 32),
    });
}

#[test]
fn current_02ass_line_16240_positive_descending_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 16240",
        duration_cs: "01.13",
        override_prefix: "\\c&HAA58FF&\\move(1510.9,85,1454.9,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
        now_ms: 730,
        shadow: rect_xywh(1455, 41, 40, 40),
        outline: rect_xywh(1454, 40, 40, 40),
        character: rect_xywh(1459, 45, 32, 32),
    });
}

#[test]
fn current_02ass_line_16242_negative_descending_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 16242",
        duration_cs: "01.23",
        override_prefix: "\\c&HAA58FF&\\move(1566.3,85,1496.3,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
        now_ms: 500,
        shadow: rect_xywh(1519, 51, 40, 40),
        outline: rect_xywh(1518, 50, 40, 40),
        character: rect_xywh(1523, 55, 32, 32),
    });
}

#[test]
fn current_02ass_line_17888_early_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 17888",
        duration_cs: "01.44",
        override_prefix: "\\c&H42E6FF&\\move(670.1,85,735.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
        now_ms: 160,
        shadow: rect_xywh(659, 61, 40, 40),
        outline: rect_xywh(658, 60, 40, 40),
        character: rect_xywh(663, 65, 32, 32),
    });
}

#[test]
fn current_02ass_line_17889_early_descending_p1_drawing_matches_libass_allocation() {
    assert_current_02ass_p1_drawing_case(Current02AssP1DrawingCase {
        name: "line 17889",
        duration_cs: "01.44",
        override_prefix: "\\c&HAA58FF&\\move(630.1,85,563.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
        now_ms: 160,
        shadow: rect_xywh(605, 63, 40, 40),
        outline: rect_xywh(604, 62, 40, 40),
        character: rect_xywh(609, 67, 32, 32),
    });
}

#[test]
fn current_02ass_line_21135_start_negative_p1_drawing_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 9,0:00:00.00,0:00:01.31,ED2,,0,0,0,fx,{{\\c&HAA58FF&\\move(1030,85,942,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)}}{}yo\n",
        current_02ass_ed2_header(),
        current_02ass_spark_drawing_path()
    );
    let track =
        parse_script_text(&script).expect("02.ass line 21135 p1 drawing probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 0);

    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1013, 68, 40, 40),
        0,
        "02.ass @ 23:11.950 line 21135 p1 drawing shadow allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1012, 67, 40, 40),
        0,
        "02.ass @ 23:11.950 line 21135 p1 drawing outline allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1017, 72, 32, 32),
        0,
        "02.ass @ 23:11.950 line 21135 p1 drawing character allocation should match libass",
    );
}

#[test]
fn current_02ass_line_21135_initial_negative_p1_drawing_matches_libass_allocation() {
    let script = format!(
        "{}Dialogue: 9,0:00:00.00,0:00:01.31,ED2,,0,0,0,fx,{{\\c&HAA58FF&\\move(1030,85,942,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)}}{}yo\n",
        current_02ass_ed2_header(),
        current_02ass_spark_drawing_path()
    );
    let track =
        parse_script_text(&script).expect("02.ass line 21135 p1 drawing probe should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 50);

    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Shadow),
        rect_xywh(1009, 66, 40, 40),
        0,
        "02.ass @ 23:12.000 line 21135 p1 drawing shadow allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Outline),
        rect_xywh(1008, 65, 40, 40),
        0,
        "02.ass @ 23:12.000 line 21135 p1 drawing outline allocation should match libass",
    );
    assert_rect_near(
        kind_bounds(&planes, ass::ImageType::Character),
        rect_xywh(1013, 70, 32, 32),
        0,
        "02.ass @ 23:12.000 line 21135 p1 drawing character allocation should match libass",
    );
}

#[test]
fn current_02ass_late_p1_drawing_wave_matches_libass_allocation() {
    let cases = [
        Current02AssP1DrawingCase {
            name: "line 17904",
            duration_cs: "01.34",
            override_prefix: "\\c&H42E6FF&\\move(1072.1,85,1142.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 970,
            shadow: rect_xywh(1105, 24, 40, 40),
            outline: rect_xywh(1104, 23, 40, 40),
            character: rect_xywh(1109, 28, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 17906",
            duration_cs: "01.13",
            override_prefix: "\\c&H42E6FF&\\move(1130.5,85,967.5,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 530,
            shadow: rect_xywh(1035, 39, 40, 40),
            outline: rect_xywh(1034, 38, 40, 40),
            character: rect_xywh(1039, 43, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 17907",
            duration_cs: "01.13",
            override_prefix: "\\c&HAA58FF&\\move(1090.5,85,983.5,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 530,
            shadow: rect_xywh(1021, 49, 40, 40),
            outline: rect_xywh(1020, 48, 40, 40),
            character: rect_xywh(1025, 52, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 17908",
            duration_cs: "01.10",
            override_prefix: "\\c&H42E6FF&\\move(1189.7,85,1016.7,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 300,
            shadow: rect_xywh(1124, 51, 40, 40),
            outline: rect_xywh(1123, 50, 40, 40),
            character: rect_xywh(1128, 55, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 17909",
            duration_cs: "01.10",
            override_prefix: "\\c&HAA58FF&\\move(1149.7,85,1068.7,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 300,
            shadow: rect_xywh(1109, 57, 40, 40),
            outline: rect_xywh(1108, 56, 40, 40),
            character: rect_xywh(1113, 61, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 17910",
            duration_cs: "01.06",
            override_prefix: "\\c&H42E6FF&\\move(1243.6,85,1134.6,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 100,
            shadow: rect_xywh(1215, 62, 40, 40),
            outline: rect_xywh(1214, 61, 40, 40),
            character: rect_xywh(1219, 66, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 17911",
            duration_cs: "01.06",
            override_prefix: "\\c&HAA58FF&\\move(1203.6,85,1129.6,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 100,
            shadow: rect_xywh(1178, 64, 40, 40),
            outline: rect_xywh(1177, 63, 40, 40),
            character: rect_xywh(1182, 68, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21126 late negative upper p1",
            duration_cs: "01.27",
            override_prefix: "\\c&H42E6FF&\\move(885.1,85,861.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 1090,
            shadow: rect_xywh(847, 16, 40, 40),
            outline: rect_xywh(846, 15, 40, 40),
            character: rect_xywh(851, 20, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21127 late negative lower p1",
            duration_cs: "01.27",
            override_prefix: "\\c&HAA58FF&\\move(845.1,85,801.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 1090,
            shadow: rect_xywh(790, 33, 40, 40),
            outline: rect_xywh(789, 32, 40, 40),
            character: rect_xywh(794, 37, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21128 mid positive upper p1",
            duration_cs: "01.23",
            override_prefix: "\\c&H42E6FF&\\move(933.1,85,847.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 720,
            shadow: rect_xywh(863, 32, 40, 40),
            outline: rect_xywh(862, 31, 40, 40),
            character: rect_xywh(867, 36, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21129 mid positive lower p1",
            duration_cs: "01.23",
            override_prefix: "\\c&HAA58FF&\\move(893.1,85,831.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 720,
            shadow: rect_xywh(837, 44, 40, 40),
            outline: rect_xywh(836, 43, 40, 40),
            character: rect_xywh(841, 48, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21130 late negative upper p1",
            duration_cs: "01.14",
            override_prefix: "\\c&H42E6FF&\\move(982.9,85,829.9,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 390,
            shadow: rect_xywh(912, 47, 40, 40),
            outline: rect_xywh(911, 46, 40, 40),
            character: rect_xywh(916, 51, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21131 late negative lower p1",
            duration_cs: "01.14",
            override_prefix: "\\c&HAA58FF&\\move(942.9,85,892.9,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 390,
            shadow: rect_xywh(907, 54, 40, 40),
            outline: rect_xywh(906, 53, 40, 40),
            character: rect_xywh(911, 58, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21132 start positive upper p1",
            duration_cs: "01.05",
            override_prefix: "\\c&H42E6FF&\\move(1019.7,85,1046.7,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 150,
            shadow: rect_xywh(1005, 59, 40, 40),
            outline: rect_xywh(1004, 58, 40, 40),
            character: rect_xywh(1009, 63, 32, 32),
        },
        Current02AssP1DrawingCase {
            name: "line 21133 start positive lower p1",
            duration_cs: "01.05",
            override_prefix: "\\c&HAA58FF&\\move(979.7,85,934.7,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 150,
            shadow: rect_xywh(955, 62, 40, 40),
            outline: rect_xywh(954, 61, 40, 40),
            character: rect_xywh(959, 66, 32, 32),
        },
    ];

    for case in cases {
        assert_current_02ass_p1_drawing_case(case);
    }
}

#[test]
fn current_02ass_late_p1_drawing_wave_at_1392050_visible_bounds_match_libass() {
    let cases = [
        Current02AssP1DrawingVisibleCase {
            name: "line 21126 @ 1392050 negative upper p1",
            duration_cs: "01.27",
            override_prefix: "\\c&H42E6FF&\\move(885.1,85,861.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 1190,
            suffix: "O",
            shadow: rect_xywh(848, 13, 31, 33),
            outline: rect_xywh(847, 12, 31, 33),
            character: rect_xywh(849, 15, 27, 27),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21127 @ 1392050 negative lower p1",
            duration_cs: "01.27",
            override_prefix: "\\c&HAA58FF&\\move(845.1,85,801.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 1190,
            suffix: "O",
            shadow: rect_xywh(789, 32, 31, 32),
            outline: rect_xywh(788, 31, 31, 32),
            character: rect_xywh(791, 34, 26, 27),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21128 @ 1392050 positive upper p1",
            duration_cs: "01.23",
            override_prefix: "\\c&H42E6FF&\\move(933.1,85,847.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 820,
            suffix: "do",
            shadow: rect_xywh(859, 30, 34, 31),
            outline: rect_xywh(858, 29, 34, 31),
            character: rect_xywh(861, 31, 28, 26),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21129 @ 1392050 positive lower p1",
            duration_cs: "01.23",
            override_prefix: "\\c&HAA58FF&\\move(893.1,85,831.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 820,
            suffix: "do",
            shadow: rect_xywh(835, 43, 34, 32),
            outline: rect_xywh(834, 42, 34, 32),
            character: rect_xywh(837, 45, 28, 26),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21130 @ 1392050 negative upper p1",
            duration_cs: "01.14",
            override_prefix: "\\c&H42E6FF&\\move(982.9,85,829.9,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 490,
            suffix: "ro",
            shadow: rect_xywh(901, 44, 33, 32),
            outline: rect_xywh(900, 43, 33, 32),
            character: rect_xywh(903, 46, 27, 26),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21131 @ 1392050 negative lower p1",
            duration_cs: "01.14",
            override_prefix: "\\c&HAA58FF&\\move(942.9,85,892.9,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 490,
            suffix: "ro",
            shadow: rect_xywh(905, 53, 33, 31),
            outline: rect_xywh(904, 52, 33, 31),
            character: rect_xywh(907, 54, 28, 27),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21132 @ 1392050 positive upper p1",
            duration_cs: "01.05",
            override_prefix: "\\c&H42E6FF&\\move(1019.7,85,1046.7,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 250,
            suffix: "u",
            shadow: rect_xywh(1009, 56, 34, 31),
            outline: rect_xywh(1008, 55, 34, 31),
            character: rect_xywh(1011, 57, 28, 26),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21133 @ 1392050 positive lower p1",
            duration_cs: "01.05",
            override_prefix: "\\c&HAA58FF&\\move(979.7,85,934.7,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
            now_ms: 250,
            suffix: "u",
            shadow: rect_xywh(952, 60, 34, 32),
            outline: rect_xywh(951, 59, 34, 32),
            character: rect_xywh(954, 62, 28, 26),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21134 @ 1392050 negative upper p1",
            duration_cs: "01.31",
            override_prefix: "\\c&H42E6FF&\\move(1070,85,940,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 100,
            suffix: "yo",
            shadow: rect_xywh(1045, 66, 32, 31),
            outline: rect_xywh(1044, 65, 32, 31),
            character: rect_xywh(1046, 67, 27, 26),
        },
        Current02AssP1DrawingVisibleCase {
            name: "line 21135 @ 1392050 negative lower p1",
            duration_cs: "01.31",
            override_prefix: "\\c&HAA58FF&\\move(1030,85,942,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
            now_ms: 100,
            suffix: "yo",
            shadow: rect_xywh(1008, 67, 32, 32),
            outline: rect_xywh(1007, 66, 32, 32),
            character: rect_xywh(1010, 69, 26, 26),
        },
    ];

    for case in cases {
        assert_current_02ass_p1_drawing_visible_case(case);
    }
}

#[test]
fn current_02ass_late_p1_drawing_wave_at_1392000_matches_libass_allocation() {
    let cases = [
        (
            Current02AssP1DrawingCase {
                name: "line 21126 @ 1392000 negative upper p1",
                duration_cs: "01.27",
                override_prefix: "\\c&H42E6FF&\\move(885.1,85,861.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 1140,
                shadow: rect_xywh(847, 14, 40, 40),
                outline: rect_xywh(846, 13, 40, 40),
                character: rect_xywh(850, 17, 32, 32),
            },
            "O",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21127 @ 1392000 negative lower p1",
                duration_cs: "01.27",
                override_prefix: "\\c&HAA58FF&\\move(845.1,85,801.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 1140,
                shadow: rect_xywh(789, 31, 40, 40),
                outline: rect_xywh(788, 30, 40, 40),
                character: rect_xywh(792, 35, 32, 32),
            },
            "O",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21128 @ 1392000 positive upper p1",
                duration_cs: "01.23",
                override_prefix: "\\c&H42E6FF&\\move(933.1,85,847.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 770,
                shadow: rect_xywh(860, 30, 40, 40),
                outline: rect_xywh(859, 29, 40, 40),
                character: rect_xywh(864, 34, 32, 32),
            },
            "do",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21129 @ 1392000 positive lower p1",
                duration_cs: "01.23",
                override_prefix: "\\c&HAA58FF&\\move(893.1,85,831.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 770,
                shadow: rect_xywh(835, 42, 40, 40),
                outline: rect_xywh(834, 41, 40, 40),
                character: rect_xywh(839, 46, 32, 32),
            },
            "do",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21130 @ 1392000 negative upper p1",
                duration_cs: "01.14",
                override_prefix: "\\c&H42E6FF&\\move(982.9,85,829.9,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 440,
                shadow: rect_xywh(905, 44, 40, 40),
                outline: rect_xywh(904, 43, 40, 40),
                character: rect_xywh(909, 48, 32, 32),
            },
            "ro",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21131 @ 1392000 negative lower p1",
                duration_cs: "01.14",
                override_prefix: "\\c&HAA58FF&\\move(942.9,85,892.9,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 440,
                shadow: rect_xywh(905, 52, 40, 40),
                outline: rect_xywh(904, 51, 40, 40),
                character: rect_xywh(909, 56, 32, 32),
            },
            "ro",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21133 @ 1392000 positive lower p1",
                duration_cs: "01.05",
                override_prefix: "\\c&HAA58FF&\\move(979.7,85,934.7,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 200,
                shadow: rect_xywh(952, 60, 40, 40),
                outline: rect_xywh(951, 59, 40, 40),
                character: rect_xywh(956, 64, 32, 32),
            },
            "u",
        ),
    ];

    for (case, suffix) in cases {
        assert_current_02ass_p1_drawing_case_with_suffix(case, suffix);
    }
}

#[test]
fn current_02ass_late_p1_drawing_wave_at_1392050_matches_libass_allocation() {
    let cases = [
        (
            Current02AssP1DrawingCase {
                name: "line 21126 @ 1392050 negative upper p1",
                duration_cs: "01.27",
                override_prefix: "\\c&H42E6FF&\\move(885.1,85,861.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 1190,
                shadow: rect_xywh(846, 11, 40, 40),
                outline: rect_xywh(845, 10, 40, 40),
                character: rect_xywh(849, 15, 32, 32),
            },
            "O",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21127 @ 1392050 negative lower p1",
                duration_cs: "01.27",
                override_prefix: "\\c&HAA58FF&\\move(845.1,85,801.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 1190,
                shadow: rect_xywh(787, 29, 40, 40),
                outline: rect_xywh(786, 28, 40, 40),
                character: rect_xywh(791, 33, 32, 32),
            },
            "O",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21128 @ 1392050 positive upper p1",
                duration_cs: "01.23",
                override_prefix: "\\c&H42E6FF&\\move(933.1,85,847.1,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 820,
                shadow: rect_xywh(856, 27, 40, 40),
                outline: rect_xywh(855, 26, 40, 40),
                character: rect_xywh(860, 31, 32, 32),
            },
            "do",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21129 @ 1392050 positive lower p1",
                duration_cs: "01.23",
                override_prefix: "\\c&HAA58FF&\\move(893.1,85,831.1,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 820,
                shadow: rect_xywh(832, 41, 40, 40),
                outline: rect_xywh(831, 40, 40, 40),
                character: rect_xywh(836, 45, 32, 32),
            },
            "do",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21130 @ 1392050 negative upper p1",
                duration_cs: "01.14",
                override_prefix: "\\c&H42E6FF&\\move(982.9,85,829.9,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 490,
                shadow: rect_xywh(898, 42, 40, 40),
                outline: rect_xywh(897, 41, 40, 40),
                character: rect_xywh(902, 46, 32, 32),
            },
            "ro",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21131 @ 1392050 negative lower p1",
                duration_cs: "01.14",
                override_prefix: "\\c&HAA58FF&\\move(942.9,85,892.9,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 490,
                shadow: rect_xywh(903, 50, 40, 40),
                outline: rect_xywh(902, 49, 40, 40),
                character: rect_xywh(907, 54, 32, 32),
            },
            "ro",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21132 @ 1392050 positive upper p1",
                duration_cs: "01.05",
                override_prefix: "\\c&H42E6FF&\\move(1019.7,85,1046.7,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 250,
                shadow: rect_xywh(1007, 53, 40, 40),
                outline: rect_xywh(1006, 52, 40, 40),
                character: rect_xywh(1011, 57, 32, 32),
            },
            "u",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21133 @ 1392050 positive lower p1",
                duration_cs: "01.05",
                override_prefix: "\\c&HAA58FF&\\move(979.7,85,934.7,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz15)\\fad(0,400)",
                now_ms: 250,
                shadow: rect_xywh(950, 58, 40, 40),
                outline: rect_xywh(949, 57, 40, 40),
                character: rect_xywh(954, 62, 32, 32),
            },
            "u",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21134 @ 1392050 negative upper p1",
                duration_cs: "01.31",
                override_prefix: "\\c&H42E6FF&\\move(1070,85,940,25)\\bord1\\blur0.8\\shad1\\fscy30\\fscx30\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 100,
                shadow: rect_xywh(1042, 63, 40, 40),
                outline: rect_xywh(1041, 62, 40, 40),
                character: rect_xywh(1046, 67, 32, 32),
            },
            "yo",
        ),
        (
            Current02AssP1DrawingCase {
                name: "line 21135 @ 1392050 negative lower p1",
                duration_cs: "01.31",
                override_prefix: "\\c&HAA58FF&\\move(1030,85,942,45)\\fscy30\\fscx30\\bord1\\blur0.8\\shad1\\an5\\p1\\t(\\frz90)\\t(\\frz-15)\\fad(0,400)",
                now_ms: 100,
                shadow: rect_xywh(1006, 65, 40, 40),
                outline: rect_xywh(1005, 64, 40, 40),
                character: rect_xywh(1010, 69, 32, 32),
            },
            "yo",
        ),
    ];

    for (case, suffix) in cases {
        assert_current_02ass_p1_drawing_case_with_suffix(case, suffix);
    }
}

#[test]
fn current_02ass_static_top_center_blurred_y_matches_libass_allocation() {
    assert_current_02ass_static_top_center_blurred_glyph(
        'y',
        944.6,
        Rect {
            x_min: 924,
            y_min: 49,
            x_max: 980,
            y_max: 121,
        },
        Rect {
            x_min: 921,
            y_min: 46,
            x_max: 977,
            y_max: 118,
        },
        Rect {
            x_min: 929,
            y_min: 53,
            x_max: 961,
            y_max: 101,
        },
    );
}

#[test]
fn current_02ass_static_top_center_blurred_k_matches_libass_allocation() {
    assert_current_02ass_static_top_center_blurred_glyph(
        'k',
        700.2,
        Rect {
            x_min: 684,
            y_min: 36,
            x_max: 740,
            y_max: 108,
        },
        Rect {
            x_min: 681,
            y_min: 33,
            x_max: 737,
            y_max: 105,
        },
        Rect {
            x_min: 688,
            y_min: 41,
            x_max: 720,
            y_max: 89,
        },
    );
}

fn assert_current_02ass_static_top_center_fill_only_glyph(text: char, x: f64, character: Rect) {
    let script = format!(
        "{}Dialogue: 4,0:00:00.00,0:00:04.21,ED2,,0,0,0,fx,{{\\pos({x:.1},65)\\bord0\\blur0.6\\shad0\\fs70\\fsp0\\an5\\fad(0,400)\\b0}}{text}\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&script, 4050, ass::ImageType::Character),
        character,
        0,
        &format!(
            "02.ass @ 22:56.500 static top-center fill-only blurred {text} character allocation should match libass"
        ),
    );
}

#[test]
fn current_02ass_static_top_center_fill_only_latin_glyphs_match_libass_allocation() {
    let cases = [
        ('S', 613.9, rect_xywh(591, 38, 56, 56)),
        ('a', 739.2, rect_xywh(720, 49, 56, 56)),
        ('a', 958.5, rect_xywh(939, 49, 56, 56)),
        ('b', 918.9, rect_xywh(901, 37, 40, 56)),
        ('h', 643.3, rect_xywh(626, 37, 40, 56)),
        ('h', 1003.7, rect_xywh(986, 37, 40, 56)),
        ('h', 1097.0, rect_xywh(1080, 37, 40, 56)),
        ('h', 1172.4, rect_xywh(1155, 37, 40, 56)),
        ('O', 865.1, rect_xywh(839, 38, 56, 56)),
        ('d', 899.6, rect_xywh(880, 37, 40, 56)),
        ('i', 664.8, rect_xywh(658, 37, 24, 56)),
        ('i', 795.5, rect_xywh(788, 37, 24, 56)),
        ('i', 1025.3, rect_xywh(1018, 37, 24, 56)),
        ('i', 1193.9, rect_xywh(1187, 37, 24, 56)),
        ('j', 1393.5, rect_xywh(1381, 37, 24, 72)),
        ('m', 643.7, rect_xywh(617, 49, 56, 56)),
        ('n', 827.0, rect_xywh(809, 49, 40, 56)),
        ('o', 836.9, rect_xywh(818, 49, 40, 56)),
        ('o', 926.8, rect_xywh(908, 49, 40, 56)),
        ('s', 979.5, rect_xywh(961, 49, 40, 56)),
        ('s', 1121.4, rect_xywh(1103, 49, 40, 56)),
        ('y', 944.6, rect_xywh(925, 49, 40, 56)),
    ];

    for (text, x, character) in cases {
        assert_current_02ass_static_top_center_fill_only_glyph(text, x, character);
    }
}

#[test]
fn current_02ass_static_top_center_blurred_latin_glyphs_match_libass_allocation() {
    let cases = [
        (
            'S',
            613.9,
            rect_xywh(591, 38, 56, 72),
            rect_xywh(588, 35, 56, 72),
            rect_xywh(595, 42, 48, 48),
        ),
        (
            'a',
            675.4,
            rect_xywh(656, 48, 56, 56),
            rect_xywh(653, 45, 56, 56),
            rect_xywh(660, 53, 48, 48),
        ),
        (
            'a',
            958.5,
            rect_xywh(939, 48, 56, 56),
            rect_xywh(936, 45, 56, 56),
            rect_xywh(943, 53, 48, 48),
        ),
        (
            'b',
            918.9,
            rect_xywh(901, 36, 56, 72),
            rect_xywh(898, 33, 56, 72),
            rect_xywh(905, 41, 32, 48),
        ),
        (
            'd',
            1036.2,
            rect_xywh(1017, 36, 56, 72),
            rect_xywh(1014, 33, 56, 72),
            rect_xywh(1021, 41, 32, 48),
        ),
        (
            'd',
            899.6,
            rect_xywh(880, 36, 56, 72),
            rect_xywh(877, 33, 56, 72),
            rect_xywh(884, 41, 32, 48),
        ),
        (
            'e',
            725.1,
            rect_xywh(705, 48, 56, 56),
            rect_xywh(702, 45, 56, 56),
            rect_xywh(710, 53, 32, 48),
        ),
        (
            'e',
            908.2,
            rect_xywh(889, 48, 56, 56),
            rect_xywh(886, 45, 56, 56),
            rect_xywh(893, 53, 32, 48),
        ),
        (
            'e',
            1061.5,
            rect_xywh(1042, 48, 56, 56),
            rect_xywh(1039, 45, 56, 56),
            rect_xywh(1046, 53, 32, 48),
        ),
        (
            'g',
            984.1,
            rect_xywh(965, 48, 56, 72),
            rect_xywh(962, 45, 56, 72),
            rect_xywh(969, 53, 32, 48),
        ),
        (
            'I',
            727.1,
            rect_xywh(719, 39, 24, 72),
            rect_xywh(716, 36, 24, 72),
            rect_xywh(724, 43, 16, 48),
        ),
        (
            '\'',
            741.5,
            rect_xywh(734, 39, 24, 40),
            rect_xywh(731, 36, 24, 40),
            rect_xywh(738, 43, 16, 16),
        ),
        (
            'i',
            664.8,
            rect_xywh(657, 36, 24, 72),
            rect_xywh(654, 33, 24, 72),
            rect_xywh(662, 41, 16, 48),
        ),
        (
            'i',
            795.5,
            rect_xywh(788, 36, 24, 72),
            rect_xywh(785, 33, 24, 72),
            rect_xywh(792, 41, 16, 48),
        ),
        (
            'h',
            643.3,
            rect_xywh(625, 36, 56, 72),
            rect_xywh(622, 33, 56, 72),
            rect_xywh(630, 41, 32, 48),
        ),
        (
            'l',
            1035.1,
            rect_xywh(1027, 36, 24, 72),
            rect_xywh(1024, 33, 24, 72),
            rect_xywh(1032, 41, 16, 48),
        ),
        (
            'i',
            1407.4,
            rect_xywh(1400, 36, 24, 72),
            rect_xywh(1397, 33, 24, 72),
            rect_xywh(1404, 41, 16, 48),
        ),
        (
            'i',
            1193.9,
            rect_xywh(1186, 36, 24, 72),
            rect_xywh(1183, 33, 24, 72),
            rect_xywh(1191, 41, 16, 48),
        ),
        (
            'j',
            1393.5,
            rect_xywh(1380, 36, 40, 88),
            rect_xywh(1377, 33, 40, 88),
            rect_xywh(1385, 41, 16, 64),
        ),
        (
            'm',
            643.7,
            rect_xywh(617, 48, 72, 56),
            rect_xywh(614, 45, 72, 56),
            rect_xywh(621, 53, 48, 48),
        ),
        (
            'n',
            751.2,
            rect_xywh(733, 48, 56, 56),
            rect_xywh(730, 45, 56, 56),
            rect_xywh(738, 53, 32, 48),
        ),
        (
            'n',
            1334.6,
            rect_xywh(1316, 48, 56, 56),
            rect_xywh(1313, 45, 56, 56),
            rect_xywh(1321, 53, 32, 48),
        ),
        (
            'O',
            865.1,
            rect_xywh(839, 38, 72, 72),
            rect_xywh(836, 35, 72, 72),
            rect_xywh(843, 42, 48, 48),
        ),
        (
            'o',
            970.3,
            rect_xywh(951, 48, 56, 56),
            rect_xywh(948, 45, 56, 56),
            rect_xywh(955, 53, 32, 48),
        ),
        (
            'o',
            926.8,
            rect_xywh(907, 48, 56, 56),
            rect_xywh(904, 45, 56, 56),
            rect_xywh(912, 53, 32, 48),
        ),
        (
            'o',
            1144.5,
            rect_xywh(1125, 48, 56, 56),
            rect_xywh(1122, 45, 56, 56),
            rect_xywh(1129, 53, 32, 48),
        ),
        (
            'r',
            1227.8,
            rect_xywh(1217, 48, 40, 56),
            rect_xywh(1214, 45, 40, 56),
            rect_xywh(1221, 53, 32, 48),
        ),
        (
            'r',
            949.4,
            rect_xywh(938, 48, 40, 56),
            rect_xywh(935, 45, 40, 56),
            rect_xywh(943, 53, 32, 48),
        ),
        (
            's',
            1121.4,
            rect_xywh(1103, 48, 56, 56),
            rect_xywh(1100, 45, 56, 56),
            rect_xywh(1107, 53, 32, 48),
        ),
        (
            't',
            1040.6,
            rect_xywh(1028, 41, 40, 72),
            rect_xywh(1025, 38, 40, 72),
            rect_xywh(1032, 46, 32, 48),
        ),
        (
            'u',
            891.3,
            rect_xywh(873, 49, 56, 56),
            rect_xywh(870, 46, 56, 56),
            rect_xywh(878, 53, 32, 48),
        ),
        (
            'u',
            1097.6,
            rect_xywh(1079, 49, 56, 56),
            rect_xywh(1076, 46, 56, 56),
            rect_xywh(1084, 53, 32, 48),
        ),
        (
            'u',
            999.7,
            rect_xywh(982, 49, 56, 56),
            rect_xywh(979, 46, 56, 56),
            rect_xywh(986, 53, 32, 48),
        ),
        (
            's',
            979.5,
            rect_xywh(961, 48, 56, 56),
            rect_xywh(958, 45, 56, 56),
            rect_xywh(965, 53, 32, 48),
        ),
        (
            'o',
            1125.1,
            rect_xywh(1105, 48, 56, 56),
            rect_xywh(1102, 45, 56, 56),
            rect_xywh(1110, 53, 32, 48),
        ),
        (
            'u',
            855.8,
            rect_xywh(838, 49, 56, 56),
            rect_xywh(835, 46, 56, 56),
            rect_xywh(842, 53, 32, 48),
        ),
        (
            's',
            1148.2,
            rect_xywh(1129, 48, 56, 56),
            rect_xywh(1126, 45, 56, 56),
            rect_xywh(1134, 53, 32, 48),
        ),
        (
            'n',
            827.0,
            rect_xywh(809, 48, 56, 56),
            rect_xywh(806, 45, 56, 56),
            rect_xywh(813, 53, 32, 48),
        ),
    ];

    for (text, x, shadow, outline, character) in cases {
        assert_current_02ass_static_top_center_blurred_glyph(text, x, shadow, outline, character);
    }
}

#[test]
fn current_02ass_blurred_s_transform_keeps_libass_per_kind_allocation() {
    let script = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 7,0:00:00.00,0:00:00.54,ED2,,0,0,0,fx,{\move(613.9,73,613.9,65)\org(523.9,-25)\t(38.571428571429,77.142857142857,\frz4)\t(77.142857142857,115.71428571429,\frz-4)\t(115.71428571429,154.28571428571,\frz4\t(154.28571428571,192.85714285714,\frz-4\t(192.85714285714,231.42857142857,\frz4\t(231.42857142857,270,\frz-4\t(270,308.57142857143,\frz4\t(617.14285714286,347.14285714286,\frz-4\t(347.14285714286,385.71428571429,\frz4\t(385.71428571429,424.28571428571,\frz-4\t(424.28571428571,462.85714285714,\frz4\t(462.85714285714,501.42857142857,\frz-4\t(501.42857142857,540,\frz0)))))))))))\b0\bord3.5\blur1.5\fs80\an5\c&HFFFFFF&\3c&HFFFFFF&\t(0,540,\fs70\frz0)\1a&H70&}S
"#;
    assert_rect_near(
        render_text_kind_bounds_at(script, 160, ass::ImageType::Shadow),
        Rect {
            x_min: 593,
            y_min: 38,
            x_max: 649,
            y_max: 110,
        },
        0,
        "02.ass @ 22:56.500 line 17918 shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(script, 160, ass::ImageType::Outline),
        Rect {
            x_min: 590,
            y_min: 35,
            x_max: 646,
            y_max: 107,
        },
        0,
        "02.ass @ 22:56.500 line 17918 outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(script, 160, ass::ImageType::Character),
        Rect {
            x_min: 597,
            y_min: 43,
            x_max: 645,
            y_max: 107,
        },
        0,
        "02.ass @ 22:56.500 line 17918 character allocation should match libass",
    );
}

#[test]
fn current_02ass_blurred_z_o_transform_keeps_libass_per_kind_allocation() {
    let cases = [
        (
            "z",
            "\\move(1210.1,57,1210.1,65)\\org(1120.1,-25)",
            rect_xywh(1194, 42, 56, 56),
            rect_xywh(1191, 39, 56, 56),
            rect_xywh(1199, 47, 32, 48),
        ),
        (
            "o",
            "\\move(1232.9,73,1232.9,65)\\org(1142.9,-25)",
            rect_xywh(1215, 48, 56, 56),
            rect_xywh(1212, 45, 56, 56),
            rect_xywh(1220, 52, 48, 48),
        ),
    ];

    for (glyph, placement, shadow, outline, character) in cases {
        let script = format!(
            "{}Dialogue: 7,0:00:00.00,0:00:00.16,ED2,,0,0,0,fx,{{{}\\t(11.428571428571,22.857142857143,\\frz4)\\t(22.857142857143,34.285714285714,\\frz-4)\\t(34.285714285714,45.714285714286,\\frz4\\t(45.714285714286,57.142857142857,\\frz-4\\t(57.142857142857,68.571428571429,\\frz4\\t(68.571428571429,80,\\frz-4\\t(80,91.428571428571,\\frz4\\t(182.85714285714,102.85714285714,\\frz-4\\t(102.85714285714,114.28571428571,\\frz4\\t(114.28571428571,125.71428571429,\\frz-4\\t(125.71428571429,137.14285714286,\\frz4\\t(137.14285714286,148.57142857143,\\frz-4\\t(148.57142857143,160,\\frz0)))))))))))\\b0\\bord3.5\\blur1.5\\fs80\\an5\\c&HFFFFFF&\\3c&HFFFFFF&\\t(0,160,\\fs70\\frz0)\\1a&H70&}}{}\n",
            current_02ass_ed2_header(),
            placement,
            glyph,
        );
        assert_rect_near(
            render_text_kind_bounds_at(&script, 100, ass::ImageType::Shadow),
            shadow,
            0,
            &format!(
                "02.ass @ 23:00.000 full transformed {glyph} shadow allocation should match libass"
            ),
        );
        assert_rect_near(
            render_text_kind_bounds_at(&script, 100, ass::ImageType::Outline),
            outline,
            0,
            &format!(
                "02.ass @ 23:00.000 full transformed {glyph} outline allocation should match libass"
            ),
        );
        assert_rect_near(
            render_text_kind_bounds_at(&script, 100, ass::ImageType::Character),
            character,
            0,
            &format!(
                "02.ass @ 23:00.000 full transformed {glyph} character allocation should match libass"
            ),
        );
    }
}

#[test]
fn current_02ass_moving_blurred_a_keeps_libass_per_kind_allocation() {
    let script = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:00.70,ED2,,0,0,0,fx,{{\\move(759.2,32,739.2,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}a\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&script, 320, ass::ImageType::Shadow),
        Rect {
            x_min: 720,
            y_min: 49,
            x_max: 776,
            y_max: 105,
        },
        0,
        "02.ass @ 22:56.500 line 18091 shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&script, 320, ass::ImageType::Outline),
        Rect {
            x_min: 717,
            y_min: 46,
            x_max: 773,
            y_max: 102,
        },
        0,
        "02.ass @ 22:56.500 line 18091 outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&script, 320, ass::ImageType::Character),
        Rect {
            x_min: 725,
            y_min: 53,
            x_max: 757,
            y_max: 101,
        },
        0,
        "02.ass @ 22:56.500 line 18091 character allocation should match libass",
    );
}

#[test]
fn current_02ass_non_projective_center_move_transform_variants_match_libass_allocation() {
    let outlined_late_k = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:00.72,ED2,,0,0,0,fx,{{\\move(734.4,98,714.4,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}k\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_late_k, 340, ass::ImageType::Shadow),
        Rect {
            x_min: 698,
            y_min: 37,
            x_max: 754,
            y_max: 109,
        },
        0,
        "02.ass @ 22:56.500 line 18056 shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_late_k, 340, ass::ImageType::Outline),
        Rect {
            x_min: 695,
            y_min: 34,
            x_max: 751,
            y_max: 106,
        },
        0,
        "02.ass @ 22:56.500 line 18056 outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_late_k, 340, ass::ImageType::Character),
        Rect {
            x_min: 703,
            y_min: 42,
            x_max: 735,
            y_max: 90,
        },
        0,
        "02.ass @ 22:56.500 line 18056 character allocation should match libass",
    );

    let fill_late_a = format!(
        "{}Dialogue: 6,0:00:00.00,0:00:00.70,ED2,,0,0,0,fx,{{\\move(759.2,32,739.2,65,0,200)\\b0\\bord0\\shad0\\blur2\\fs50\\t(0,400,\\fs70\\blur0.6)\\an5\\fad(200,0)}}a\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&fill_late_a, 320, ass::ImageType::Character),
        Rect {
            x_min: 721,
            y_min: 49,
            x_max: 761,
            y_max: 105,
        },
        0,
        "02.ass @ 22:56.500 line 18092 fill-only character allocation should match libass",
    );

    let outlined_mid_e = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:01.90,ED2,,0,0,0,fx,{{\\move(928.2,32,908.2,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}e\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_mid_e, 160, ass::ImageType::Shadow),
        Rect {
            x_min: 895,
            y_min: 44,
            x_max: 951,
            y_max: 100,
        },
        0,
        "02.ass @ 22:56.500 line 18301 shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_mid_e, 160, ass::ImageType::Outline),
        Rect {
            x_min: 892,
            y_min: 41,
            x_max: 948,
            y_max: 97,
        },
        0,
        "02.ass @ 22:56.500 line 18301 outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_mid_e, 160, ass::ImageType::Character),
        Rect {
            x_min: 900,
            y_min: 48,
            x_max: 932,
            y_max: 80,
        },
        0,
        "02.ass @ 22:56.500 line 18301 character allocation should match libass",
    );

    let outlined_early_e = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:02.55,ED2,,0,0,0,fx,{{\\move(1080.5,98,1060.5,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}e\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_early_e, 20, ass::ImageType::Shadow),
        Rect {
            x_min: 1063,
            y_min: 81,
            x_max: 1103,
            y_max: 137,
        },
        0,
        "02.ass @ 22:56.500 line 18546 shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_early_e, 20, ass::ImageType::Outline),
        Rect {
            x_min: 1060,
            y_min: 78,
            x_max: 1100,
            y_max: 134,
        },
        0,
        "02.ass @ 22:56.500 line 18546 outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_early_e, 20, ass::ImageType::Character),
        Rect {
            x_min: 1067,
            y_min: 85,
            x_max: 1099,
            y_max: 117,
        },
        0,
        "02.ass @ 22:56.500 line 18546 character allocation should match libass",
    );

    let fill_early_e = format!(
        "{}Dialogue: 6,0:00:00.00,0:00:02.55,ED2,,0,0,0,fx,{{\\move(1080.5,98,1060.5,65,0,200)\\b0\\bord0\\shad0\\blur2\\fs50\\t(0,400,\\fs70\\blur0.6)\\an5\\fad(200,0)}}e\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&fill_early_e, 20, ass::ImageType::Character),
        Rect {
            x_min: 1062,
            y_min: 80,
            x_max: 1104,
            y_max: 122,
        },
        0,
        "02.ass @ 22:56.500 line 18547 fill-only character allocation should match libass",
    );

    let outlined_post_r = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:03.40,ED2,,0,0,0,fx,{{\\move(1275.5,32,1255.5,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}r\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_r, 3340, ass::ImageType::Shadow),
        Rect {
            x_min: 1244,
            y_min: 48,
            x_max: 1284,
            y_max: 104,
        },
        0,
        "02.ass @ 23:00.000 line 18826 post-transform r shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_r, 3340, ass::ImageType::Outline),
        Rect {
            x_min: 1241,
            y_min: 45,
            x_max: 1281,
            y_max: 101,
        },
        0,
        "02.ass @ 23:00.000 line 18826 post-transform r outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_r, 3340, ass::ImageType::Character),
        Rect {
            x_min: 1249,
            y_min: 53,
            x_max: 1281,
            y_max: 101,
        },
        0,
        "02.ass @ 23:00.000 line 18826 post-transform r character allocation should match libass",
    );

    let outlined_post_a = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:03.38,ED2,,0,0,0,fx,{{\\move(1296.1,98,1276.1,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}a\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_a, 3320, ass::ImageType::Shadow),
        Rect {
            x_min: 1256,
            y_min: 48,
            x_max: 1312,
            y_max: 104,
        },
        0,
        "02.ass @ 23:00.000 line 18861 post-transform a shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_a, 3320, ass::ImageType::Outline),
        Rect {
            x_min: 1253,
            y_min: 45,
            x_max: 1309,
            y_max: 101,
        },
        0,
        "02.ass @ 23:00.000 line 18861 post-transform a outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_a, 3320, ass::ImageType::Character),
        Rect {
            x_min: 1261,
            y_min: 53,
            x_max: 1309,
            y_max: 101,
        },
        0,
        "02.ass @ 23:00.000 line 18861 post-transform a character allocation should match libass",
    );

    let outlined_post_e = format!(
        "{}Dialogue: 5,0:00:00.00,0:00:03.64,ED2,,0,0,0,fx,{{\\move(1329.4,98,1309.4,65,0,200)\\b0\\bord3.5\\blur1.2\\fs50\\t(0,400,\\fs70\\blur1.5)\\an5\\fad(200,0)}}e\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_e, 3280, ass::ImageType::Shadow),
        Rect {
            x_min: 1290,
            y_min: 48,
            x_max: 1346,
            y_max: 104,
        },
        0,
        "02.ass @ 23:00.000 line 18896 post-transform e shadow allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_e, 3280, ass::ImageType::Outline),
        Rect {
            x_min: 1287,
            y_min: 45,
            x_max: 1343,
            y_max: 101,
        },
        0,
        "02.ass @ 23:00.000 line 18896 post-transform e outline allocation should match libass",
    );
    assert_rect_near(
        render_text_kind_bounds_at(&outlined_post_e, 3280, ass::ImageType::Character),
        Rect {
            x_min: 1294,
            y_min: 53,
            x_max: 1326,
            y_max: 101,
        },
        0,
        "02.ass @ 23:00.000 line 18896 post-transform e character allocation should match libass",
    );

    let fill_post_r = format!(
        "{}Dialogue: 6,0:00:00.00,0:00:03.40,ED2,,0,0,0,fx,{{\\move(1275.5,32,1255.5,65,0,200)\\b0\\bord0\\shad0\\blur2\\fs50\\t(0,400,\\fs70\\blur0.6)\\an5\\fad(200,0)}}r\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&fill_post_r, 3340, ass::ImageType::Character),
        Rect {
            x_min: 1245,
            y_min: 49,
            x_max: 1285,
            y_max: 105,
        },
        0,
        "02.ass @ 23:00.000 line 18827 post-transform fill-only r character allocation should match libass",
    );

    let fill_post_a = format!(
        "{}Dialogue: 6,0:00:00.00,0:00:03.38,ED2,,0,0,0,fx,{{\\move(1296.1,98,1276.1,65,0,200)\\b0\\bord0\\shad0\\blur2\\fs50\\t(0,400,\\fs70\\blur0.6)\\an5\\fad(200,0)}}a\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&fill_post_a, 3320, ass::ImageType::Character),
        Rect {
            x_min: 1257,
            y_min: 49,
            x_max: 1313,
            y_max: 105,
        },
        0,
        "02.ass @ 23:00.000 line 18862 post-transform fill-only a character allocation should match libass",
    );

    let fill_post_e = format!(
        "{}Dialogue: 6,0:00:00.00,0:00:03.64,ED2,,0,0,0,fx,{{\\move(1329.4,98,1309.4,65,0,200)\\b0\\bord0\\shad0\\blur2\\fs50\\t(0,400,\\fs70\\blur0.6)\\an5\\fad(200,0)}}e\n",
        current_02ass_ed2_header()
    );
    assert_rect_near(
        render_text_kind_bounds_at(&fill_post_e, 3280, ass::ImageType::Character),
        Rect {
            x_min: 1290,
            y_min: 49,
            x_max: 1330,
            y_max: 105,
        },
        0,
        "02.ass @ 23:00.000 line 18897 post-transform fill-only e character allocation should match libass",
    );
}

#[test]
fn transformed_move_origin_single_char_keeps_libass_like_plane_padding() {
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,Arial,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 7,0:00:00.00,0:00:01.00,ED2,,0,0,0,fx,{\move(808.8,73,808.8,65)\org(718.8,-25)\t(27.857142857143,55.714285714286,\frz4)\t(55.714285714286,83.571428571429,\frz-4)\t(83.571428571429,111.42857142857,\frz4\t(111.42857142857,139.28571428571,\frz-4\t(139.28571428571,167.14285714286,\frz4\t(167.14285714286,195,\frz-4\t(195,222.85714285714,\frz4\t(445.71428571429,250.71428571429,\frz-4\t(250.71428571429,278.57142857143,\frz4\t(278.57142857143,306.42857142857,\frz-4\t(306.42857142857,334.28571428571,\frz4\t(334.28571428571,362.14285714286,\frz-4\t(362.14285714286,390,\frz0)))))))))))\b0\bord3.5\blur1.5\fs80\an5\c&HFFFFFF&\3c&HFFFFFF&\t(0,390,\fs70\frz0)\1a&H70&}s
"#;
    assert_rect_near(
        render_text_kind_bounds_at(script, 195, ass::ImageType::Shadow),
        Rect {
            x_min: 785,
            y_min: 57,
            x_max: 841,
            y_max: 113,
        },
        4,
        "shadow ASS_Image plane should stay near libass for the 02.ass move/origin transform fixture",
    );
    assert_rect_near(
        render_text_kind_bounds_at(script, 195, ass::ImageType::Outline),
        Rect {
            x_min: 782,
            y_min: 54,
            x_max: 838,
            y_max: 110,
        },
        4,
        "outline ASS_Image plane should stay near libass for the 02.ass move/origin transform fixture",
    );
    assert_rect_near(
        render_text_kind_bounds_at(script, 195, ass::ImageType::Character),
        Rect {
            x_min: 789,
            y_min: 61,
            x_max: 821,
            y_max: 109,
        },
        4,
        "character ASS_Image plane should stay near libass for the 02.ass move/origin transform fixture",
    );
}

#[test]
fn positioned_drawing_fry_uses_libass_like_projective_camera() {
    let script = |override_tags: &str| {
        format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{{override_tags}\\p1}}m 0 0 l 710 0 710 18 0 18\n"
        )
    };
    let plain = parse_script_text(&script("\\an2\\pos(953,563)"))
        .expect("plain positioned drawing should parse");
    let projected = parse_script_text(&script("\\an2\\pos(953,563)\\frx14\\fry4"))
        .expect("projected positioned drawing should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let plain_bounds = character_bounds(&engine.render_frame_with_provider(&plain, &provider, 500))
        .expect("plain drawing should render");
    let projected_bounds =
        character_bounds(&engine.render_frame_with_provider(&projected, &provider, 500))
            .expect("projected drawing should render");

    assert!(
        projected_bounds.x_min <= plain_bounds.x_min - 24,
        "libass \\fry perspective shifts bottom-centered drawings left: plain={plain_bounds:?} projected={projected_bounds:?}"
    );
    assert!(
        (projected_bounds.x_min - 568).abs() <= 4,
        "projective camera should match libass-probed left edge for this fixture: projected={projected_bounds:?}"
    );
    assert!(
        (projected_bounds.y_min - 544).abs() <= 2,
        "projective transform should preserve libass-probed vertical placement: projected={projected_bounds:?}"
    );
}

#[test]
fn borderstyle3_opaque_box_follows_text_transform() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,Arial,42,&H00000000,&H000000FF,&H00FFFFFF,&H00000000,0,0,0,0,100,100,0,0,3,4,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Box,,0,0,0,,{\\pos(320,180)\\frz-18\\fax0.25}TRANSFORM BOX\n";
    let track = parse_script_text(script).expect("borderstyle transform script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let box_bounds = kind_bounds(&planes, ass::ImageType::Outline)
        .expect("BorderStyle=3 should emit an opaque box outline plane");

    assert!(
        box_bounds.height() > 90,
        "opaque box must be transformed with the rotated/sheared text, got bounds {box_bounds:?}"
    );
}

#[test]
fn positioned_drawing_an_anchors_match_libass_for_all_alignments() {
    // Expected boxes were probed from libass/ffmpeg for a 40x20 vector drawing at \pos(x,y):
    // bottom align => y - 20, middle align => y - 10, top align => y.
    let cases = [
        (
            1,
            "\\an1\\pos(60,60)",
            Rect {
                x_min: 60,
                y_min: 40,
                x_max: 100,
                y_max: 60,
            },
        ),
        (
            2,
            "\\an2\\pos(160,60)",
            Rect {
                x_min: 140,
                y_min: 40,
                x_max: 180,
                y_max: 60,
            },
        ),
        (
            3,
            "\\an3\\pos(260,60)",
            Rect {
                x_min: 220,
                y_min: 40,
                x_max: 260,
                y_max: 60,
            },
        ),
        (
            4,
            "\\an4\\pos(60,100)",
            Rect {
                x_min: 60,
                y_min: 90,
                x_max: 100,
                y_max: 110,
            },
        ),
        (
            5,
            "\\an5\\pos(160,100)",
            Rect {
                x_min: 140,
                y_min: 90,
                x_max: 180,
                y_max: 110,
            },
        ),
        (
            6,
            "\\an6\\pos(260,100)",
            Rect {
                x_min: 220,
                y_min: 90,
                x_max: 260,
                y_max: 110,
            },
        ),
        (
            7,
            "\\an7\\pos(60,140)",
            Rect {
                x_min: 60,
                y_min: 140,
                x_max: 100,
                y_max: 160,
            },
        ),
        (
            8,
            "\\an8\\pos(160,140)",
            Rect {
                x_min: 140,
                y_min: 140,
                x_max: 180,
                y_max: 160,
            },
        ),
        (
            9,
            "\\an9\\pos(260,140)",
            Rect {
                x_min: 220,
                y_min: 140,
                x_max: 260,
                y_max: 160,
            },
        ),
    ];

    for (alignment, override_tags, expected) in cases {
        let script = drawing_alignment_script(alignment, override_tags, "0,0,0");
        assert_eq!(
            render_drawing_bounds(&script),
            expected,
            "\\an{alignment} positioned drawing anchor should match libass"
        );
    }
}

#[test]
fn moved_drawing_an_anchors_match_libass_for_all_alignments_at_midpoint() {
    let cases = [
        (
            1,
            "\\an1\\move(40,60,80,60)",
            Rect {
                x_min: 60,
                y_min: 40,
                x_max: 100,
                y_max: 60,
            },
        ),
        (
            2,
            "\\an2\\move(140,60,180,60)",
            Rect {
                x_min: 140,
                y_min: 40,
                x_max: 180,
                y_max: 60,
            },
        ),
        (
            3,
            "\\an3\\move(240,60,280,60)",
            Rect {
                x_min: 220,
                y_min: 40,
                x_max: 260,
                y_max: 60,
            },
        ),
        (
            4,
            "\\an4\\move(40,100,80,100)",
            Rect {
                x_min: 60,
                y_min: 90,
                x_max: 100,
                y_max: 110,
            },
        ),
        (
            5,
            "\\an5\\move(140,100,180,100)",
            Rect {
                x_min: 140,
                y_min: 90,
                x_max: 180,
                y_max: 110,
            },
        ),
        (
            6,
            "\\an6\\move(240,100,280,100)",
            Rect {
                x_min: 220,
                y_min: 90,
                x_max: 260,
                y_max: 110,
            },
        ),
        (
            7,
            "\\an7\\move(40,140,80,140)",
            Rect {
                x_min: 60,
                y_min: 140,
                x_max: 100,
                y_max: 160,
            },
        ),
        (
            8,
            "\\an8\\move(140,140,180,140)",
            Rect {
                x_min: 140,
                y_min: 140,
                x_max: 180,
                y_max: 160,
            },
        ),
        (
            9,
            "\\an9\\move(240,140,280,140)",
            Rect {
                x_min: 220,
                y_min: 140,
                x_max: 260,
                y_max: 160,
            },
        ),
    ];

    for (alignment, override_tags, expected) in cases {
        let script = drawing_alignment_script(alignment, override_tags, "0,0,0");
        assert_eq!(
            render_drawing_bounds(&script),
            expected,
            "\\an{alignment} moved drawing anchor should match libass at the event midpoint"
        );
    }
}

#[test]
fn margin_positioned_text_uses_style_and_event_margins_like_libass() {
    let cases = [
        (
            1,
            "0,0,0",
            Rect {
                x_min: 32,
                y_min: 138,
                x_max: 116,
                y_max: 165,
            },
        ),
        (
            2,
            "0,0,0",
            Rect {
                x_min: 108,
                y_min: 138,
                x_max: 192,
                y_max: 165,
            },
        ),
        (
            3,
            "0,0,0",
            Rect {
                x_min: 184,
                y_min: 138,
                x_max: 269,
                y_max: 165,
            },
        ),
        (
            5,
            "0,0,0",
            Rect {
                x_min: 108,
                y_min: 79,
                x_max: 192,
                y_max: 106,
            },
        ),
        (
            7,
            "0,0,0",
            Rect {
                x_min: 32,
                y_min: 20,
                x_max: 116,
                y_max: 47,
            },
        ),
        (
            8,
            "0,0,0",
            Rect {
                x_min: 108,
                y_min: 20,
                x_max: 192,
                y_max: 47,
            },
        ),
        (
            9,
            "7,9,11",
            Rect {
                x_min: 225,
                y_min: 16,
                x_max: 310,
                y_max: 43,
            },
        ),
    ];

    for (alignment, event_margins, expected) in cases {
        let script = text_alignment_script(alignment, event_margins);
        let Some(actual) = render_text_bounds(&script) else {
            return;
        };
        // Text rasterization can have a few pixels of coverage-width drift from libass even
        // with the same Fontconfig face. This regression guards the placement bug: the
        // effective style/event margin anchor must no longer be shifted left or sunk.
        assert!(
            (actual.x_min - expected.x_min).abs() <= 1,
            "text style/event margins and \\an{alignment} x placement should match libass within raster rounding: actual={actual:?} expected={expected:?}"
        );
        assert_eq!(
            actual.y_min, expected.y_min,
            "text style/event margins and \\an{alignment} vertical anchor should match libass"
        );
        assert!(
            (actual.y_max - expected.y_max).abs() <= 1,
            "text style/event margins and \\an{alignment} visible height may drift by one raster row: actual={actual:?} expected={expected:?}"
        );
    }
}

#[test]
fn margin_positioned_drawing_uses_style_and_event_margins_like_libass() {
    // Expected boxes were probed from libass/ffmpeg for a 40x20 vector drawing with
    // style margins L=30/R=50/V=15. Event margins of 0 should fall back to style margins.
    let cases = [
        (
            1,
            Rect {
                x_min: 30,
                y_min: 145,
                x_max: 70,
                y_max: 165,
            },
        ),
        (
            2,
            Rect {
                x_min: 130,
                y_min: 145,
                x_max: 170,
                y_max: 165,
            },
        ),
        (
            3,
            Rect {
                x_min: 230,
                y_min: 145,
                x_max: 270,
                y_max: 165,
            },
        ),
        (
            4,
            Rect {
                x_min: 30,
                y_min: 80,
                x_max: 70,
                y_max: 100,
            },
        ),
        (
            5,
            Rect {
                x_min: 130,
                y_min: 80,
                x_max: 170,
                y_max: 100,
            },
        ),
        (
            6,
            Rect {
                x_min: 230,
                y_min: 80,
                x_max: 270,
                y_max: 100,
            },
        ),
        (
            7,
            Rect {
                x_min: 30,
                y_min: 15,
                x_max: 70,
                y_max: 35,
            },
        ),
        (
            8,
            Rect {
                x_min: 130,
                y_min: 15,
                x_max: 170,
                y_max: 35,
            },
        ),
        (
            9,
            Rect {
                x_min: 230,
                y_min: 15,
                x_max: 270,
                y_max: 35,
            },
        ),
    ];

    for (alignment, expected) in cases {
        let script = drawing_alignment_script(alignment, "", "0,0,0");
        assert_eq!(
            render_drawing_bounds(&script),
            expected,
            "style margins and \\an{alignment} should match libass when no explicit position exists"
        );
    }

    let script = drawing_alignment_script(7, "", "7,9,11");
    assert_eq!(
        render_drawing_bounds(&script),
        Rect {
            x_min: 7,
            y_min: 11,
            x_max: 47,
            y_max: 31
        },
        "non-zero event margins should override style margins for top-left alignment"
    );
}

#[test]
fn projective_transform_keeps_frx_and_fry_axes_distinct() {
    let origin = (320.0, 180.0);
    let frx = ProjectiveMatrix::from_ass_transform_at_origin(
        EventTransform {
            rotation_x: 45.0,
            ..EventTransform::default()
        },
        origin.0,
        origin.1,
        1.0,
    );
    let fry = ProjectiveMatrix::from_ass_transform_at_origin(
        EventTransform {
            rotation_y: 45.0,
            ..EventTransform::default()
        },
        origin.0,
        origin.1,
        1.0,
    );

    let (frx_x, frx_y) = frx.transform_point(320.0, 140.0);
    let (fry_x, fry_y) = fry.transform_point(360.0, 180.0);

    assert!(
        (frx_x - 320.0).abs() < 0.5,
        "frx must not act like fry: {frx_x}"
    );
    assert!(
        frx_y > 140.0,
        "positive frx should pitch the top edge downward: {frx_y}"
    );
    assert!(
        fry_x < 360.0,
        "positive fry should yaw the right edge leftward: {fry_x}"
    );
    assert!(
        (fry_y - 180.0).abs() < 0.5,
        "fry must not act like frx: {fry_y}"
    );
}

#[test]
fn projective_transform_uses_deep_org_as_perspective_lever_arm() {
    let transform = EventTransform {
        rotation_x: 55.0,
        ..EventTransform::default()
    };
    let shallow = ProjectiveMatrix::from_ass_transform_at_origin(transform, 320.0, 240.0, 1.0);
    let deep = ProjectiveMatrix::from_ass_transform_at_origin(transform, 320.0, 420.0, 1.0);

    let (_, shallow_y) = shallow.transform_point(320.0, 240.0);
    let (_, deep_y) = deep.transform_point(320.0, 240.0);

    assert!((shallow_y - 240.0).abs() < 0.5);
    assert!(
        deep_y > shallow_y + 70.0,
        "deep \\org below text should pull frx text substantially downward like libass, got shallow={shallow_y} deep={deep_y}"
    );
}

#[test]
fn prepare_frame_only_keeps_active_events() {
    let track = parse_script_text("[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,First\nDialogue: 0,0:00:02.00,0:00:03.00,Default,,0000,0000,0000,,Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let frame = engine.prepare_frame(&track, &provider, 500);

    assert_eq!(frame.active_events.len(), 1);
    assert_eq!(frame.active_events[0].text, "First");
}

#[test]
fn render_frame_produces_image_planes_for_active_text() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
    assert!(planes.iter().all(|plane| plane.size.width >= 0));
    assert!(planes.iter().all(|plane| plane.size.height >= 0));
}

#[test]
fn render_frame_supports_multiple_override_runs() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fnDejaVu Sans}Hi{\\fnArial} there").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
}

#[test]
fn render_frame_uses_axis_specific_shadow_offsets() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(30,30)\\xshad9\\yshad3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let character_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .cloned()
        .collect::<Vec<_>>();
    let shadow_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Shadow)
        .cloned()
        .collect::<Vec<_>>();

    let character = visible_bounds(&character_planes).expect("character bounds");
    let shadow = visible_bounds(&shadow_planes).expect("axis-specific shadow should render");
    assert_eq!(shadow.x_min - character.x_min, 9);
    assert_eq!(shadow.y_min - character.y_min, 3);
}

#[test]
fn render_frame_renders_underline_and_strikeout_decorations() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(30,30)\\u1\\s1}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let decoration_planes = planes
        .iter()
        .filter(|plane| {
            plane.kind == ass::ImageType::Character
                && plane.size.height <= 3
                && plane.size.width > plane.size.height * 4
        })
        .collect::<Vec<_>>();

    assert!(decoration_planes.len() >= 2);
}

#[test]
fn render_frame_uses_override_colors_and_shadow_planes() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\1c&H112233&\\4c&H445566&\\shad3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x6655_4400)
    );
}

#[test]
fn render_frame_orders_events_by_layer_then_read_order() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 5,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\1c&H0000FF&}High\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,40)\\1c&H00FF00&}Low").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let first_character = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("character plane");
    assert_eq!(first_character.color.0, 0x00FF_0000);
}

#[test]
fn render_frame_orders_shadow_outline_before_character_within_event() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00111111,&H0000FFFF,&H00222222,&H00333333,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let kinds = planes.iter().map(|plane| plane.kind).collect::<Vec<_>>();

    let first_shadow = kinds
        .iter()
        .position(|kind| *kind == ass::ImageType::Shadow)
        .expect("shadow plane");
    let first_outline = kinds
        .iter()
        .position(|kind| *kind == ass::ImageType::Outline)
        .expect("outline plane");
    let first_character = kinds
        .iter()
        .position(|kind| *kind == ass::ImageType::Character)
        .expect("character plane");

    assert!(first_shadow < first_outline);
    assert!(first_outline < first_character);
}

#[test]
fn render_frame_emits_outline_planes_for_border_override() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord3\\3c&H0A0B0C&}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0C0B_0A00)
    );
}

#[test]
fn render_frame_emits_opaque_box_for_border_style_3() {
    let track = parse_script_text("[Script Info]\nPlayResX: 500\nPlayResY: 160\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,DejaVu Sans,30,&H00000000,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,3,2,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Box,,0000,0000,0000,,{\\an5\\pos(250,80)}BorderStyle=3 opaque box").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let character_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .cloned()
        .collect::<Vec<_>>();
    let outline_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(
        outline_planes.len(),
        1,
        "BorderStyle=3 should emit only the opaque box outline plane, not a separate stroked glyph outline"
    );
    let _character = visible_bounds(&character_planes).expect("character bounds");
    let outline = outline_planes
        .iter()
        .find(|plane| plane.color.0 == 0x0000_0000 && plane.bitmap.contains(&255))
        .expect("opaque border-style box plane uses outline colour");
    assert!(outline.size.width > 0);
    assert!(outline.size.height > 0);
    let bounds = visible_bounds(std::slice::from_ref(outline)).expect("opaque box bounds");
    let center_x = (bounds.x_min + bounds.x_max) / 2;
    assert!(
        (center_x - 250).abs() <= 2,
        "opaque box should stay centered at \\pos, got {bounds:?}"
    );
    let center_y = (bounds.y_min + bounds.y_max) / 2;
    assert!(
        (center_y - 80).abs() <= 1,
        "opaque box should stay vertically centered at \\pos like libass, got {bounds:?}"
    );
    assert_eq!(
        bounds.height(),
        36,
        "BorderStyle=3 box plane height should be font size plus two borders plus edge rows like libass"
    );
    assert!(
        bounds.width() < 370,
        "opaque box should use actual raster advance like libass, not inflated layout width: {bounds:?}"
    );
}

#[test]
fn render_frame_blurs_outline_and_shadow_layers() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord2\\blur2\\3c&H0A0B0C&\\shad2}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline
                && plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Shadow
                && plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
}

#[test]
fn render_frame_blurs_fill_only_without_outline_or_shadow() {
    let base = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)}Hi").expect("script should parse");
    let blurred = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\blur3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let base_planes = engine.render_frame_with_provider(&base, &provider, 500);
    let blurred_planes = engine.render_frame_with_provider(&blurred, &provider, 500);
    let base_character = visible_bounds(
        &base_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .cloned()
            .collect::<Vec<_>>(),
    )
    .expect("base character bounds");
    let blurred_character = visible_bounds(
        &blurred_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .cloned()
            .collect::<Vec<_>>(),
    )
    .expect("blurred character bounds");

    assert!(blurred_character.x_min < base_character.x_min);
    assert!(blurred_character.x_max > base_character.x_max);
    assert!(blurred_character.y_min < base_character.y_min);
    assert!(blurred_character.y_max > base_character.y_max);
}

#[test]
fn render_frame_does_not_blur_fill_when_outline_or_shadow_exists() {
    let base = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)}Hi").expect("script should parse");
    let blurred = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\blur3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let base_planes = engine.render_frame_with_provider(&base, &provider, 500);
    let blurred_planes = engine.render_frame_with_provider(&blurred, &provider, 500);
    let character_bounds = |planes: &[ImagePlane]| {
        visible_bounds(
            &planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character)
                .cloned()
                .collect::<Vec<_>>(),
        )
        .expect("character bounds")
    };

    assert_eq!(
        character_bounds(&blurred_planes),
        character_bounds(&base_planes)
    );
    assert!(
        blurred_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Outline)
            .any(|plane| plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
    assert!(
        blurred_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Shadow)
            .any(|plane| plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
}

#[test]
fn render_frame_applies_rectangular_clip() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(0,0,64,64)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
    assert!(planes.iter().all(|plane| plane.destination.x >= 0));
    assert!(planes.iter().all(|plane| plane.destination.y >= 0));
    assert!(
        planes
            .iter()
            .all(|plane| plane.destination.x + plane.size.width <= 64)
    );
    assert!(
        planes
            .iter()
            .all(|plane| plane.destination.y + plane.size.height <= 64)
    );
}

#[test]
fn render_frame_accepts_renderer_shaping_mode() {
    let track = parse_script_text("[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,48,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,office").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let simple = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            shaping: ass::ShapingLevel::Simple,
            ..default_renderer_config(&track)
        },
    );
    let complex = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            shaping: ass::ShapingLevel::Complex,
            ..default_renderer_config(&track)
        },
    );

    assert!(!simple.is_empty());
    assert!(!complex.is_empty());
}

#[test]
fn render_frame_applies_inverse_rectangular_clip() {
    let plane = ImagePlane {
        size: Size {
            width: 6,
            height: 4,
        },
        stride: 6,
        color: RgbaColor(0x00FF_FFFF),
        destination: Point { x: 0, y: 0 },
        kind: ass::ImageType::Character,
        bitmap: vec![255; 24],
    };
    let parts = inverse_clip_plane(
        plane,
        Rect {
            x_min: 2,
            y_min: 1,
            x_max: 4,
            y_max: 3,
        },
    );

    assert_eq!(parts.len(), 4);
    assert_eq!(
        parts.iter().map(|plane| plane.bitmap.len()).sum::<usize>(),
        20
    );
}

#[test]
fn inverse_clip_bleed_covers_outline_growth_to_prevent_stray_glyph_leakage() {
    let style = ParsedSpanStyle {
        border: 5.0,
        border_x: 5.0,
        border_y: 5.0,
        shadow: 0.0,
        shadow_x: 0.0,
        shadow_y: 0.0,
        blur: 0.0,
        be: 0.0,
        ..ParsedSpanStyle::default()
    };
    let clip = Rect {
        x_min: 20,
        y_min: 0,
        x_max: 24,
        y_max: 10,
    };
    let glyph = ImagePlane {
        size: Size {
            width: 44,
            height: 10,
        },
        stride: 44,
        color: RgbaColor(0x00FF_FFFF),
        destination: Point { x: 0, y: 0 },
        kind: ass::ImageType::Outline,
        bitmap: vec![255; 440],
    };

    let expanded = expand_rect(clip, style_clip_bleed(&style));
    let parts = inverse_clip_plane(glyph, expanded);

    assert!(
        parts
            .iter()
            .all(|plane| plane.destination.x + plane.size.width <= 0 || plane.destination.x >= 44),
        "inverse clip must mask outline bleed around the nominal clip, got {parts:?}"
    );
}

#[test]
fn render_frame_applies_vector_clip() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(m 0 0 l 32 0 32 32 0 32)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
    assert!(
        planes
            .iter()
            .all(|plane| plane.bitmap.iter().any(|value| *value > 0))
    );
    assert!(planes.iter().all(|plane| plane.destination.x >= 0));
    assert!(planes.iter().all(|plane| plane.destination.y >= 0));
}

#[test]
fn render_frame_clips_to_frame_bounds() {
    let plane = ImagePlane {
        size: Size {
            width: 20,
            height: 20,
        },
        stride: 20,
        color: RgbaColor(0x00FF_FFFF),
        destination: Point { x: 50, y: 50 },
        kind: ass::ImageType::Character,
        bitmap: vec![255; 400],
    };
    let clipped = apply_event_clip(
        vec![plane],
        Rect {
            x_min: 0,
            y_min: 0,
            x_max: 60,
            y_max: 60,
        },
        false,
    );

    assert_eq!(clipped.len(), 1);
    assert_eq!(clipped[0].size.width, 10);
    assert_eq!(clipped[0].size.height, 10);
}

#[test]
fn render_frame_applies_margin_clip_when_enabled() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &config(
            100,
            100,
            rassa_core::Margins {
                top: 10,
                bottom: 10,
                left: 10,
                right: 10,
            },
            true,
        ),
    );

    assert!(!planes.is_empty());
    assert!(planes.iter().all(|plane| plane.destination.x >= 10));
    assert!(planes.iter().all(|plane| plane.destination.y >= 10));
    assert!(
        planes
            .iter()
            .all(|plane| plane.destination.x + plane.size.width <= 90)
    );
    assert!(
        planes
            .iter()
            .all(|plane| plane.destination.y + plane.size.height <= 90)
    );
}

#[test]
fn render_frame_maps_into_content_area_when_margins_are_not_used() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &config(
            120,
            120,
            rassa_core::Margins {
                top: 10,
                bottom: 10,
                left: 10,
                right: 10,
            },
            false,
        ),
    );

    assert!(!planes.is_empty());
    let bounds = visible_bounds(&planes).expect("visible bounds");
    assert!(
        bounds.x_min >= 10,
        "visible bounds should start inside content area: {bounds:?}"
    );
    assert!(
        bounds.y_min >= 9,
        "libass-style antialiasing may allocate one guard row above the content area: {bounds:?}"
    );
    assert!(
        bounds.x_max <= 110,
        "visible bounds should end inside content area: {bounds:?}"
    );
    assert!(
        bounds.y_max <= 110,
        "visible bounds should end inside content area: {bounds:?}"
    );
}

#[test]
fn render_frame_keeps_border_closer_to_device_size_when_scaled_border_is_disabled() {
    let enabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
    let disabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: no\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let config = config(200, 200, rassa_core::Margins::default(), true);
    let enabled_planes =
        engine.render_frame_with_provider_and_config(&enabled, &provider, 500, &config);
    let disabled_planes =
        engine.render_frame_with_provider_and_config(&disabled, &provider, 500, &config);
    let enabled_outline_area: i32 = enabled_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
        .map(|plane| plane.size.width * plane.size.height)
        .sum();
    let disabled_outline_area: i32 = disabled_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
        .map(|plane| plane.size.width * plane.size.height)
        .sum();

    assert!(disabled_outline_area > 0);
    assert!(disabled_outline_area < enabled_outline_area);
}

#[test]
fn render_frame_applies_font_scale_to_output() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let scaled = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 120,
            },
            font_scale: 2.0,
            ..RendererConfig::default()
        },
    );

    assert!(!baseline.is_empty());
    assert!(!scaled.is_empty());
    assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
}

#[test]
fn render_frame_applies_text_scale_overrides() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Scale").expect("script should parse");
    let stretched = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50}Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let scaled = engine.render_frame_with_provider(&stretched, &provider, 500);
    let baseline_width = baseline
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.destination.x + plane.size.width)
        .max()
        .expect("baseline max x")
        - baseline
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.x)
            .min()
            .expect("baseline min x");
    let scaled_width = scaled
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.destination.x + plane.size.width)
        .max()
        .expect("scaled max x")
        - scaled
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.x)
            .min()
            .expect("scaled min x");

    assert!(scaled_width > baseline_width);
    assert!(total_plane_area(&scaled) < total_plane_area(&baseline) * 2);
}

#[test]
fn render_frame_applies_drawing_scale_overrides() {
    let baseline = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 10 0 10 10 0 10").expect("script should parse");
    let scaled = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50\\p1}m 0 0 l 10 0 10 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
    let scaled_planes = engine.render_frame_with_provider(&scaled, &provider, 500);
    let baseline_plane = baseline_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("baseline drawing plane");
    let scaled_plane = scaled_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("scaled drawing plane");

    assert!(scaled_plane.size.width > baseline_plane.size.width);
    assert!(scaled_plane.size.height < baseline_plane.size.height);
    assert_eq!(scaled_plane.destination, Point { x: 10, y: 10 });
}

#[test]
fn non_positioned_drawing_does_not_receive_positioned_overhang_compensation() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 l 10 0 10 10 0 10{\\p0}").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let plane = engine
        .render_frame_with_provider(&track, &provider, 500)
        .into_iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");

    assert_eq!(
        plane.size.width, 11,
        "libass-style positioned overhang compensation is specific to explicit \\pos vector drawings"
    );
}

#[test]
#[ignore = "parked while rassa stops treating pixel-perfect libass drawing pbo residuals as an optimization blocker"]
fn render_frame_applies_drawing_baseline_offset() {
    fn pbo_track(pbo_tag: &str) -> ParsedTrack {
        parse_script_text(&format!("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(10,40)}}X{{{pbo_tag}\\p1}}m 0 0 l 10 0 10 10 0 10{{\\p0}}X"))
                .expect("script should parse")
    }

    let baseline = pbo_track("");
    let pbo5 = pbo_track("\\pbo5");
    let shifted = pbo_track("\\pbo12");
    let negative = pbo_track("\\pbo-12");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let drawing_plane = |track: &ParsedTrack| {
        engine
            .render_frame_with_provider(track, &provider, 500)
            .into_iter()
            .find(|plane| {
                plane.kind == ass::ImageType::Character
                    && plane.size.width == 11
                    && plane.size.height == 11
            })
            .expect("drawing plane")
    };
    let baseline_drawing = drawing_plane(&baseline);
    let pbo5_drawing = drawing_plane(&pbo5);
    let shifted_drawing = drawing_plane(&shifted);
    let negative_drawing = drawing_plane(&negative);

    assert_eq!(
        pbo5_drawing.destination, baseline_drawing.destination,
        "libass keeps pbo below drawing height anchored for this 10-unit positioned drawing"
    );
    assert_eq!(
        shifted_drawing.destination.x,
        baseline_drawing.destination.x
    );
    assert_eq!(
        shifted_drawing.destination.y,
        baseline_drawing.destination.y + 2,
        "libass applies \\pbo as max(pbo - drawing_height, 0) for this top-anchored positioned drawing"
    );
    assert_eq!(
        negative_drawing.destination, baseline_drawing.destination,
        "libass keeps negative \\pbo top-anchored for this positioned drawing"
    );
}

#[test]
fn render_frame_applies_banner_effect_motion() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Banner;25;0;0,Banner").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let early = character_bounds(&engine.render_frame_with_provider(&track, &provider, 100))
        .expect("early banner bounds");
    let late = character_bounds(&engine.render_frame_with_provider(&track, &provider, 1500))
        .expect("late banner bounds");

    assert!(
        late.x_min < early.x_min,
        "right-to-left banner should move left over time"
    );
    assert!(
        (194..=198).contains(&early.x_min),
        "libass positions a right-to-left banner by PlayResX - elapsed/delay, got {early:?}"
    );
}

#[test]
fn banner_effect_delay_uses_layout_scale_not_render_supersampling() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Banner;25;0;0,Banner").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let bounds = character_bounds(&engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        1500,
        &RendererConfig {
            frame: Size {
                width: 1600,
                height: 800,
            },
            storage: Size {
                width: 200,
                height: 100,
            },
            ..RendererConfig::default()
        },
    ))
    .expect("supersampled banner bounds");

    assert!(
        bounds.x_min >= 1112,
        "Banner delay should be based on layout/storage resolution rather than render supersampling; got {bounds:?}"
    );
}

#[test]
fn render_frame_applies_scroll_effect_motion() {
    let up = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Scroll up;20;100;25;0,Scroll").expect("script should parse");
    let down = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Scroll down;20;100;25;0,Scroll").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let up_early = character_bounds(&engine.render_frame_with_provider(&up, &provider, 100))
        .expect("early scroll-up bounds");
    let up_late = character_bounds(&engine.render_frame_with_provider(&up, &provider, 1500))
        .expect("late scroll-up bounds");
    let down_early = character_bounds(&engine.render_frame_with_provider(&down, &provider, 100))
        .expect("early scroll-down bounds");
    let down_late = character_bounds(&engine.render_frame_with_provider(&down, &provider, 1500))
        .expect("late scroll-down bounds");

    assert!(
        up_late.y_min < up_early.y_min,
        "scroll up should move upward"
    );
    assert!(
        down_late.y_min > down_early.y_min,
        "scroll down should move downward"
    );
}

#[test]
fn render_frame_applies_text_spacing_override() {
    let baseline = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}IIII").expect("script should parse");
    let spaced = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fsp8}IIII").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
    let spaced_planes = engine.render_frame_with_provider(&spaced, &provider, 500);
    let baseline_width = character_bounds(&baseline_planes)
        .expect("baseline bounds")
        .width();
    let spaced_width = character_bounds(&spaced_planes)
        .expect("spaced bounds")
        .width();

    assert!(spaced_width > baseline_width);
}

#[test]
fn render_frame_scales_output_to_frame_size() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let scaled = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            ..default_renderer_config(&track)
        },
    );

    assert!(total_plane_area(&baseline) > 0);
    assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
}

#[test]
fn render_frame_applies_pixel_aspect_horizontally() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 120,
            },
            ..default_renderer_config(&track)
        },
    );
    let widened = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 120,
            },
            pixel_aspect: 2.0,
            ..default_renderer_config(&track)
        },
    );

    let baseline_bounds = character_bounds(&baseline).expect("baseline character bounds");
    let widened_bounds = character_bounds(&widened).expect("widened character bounds");
    assert!(
        widened_bounds.x_min > baseline_bounds.x_min,
        "pixel aspect should affect horizontal placement: baseline={baseline_bounds:?} widened={widened_bounds:?}"
    );
}

#[test]
fn render_frame_derives_pixel_aspect_from_storage_size_when_unset() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Storage").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            ..default_renderer_config(&track)
        },
    );
    let storage_adjusted = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            storage: Size {
                width: 400,
                height: 120,
            },
            ..default_renderer_config(&track)
        },
    );

    assert!(total_plane_area(&baseline) > 0);
    assert!(total_plane_area(&storage_adjusted) < total_plane_area(&baseline));
}

#[test]
fn render_frame_layout_resolution_takes_precedence_over_storage_and_explicit_aspect() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\nLayoutResX: 400\nLayoutResY: 240\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Layout").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            ..default_renderer_config(&track)
        },
    );
    let overridden_inputs = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            storage: Size {
                width: 400,
                height: 120,
            },
            pixel_aspect: 2.0,
            ..default_renderer_config(&track)
        },
    );

    assert_eq!(
        total_plane_area(&overridden_inputs),
        total_plane_area(&baseline)
    );
}

#[test]
fn render_frame_applies_line_position_to_subtitles() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Shift").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let shifted = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 120,
            },
            line_position: 50.0,
            ..RendererConfig::default()
        },
    );

    let baseline_y = baseline
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("baseline plane");
    let shifted_y = shifted
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("shifted plane");

    assert!(shifted_y < baseline_y);
}

#[test]
fn render_frame_applies_line_spacing_to_multiline_subtitles() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,One\\NTwo").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let spaced = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 140,
            },
            line_spacing: 20.0,
            ..RendererConfig::default()
        },
    );

    assert!(vertical_span(&spaced) > vertical_span(&baseline));
}

#[test]
fn render_frame_avoids_basic_bottom_collision_for_unpositioned_events() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,First\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let mut ys = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.destination.y)
        .collect::<Vec<_>>();
    ys.sort_unstable();
    ys.dedup();

    assert!(ys.len() >= 2);
    assert!(ys.last().expect("max y") - ys.first().expect("min y") >= 20);
}

#[test]
fn render_frame_allows_basic_collision_across_different_layers() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 1,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H00FF00&}Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let layer0_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("layer 0 character plane");
    let layer1_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("layer 1 character plane");

    assert_eq!(layer0_y, layer1_y);
}

#[test]
fn render_frame_interpolates_move_position() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\move(0,0,100,0,0,1000)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

    let start_x = start_planes
        .iter()
        .map(|plane| plane.destination.x)
        .min()
        .expect("start plane");
    let mid_x = mid_planes
        .iter()
        .map(|plane| plane.destination.x)
        .min()
        .expect("mid plane");
    let end_x = end_planes
        .iter()
        .map(|plane| plane.destination.x)
        .min()
        .expect("end plane");

    assert!(start_x <= mid_x);
    assert!(mid_x <= end_x);
    assert!(end_x - start_x >= 80);
}

#[test]
fn center_positioned_move_without_geometric_transform_anchors_like_pos() {
    let script = |tag: &str| {
        format!(
            "[Script Info]\nPlayResX: 240\nPlayResY: 160\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,72,&H00FFFFFF,&H0000FFFF,&H00FFFFFF,&H00000000,0,0,0,0,100,100,0,0,1,3.5,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an5{tag}\\blur1.5}}S"
        )
    };
    let pos = parse_script_text(&script("\\pos(120,80)")).expect("pos script should parse");
    let movement = parse_script_text(&script("\\move(120,80,120,80)\\org(80,-20)"))
        .expect("move script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let pos_bounds = visible_bounds(&engine.render_frame_with_provider(&pos, &provider, 20))
        .expect("pos should render");
    let move_bounds = visible_bounds(&engine.render_frame_with_provider(&movement, &provider, 20))
        .expect("move should render");

    assert!(
        (move_bounds.y_min - pos_bounds.y_min).abs() <= 2,
        "libass treats a zero-distance \\move and unused \\org like an equivalent \\pos for \\an5 center anchoring; pos={pos_bounds:?} move={move_bounds:?}"
    );
}

#[test]
fn render_frame_applies_z_rotation_to_event_planes() {
    let baseline = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
    let rotated = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\frz90\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
    let rotated_planes = engine.render_frame_with_provider(&rotated, &provider, 500);
    let baseline_bounds = character_bounds(&baseline_planes).expect("baseline bounds");
    let rotated_bounds = character_bounds(&rotated_planes).expect("rotated bounds");

    assert!(baseline_bounds.width() > baseline_bounds.height());
    assert!(rotated_bounds.height() > rotated_bounds.width());
}

#[test]
#[ignore = "strict libass positioned-vector overhang coverage residual kept as diagnostic after optimization pivot"]
fn positioned_drawing_uses_position_y_before_compare_supersample_offset() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(20,24)\\p1}m 0 0 l 42 0 42 12 0 12{\\p0}").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 1760,
                height: 1120,
            },
            storage: Size {
                width: 220,
                height: 140,
            },
            ..RendererConfig::default()
        },
    );
    let bounds = character_bounds(&planes).expect("positioned drawing bounds");
    let visible = visible_bounds(&planes).expect("positioned drawing visible bounds");

    assert_eq!(
        bounds.y_min,
        24 * 8,
        "libass keeps top-aligned positioned vector drawings anchored at \\pos y before final supersample offset; got {bounds:?}"
    );
    assert_eq!(
        bounds.x_min,
        19 * 8,
        "libass gives positioned vector drawings one output-pixel left overhang at compare superscale; got {bounds:?}"
    );
    assert_eq!(
        bounds.x_max,
        63 * 8,
        "libass keeps the allocated right drawing edge available for transforms; got {bounds:?}"
    );
    assert_eq!(
        visible.x_min,
        19 * 8 + 7,
        "libass leaves only a subpixel-thin antialias sample in the positioned drawing's left overhang; got visible {visible:?}"
    );
    assert_eq!(
        visible.x_max,
        62 * 8 + 1,
        "positioned vector drawing keeps a subpixel-thin antialias sample in the allocated right overhang; got visible {visible:?}"
    );
}

#[test]
fn render_frame_shears_positioned_drawing_from_run_baseline_not_org() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(120,24)\\org(120,80)\\frx45\\fax0.25\\p1}m 0 0 l 50 0 50 14 0 14{\\p0}")
            .expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let bounds = planes_bounds(&planes).expect("drawing plane should render");

    assert!(
        bounds.x_min >= 116,
        "libass applies \\fax in drawing-local baseline space before \\org perspective; global \\org shear pulls this too far left: {bounds:?}"
    );
}

#[test]
fn render_frame_applies_z_rotation_per_override_run() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\c&H0000FF&}MMMM{\\frz90\\c&H00FF00&}MMMM").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let red_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
        .collect::<Vec<_>>();
    let green = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .expect("rotated green drawing plane");

    assert!(
        red_planes.len() >= 2,
        "expected multiple unrotated red glyph planes"
    );
    let red_y_min = red_planes
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("red y min");
    let red_y_max = red_planes
        .iter()
        .map(|plane| plane.destination.y)
        .max()
        .expect("red y max");
    assert!(
        red_y_max - red_y_min <= 1,
        "unrotated run should stay on a horizontal baseline: {red_planes:?}"
    );
    assert!(
        green.size.height >= green.size.width,
        "rotated run should become vertical-ish: {green:?}"
    );
}

#[test]
fn render_frame_interpolates_z_rotation_transform() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\t(0,1000,\\frz90)\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);
    let start_bounds = character_bounds(&start_planes).expect("start bounds");
    let end_bounds = character_bounds(&end_planes).expect("end bounds");

    assert!(start_bounds.width() > start_bounds.height());
    assert!(end_bounds.height() > end_bounds.width());
}

#[test]
fn render_frame_applies_fad_alpha() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fad(200,200)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

    let start_alpha = start_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("start alpha");
    let mid_alpha = mid_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("mid alpha");
    let end_alpha = end_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("end alpha");

    assert!(start_alpha > mid_alpha);
    assert!(end_alpha > mid_alpha);
}

#[test]
fn render_frame_applies_full_fade_alpha() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fade(255,0,128,0,200,700,1000)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let middle_planes = engine.render_frame_with_provider(&track, &provider, 400);
    let late_planes = engine.render_frame_with_provider(&track, &provider, 850);

    let start_alpha = start_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("start alpha");
    let middle_alpha = middle_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("middle alpha");
    let late_alpha = late_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("late alpha");

    assert!(start_alpha > middle_alpha);
    assert!(late_alpha > middle_alpha);
    assert!(late_alpha < start_alpha);
}

#[test]
fn render_frame_switches_karaoke_fill_after_elapsed_span() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\k50}Ka").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
    let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

    assert!(
        early_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x6655_4400)
    );
    assert!(
        late_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
}

#[test]
fn render_frame_sweeps_karaoke_fill_during_active_span() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\K100}Kara").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        mid_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
    assert!(
        mid_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x6655_4400)
    );
}

#[test]
fn render_frame_hides_outline_for_ko_until_span_ends() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H000A0B0C,&H00000000,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\ko50}Ko").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
    let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

    assert!(
        !early_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );
    assert!(
        late_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );
}

#[test]
fn vertical_font_raster_advances_rotate_bitmap_like_libass_vertical_faces() {
    let glyph = RasterGlyph {
        width: 2,
        height: 3,
        stride: 2,
        offset_x: 1,
        offset_y: 2,
        advance_x: 7,
        bitmap: vec![1, 2, 3, 4, 5, 6],
        ..RasterGlyph::default()
    };
    let style = ParsedSpanStyle {
        font_name: "@Vertical".to_string(),
        font_size: 50.0,
        ..ParsedSpanStyle::default()
    };

    let glyphs = apply_vertical_font_raster_advances(vec![glyph], &style);
    let rotated = &glyphs[0];

    assert_eq!(rotated.width, 3);
    assert_eq!(rotated.height, 2);
    assert_eq!(rotated.stride, 3);
    assert_eq!(rotated.bitmap, vec![5, 3, 1, 6, 4, 2]);
    assert_eq!(rotated.offset_x, 13);
    assert_eq!(rotated.offset_y, 20);
    assert_eq!(rotated.advance_x, 50);
    assert_eq!(rotated.advance_y, 0);
}

#[test]
fn clipped_vector_drawing_keeps_libass_like_transparent_plane_padding() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\clip(801,103,806,186)\\pos(627.43,184.01)\\p1\\fscx251\\fscy258\\c&HFFFFFF&}m 0 0 l 132 -1 l 135 0 l 136 26 l 1 28 l 0 -1\n";
    let track = parse_script_text(script).expect("clip/drawing probe should parse");
    let engine = RenderEngine::new();
    let planes = engine.render_frame_with_provider(&track, &NullFontProvider, 500);

    assert_eq!(
        planes.len(),
        1,
        "narrow \\clip should retain the clipped drawing plane like libass"
    );
    let plane = &planes[0];
    assert_eq!(plane.destination.x, 801);
    assert_eq!(plane.size.width, 5);
    assert_eq!(plane.size.height, 80);
}

#[test]
fn blurred_vector_drawing_expands_fill_plane_like_libass() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\blur6\\p1\\c&HC6BECA&\\fscx165\\fscy138\\pos(948,1324)}m 0 0 b -3 -28 -6 -56 -9 -84 b -18 -113 -6 -135 -5 -160 b -3 -184 1 -208 3 -232 b 125 -233 248 -235 370 -236 b 377 -220 386 -204 393 -188 b 397 -167 403 -146 407 -125 b 409 -109 411 -93 413 -77 b 421 -61 431 -44 439 -28 b 440 -18 441 -7 442 3 b 295 3 147 1 0 1\n";
    let track = parse_script_text(script).expect("track parses");
    let engine = RenderEngine::new();
    let planes = engine.render_frame_with_provider(&track, &NullFontProvider, 500);

    assert_eq!(planes.len(), 1);
    let plane = &planes[0];
    assert_eq!(plane.destination.y, 650);
    assert_eq!(plane.size.width, 788);
    assert_eq!(plane.size.height, 372);
}

#[test]
fn render_frame_renders_drawing_plane() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert_eq!(plane.destination.x, 10);
    assert_eq!(plane.destination.y, 10);
    assert!(plane.bitmap.contains(&255));
}

#[test]
fn render_frame_renders_drawing_holes_with_even_odd_fill() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 20 0 20 20 0 20 m 5 5 l 15 5 15 15 5 15").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    let center_x = 10 - (plane.destination.x - 10);
    let center_y = 10 - (plane.destination.y - 10);
    assert_eq!(
        plane.bitmap[center_y as usize * plane.stride as usize + center_x as usize],
        0,
        "nested drawing contours should punch libass-like hollow holes instead of union-filling"
    );
    assert!(plane.bitmap.contains(&255));
}

#[test]
fn render_frame_antialiases_vector_drawing_edges() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 20 0 20 20").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");

    assert!(
        plane.bitmap.iter().any(|value| *value > 0 && *value < 255),
        "vector drawing edges should keep libass-like partial coverage instead of binary rasterization"
    );
}

#[test]
fn render_frame_renders_bezier_drawing_plane() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 b 10 0 10 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert!(plane.bitmap.contains(&255));
    assert!(plane.size.width >= 8);
    assert!(plane.size.height >= 8);
}

#[test]
fn render_frame_emits_outline_and_shadow_for_drawings() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H000A0B0C,&H00445566,0,0,0,0,100,100,0,0,1,2,3,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0C0B_0A00)
    );
    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x6655_4400)
    );
}

#[test]
fn render_frame_renders_spline_drawing_plane() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 s 10 0 10 10 0 10 p -5 5 c").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert!(plane.bitmap.contains(&255));
    assert!(plane.size.width >= 10);
    assert!(plane.size.height >= 10);
}

#[test]
fn render_frame_renders_non_closing_move_subpaths() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8 n 20 20 l 28 20 28 28 20 28").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert!(plane.bitmap.contains(&255));
    assert!(plane.size.width >= 28);
    assert!(plane.size.height >= 28);
}

#[test]
fn render_frame_applies_timed_transform_style() {
    let track = parse_script_text("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H000000FF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\t(0,1000,\\1c&H00112233&\\fs48\\bord4)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

    assert!(
        !start_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );
    assert!(
        mid_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );
    assert!(
        end_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );

    let start_fill = start_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("start fill")
        .color
        .0;
    let end_fill = end_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("end fill")
        .color
        .0;
    assert_ne!(start_fill, end_fill);
    assert!(total_plane_area(&end_planes) > total_plane_area(&start_planes));
}
