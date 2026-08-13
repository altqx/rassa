use rassa_core::{ImagePlane, Rect};
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

fn fay_cluster_script(fay: Option<f64>) -> String {
    let fay = fay.map(|value| format!("\\fay{value}")).unwrap_or_default();
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,96,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{{\\an7\\pos(100,150){fay}}}a\u{326}"
    )
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
fn positive_fay_shears_every_glyph_inside_a_harfbuzz_cluster_downward() {
    // a+U+0326 is one two-glyph cluster; \fay0.5 must lower the visible bottom by ~26px.
    let provider = FixtureFontProvider::new();
    let plain = parse_script_text(&fay_cluster_script(None)).expect("plain fixture parses");
    let sheared =
        parse_script_text(&fay_cluster_script(Some(0.5))).expect("sheared fixture parses");
    let engine = RenderEngine::new();

    let prepared = engine.prepare_frame(&sheared, &provider, 1_000);
    let glyphs = &prepared.active_events[0].lines[0].runs[0].glyphs;
    assert!(
        glyphs.len() >= 2,
        "fixture must remain a multi-glyph HarfBuzz cluster"
    );
    assert!(
        glyphs
            .windows(2)
            .all(|pair| pair[0].cluster == pair[1].cluster),
        "fixture glyphs must share one cluster: {glyphs:?}"
    );

    let plain_bounds = visible_bounds(&engine.render_frame_with_provider(&plain, &provider, 1_000))
        .expect("plain cluster renders");
    let sheared_bounds =
        visible_bounds(&engine.render_frame_with_provider(&sheared, &provider, 1_000))
            .expect("sheared cluster renders");

    assert_eq!(sheared_bounds.x_min, plain_bounds.x_min);
    assert_eq!(sheared_bounds.x_max, plain_bounds.x_max);
    assert!(
        (4..=10).contains(&(sheared_bounds.y_min - plain_bounds.y_min)),
        "positive \\fay starts the cluster lower like libass: plain={plain_bounds:?} sheared={sheared_bounds:?}"
    );
    assert!(
        (24..=28).contains(&(sheared_bounds.y_max - plain_bounds.y_max)),
        "intra-cluster advances must lower the later glyph like libass 582630d: plain={plain_bounds:?} sheared={sheared_bounds:?}"
    );
}
