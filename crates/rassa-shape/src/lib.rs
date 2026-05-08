use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
};

static FONT_BYTES_CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();

fn font_bytes_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<u8>>>> {
    FONT_BYTES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a virtual font file in memory.
///
/// This is primarily used by wasm/browser hosts that do not have a real
/// filesystem/fontconfig database. Callers can return the same virtual `path`
/// from their `FontProvider`; shaping and rasterization will then load bytes
/// from this cache instead of `std::fs`.
pub fn register_virtual_font_bytes(path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) {
    font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned")
        .insert(path.into(), Arc::new(bytes.into()));
}

/// Look up previously registered virtual font bytes.
pub fn virtual_font_bytes(path: &Path) -> Option<Arc<Vec<u8>>> {
    font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned")
        .get(path)
        .cloned()
}

fn cached_font_bytes(path: &Path) -> Option<Arc<Vec<u8>>> {
    if let Some(bytes) = virtual_font_bytes(path) {
        return Some(bytes);
    }

    let bytes = Arc::new(fs::read(path).ok()?);
    font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned")
        .insert(path.to_path_buf(), bytes.clone());
    Some(bytes)
}

use harfrust::{Direction, FontRef, Language, ShaperData, UnicodeBuffer};
use rassa_core::RassaResult;
use rassa_fonts::{FontMatch, FontProvider, FontQuery};
use rassa_unicode::{BidiDirection, TextSegment, UnicodeAnalysis, UnicodePipeline};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShapingMode {
    #[default]
    Simple,
    Complex,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeRequest {
    pub text: String,
    pub family: String,
    pub style: Option<String>,
    pub weight: Option<i32>,
    pub language: Option<String>,
    pub mode: ShapingMode,
    pub font_size: Option<f32>,
}

impl ShapeRequest {
    pub fn new(text: impl Into<String>, family: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            family: family.into(),
            style: None,
            weight: None,
            language: None,
            mode: ShapingMode::Simple,
            font_size: None,
        }
    }

    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    pub fn with_weight(mut self, weight: i32) -> Self {
        self.weight = Some(weight);
        self
    }

    pub fn with_optional_weight(mut self, weight: Option<i32>) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn with_mode(mut self, mode: ShapingMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size.is_finite().then_some(font_size.max(0.0));
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlyphInfo {
    pub glyph_id: u32,
    pub cluster: usize,
    pub x_advance: f32,
    pub y_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    pub text: String,
    pub char_range: std::ops::Range<usize>,
    pub byte_range: std::ops::Range<usize>,
    pub direction: BidiDirection,
    pub font: FontMatch,
    pub glyphs: Vec<GlyphInfo>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    pub analysis: UnicodeAnalysis,
    pub font: FontMatch,
    pub mode: ShapingMode,
    pub runs: Vec<ShapedRun>,
}

pub trait Shaper {
    fn shape_segment(
        &self,
        segment: &TextSegment,
        font: &FontMatch,
        direction: BidiDirection,
    ) -> Vec<GlyphInfo>;
}

#[derive(Default)]
pub struct SimpleShaper;

impl Shaper for SimpleShaper {
    fn shape_segment(
        &self,
        segment: &TextSegment,
        _font: &FontMatch,
        direction: BidiDirection,
    ) -> Vec<GlyphInfo> {
        let char_count = segment.text.chars().count();
        let mut glyphs = Vec::with_capacity(char_count);
        match direction {
            BidiDirection::RightToLeft | BidiDirection::WeakRightToLeft => {
                let characters = segment.text.chars().collect::<Vec<_>>();
                for (cluster, character) in characters.into_iter().enumerate().rev() {
                    glyphs.push(GlyphInfo {
                        glyph_id: character as u32,
                        cluster,
                        x_advance: 1.0,
                        y_advance: 0.0,
                        x_offset: 0.0,
                        y_offset: 0.0,
                    });
                }
            }
            _ => {
                for (cluster, character) in segment.text.chars().enumerate() {
                    glyphs.push(GlyphInfo {
                        glyph_id: character as u32,
                        cluster,
                        x_advance: 1.0,
                        y_advance: 0.0,
                        x_offset: 0.0,
                        y_offset: 0.0,
                    });
                }
            }
        }
        glyphs
    }
}

#[derive(Default)]
pub struct ShapeEngine {
    unicode: UnicodePipeline,
    simple: SimpleShaper,
}

impl ShapeEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shape_text<P: FontProvider>(
        &self,
        provider: &P,
        request: &ShapeRequest,
    ) -> RassaResult<ShapedText> {
        let analysis = self
            .unicode
            .analyze_text(&request.text, request.language.as_deref())?;
        let font = provider.resolve(&FontQuery {
            family: request.family.clone(),
            style: request.style.clone(),
            weight: request.weight,
        });
        let direction = analysis.bidi_analysis.direction;

        let runs = analysis
            .segments
            .iter()
            .map(|segment| ShapedRun {
                text: segment.text.clone(),
                char_range: segment.char_range.clone(),
                byte_range: segment.byte_range.clone(),
                direction,
                font: font.clone(),
                glyphs: match request.mode {
                    ShapingMode::Simple => self.simple.shape_segment(segment, &font, direction),
                    ShapingMode::Complex => self
                        .shape_segment_complex(
                            segment,
                            &font,
                            direction,
                            request.language.as_deref(),
                            request.font_size,
                        )
                        .unwrap_or_else(|| self.simple.shape_segment(segment, &font, direction)),
                },
            })
            .collect();

        Ok(ShapedText {
            analysis,
            font,
            mode: request.mode,
            runs,
        })
    }

    fn shape_segment_complex(
        &self,
        segment: &TextSegment,
        font: &FontMatch,
        direction: BidiDirection,
        language: Option<&str>,
        font_size: Option<f32>,
    ) -> Option<Vec<GlyphInfo>> {
        let font_path = font.path.as_ref()?;
        let bytes = cached_font_bytes(font_path)?;
        let font_ref = FontRef::from_index(bytes.as_slice(), font.face_index.unwrap_or(0)).ok()?;
        let shaper_data = ShaperData::new(&font_ref);
        let shaper = shaper_data.shaper(&font_ref).build();

        let mut buffer = UnicodeBuffer::new();
        buffer.push_str(&segment.text);
        buffer.guess_segment_properties();
        buffer.set_direction(convert_direction(direction));
        if let Some(language) = language.and_then(|value| Language::from_str(value).ok()) {
            buffer.set_language(language);
        }

        let glyph_buffer = shaper.shape(buffer, &[]);
        let units_per_em = shaper.units_per_em().max(1) as f32;
        let scale = font_size
            .filter(|size| size.is_finite() && *size > 0.0)
            .unwrap_or(1.0)
            / units_per_em;
        let glyph_infos = glyph_buffer.glyph_infos();
        let glyph_positions = glyph_buffer.glyph_positions();
        if glyph_infos.len() != glyph_positions.len() {
            return None;
        }

        Some(
            glyph_infos
                .iter()
                .zip(glyph_positions.iter())
                .map(|(info, position)| GlyphInfo {
                    glyph_id: info.glyph_id,
                    cluster: info.cluster as usize,
                    x_advance: position.x_advance as f32 * scale,
                    y_advance: position.y_advance as f32 * scale,
                    x_offset: position.x_offset as f32 * scale,
                    y_offset: position.y_offset as f32 * scale,
                })
                .collect(),
        )
    }
}

fn convert_direction(direction: BidiDirection) -> Direction {
    match direction {
        BidiDirection::RightToLeft | BidiDirection::WeakRightToLeft => Direction::RightToLeft,
        _ => Direction::LeftToRight,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_fonts::{FontProviderKind, FontconfigProvider, NullFontProvider};

    #[test]
    fn shape_engine_produces_one_run_for_single_line_text() {
        let engine = ShapeEngine::new();
        let provider = NullFontProvider;
        let shaped = engine
            .shape_text(&provider, &ShapeRequest::new("hello", "Sans"))
            .expect("shaping should succeed");

        assert_eq!(shaped.runs.len(), 1);
        assert_eq!(shaped.runs[0].glyphs.len(), 5);
        assert_eq!(shaped.font.provider, FontProviderKind::Null);
    }

    #[test]
    fn shape_engine_splits_runs_on_mandatory_breaks() {
        let engine = ShapeEngine::new();
        let provider = NullFontProvider;
        let shaped = engine
            .shape_text(&provider, &ShapeRequest::new("a\nb", "Sans"))
            .expect("shaping should succeed");

        assert_eq!(shaped.runs.len(), 2);
        assert_eq!(shaped.runs[0].text, "a\n");
        assert_eq!(shaped.runs[1].text, "b");
    }

    #[test]
    fn complex_shaping_uses_resolved_font_path() {
        let engine = ShapeEngine::new();
        let provider = FontconfigProvider::new();
        let shaped = engine
            .shape_text(
                &provider,
                &ShapeRequest::new("office", "sans")
                    .with_language("en")
                    .with_mode(ShapingMode::Complex),
            )
            .expect("complex shaping should succeed");

        assert_eq!(shaped.mode, ShapingMode::Complex);
        assert!(!shaped.runs.is_empty());
        assert!(!shaped.runs[0].glyphs.is_empty());
        assert!(shaped.font.path.is_some());
    }
}
