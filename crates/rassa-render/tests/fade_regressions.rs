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

fn fade_script(border_style: i32, text: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,60,&H20332211,&H40554433,&H60776655,&H80998877,0,0,0,0,100,100,0,0,{border_style},4,5,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{{\\an7\\pos(100,100)}}{text}"
    )
}

fn render(border_style: i32, text: &str) -> Vec<rassa_core::ImagePlane> {
    let track = parse_script_text(&fade_script(border_style, text)).expect("fade fixture parses");
    RenderEngine::new().render_frame_with_provider(&track, &FixtureFontProvider::new(), 500)
}

fn has_plane(planes: &[rassa_core::ImagePlane], kind: ass::ImageType, color: u32) -> bool {
    planes
        .iter()
        .any(|plane| plane.kind == kind && plane.color.0 == color)
}

#[test]
fn border_style_four_background_honors_fade() {
    // libass 084333f applies the event fade to state color 4 before drawing
    // the BorderStyle=4 background. At 500 ms, &H80998877 becomes RGBA
    // 0x778899BF rather than retaining its pre-fade 0x80 alpha.
    let planes = render(4, r"{\fad(1000,0)}AB");
    assert!(has_plane(&planes, ass::ImageType::Shadow, 0x7788_99BF));
    assert!(!has_plane(&planes, ass::ImageType::Shadow, 0x7788_9980));
}

#[test]
fn fade_applies_to_all_four_ass_colors() {
    // libass c8ccdfd fixed the delayed-fade path to apply to primary,
    // secondary, outline and back colours. Half-progress \\kf exposes both
    // fill colours in one frame.
    let planes = render(1, r"{\fad(1000,0)\kf100}A");
    assert!(has_plane(&planes, ass::ImageType::Character, 0x1122_338F));
    assert!(has_plane(&planes, ass::ImageType::Character, 0x3344_559F));
    assert!(has_plane(&planes, ass::ImageType::Outline, 0x5566_77AF));
    assert!(has_plane(&planes, ass::ImageType::Shadow, 0x7788_99BF));
}

#[test]
fn adjacent_pre_fade_alpha_changes_remain_distinct() {
    // libass 12f3e45 delays fade application until after style-run splitting.
    // Otherwise adjacent secondary alpha 0 and 1 both round to the same
    // intermediate value and the runs are incorrectly coalesced.
    let planes = render(1, r"{\fad(1000,0)\2a&H00&\kf100}f{\2a&H01&\kf100}i");
    assert!(has_plane(&planes, ass::ImageType::Character, 0x3344_557F));
    assert!(has_plane(&planes, ass::ImageType::Character, 0x3344_5580));
}
