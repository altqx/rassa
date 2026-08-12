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

fn wrap_script(text: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,60,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{{\\an7\\pos(100,100)}}{text}"
    )
}

fn render(text: &str) -> Vec<rassa_core::ImagePlane> {
    let track = parse_script_text(&wrap_script(text)).expect("wrap regression fixture parses");
    RenderEngine::new().render_frame_with_provider(&track, &FixtureFontProvider::new(), 500)
}

#[test]
fn trailing_soft_newline_matches_visible_text_without_it() {
    // Current libass treats a trailing soft \\n as trimmable whitespace in
    // wrap modes other than 2; it must not allocate a phantom line.
    assert_eq!(render("A\\n"), render("A"));
}

#[test]
fn all_space_line_ending_in_soft_newline_is_empty_and_safe() {
    // This is the all-skippable fast path added for CVE-2026-61627 /
    // GHSA-pjjp-65r7-ppgm.
    assert!(render("     \\n").is_empty());
}

#[test]
fn wrap_exact_capacity_trailing_soft_break_is_safe() {
    // libass starts with exactly 1024 GlyphInfo slots. This payload produces
    // 1024 glyphs (A, hard newline, 1021 spaces, soft newline), leaves the
    // final line entirely skippable, and used to step one slot out of bounds.
    let exact_capacity = format!("A\\N{}\\n", " ".repeat(1021));
    assert_eq!(render(&exact_capacity), render("A\\n"));
}
