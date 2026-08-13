use rassa_core::{ImagePlane, Rect, ass};
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
            provider: FontProviderKind::Attached,
        }
    }
}

fn render(text: &str) -> Vec<ImagePlane> {
    let script = format!(
        r#"[Script Info]
ScriptType: v4.00+
PlayResX: 800
PlayResY: 600
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,FixtureFont,40,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{text}
"#
    );
    let track = parse_script_text(&script).expect("whitespace fixture parses");
    RenderEngine::new().render_frame_with_provider(&track, &FixtureFontProvider::new(), 2_000)
}

fn visible_character_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for plane in planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
    {
        let stride = usize::try_from(plane.stride).ok()?;
        for y in 0..usize::try_from(plane.size.height).ok()? {
            for x in 0..usize::try_from(plane.size.width).ok()? {
                if plane.bitmap.get(y * stride + x).copied().unwrap_or(0) == 0 {
                    continue;
                }
                let x = plane.destination.x + x as i32;
                let y = plane.destination.y + y as i32;
                match &mut bounds {
                    Some(bounds) => {
                        bounds.x_min = bounds.x_min.min(x);
                        bounds.y_min = bounds.y_min.min(y);
                        bounds.x_max = bounds.x_max.max(x + 1);
                        bounds.y_max = bounds.y_max.max(y + 1);
                    }
                    None => {
                        bounds = Some(Rect {
                            x_min: x,
                            y_min: y,
                            x_max: x + 1,
                            y_max: y + 1,
                        });
                    }
                }
            }
        }
    }
    bounds
}

#[test]
fn centered_edge_spaces_across_style_runs_do_not_change_placement() {
    let plain = render("{\\fs40}A");
    let padded = render("{\\fs80}   {\\fs40}A{\\fs100}   {\\fs40}");

    assert_eq!(
        visible_character_bounds(&padded),
        visible_character_bounds(&plain),
        "trimmed edge whitespace must affect neither centered advance nor vertical metrics"
    );
}

#[test]
fn an_all_space_middle_line_keeps_the_empty_line_height() {
    let empty = render("A\\N\\NA");
    let spaces = render("A\\N   \\NA");

    assert_eq!(
        visible_character_bounds(&spaces),
        visible_character_bounds(&empty),
        "a line emptied by libass whitespace trimming still contributes empty-line metrics"
    );
}
