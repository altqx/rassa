use rassa_core::ass;
use rassa_fonts::{FontMatch, FontProvider, FontProviderKind, FontQuery};
use rassa_parse::parse_script_text;
use rassa_render::RenderEngine;

struct FixtureFontProvider {
    path: std::path::PathBuf,
}

impl FixtureFontProvider {
    fn new() -> Self {
        Self {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
        }
    }
}

impl FontProvider for FixtureFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        FontMatch {
            family: query.family.clone(),
            path: Some(self.path.clone()),
            face_index: Some(0),
            style: query.style.clone(),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Fontconfig,
        }
    }
}

fn border_collision_script(last_line_border: &str) -> String {
    r#"[Script Info]
ScriptType: v4.00+
ScaledBorderAndShadow: yes
PlayResX: 384
PlayResY: 288

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,FixtureFont,24,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1
Style: Marker,FixtureFont,24,&H000000FF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\bord30\3a&HFF}A
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\bord29.5\3a&HFF}A
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\bord31.2\3a&HFF}A\N{\bord$LAST_BORDER}A
Dialogue: 0,0:00:00.00,0:00:05.00,Marker,,0,0,0,,{\bord0}A
"#
    .replace("$LAST_BORDER", last_line_border)
}

fn marker_y(last_line_border: &str) -> i32 {
    let track = parse_script_text(&border_collision_script(last_line_border))
        .expect("border-collision fixture parses");
    RenderEngine::new()
        .render_frame_with_provider(&track, &FixtureFontProvider::new(), 2_000)
        .into_iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("marker event renders")
}

#[test]
fn multiline_collision_uses_last_line_border_for_bottom_padding() {
    // border_bottom is the last line's max; raising 0.9 → 31.2 moves the next collision 30px up.
    let thin_last_line = marker_y("0.9");
    let thick_last_line = marker_y("31.2");

    assert_eq!(
        thin_last_line - thick_last_line,
        30,
        "the first line's 31.2px border must not also pad the multiline event's bottom"
    );
}
