use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum FontBytesSource {
    Virtual,
    File {
        len: u64,
        modified: Option<SystemTime>,
    },
}

#[derive(Clone, Debug)]
struct CachedFontBytes {
    bytes: Arc<Vec<u8>>,
    identity: u64,
    source: FontBytesSource,
}

static FONT_BYTES_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedFontBytes>>> = OnceLock::new();

fn font_bytes_cache() -> &'static Mutex<HashMap<PathBuf, CachedFontBytes>> {
    FONT_BYTES_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bytes_identity(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn file_source(path: &Path) -> Option<FontBytesSource> {
    let metadata = fs::metadata(path).ok()?;
    Some(FontBytesSource::File {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// Register a virtual font file in memory.
///
/// This is primarily used by wasm/browser hosts that do not have a real
/// filesystem/fontconfig database. Callers can return the same virtual `path`
/// from their `FontProvider`; shaping and rasterization will then load bytes
/// from this cache instead of `std::fs`.
pub fn register_virtual_font_bytes(path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) {
    let bytes = Arc::new(bytes.into());
    font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned")
        .insert(
            path.into(),
            CachedFontBytes {
                identity: bytes_identity(bytes.as_slice()),
                bytes,
                source: FontBytesSource::Virtual,
            },
        );
}

/// Look up previously registered virtual font bytes.
pub fn virtual_font_bytes(path: &Path) -> Option<Arc<Vec<u8>>> {
    font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned")
        .get(path)
        .filter(|entry| entry.source == FontBytesSource::Virtual)
        .map(|entry| entry.bytes.clone())
}

fn cached_font_bytes(path: &Path) -> Option<Arc<Vec<u8>>> {
    let source = file_source(path);
    {
        let cache = font_bytes_cache()
            .lock()
            .expect("font bytes cache mutex poisoned");
        if let Some(entry) = cache.get(path) {
            if entry.source == FontBytesSource::Virtual || source.as_ref() == Some(&entry.source) {
                return Some(entry.bytes.clone());
            }
        }
    }

    let bytes = Arc::new(fs::read(path).ok()?);
    let mut cache = font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned");
    // A concurrent virtual registration owns this path and must not be
    // replaced by a filesystem read that started just before registration.
    if let Some(entry) = cache
        .get(path)
        .filter(|entry| entry.source == FontBytesSource::Virtual)
    {
        return Some(entry.bytes.clone());
    }
    cache.insert(
        path.to_path_buf(),
        CachedFontBytes {
            identity: bytes_identity(bytes.as_slice()),
            bytes: bytes.clone(),
            source: source?,
        },
    );
    Some(bytes)
}

/// Return a content identity for the font currently registered or stored at
/// `path`. The identity changes when virtual bytes are replaced and when a
/// filesystem font's size or modification time changes.
///
/// Raster caches use this together with the path, provider, and face index so
/// a bitmap can never be reused for a different font payload.
pub fn font_bytes_identity(path: &Path) -> Option<u64> {
    cached_font_bytes(path)?;
    font_bytes_cache()
        .lock()
        .expect("font bytes cache mutex poisoned")
        .get(path)
        .map(|entry| entry.identity)
}

use harfrust::{Direction, Feature, FontRef, Language, ShaperData, UnicodeBuffer};
use rassa_core::RassaResult;
use rassa_fonts::{
    FontMatch, FontProvider, FontQuery, font_face_glyph_index, font_face_uses_legacy_charmap,
};
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
    /// Script Info `Kerning`; libass forwards this to HarfBuzz's `kern`
    /// feature for every event.
    pub kerning: bool,
    /// Whether this is an ASS vertical (`@font`) run. libass explicitly
    /// enables the OpenType `vert` and `vkna` substitutions for these runs.
    pub vertical: bool,
    /// Non-default horizontal ASS spacing disables standard/contextual
    /// ligatures in libass so the added spacing remains per character.
    pub horizontal_spacing: bool,
    /// Pre-resolved embedding levels from the complete ASS event line. This
    /// lets whole-text layout keep one bidi paragraph across style/font runs.
    pub resolved_bidi_levels: Option<Vec<u8>>,
    /// Keep runs in logical order so the layout layer can apply L2 once over
    /// all style/font chunks of a whole-text paragraph.
    pub defer_visual_reorder: bool,
    /// Enable Unicode paired-bracket resolution. libass exposes this as the
    /// opt-in `ASS_FEATURE_BIDI_BRACKETS` compatibility feature.
    pub bidi_brackets: bool,
    /// Base paragraph direction; libass forces LTR unless \fe-1 requests
    /// auto-detection (ass_resolve_base_direction).
    pub base_direction: Option<BidiDirection>,
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
            kerning: true,
            vertical: false,
            horizontal_spacing: false,
            resolved_bidi_levels: None,
            defer_visual_reorder: false,
            bidi_brackets: false,
            base_direction: None,
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

    pub fn with_base_direction(mut self, base_direction: BidiDirection) -> Self {
        self.base_direction = Some(base_direction);
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size.is_finite().then_some(font_size.max(0.0));
        self
    }

    pub fn with_kerning(mut self, kerning: bool) -> Self {
        self.kerning = kerning;
        self
    }

    pub fn with_vertical(mut self, vertical: bool) -> Self {
        self.vertical = vertical;
        self
    }

    pub fn with_horizontal_spacing(mut self, horizontal_spacing: bool) -> Self {
        self.horizontal_spacing = horizontal_spacing;
        self
    }

    pub fn with_resolved_bidi_levels(mut self, levels: Vec<u8>) -> Self {
        self.resolved_bidi_levels = Some(levels);
        self
    }

    pub fn with_deferred_visual_reorder(mut self, defer: bool) -> Self {
        self.defer_visual_reorder = defer;
        self
    }

    pub fn with_bidi_brackets(mut self, enable: bool) -> Self {
        self.bidi_brackets = enable;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlyphInfo {
    pub glyph_id: u32,
    pub cluster: usize,
    /// Whether libass would apply `DECO_ROTATE` to this glyph in an ASS
    /// vertical (`@font`) run. This follows the source Unicode cluster rather
    /// than the shaped glyph ID, whose cmap relationship may no longer be
    /// recoverable after substitutions or ligation.
    pub vertical_rotation_eligible: bool,
    pub x_advance: f32,
    pub y_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    /// Whether advances and offsets came from a real shaping engine and must
    /// take precedence over nominal metrics reported by the raster backend.
    pub positioning: GlyphPositioning,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GlyphPositioning {
    #[default]
    Nominal,
    Shaped,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    pub text: String,
    pub char_range: std::ops::Range<usize>,
    pub byte_range: std::ops::Range<usize>,
    pub direction: BidiDirection,
    pub bidi_level: u8,
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
        font_size: Option<f32>,
    ) -> Vec<GlyphInfo>;
}

#[derive(Default)]
pub struct SimpleShaper;

impl Shaper for SimpleShaper {
    fn shape_segment(
        &self,
        segment: &TextSegment,
        font: &FontMatch,
        direction: BidiDirection,
        font_size: Option<f32>,
    ) -> Vec<GlyphInfo> {
        let char_count = segment.text.chars().count();
        let mut glyphs = Vec::with_capacity(char_count);
        let font_bytes = font.path.as_ref().and_then(|path| cached_font_bytes(path));
        let face = font_bytes.as_ref().and_then(|bytes| {
            ttf_parser::Face::parse(bytes.as_slice(), font.face_index.unwrap_or(0)).ok()
        });
        let scale = face.as_ref().and_then(|face| {
            let gdi_height = gdi_font_height_face(face)?;
            Some(font_size.filter(|size| size.is_finite() && *size > 0.0)? / gdi_height)
        });
        let glyph = |cluster: usize, character: char| {
            let metrics = face.as_ref().map(|face| {
                let glyph_id =
                    font_face_glyph_index(face, character).unwrap_or(ttf_parser::GlyphId(0));
                let advance = face.glyph_hor_advance(glyph_id).unwrap_or(0) as f32;
                (glyph_id.0 as u32, advance * scale.unwrap_or(1.0))
            });
            let (glyph_id, x_advance) = metrics.unwrap_or((character as u32, 1.0));
            GlyphInfo {
                glyph_id,
                cluster,
                vertical_rotation_eligible: vertical_rotation_eligible(character),
                x_advance,
                y_advance: 0.0,
                x_offset: 0.0,
                y_offset: 0.0,
                positioning: GlyphPositioning::Nominal,
            }
        };
        match direction {
            BidiDirection::RightToLeft | BidiDirection::WeakRightToLeft => {
                let characters = segment.text.chars().collect::<Vec<_>>();
                for (cluster, character) in characters.into_iter().enumerate().rev() {
                    glyphs.push(glyph(cluster, character));
                }
            }
            _ => {
                for (cluster, character) in segment.text.chars().enumerate() {
                    glyphs.push(glyph(cluster, character));
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
        let mut analysis = self.unicode.analyze_text_with_base_and_brackets(
            &request.text,
            request.language.as_deref(),
            request.base_direction.unwrap_or(BidiDirection::Neutral),
            request.bidi_brackets,
        )?;
        if let Some(levels) = request
            .resolved_bidi_levels
            .as_ref()
            .filter(|levels| levels.len() == request.text.chars().count())
        {
            analysis.bidi_analysis.embedding_levels.clone_from(levels);
        }
        let font = provider.resolve(&FontQuery {
            family: request.family.clone(),
            style: request.style.clone(),
            weight: request.weight,
        });
        let mut runs = Vec::new();
        for segment in &analysis.segments {
            // FriBidi/libass shapes each resolved embedding-level run in its
            // own direction, then applies UAX #9 L2 visual ordering. Shaping
            // an entire mixed paragraph in its first-strong direction loses
            // both RTL glyph order and the placement of nested number runs.
            let mut bidi_segments = logical_bidi_segments(segment, &analysis);
            if !request.defer_visual_reorder {
                reorder_bidi_runs(&mut bidi_segments);
            }
            for (bidi_segment, level) in bidi_segments {
                let direction = if level & 1 == 0 {
                    BidiDirection::LeftToRight
                } else {
                    BidiDirection::RightToLeft
                };
                let glyphs = match request.mode {
                    ShapingMode::Simple => self.simple.shape_segment(
                        &bidi_segment,
                        &font,
                        direction,
                        request.font_size,
                    ),
                    ShapingMode::Complex => self
                        .shape_segment_complex(
                            &bidi_segment,
                            &font,
                            direction,
                            request.language.as_deref(),
                            request.font_size,
                            request.kerning,
                            request.vertical,
                            request.horizontal_spacing,
                        )
                        .unwrap_or_else(|| {
                            self.simple.shape_segment(
                                &bidi_segment,
                                &font,
                                direction,
                                request.font_size,
                            )
                        }),
                };
                runs.push(ShapedRun {
                    text: bidi_segment.text,
                    char_range: bidi_segment.char_range,
                    byte_range: bidi_segment.byte_range,
                    direction,
                    bidi_level: level,
                    font: font.clone(),
                    glyphs,
                });
            }
        }

        Ok(ShapedText {
            analysis,
            font,
            mode: request.mode,
            runs,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_segment_complex(
        &self,
        segment: &TextSegment,
        font: &FontMatch,
        direction: BidiDirection,
        language: Option<&str>,
        font_size: Option<f32>,
        kerning: bool,
        vertical: bool,
        horizontal_spacing: bool,
    ) -> Option<Vec<GlyphInfo>> {
        let font_path = font.path.as_ref()?;
        let bytes = cached_font_bytes(font_path)?;
        let face = ttf_parser::Face::parse(bytes.as_slice(), font.face_index.unwrap_or(0)).ok()?;
        // HarfBuzz-compatible engines operate on Unicode cmap semantics and
        // cannot reproduce FreeType/libass's codepage remap for a face whose
        // only Microsoft cmap is legacy. Route this uncommon case through the
        // glyph-ID-aware simple path; it still uses the face's real advances.
        if font_face_uses_legacy_charmap(&face) {
            return None;
        }
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

        let features = libass_run_features(kerning, vertical, horizontal_spacing);
        let glyph_buffer = shaper.shape(buffer, &features);
        // VSFilter/libass scale the em against the GDI font height
        // (FT_SIZE_REQUEST_TYPE_REAL_DIM after set_font_metrics), not
        // units_per_em, so an ASS font size means "line height".
        let units_per_em = shaper.units_per_em().max(1) as f32;
        let gdi_height = gdi_font_height_units(bytes.as_slice(), font.face_index.unwrap_or(0))
            .unwrap_or(units_per_em);
        let scale = font_size
            .filter(|size| size.is_finite() && *size > 0.0)
            .unwrap_or(1.0)
            / gdi_height;
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
                    vertical_rotation_eligible: segment
                        .text
                        .get(info.cluster as usize..)
                        .and_then(|text| text.chars().next())
                        .is_some_and(vertical_rotation_eligible),
                    x_advance: position.x_advance as f32 * scale,
                    y_advance: position.y_advance as f32 * scale,
                    x_offset: position.x_offset as f32 * scale,
                    y_offset: position.y_offset as f32 * scale,
                    positioning: GlyphPositioning::Shaped,
                })
                .collect(),
        )
    }
}

const LIBASS_VERTICAL_ROTATION_LOWER_BOUND: u32 = 0x02F1;

fn vertical_rotation_eligible(character: char) -> bool {
    u32::from(character) >= LIBASS_VERTICAL_ROTATION_LOWER_BOUND
}

fn logical_bidi_segments(
    segment: &TextSegment,
    analysis: &UnicodeAnalysis,
) -> Vec<(TextSegment, u8)> {
    let char_count = segment.text.chars().count();
    if char_count == 0 {
        return Vec::new();
    }
    let fallback_level = u8::from(matches!(
        analysis.bidi_analysis.direction,
        BidiDirection::RightToLeft | BidiDirection::WeakRightToLeft
    ));
    let levels = (0..char_count)
        .map(|offset| {
            analysis
                .bidi_analysis
                .embedding_levels
                .get(segment.char_range.start + offset)
                .copied()
                .unwrap_or(fallback_level)
        })
        .collect::<Vec<_>>();
    let byte_offsets = segment
        .text
        .char_indices()
        .map(|(offset, _)| offset)
        .chain(std::iter::once(segment.text.len()))
        .collect::<Vec<_>>();

    let mut logical = Vec::new();
    let mut start = 0;
    while start < char_count {
        let level = levels[start];
        let mut end = start + 1;
        while end < char_count && levels[end] == level {
            end += 1;
        }
        let local_bytes = byte_offsets[start]..byte_offsets[end];
        logical.push((
            TextSegment {
                text: segment.text[local_bytes.clone()].to_string(),
                byte_range: (segment.byte_range.start + local_bytes.start)
                    ..(segment.byte_range.start + local_bytes.end),
                char_range: (segment.char_range.start + start)..(segment.char_range.start + end),
                line_breaks: segment.line_breaks[start..end].to_vec(),
                word_breaks: segment.word_breaks[start..end].to_vec(),
            },
            level,
        ));
        start = end;
    }

    logical
}

/// UAX #9 L2 over level-tagged runs. Keeping the exact level (rather than
/// only direction parity) handles nested LTR number runs inside RTL text.
pub fn reorder_bidi_runs<T>(runs: &mut [(T, u8)]) {
    let Some(lowest_odd) = runs
        .iter()
        .map(|(_, level)| *level)
        .filter(|level| level & 1 == 1)
        .min()
    else {
        return;
    };
    let highest = runs.iter().map(|(_, level)| *level).max().unwrap_or(0);
    for threshold in (lowest_odd..=highest).rev() {
        let mut index = 0;
        while index < runs.len() {
            if runs[index].1 < threshold {
                index += 1;
                continue;
            }
            let start = index;
            while index < runs.len() && runs[index].1 >= threshold {
                index += 1;
            }
            runs[start..index].reverse();
        }
    }
}

/// Standard OpenType feature policy from libass `set_run_features` and
/// `ass_shaper_set_kerning`.
fn libass_run_features(kerning: bool, vertical: bool, horizontal_spacing: bool) -> [Feature; 5] {
    let feature = |tag: &str, enabled: bool| {
        Feature::from_str(&format!("{tag}={}", u8::from(enabled)))
            .expect("static OpenType feature tag is valid")
    };
    [
        feature("vert", vertical),
        feature("vkna", vertical),
        feature("kern", kerning),
        feature("liga", !horizontal_spacing),
        feature("clig", !horizontal_spacing),
    ]
}

/// Font height in design units the way GDI (and libass set_font_metrics)
/// derives it: OS/2 usWinAscent + usWinDescent read as signed shorts,
/// falling back to the hhea metrics, the typo metrics, and the head bbox.
fn gdi_font_height_units(bytes: &[u8], face_index: u32) -> Option<f32> {
    let face = ttf_parser::Face::parse(bytes, face_index).ok()?;
    gdi_font_height_face(&face)
}

fn gdi_font_height_face(face: &ttf_parser::Face<'_>) -> Option<f32> {
    if let Some(os2) = face.tables().os2 {
        let win_height = i32::from(os2.windows_ascender()) - i32::from(os2.windows_descender());
        if win_height != 0 {
            return Some(win_height as f32);
        }
    }
    let hhea = face.tables().hhea;
    let hhea_height = i32::from(hhea.ascender) - i32::from(hhea.descender);
    if hhea_height != 0 {
        return Some(hhea_height as f32);
    }
    if let Some(os2) = face.tables().os2 {
        let typo_height =
            i32::from(os2.typographic_ascender()) - i32::from(os2.typographic_descender());
        if typo_height != 0 {
            return Some(typo_height as f32);
        }
    }
    let bbox = face.global_bounding_box();
    let bbox_height = i32::from(bbox.y_max) - i32::from(bbox.y_min);
    (bbox_height != 0).then_some(bbox_height as f32)
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
    use rassa_fonts::{
        DefaultFontFileProvider, FontProviderKind, FontconfigProvider, NullFontProvider,
    };

    fn official_nonunicode_fixture(name: &str) -> Option<PathBuf> {
        let path = Path::new("/tmp/rassa-libass-tests/regression/font_nonunicode").join(name);
        path.is_file().then_some(path)
    }

    fn official_dialogue_text(name: &str) -> Option<String> {
        let path = official_nonunicode_fixture(name)?;
        fs::read_to_string(path)
            .ok()?
            .lines()
            .find(|line| line.starts_with("Dialogue:"))?
            .splitn(10, ',')
            .nth(9)
            .map(str::to_owned)
    }

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
    fn mixed_bidi_text_is_split_shaped_and_reordered_by_embedding_level() {
        let engine = ShapeEngine::new();
        let provider = NullFontProvider;
        let shaped = engine
            .shape_text(
                &provider,
                &ShapeRequest::new("abc אבג 123", "Sans")
                    .with_base_direction(BidiDirection::Neutral),
            )
            .expect("mixed bidi shaping succeeds");

        assert!(
            shaped
                .runs
                .iter()
                .any(|run| run.direction == BidiDirection::RightToLeft),
            "a resolved RTL level must become an RTL shaping run"
        );
        let visual = shaped
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter_map(|glyph| char::from_u32(glyph.glyph_id))
            .collect::<String>();
        assert_eq!(visual, shaped.analysis.bidi_analysis.visual_text);
        assert_ne!(visual, shaped.analysis.text);
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
        assert!(
            shaped
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .all(|glyph| glyph.positioning == GlyphPositioning::Shaped),
            "Harfrust positions must remain authoritative through rasterization"
        );
    }

    #[test]
    fn vertical_rotation_eligibility_tracks_each_source_cluster_in_mixed_run() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test/font2.otf");
        let provider =
            DefaultFontFileProvider::new(NullFontProvider, path).with_family("BundledAileron");
        let engine = ShapeEngine::new();

        for mode in [ShapingMode::Simple, ShapingMode::Complex] {
            let shaped = engine
                .shape_text(
                    &provider,
                    &ShapeRequest::new("A\u{02F0}\u{02F1}", "BundledAileron")
                        .with_font_size(40.0)
                        .with_vertical(true)
                        .with_mode(mode),
                )
                .expect("mixed vertical run shapes");
            assert_eq!(shaped.runs.len(), 1, "{mode:?}");
            let eligibility = shaped
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .map(|glyph| glyph.vertical_rotation_eligible)
                .collect::<Vec<_>>();

            assert_eq!(eligibility, [false, false, true], "{mode:?}");
        }
    }

    #[test]
    fn replacing_virtual_font_bytes_changes_font_identity() {
        let path = PathBuf::from("virtual://rassa-shape/font-identity-regression.ttf");
        register_virtual_font_bytes(&path, vec![1, 2, 3, 4]);
        let first = font_bytes_identity(&path).expect("registered bytes have an identity");

        register_virtual_font_bytes(&path, vec![4, 3, 2, 1]);
        let second = font_bytes_identity(&path).expect("replacement bytes have an identity");

        assert_ne!(first, second);
        assert_eq!(
            virtual_font_bytes(&path).as_deref().map(Vec::as_slice),
            Some([4, 3, 2, 1].as_slice())
        );
    }

    #[test]
    fn replacing_filesystem_font_bytes_changes_font_identity() {
        let path = std::env::temp_dir().join(format!(
            "rassa-shape-font-identity-{}-{}.bin",
            std::process::id(),
            line!()
        ));
        fs::write(&path, [1, 2, 3]).expect("temporary font payload should be writable");
        let first = font_bytes_identity(&path).expect("file bytes have an identity");

        fs::write(&path, [4, 3, 2, 1]).expect("temporary font payload should be replaceable");
        let second = font_bytes_identity(&path).expect("replacement bytes have an identity");

        let _ = fs::remove_file(&path);
        assert_ne!(first, second);
    }

    #[test]
    fn simple_shaping_uses_font_cmap_and_real_scaled_advance() {
        let engine = ShapeEngine::new();
        let provider = FontconfigProvider::new();
        let shaped = engine
            .shape_text(
                &provider,
                &ShapeRequest::new("A", "Noto Serif")
                    .with_font_size(64.0)
                    .with_mode(ShapingMode::Simple),
            )
            .expect("simple shaping succeeds");
        let Some(path) = shaped.font.path.as_ref() else {
            eprintln!("skipping: Noto Serif is unavailable");
            return;
        };
        let bytes = cached_font_bytes(path).expect("resolved font bytes load");
        let face = ttf_parser::Face::parse(bytes.as_slice(), shaped.font.face_index.unwrap_or(0))
            .expect("resolved face parses");
        let expected_id = face.glyph_index('A').expect("font covers A");
        let expected_advance = face.glyph_hor_advance(expected_id).unwrap() as f32 * 64.0
            / gdi_font_height_face(&face).unwrap();
        let glyph = &shaped.runs[0].glyphs[0];

        assert_eq!(glyph.glyph_id, u32::from(expected_id.0));
        assert_ne!(glyph.glyph_id, u32::from('A'));
        assert!((glyph.x_advance - expected_advance).abs() < 0.001);
        assert_eq!(glyph.positioning, GlyphPositioning::Nominal);
    }

    #[test]
    fn official_nonunicode_regressions_shape_mapped_ids_with_real_metrics() {
        let cases = [
            (
                "legacy-arabic-simplified-SimplifiedArabic.ttf",
                "legacy-arabic-simplified.ass",
                'ج',
                56_u32,
            ),
            (
                "legacy-arabic-traditional-AGACairoRegular.ttf",
                "legacy-arabic-traditional.ass",
                'ج',
                91,
            ),
            ("shiftjis_Reishoreiryu.ttf", "shiftjis.ass", '君', 1439),
            ("big5-hkscs_SingYi-Ultra.ttf", "big5-hkscs.ass", '訐', 2628),
        ];
        let engine = ShapeEngine::new();
        for (font_name, ass_name, sample, expected_sample_id) in cases {
            let (Some(path), Some(text)) = (
                official_nonunicode_fixture(font_name),
                official_dialogue_text(ass_name),
            ) else {
                eprintln!("skipping: official libass compatible_0.17.5 fixture is unavailable");
                return;
            };
            let bytes = fs::read(&path).expect("official fixture remains readable");
            let face = ttf_parser::Face::parse(&bytes, 0).expect("official fixture face parses");
            let scale = 64.0 / gdi_font_height_face(&face).expect("fixture has usable metrics");
            let expected = text
                .chars()
                .map(|character| {
                    let glyph =
                        font_face_glyph_index(&face, character).unwrap_or(ttf_parser::GlyphId(0));
                    (
                        u32::from(glyph.0),
                        face.glyph_hor_advance(glyph).unwrap_or(0) as f32 * scale,
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                font_face_glyph_index(&face, sample).map(|glyph| u32::from(glyph.0)),
                Some(expected_sample_id),
            );

            for mode in [ShapingMode::Simple, ShapingMode::Complex] {
                let provider = DefaultFontFileProvider::new(NullFontProvider, path.clone())
                    .with_family(font_name);
                let shaped = engine
                    .shape_text(
                        &provider,
                        &ShapeRequest::new(&text, font_name)
                            .with_font_size(64.0)
                            .with_mode(mode),
                    )
                    .expect("legacy regression text shapes");
                let actual = shaped
                    .runs
                    .iter()
                    .flat_map(|run| &run.glyphs)
                    .collect::<Vec<_>>();
                assert_eq!(actual.len(), expected.len(), "{ass_name} in {mode:?}");
                for (glyph, (expected_id, expected_advance)) in actual.iter().zip(&expected) {
                    assert_eq!(glyph.glyph_id, *expected_id, "{ass_name} in {mode:?}");
                    assert!(
                        (glyph.x_advance - expected_advance).abs() < 0.001,
                        "{ass_name} must preserve real face metrics in {mode:?}",
                    );
                    assert_eq!(glyph.positioning, GlyphPositioning::Nominal);
                }
            }
        }
    }

    #[test]
    fn libass_run_features_toggle_kerning_vertical_forms_and_spacing_ligatures() {
        let enabled = libass_run_features(true, true, false);
        let expected = ["vert=1", "vkna=1", "kern=1", "liga=1", "clig=1"]
            .map(|value| Feature::from_str(value).expect("test feature parses"));
        assert_eq!(enabled, expected);

        let disabled = libass_run_features(false, false, true);
        let expected = ["vert=0", "vkna=0", "kern=0", "liga=0", "clig=0"]
            .map(|value| Feature::from_str(value).expect("test feature parses"));
        assert_eq!(disabled, expected);
    }

    #[test]
    fn complex_shaping_honors_track_kerning_and_ass_spacing_ligature_policy() {
        let engine = ShapeEngine::new();
        let provider = FontconfigProvider::new();
        let shape = |text: &str, kerning: bool, horizontal_spacing: bool| {
            engine
                .shape_text(
                    &provider,
                    &ShapeRequest::new(text, "Noto Serif")
                        .with_font_size(64.0)
                        .with_kerning(kerning)
                        .with_horizontal_spacing(horizontal_spacing)
                        .with_mode(ShapingMode::Complex),
                )
                .expect("complex shaping succeeds")
        };

        let kerned = shape("AVAV", true, false);
        let unkerned = shape("AVAV", false, false);
        if kerned.font.path.is_none() || unkerned.font.path.is_none() {
            eprintln!("skipping: Noto Serif is unavailable");
            return;
        }
        let width = |shaped: &ShapedText| {
            shaped
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .map(|glyph| glyph.x_advance)
                .sum::<f32>()
        };
        assert!(
            width(&kerned) < width(&unkerned),
            "kern=0 must retain the unkerned AV advances"
        );

        let ligated = shape("office", true, false);
        let spaced = shape("office", true, true);
        let glyph_count = |shaped: &ShapedText| {
            shaped
                .runs
                .iter()
                .map(|run| run.glyphs.len())
                .sum::<usize>()
        };
        assert!(
            glyph_count(&ligated) < glyph_count(&spaced),
            "non-default ASS spacing must disable liga/clig: ligated={} spaced={}",
            glyph_count(&ligated),
            glyph_count(&spaced),
        );
        assert_eq!(glyph_count(&spaced), "office".chars().count());
    }
}
