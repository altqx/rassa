#![allow(dead_code)]

mod crossfont;

use std::{
    cell::Cell,
    collections::{BTreeMap, HashMap},
    marker::PhantomData,
    path::PathBuf,
    rc::Rc,
    sync::{Mutex, OnceLock},
};

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
use freetype::{
    Bitmap, GlyphSlot, Library, Matrix, RenderMode, StrokerLineCap, StrokerLineJoin, Vector,
    face::LoadFlag, ffi,
};

use crate::crossfont::{BitmapBuffer, FontDesc, GlyphIdKey, Rasterize, Size, Style};
use rassa_core::{Point, RassaError, RassaResult, ass};
use rassa_fonts::{FontMatch, FontProviderKind};
use rassa_shape::{GlyphInfo, GlyphPositioning, ShapedRun, font_bytes_identity};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RasterPixelMode {
    Mono,
    #[default]
    Gray,
    Other,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterGlyph {
    pub glyph_id: u32,
    pub cluster: usize,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub left: i32,
    pub top: i32,
    pub offset_x: i32,
    pub offset_y: i32,
    /// Horizontal shaped offset in 26.6 fixed point.
    pub offset_x_26_6: i32,
    /// Vertical shaped offset in 26.6 fixed point, in screen coordinates
    /// (positive values move the glyph down).
    pub offset_y_26_6: i32,
    pub advance_x: i32,
    pub advance_y: i32,
    /// Horizontal advance in 26.6 fixed point.  libass accumulates the pen in
    /// 26.6 units and floors per glyph; rounding each advance to whole pixels
    /// drifts up to half a pixel per glyph across a run.
    pub advance_x_26_6: i32,
    /// Vertical advance in 26.6 fixed point, in font coordinates. Renderers
    /// subtract this from the screen-space pen position.
    pub advance_y_26_6: i32,
    /// Vertical advance in 26.6 fixed point (FT_Glyph_Metrics.vertAdvance),
    /// used by libass for rotated @font glyphs (DECO_ROTATE).
    pub vert_advance_26_6: i32,
    pub pixel_mode: RasterPixelMode,
    pub bitmap: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterOptions {
    pub size_26_6: i32,
    pub hinting: ass::Hinting,
}

/// Quantized affine transform applied to a glyph outline before rasterization.
///
/// ASS text animation needs the scale and the subpixel translation to be part
/// of the raster-cache identity.  Keeping the representation integral also
/// makes cache hits deterministic across platforms and floating-point modes.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct RasterOutlineTransform {
    /// Horizontal outline scale in signed 16.16 fixed point.
    pub scale_x_16_16: i32,
    /// Vertical outline scale in signed 16.16 fixed point.
    pub scale_y_16_16: i32,
    /// Horizontal translation in FreeType 26.6 outline coordinates.
    pub translate_x_26_6: i32,
    /// Vertical translation in FreeType 26.6 outline coordinates (y up).
    pub translate_y_26_6: i32,
}

impl RasterOutlineTransform {
    pub const IDENTITY_SCALE_16_16: i32 = 1 << 16;
}

impl Default for RasterOutlineTransform {
    fn default() -> Self {
        Self {
            scale_x_16_16: Self::IDENTITY_SCALE_16_16,
            scale_y_16_16: Self::IDENTITY_SCALE_16_16,
            translate_x_26_6: 0,
            translate_y_26_6: 0,
        }
    }
}

/// A transformed glyph bitmap together with its absolute output destination.
///
/// The integer destination is deliberately kept out of the glyph-cache key.
/// Only the restored outline matrix and its 1/8-pixel phase affect coverage;
/// moving the same bitmap by whole pixels must remain a cache hit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PositionedRasterGlyph {
    pub glyph: RasterGlyph,
    pub destination: Point,
}

impl Default for RasterOptions {
    fn default() -> Self {
        Self {
            size_26_6: 32 * 64,
            hinting: ass::Hinting::None,
        }
    }
}

#[derive(Default)]
pub struct Rasterizer {
    options: RasterOptions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RasterCacheStats {
    pub glyph_entries: usize,
    pub bitmap_bytes: usize,
}

pub const DEFAULT_GLYPH_CACHE_MAX: usize = 10_000;
pub const DEFAULT_BITMAP_CACHE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterCacheLimits {
    pub glyph_max: usize,
    pub bitmap_max_bytes: usize,
}

impl Default for RasterCacheLimits {
    fn default() -> Self {
        Self {
            glyph_max: DEFAULT_GLYPH_CACHE_MAX,
            bitmap_max_bytes: DEFAULT_BITMAP_CACHE_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RasterCacheContext {
    namespace: u64,
    limits: RasterCacheLimits,
}

thread_local! {
    static RASTER_CACHE_CONTEXT: Cell<RasterCacheContext> =
        const { Cell::new(RasterCacheContext {
            namespace: 0,
            limits: RasterCacheLimits {
                glyph_max: DEFAULT_GLYPH_CACHE_MAX,
                bitmap_max_bytes: DEFAULT_BITMAP_CACHE_BYTES,
            },
        }) };
}

/// Temporarily route raster-cache traffic into a renderer-owned namespace.
/// The guard is deliberately `!Send`; it must be dropped on the thread where
/// it installed the thread-local context.
pub struct RasterCacheScope {
    previous: RasterCacheContext,
    _not_send: PhantomData<Rc<()>>,
}

impl RasterCacheScope {
    pub fn enter(namespace: u64, limits: RasterCacheLimits) -> Self {
        Rasterizer::set_cache_limits(namespace, limits);
        let previous = RASTER_CACHE_CONTEXT
            .with(|context| context.replace(RasterCacheContext { namespace, limits }));
        Self {
            previous,
            _not_send: PhantomData,
        }
    }
}

impl Drop for RasterCacheScope {
    fn drop(&mut self) {
        RASTER_CACHE_CONTEXT.with(|context| context.set(self.previous));
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct GlyphCacheKey {
    namespace: u64,
    font: FontCacheIdentity,
    family: String,
    style: Option<String>,
    synthetic_bold: bool,
    synthetic_italic: bool,
    face_index: Option<u32>,
    glyph_id: u32,
    size_26_6: i32,
    hinting: ass::Hinting,
    outline_transform: Option<RasterOutlineTransform>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct FontCacheIdentity {
    provider: FontProviderKind,
    path: Option<PathBuf>,
    bytes: Option<u64>,
}

impl From<&FontMatch> for FontCacheIdentity {
    fn from(font: &FontMatch) -> Self {
        Self {
            provider: font.provider,
            path: font.path.clone(),
            bytes: font.path.as_deref().and_then(font_bytes_identity),
        }
    }
}

#[derive(Clone, Debug)]
struct CachedGlyph {
    glyph: RasterGlyph,
    last_used: u64,
}

#[derive(Debug, Default)]
struct NamespaceCache {
    limits: RasterCacheLimits,
    stats: RasterCacheStats,
    lru: BTreeMap<u64, GlyphCacheKey>,
}

#[derive(Debug, Default)]
struct GlyphCache {
    entries: HashMap<GlyphCacheKey, CachedGlyph>,
    namespaces: HashMap<u64, NamespaceCache>,
    access_clock: u64,
}

static GLYPH_CACHE: OnceLock<Mutex<GlyphCache>> = OnceLock::new();

impl Rasterizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: RasterOptions) -> Self {
        Self { options }
    }

    pub fn rasterize(&self, glyphs: &[GlyphInfo]) -> Vec<RasterGlyph> {
        glyphs
            .iter()
            .map(|glyph| RasterGlyph {
                glyph_id: glyph.glyph_id,
                cluster: glyph.cluster,
                offset_x: glyph.x_offset.round() as i32,
                offset_y: (-glyph.y_offset).round() as i32,
                offset_x_26_6: to_26_6(glyph.x_offset),
                offset_y_26_6: -to_26_6(glyph.y_offset),
                advance_x: glyph.x_advance.round() as i32,
                advance_y: glyph.y_advance.round() as i32,
                advance_x_26_6: to_26_6(glyph.x_advance),
                advance_y_26_6: to_26_6(glyph.y_advance),
                ..RasterGlyph::default()
            })
            .collect()
    }

    pub fn rasterize_glyphs(
        &self,
        font: &FontMatch,
        glyphs: &[GlyphInfo],
    ) -> RassaResult<Vec<RasterGlyph>> {
        rasterize_system_glyphs(font, glyphs, self.options)
    }

    /// Rasterize a positioned identity-transform ASS run in outline space.
    ///
    /// `baseline_positions` are absolute output-space glyph origins after
    /// shaped offsets have been applied.  The implementation mirrors
    /// libass's `quantize_transform`: it restores a cbox-dependent scale,
    /// quantizes the transformed cbox centre to 1/8 pixel, and carries the
    /// first glyph's residual through the rest of this composite/style run.
    /// This keeps slow `\pos` + `\fscx/\fscy` animation smooth without
    /// resampling an already rasterized (and possibly blurred) bitmap.
    pub fn rasterize_positioned_identity_glyphs(
        &self,
        _font: &FontMatch,
        glyphs: &[GlyphInfo],
        baseline_positions: &[(f64, f64)],
        scale_x: f64,
        scale_y: f64,
    ) -> RassaResult<Vec<PositionedRasterGlyph>> {
        if glyphs.len() != baseline_positions.len() {
            return Err(RassaError::new(
                "glyph and positioned-origin counts do not match",
            ));
        }
        if !(scale_x.is_finite() && scale_x > 0.0 && scale_y.is_finite() && scale_y > 0.0) {
            return Err(RassaError::new("invalid outline transform scale"));
        }
        // The exact path below mirrors libass's unhinted `fix_glyph_scaling`
        // pipeline. Hinted rendering loads the face at a scale-dependent size
        // and needs a different matrix normalization, so reject it explicitly
        // instead of exposing subtly incorrect public behavior.
        if self.options.hinting != ass::Hinting::None {
            return Err(RassaError::new(
                "outline-space positioned rasterization requires unhinted glyphs",
            ));
        }

        #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
        if _font.path.is_some() {
            return rasterize_freetype_positioned_identity_glyphs(
                _font,
                glyphs,
                baseline_positions,
                scale_x,
                scale_y,
                self.options,
            );
        }

        Err(RassaError::new(
            "outline-space glyph transforms are unavailable on this raster backend",
        ))
    }

    pub fn rasterize_run(&self, run: &ShapedRun) -> RassaResult<Vec<RasterGlyph>> {
        self.rasterize_glyphs(&run.font, &run.glyphs)
    }

    pub fn outline_glyphs(&self, glyphs: &[RasterGlyph], radius: i32) -> Vec<RasterGlyph> {
        glyphs
            .iter()
            .map(|glyph| expand_outline_xy(glyph, radius, radius))
            .collect()
    }

    /// Anisotropic outline expansion: libass strokes borders with separate
    /// x/y radii (\xbord/\ybord), so the ink may grow on one axis only.
    pub fn outline_glyphs_xy(
        &self,
        glyphs: &[RasterGlyph],
        radius_x: i32,
        radius_y: i32,
    ) -> Vec<RasterGlyph> {
        glyphs
            .iter()
            .map(|glyph| expand_outline_xy(glyph, radius_x, radius_y))
            .collect()
    }

    pub fn rasterize_outline_glyphs(
        &self,
        font: &FontMatch,
        glyphs: &[GlyphInfo],
        radius: i32,
    ) -> RassaResult<Vec<RasterGlyph>> {
        if radius <= 0 {
            return self.rasterize_glyphs(font, glyphs);
        }

        #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
        if let Some(font_path) = font.path.as_ref() {
            let library = Library::init()
                .map_err(|error| RassaError::new(format!("freetype init failed: {error:?}")))?;
            let mut face = library
                .new_face(font_path, font.face_index.unwrap_or(0) as isize)
                .map_err(|error| {
                    RassaError::new(format!(
                        "failed to load font '{}': {error:?}",
                        font_path.display()
                    ))
                })?;
            request_real_dim_size(&mut face, self.options.size_26_6.max(64))?;
            apply_synthetic_style_transform(&face, font.synthetic_italic);
            let stroker = library.new_stroker().map_err(|error| {
                RassaError::new(format!("freetype stroker init failed: {error:?}"))
            })?;
            stroker.set(
                (radius.max(1) * 64).into(),
                StrokerLineCap::Round,
                StrokerLineJoin::Round,
                0,
            );

            let mut load_flags = load_flags_for_hinting(self.options.hinting);
            load_flags.remove(LoadFlag::RENDER);
            let mut outlined = Vec::with_capacity(glyphs.len());
            for glyph in glyphs {
                face.load_glyph(glyph.glyph_id, load_flags)
                    .map_err(|error| {
                        RassaError::new(format!(
                            "failed to load outline glyph {}: {error:?}",
                            glyph.glyph_id
                        ))
                    })?;
                let slot = face.glyph();
                maybe_embolden_slot(slot, font.synthetic_bold);
                let advance = slot.advance();
                let stroked = slot
                    .get_glyph()
                    .and_then(|glyph| glyph.stroke(&stroker))
                    .map_err(|error| {
                        RassaError::new(format!(
                            "failed to stroke outline glyph {}: {error:?}",
                            glyph.glyph_id
                        ))
                    })?;
                let bitmap_glyph =
                    stroked
                        .to_bitmap(RenderMode::Normal, None)
                        .map_err(|error| {
                            RassaError::new(format!(
                                "failed to render outline glyph {}: {error:?}",
                                glyph.glyph_id
                            ))
                        })?;
                let bitmap = bitmap_glyph.bitmap();
                let stride = bitmap.pitch().abs();
                let rasterized = RasterGlyph {
                    glyph_id: glyph.glyph_id,
                    width: bitmap.width(),
                    height: bitmap.rows(),
                    stride,
                    left: bitmap_glyph.left(),
                    top: bitmap_glyph.top(),
                    advance_x: (advance.x >> 6) as i32,
                    advance_y: (advance.y >> 6) as i32,
                    advance_x_26_6: advance.x as i32,
                    advance_y_26_6: advance.y as i32,
                    vert_advance_26_6: slot.metrics().vertAdvance as i32,
                    pixel_mode: classify_pixel_mode(&bitmap),
                    bitmap: copy_bitmap_rows(&bitmap),
                    ..RasterGlyph::default()
                };
                outlined.push(glyph_from_cache(glyph, rasterized));
            }
            return Ok(outlined);
        }

        let glyphs = self.rasterize_glyphs(font, glyphs)?;
        Ok(self.outline_glyphs(&glyphs, radius))
    }

    pub fn blur_glyphs(&self, glyphs: &[RasterGlyph], radius: u32) -> Vec<RasterGlyph> {
        glyphs
            .iter()
            .map(|glyph| blur_glyph(glyph, radius))
            .collect()
    }

    pub fn clear_cache() {
        lock_glyph_cache().clear();
    }

    pub fn clear_cache_namespace(namespace: u64) {
        lock_glyph_cache().clear_namespace(namespace);
    }

    pub fn set_cache_limits(namespace: u64, limits: RasterCacheLimits) {
        lock_glyph_cache().set_limits(namespace, limits);
    }

    pub fn cache_stats() -> RasterCacheStats {
        lock_glyph_cache().stats()
    }

    pub fn cache_stats_for_namespace(namespace: u64) -> RasterCacheStats {
        lock_glyph_cache().stats_for_namespace(namespace)
    }
}

impl GlyphCache {
    fn next_access(&mut self) -> u64 {
        // Avoid duplicate LRU keys after the counter wraps. Reaching this path
        // requires 2^64 cache operations; clearing is deterministic and keeps
        // the cache/accounting invariants intact.
        if self.access_clock == u64::MAX {
            self.clear();
        }
        self.access_clock += 1;
        self.access_clock
    }

    fn get(&mut self, key: &GlyphCacheKey) -> Option<RasterGlyph> {
        let (glyph, previous_access) = self
            .entries
            .get(key)
            .map(|entry| (entry.glyph.clone(), entry.last_used))?;
        let access = self.next_access();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = access;
        }
        if let Some(namespace) = self.namespaces.get_mut(&key.namespace) {
            namespace.lru.remove(&previous_access);
            namespace.lru.insert(access, key.clone());
        }
        Some(glyph)
    }

    fn insert(&mut self, key: GlyphCacheKey, glyph: RasterGlyph, limits: RasterCacheLimits) {
        self.set_limits(key.namespace, limits);
        let bitmap_bytes = glyph.bitmap.len();
        if limits.glyph_max == 0 || bitmap_bytes > limits.bitmap_max_bytes {
            return;
        }

        self.remove(&key);
        let access = self.next_access();
        self.entries.insert(
            key.clone(),
            CachedGlyph {
                glyph,
                last_used: access,
            },
        );
        let namespace = self.namespaces.entry(key.namespace).or_default();
        namespace.stats.glyph_entries += 1;
        namespace.stats.bitmap_bytes = namespace.stats.bitmap_bytes.saturating_add(bitmap_bytes);
        namespace.lru.insert(access, key.clone());
        self.enforce_limits(key.namespace);
    }

    fn remove(&mut self, key: &GlyphCacheKey) -> Option<RasterGlyph> {
        let entry = self.entries.remove(key)?;
        if let Some(namespace) = self.namespaces.get_mut(&key.namespace) {
            namespace.lru.remove(&entry.last_used);
            namespace.stats.glyph_entries = namespace.stats.glyph_entries.saturating_sub(1);
            namespace.stats.bitmap_bytes = namespace
                .stats
                .bitmap_bytes
                .saturating_sub(entry.glyph.bitmap.len());
        }
        Some(entry.glyph)
    }

    fn set_limits(&mut self, namespace: u64, limits: RasterCacheLimits) {
        self.namespaces.entry(namespace).or_default().limits = limits;
        self.enforce_limits(namespace);
    }

    fn enforce_limits(&mut self, namespace_id: u64) {
        loop {
            let eviction = self.namespaces.get(&namespace_id).and_then(|namespace| {
                (namespace.stats.glyph_entries > namespace.limits.glyph_max
                    || namespace.stats.bitmap_bytes > namespace.limits.bitmap_max_bytes)
                    .then(|| namespace.lru.first_key_value().map(|(_, key)| key.clone()))
                    .flatten()
            });
            let Some(key) = eviction else {
                break;
            };
            self.remove(&key);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.namespaces.clear();
        self.access_clock = 0;
    }

    fn clear_namespace(&mut self, namespace: u64) {
        let Some(namespace_cache) = self.namespaces.remove(&namespace) else {
            return;
        };
        for key in namespace_cache.lru.into_values() {
            self.entries.remove(&key);
        }
    }

    fn stats(&self) -> RasterCacheStats {
        self.namespaces
            .values()
            .fold(RasterCacheStats::default(), |mut total, namespace| {
                total.glyph_entries = total
                    .glyph_entries
                    .saturating_add(namespace.stats.glyph_entries);
                total.bitmap_bytes = total
                    .bitmap_bytes
                    .saturating_add(namespace.stats.bitmap_bytes);
                total
            })
    }

    fn stats_for_namespace(&self, namespace: u64) -> RasterCacheStats {
        self.namespaces
            .get(&namespace)
            .map(|namespace| namespace.stats.clone())
            .unwrap_or_default()
    }
}

fn current_raster_cache_context() -> RasterCacheContext {
    RASTER_CACHE_CONTEXT.with(Cell::get)
}

fn glyph_cache() -> &'static Mutex<GlyphCache> {
    GLYPH_CACHE.get_or_init(|| Mutex::new(GlyphCache::default()))
}

fn lock_glyph_cache() -> std::sync::MutexGuard<'static, GlyphCache> {
    glyph_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn to_26_6(value: f32) -> i32 {
    (value * 64.0).round() as i32
}

fn glyph_cache_key(
    font: &FontMatch,
    font_identity: &FontCacheIdentity,
    glyph_id: u32,
    options: RasterOptions,
    outline_transform: Option<RasterOutlineTransform>,
) -> GlyphCacheKey {
    let context = current_raster_cache_context();
    GlyphCacheKey {
        namespace: context.namespace,
        font: font_identity.clone(),
        family: font.family.clone(),
        style: font.style.clone(),
        synthetic_bold: font.synthetic_bold,
        synthetic_italic: font.synthetic_italic,
        face_index: font.face_index,
        glyph_id,
        size_26_6: options.size_26_6,
        hinting: options.hinting,
        outline_transform,
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
/// Mirror libass ass_face_is_postscript (ass_font.c): CFF/Type1-based faces
/// take a gentler synthetic-italic slant than TrueType.
fn face_is_postscript(face: &freetype::Face) -> bool {
    unsafe extern "C" {
        fn FT_Get_Font_Format(face: ffi::FT_Face) -> *const core::ffi::c_char;
    }
    unsafe {
        let ptr = FT_Get_Font_Format(face.raw() as *const ffi::FT_FaceRec as ffi::FT_Face);
        if ptr.is_null() {
            return false;
        }
        matches!(
            core::ffi::CStr::from_ptr(ptr).to_bytes(),
            b"CFF" | b"Type 1" | b"CID Type 1"
        )
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn apply_synthetic_style_transform(face: &freetype::Face, synthetic_italic: bool) {
    if synthetic_italic {
        // Match libass ass_glyph_italicize (ass_font.c): TrueType faces shear by
        // 0x05700 (~tan 18.77deg), PostScript (CFF/Type1) faces by 0x02d24
        // (~tan 10deg).
        let xy = if face_is_postscript(face) {
            0x02d24
        } else {
            0x05700
        };
        let mut matrix = Matrix {
            xx: 0x10000,
            xy,
            yx: 0,
            yy: 0x10000,
        };
        let mut delta = Vector { x: 0, y: 0 };
        face.set_transform(&mut matrix, &mut delta);
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn maybe_embolden_slot(slot: &GlyphSlot, synthetic_bold: bool) {
    if !synthetic_bold {
        return;
    }
    // Match libass ass_glyph_embolden (ass_font.c): emboldening strength is
    // FT_MulFix(units_per_EM, y_scale) / 64, applied to the outline. FreeType's
    // FT_GlyphSlot_Embolden uses /24, which over-emboldens by 64/24 ~= 2.67x.
    unsafe {
        let raw = slot.raw() as *const ffi::FT_GlyphSlotRec as *mut ffi::FT_GlyphSlotRec;
        if (*raw).format != ffi::FT_GLYPH_FORMAT_OUTLINE {
            ffi::FT_GlyphSlot_Embolden(raw);
            return;
        }
        let face = (*raw).face;
        let strength = ffi::FT_MulFix(
            ffi::FT_Long::from((*face).units_per_EM),
            (*(*face).size).metrics.y_scale,
        ) / 64;
        ffi::FT_Outline_Embolden(&mut (*raw).outline, strength);
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn rasterize_freetype_glyphs(
    font: &FontMatch,
    glyphs: &[GlyphInfo],
    options: RasterOptions,
) -> RassaResult<Vec<RasterGlyph>> {
    let font_identity = FontCacheIdentity::from(font);
    let cache_keys = glyphs
        .iter()
        .map(|glyph| glyph_cache_key(font, &font_identity, glyph.glyph_id, options, None))
        .collect::<Vec<_>>();
    let mut cached_glyphs = {
        let mut cache = lock_glyph_cache();
        cache_keys
            .iter()
            .map(|key| cache.get(key))
            .collect::<Vec<_>>()
    };
    if cached_glyphs.iter().all(Option::is_some) {
        return Ok(glyphs
            .iter()
            .zip(cached_glyphs)
            .map(|(glyph, cached)| glyph_from_cache(glyph, cached.expect("checked all cache hits")))
            .collect());
    }

    let font_path = font
        .path
        .as_ref()
        .ok_or_else(|| RassaError::new(format!("font '{}' is unresolved", font.family)))?;
    let library = Library::init()
        .map_err(|error| RassaError::new(format!("freetype init failed: {error:?}")))?;
    let mut face = library
        .new_face(font_path, font.face_index.unwrap_or(0) as isize)
        .map_err(|error| {
            RassaError::new(format!(
                "failed to load font '{}': {error:?}",
                font_path.display()
            ))
        })?;
    request_real_dim_size(&mut face, options.size_26_6.max(64))?;
    apply_synthetic_style_transform(&face, font.synthetic_italic);

    let mut rasterized = Vec::with_capacity(glyphs.len());
    let mut load_flags = load_flags_for_hinting(options.hinting);
    load_flags.remove(LoadFlag::RENDER);
    for ((glyph, cache_key), cached) in glyphs.iter().zip(cache_keys).zip(cached_glyphs.iter_mut())
    {
        if let Some(cached) = cached.take() {
            rasterized.push(glyph_from_cache(glyph, cached));
            continue;
        }

        face.load_glyph(glyph.glyph_id, load_flags)
            .map_err(|error| {
                RassaError::new(format!(
                    "failed to load glyph {}: {error:?}",
                    glyph.glyph_id
                ))
            })?;
        let slot = face.glyph();
        maybe_embolden_slot(slot, font.synthetic_bold);
        let advance = slot.advance();
        let vert_advance = slot.metrics().vertAdvance as i32;
        let rendered = render_slot_to_gray_bitmap(slot, glyph.glyph_id)?;
        let cache_entry = RasterGlyph {
            glyph_id: glyph.glyph_id,
            width: rendered.width,
            height: rendered.height,
            stride: rendered.stride,
            left: rendered.left,
            top: rendered.top,
            offset_y: rendered.offset_y,
            offset_y_26_6: rendered.offset_y * 64,
            advance_x: (advance.x >> 6) as i32,
            advance_y: (advance.y >> 6) as i32,
            advance_x_26_6: advance.x as i32,
            advance_y_26_6: advance.y as i32,
            vert_advance_26_6: vert_advance,
            pixel_mode: RasterPixelMode::Gray,
            bitmap: rendered.bitmap,
            ..RasterGlyph::default()
        };
        let context = current_raster_cache_context();
        lock_glyph_cache().insert(cache_key, cache_entry.clone(), context.limits);
        rasterized.push(glyph_from_cache(glyph, cache_entry));
    }

    Ok(rasterized)
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn rasterize_freetype_positioned_identity_glyphs(
    font: &FontMatch,
    glyphs: &[GlyphInfo],
    baseline_positions: &[(f64, f64)],
    requested_scale_x: f64,
    requested_scale_y: f64,
    options: RasterOptions,
) -> RassaResult<Vec<PositionedRasterGlyph>> {
    let font_identity = FontCacheIdentity::from(font);
    let font_path = font
        .path
        .as_ref()
        .ok_or_else(|| RassaError::new(format!("font '{}' is unresolved", font.family)))?;
    let library = Library::init()
        .map_err(|error| RassaError::new(format!("freetype init failed: {error:?}")))?;
    let mut face = library
        .new_face(font_path, font.face_index.unwrap_or(0) as isize)
        .map_err(|error| {
            RassaError::new(format!(
                "failed to load font '{}': {error:?}",
                font_path.display()
            ))
        })?;
    // libass's unhinted path shapes/rasterizes a 256px outline and folds the
    // requested font size into the bitmap transform (`fix_glyph_scaling`).
    // Loading directly at the final 50–60px size changes cbox-dependent matrix
    // buckets and causes visible mass pops during tiny scale animation.
    let outline_size_26_6 = 256 * 64;
    request_real_dim_size(&mut face, outline_size_26_6)?;
    apply_synthetic_style_transform(&face, font.synthetic_italic);

    let size_factor = f64::from(options.size_26_6.max(64)) / f64::from(256 * 64);
    let requested_scale_x = requested_scale_x * size_factor;
    let requested_scale_y = requested_scale_y * size_factor;
    let cache_options = RasterOptions {
        size_26_6: outline_size_26_6,
        hinting: options.hinting,
    };

    let mut rasterized = Vec::with_capacity(glyphs.len());
    let mut load_flags = load_flags_for_hinting(options.hinting);
    load_flags.remove(LoadFlag::RENDER);
    let mut run_offset_q8 = None::<(f64, f64)>;
    for (glyph, &(baseline_x, baseline_y)) in glyphs.iter().zip(baseline_positions) {
        face.load_glyph(glyph.glyph_id, load_flags)
            .map_err(|error| {
                RassaError::new(format!(
                    "failed to load glyph {}: {error:?}",
                    glyph.glyph_id
                ))
            })?;
        let slot = face.glyph();
        maybe_embolden_slot(slot, font.synthetic_bold);
        let advance = slot.advance();
        let vert_advance = slot.metrics().vertAdvance as i32;
        let raw_slot = slot.raw() as *const ffi::FT_GlyphSlotRec as *mut ffi::FT_GlyphSlotRec;
        if unsafe { (*raw_slot).format } != ffi::FT_GLYPH_FORMAT_OUTLINE {
            return Err(RassaError::new(format!(
                "glyph {} does not expose a transformable outline",
                glyph.glyph_id
            )));
        }
        if unsafe { (*raw_slot).outline.n_points } <= 0
            || unsafe { (*raw_slot).outline.n_contours } <= 0
        {
            continue;
        }

        let mut cbox = ffi::FT_BBox {
            xMin: 0,
            yMin: 0,
            xMax: 0,
            yMax: 0,
        };
        unsafe {
            // libass's bitmap key uses the constructed outline control box,
            // not the raster ink box. FreeType's CBox is the equivalent for
            // the loaded, synthetic-style-adjusted outline.
            ffi::FT_Outline_Get_CBox(&(*raw_slot).outline, &mut cbox);
        }
        let centre_x = (cbox.xMin as f64 + cbox.xMax as f64) * 0.5;
        let centre_y = (cbox.yMin as f64 + cbox.yMax as f64) * 0.5;
        let radius_x = (cbox.xMax as f64 - cbox.xMin as f64) * 0.5 + 64.0;
        let radius_y = (cbox.yMax as f64 - cbox.yMin as f64) * 0.5 + 64.0;
        if !(radius_x.is_finite() && radius_x > 0.0 && radius_y.is_finite() && radius_y > 0.0) {
            return Err(RassaError::new(format!(
                "glyph {} has an invalid outline cbox",
                glyph.glyph_id
            )));
        }

        // POSITION_PRECISION is 8 D6 units in libass. Quantize each diagonal
        // matrix coefficient relative to this glyph's cbox radius, then
        // restore the canonical scale stored by the bitmap cache.
        let qm_x = (requested_scale_x * radius_x / 8.0).round_ties_even();
        let qm_y = (requested_scale_y * radius_y / 8.0).round_ties_even();
        let restored_scale_x = qm_x * 8.0 / radius_x;
        let restored_scale_y = qm_y * 8.0 / radius_y;
        if !(restored_scale_x.is_finite()
            && restored_scale_x > 0.0
            && restored_scale_y.is_finite()
            && restored_scale_y > 0.0)
        {
            return Err(RassaError::new(format!(
                "glyph {} has an invalid restored outline scale",
                glyph.glyph_id
            )));
        }

        // FreeType uses y-up outlines while output positions use screen y
        // down, hence the subtraction on the transformed vertical centre.
        // render_and_combine_glyphs first applies double_to_d6 to the global
        // glyph position, before the outline cbox centre enters the matrix.
        let baseline_x_d6 = (baseline_x * 64.0).round_ties_even();
        let baseline_y_d6 = (baseline_y * 64.0).round_ties_even();
        let centre_global_x_d6 = baseline_x_d6 + requested_scale_x * centre_x;
        let centre_global_y_d6 = baseline_y_d6 - requested_scale_y * centre_y;
        let centre_q8 = (centre_global_x_d6 / 8.0, centre_global_y_d6 / 8.0);
        let offset = run_offset_q8.unwrap_or((0.0, 0.0));
        let qr_x_f = (centre_q8.0 - offset.0).round_ties_even();
        let qr_y_f = (centre_q8.1 - offset.1).round_ties_even();
        if !(qr_x_f.is_finite()
            && qr_y_f.is_finite()
            && qr_x_f >= f64::from(i32::MIN)
            && qr_x_f <= f64::from(i32::MAX)
            && qr_y_f >= f64::from(i32::MIN)
            && qr_y_f <= f64::from(i32::MAX))
        {
            return Err(RassaError::new("positioned glyph centre is out of range"));
        }
        let qr_x = qr_x_f as i32;
        let qr_y = qr_y_f as i32;
        if run_offset_q8.is_none() {
            run_offset_q8 = Some((centre_q8.0 - qr_x_f, centre_q8.1 - qr_y_f));
        }
        let phase_x_d6 = qr_x.rem_euclid(8) * 8;
        let phase_y_d6 = qr_y.rem_euclid(8) * 8;

        let to_fixed_16_16 = |value: f64| -> Option<i32> {
            let fixed = (value * 65536.0).round_ties_even();
            (fixed.is_finite() && fixed >= f64::from(i32::MIN) && fixed <= f64::from(i32::MAX))
                .then_some(fixed as i32)
        };
        let to_d6 = |value: f64| -> Option<i32> {
            let fixed = value.round_ties_even();
            (fixed.is_finite() && fixed >= f64::from(i32::MIN) && fixed <= f64::from(i32::MAX))
                .then_some(fixed as i32)
        };
        let transform = RasterOutlineTransform {
            scale_x_16_16: to_fixed_16_16(restored_scale_x)
                .ok_or_else(|| RassaError::new("horizontal outline scale is out of range"))?,
            scale_y_16_16: to_fixed_16_16(restored_scale_y)
                .ok_or_else(|| RassaError::new("vertical outline scale is out of range"))?,
            translate_x_26_6: to_d6(f64::from(phase_x_d6) - restored_scale_x * centre_x)
                .ok_or_else(|| RassaError::new("horizontal outline phase is out of range"))?,
            // Put the y-up outline centre at the negative screen-space phase.
            translate_y_26_6: to_d6(-f64::from(phase_y_d6) - restored_scale_y * centre_y)
                .ok_or_else(|| RassaError::new("vertical outline phase is out of range"))?,
        };
        let cache_key = glyph_cache_key(
            font,
            &font_identity,
            glyph.glyph_id,
            cache_options,
            Some(transform),
        );
        if let Some(cached) = lock_glyph_cache().get(&cache_key) {
            let destination = Point {
                x: qr_x.div_euclid(8).saturating_add(cached.left),
                y: qr_y.div_euclid(8).saturating_sub(cached.top),
            };
            rasterized.push(PositionedRasterGlyph {
                glyph: cached,
                destination,
            });
            continue;
        }

        let matrix = ffi::FT_Matrix {
            xx: transform.scale_x_16_16.into(),
            xy: 0,
            yx: 0,
            yy: transform.scale_y_16_16.into(),
        };
        unsafe {
            ffi::FT_Outline_Transform(&(*raw_slot).outline, &matrix);
            ffi::FT_Outline_Translate(
                &(*raw_slot).outline,
                transform.translate_x_26_6.into(),
                transform.translate_y_26_6.into(),
            );
        }
        let rendered = render_slot_to_gray_bitmap(slot, glyph.glyph_id)?;
        let cache_entry = RasterGlyph {
            glyph_id: glyph.glyph_id,
            width: rendered.width,
            height: rendered.height,
            stride: rendered.stride,
            left: rendered.left,
            top: rendered.top,
            offset_y: rendered.offset_y,
            offset_y_26_6: rendered.offset_y * 64,
            advance_x: (advance.x >> 6) as i32,
            advance_y: (advance.y >> 6) as i32,
            advance_x_26_6: advance.x as i32,
            advance_y_26_6: advance.y as i32,
            vert_advance_26_6: vert_advance,
            pixel_mode: RasterPixelMode::Gray,
            bitmap: rendered.bitmap,
            ..RasterGlyph::default()
        };
        let context = current_raster_cache_context();
        lock_glyph_cache().insert(cache_key, cache_entry.clone(), context.limits);
        rasterized.push(PositionedRasterGlyph {
            destination: Point {
                x: qr_x.div_euclid(8).saturating_add(cache_entry.left),
                y: qr_y.div_euclid(8).saturating_sub(cache_entry.top),
            },
            glyph: cache_entry,
        });
    }

    Ok(rasterized)
}

fn glyph_from_cache(glyph: &GlyphInfo, cached: RasterGlyph) -> RasterGlyph {
    let shaped_positioning = glyph.positioning == GlyphPositioning::Shaped;
    RasterGlyph {
        cluster: glyph.cluster,
        offset_x: glyph.x_offset.round() as i32,
        offset_y: (-glyph.y_offset).round() as i32 + cached.offset_y,
        offset_x_26_6: to_26_6(glyph.x_offset),
        offset_y_26_6: -to_26_6(glyph.y_offset) + cached.offset_y_26_6,
        advance_x: if shaped_positioning {
            glyph.x_advance.round() as i32
        } else {
            cached.advance_x
        },
        advance_y: if shaped_positioning {
            glyph.y_advance.round() as i32
        } else {
            cached.advance_y
        },
        advance_x_26_6: if shaped_positioning {
            to_26_6(glyph.x_advance)
        } else {
            cached.advance_x_26_6
        },
        advance_y_26_6: if shaped_positioning {
            to_26_6(glyph.y_advance)
        } else {
            cached.advance_y_26_6
        },
        vert_advance_26_6: cached.vert_advance_26_6,
        ..cached
    }
}

fn rasterize_system_glyphs(
    font: &FontMatch,
    glyphs: &[GlyphInfo],
    options: RasterOptions,
) -> RassaResult<Vec<RasterGlyph>> {
    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    if font.path.is_some() {
        return rasterize_freetype_glyphs(font, glyphs, options);
    }

    if font.path.is_none() && font.provider != FontProviderKind::Fontconfig {
        return Ok(Rasterizer::new().rasterize(glyphs));
    }

    #[cfg(target_arch = "wasm32")]
    if font.path.is_none() {
        return Ok(Rasterizer::new().rasterize(glyphs));
    }

    let mut rasterizer = crossfont::Rasterizer::new()
        .map_err(|error| RassaError::new(format!("crossfont init failed: {error:?}")))?;
    let style = font
        .style
        .clone()
        .map(Style::Specific)
        .unwrap_or_else(|| Style::Description {
            slant: crossfont::Slant::Normal,
            weight: crossfont::Weight::Normal,
        });
    let desc = FontDesc::new(font.family.clone(), style);
    let size = Size::from_px((options.size_26_6.max(64) as f32) / 64.0);
    let font_key = if let Some(path) = &font.path {
        rasterizer
            .load_font_path(path, size)
            .or_else(|_| rasterizer.load_font(&desc, size))
    } else {
        rasterizer.load_font(&desc, size)
    }
    .map_err(|error| {
        RassaError::new(format!(
            "failed to load font '{}' with crossfont: {error:?}",
            font.family
        ))
    })?;

    let font_identity = FontCacheIdentity::from(font);
    let mut rasterized = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        let cache_key = glyph_cache_key(font, &font_identity, glyph.glyph_id, options, None);
        if let Some(cached) = lock_glyph_cache().get(&cache_key) {
            rasterized.push(glyph_from_cache(glyph, cached));
            continue;
        }

        let glyph_key = GlyphIdKey {
            glyph_id: glyph.glyph_id,
            font_key,
            size,
        };
        let rendered = rasterizer.get_glyph_id(glyph_key).map_err(|error| {
            RassaError::new(format!(
                "failed to rasterize glyph id {} from font '{}': {error:?}",
                glyph.glyph_id, font.family
            ))
        })?;
        let (bitmap, stride, pixel_mode) =
            crossfont_bitmap_to_gray(rendered.width.max(0) as usize, &rendered.buffer);
        // Preserve the pre-existing simple-shaper fallback on backends that
        // report a zero nominal advance. Complex positioning is reapplied by
        // `glyph_from_cache` below and never depends on these cached values.
        let nominal_advance_x = if rendered.advance.0 != 0 {
            rendered.advance.0
        } else {
            glyph.x_advance.round() as i32
        };
        let nominal_advance_y = if rendered.advance.1 != 0 {
            rendered.advance.1
        } else {
            glyph.y_advance.round() as i32
        };
        let cache_entry = RasterGlyph {
            glyph_id: glyph.glyph_id,
            width: rendered.width,
            height: rendered.height,
            stride,
            left: rendered.left,
            top: rendered.top,
            advance_x: nominal_advance_x,
            advance_x_26_6: nominal_advance_x * 64,
            advance_y: nominal_advance_y,
            advance_y_26_6: nominal_advance_y * 64,
            vert_advance_26_6: 0,
            pixel_mode,
            bitmap,
            ..RasterGlyph::default()
        };
        let context = current_raster_cache_context();
        lock_glyph_cache().insert(cache_key, cache_entry.clone(), context.limits);
        rasterized.push(glyph_from_cache(glyph, cache_entry));
    }

    Ok(rasterized)
}

fn crossfont_bitmap_to_gray(
    width: usize,
    buffer: &BitmapBuffer,
) -> (Vec<u8>, i32, RasterPixelMode) {
    match buffer {
        BitmapBuffer::Rgb(bytes) => {
            let gray = bytes
                .chunks_exact(3)
                .map(|pixel| pixel[0])
                .collect::<Vec<_>>();
            (gray, width as i32, RasterPixelMode::Gray)
        }
        BitmapBuffer::Rgba(bytes) => {
            let gray = bytes
                .chunks_exact(4)
                .map(|pixel| pixel[3])
                .collect::<Vec<_>>();
            (gray, width as i32, RasterPixelMode::Other)
        }
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
pub fn request_real_dim_size(face: &mut freetype::Face, size_26_6: i32) -> RassaResult<()> {
    apply_gdi_font_metrics(face);
    let mut request = ffi::FT_Size_RequestRec {
        size_request_type: ffi::FT_SIZE_REQUEST_TYPE_REAL_DIM,
        width: 0,
        height: size_26_6.into(),
        horiResolution: 0,
        vertResolution: 0,
    };
    let err = unsafe {
        ffi::FT_Request_Size(
            face.raw_mut() as *mut ffi::FT_FaceRec,
            &mut request as ffi::FT_Size_Request,
        )
    };
    if err == 0 {
        Ok(())
    } else {
        Err(RassaError::new(format!(
            "failed to request freetype real-dim size {size_26_6}: {err}"
        )))
    }
}

/// Mirror libass set_font_metrics (ass_font.c): GDI uses OS/2 usWinAscent and
/// usWinDescent as the face ascender/descender, falling back to the typo
/// metrics and finally the face bbox.  Must run before FT_Request_Size so
/// FT_SIZE_REQUEST_TYPE_REAL_DIM scales the em against the win height, which
/// is what makes an ASS font size mean "line height" like VSFilter.
#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
pub fn apply_gdi_font_metrics(face: &mut freetype::Face) {
    let raw = unsafe { &mut *(face.raw_mut() as *mut ffi::FT_FaceRec) };
    let os2 = unsafe {
        ffi::FT_Get_Sfnt_Table(raw as *mut ffi::FT_FaceRec, ffi::ft_sfnt_os2) as *const ffi::TT_OS2
    };
    if !os2.is_null() {
        let os2 = unsafe { &*os2 };
        // libass reads the unsigned spec fields as signed, mirroring GDI.
        let win_ascent = os2.usWinAscent as i16;
        let win_descent = os2.usWinDescent as i16;
        if i32::from(win_ascent) + i32::from(win_descent) != 0 {
            raw.ascender = win_ascent;
            raw.descender = -win_descent;
            raw.height = raw.ascender - raw.descender;
        }
    }
    if raw.ascender - raw.descender == 0 || raw.height == 0 {
        if !os2.is_null() {
            let os2 = unsafe { &*os2 };
            if os2.sTypoAscender - os2.sTypoDescender != 0 {
                raw.ascender = os2.sTypoAscender;
                raw.descender = os2.sTypoDescender;
                raw.height = raw.ascender - raw.descender;
            }
        }
        if raw.ascender - raw.descender == 0 || raw.height == 0 {
            raw.ascender = raw.bbox.yMax.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            raw.descender = raw.bbox.yMin.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            raw.height = raw.ascender - raw.descender;
        }
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn load_flags_for_hinting(hinting: ass::Hinting) -> LoadFlag {
    let base = LoadFlag::RENDER | LoadFlag::NO_BITMAP | LoadFlag::IGNORE_GLOBAL_ADVANCE_WITH;
    match hinting {
        ass::Hinting::None => base | LoadFlag::NO_HINTING,
        ass::Hinting::Light => base | LoadFlag::FORCE_AUTOHINT | LoadFlag::TARGET_LIGHT,
        ass::Hinting::Normal => base | LoadFlag::FORCE_AUTOHINT,
        ass::Hinting::Native => base,
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn classify_pixel_mode(bitmap: &Bitmap) -> RasterPixelMode {
    match bitmap.pixel_mode() {
        Ok(freetype::bitmap::PixelMode::Mono) => RasterPixelMode::Mono,
        Ok(freetype::bitmap::PixelMode::Gray) => RasterPixelMode::Gray,
        _ => RasterPixelMode::Other,
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn copy_bitmap_rows(bitmap: &Bitmap) -> Vec<u8> {
    let stride = bitmap.pitch().unsigned_abs() as usize;
    let rows = bitmap.rows().max(0) as usize;
    let source = bitmap.buffer();
    let mut buffer = vec![0; stride * rows];

    if rows == 0 || stride == 0 || source.is_empty() {
        return buffer;
    }

    if bitmap.pitch() >= 0 {
        buffer.copy_from_slice(source);
    } else {
        for row in 0..rows {
            let src_start = row * stride;
            let dst_start = (rows - 1 - row) * stride;
            buffer[dst_start..dst_start + stride]
                .copy_from_slice(&source[src_start..src_start + stride]);
        }
    }

    buffer
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
struct OutlineBitmap {
    width: i32,
    height: i32,
    stride: i32,
    left: i32,
    top: i32,
    offset_y: i32,
    bitmap: Vec<u8>,
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn render_slot_to_gray_bitmap(slot: &GlyphSlot, glyph_id: u32) -> RassaResult<OutlineBitmap> {
    if slot.outline().is_none() {
        let bitmap = slot.bitmap();
        return Ok(OutlineBitmap {
            width: bitmap.width(),
            height: bitmap.rows(),
            stride: bitmap.pitch().abs(),
            left: slot.bitmap_left(),
            top: slot.bitmap_top(),
            offset_y: 0,
            bitmap: copy_bitmap_rows(&bitmap),
        });
    }

    rasterize_ft_outline(&slot.raw().outline, glyph_id)
}

#[derive(Clone, Copy, Debug)]
struct Point26Dot6 {
    x: i32,
    y: i32,
}

#[derive(Clone, Copy, Debug)]
struct PointF {
    x: f64,
    y: f64,
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn rasterize_ft_outline(outline: &ffi::FT_Outline, glyph_id: u32) -> RassaResult<OutlineBitmap> {
    if outline.n_points <= 0 || outline.n_contours <= 0 {
        return Ok(OutlineBitmap::default());
    }

    let points = unsafe { std::slice::from_raw_parts(outline.points, outline.n_points as usize) };
    let tags = unsafe { std::slice::from_raw_parts(outline.tags, outline.n_points as usize) };
    let contours =
        unsafe { std::slice::from_raw_parts(outline.contours, outline.n_contours as usize) };
    let mut bbox = ffi::FT_BBox {
        xMin: 0,
        yMin: 0,
        xMax: 0,
        yMax: 0,
    };
    let bbox_error = unsafe { ffi::FT_Outline_Get_BBox(outline as *const _ as *mut _, &mut bbox) };
    if bbox_error != 0 {
        return Err(RassaError::new(format!(
            "failed to compute outline bbox for glyph {glyph_id}: {bbox_error}"
        )));
    }

    let x_min = ((bbox.xMin - 1) >> 6) as i32;
    let y_min = ((bbox.yMin - 1) >> 6) as i32;
    let x_max = ((bbox.xMax + 127) >> 6) as i32;
    let y_max = ((bbox.yMax + 127) >> 6) as i32;
    let width = (x_max - x_min).max(0);
    let height = (y_max - y_min).max(0);
    if width == 0 || height == 0 {
        return Ok(OutlineBitmap::default());
    }

    let tile_mask = 15;
    let tile_width = (width + tile_mask) & !tile_mask;
    let tile_height = (height + tile_mask) & !tile_mask;
    let contours = flatten_ft_outline(points, tags, contours)?;

    let stride = tile_width;
    let mut bitmap = rasterize_contours_to_gray(&contours, x_min, y_max, tile_width, tile_height);
    apply_rectilinear_boundary_antialias(
        &mut bitmap,
        &contours,
        x_min,
        y_max,
        tile_width as usize,
        tile_height as usize,
    );

    // Bitmap row r spans glyph-space y in [y_max - r - 1, y_max - r], so the
    // top edge of row 0 sits exactly y_max above the baseline.  top must be
    // y_max itself for callers placing rows at `ascender - top + r`.  The
    // tile-aligned allocation is an internal detail; crop to ink so bitmap
    // extents reflect glyph coverage like libass bitmaps do.
    Ok(trim_outline_bitmap_to_ink(OutlineBitmap {
        width: tile_width,
        height: tile_height,
        stride,
        left: x_min,
        top: y_max,
        offset_y: 0,
        bitmap,
    }))
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn trim_outline_bitmap_to_ink(bitmap: OutlineBitmap) -> OutlineBitmap {
    if bitmap.width <= 0 || bitmap.height <= 0 || bitmap.stride <= 0 {
        return bitmap;
    }
    let stride = bitmap.stride as usize;
    let width = bitmap.width as usize;
    let height = bitmap.height as usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_usize;
    let mut max_y = 0_usize;
    for y in 0..height {
        let row = &bitmap.bitmap[y * stride..y * stride + width];
        for (x, value) in row.iter().enumerate() {
            if *value > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }
    if min_x >= max_x || min_y >= max_y {
        return OutlineBitmap::default();
    }
    if min_x == 0 && min_y == 0 && max_x == width && max_y == height {
        return bitmap;
    }
    let new_width = max_x - min_x;
    let new_height = max_y - min_y;
    let mut trimmed = vec![0_u8; new_width * new_height];
    for y in 0..new_height {
        let src_start = (min_y + y) * stride + min_x;
        trimmed[y * new_width..(y + 1) * new_width]
            .copy_from_slice(&bitmap.bitmap[src_start..src_start + new_width]);
    }
    OutlineBitmap {
        width: new_width as i32,
        height: new_height as i32,
        stride: new_width as i32,
        left: bitmap.left + min_x as i32,
        top: bitmap.top - min_y as i32,
        offset_y: bitmap.offset_y,
        bitmap: trimmed,
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn flatten_ft_outline(
    points: &[ffi::FT_Vector],
    tags: &[i8],
    contours: &[i16],
) -> RassaResult<Vec<Vec<PointF>>> {
    let mut flattened = Vec::new();
    let mut start = 0_usize;
    for &end_raw in contours {
        let end = end_raw as usize;
        if end < start || end >= points.len() {
            return Err(RassaError::new("invalid FreeType outline contour"));
        }
        let contour = flatten_contour(points, tags, start, end);
        if contour.len() >= 3 {
            flattened.push(contour);
        }
        start = end + 1;
    }
    Ok(flattened)
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn flatten_contour(
    points: &[ffi::FT_Vector],
    tags: &[i8],
    start: usize,
    end: usize,
) -> Vec<PointF> {
    let n = end - start + 1;
    if n == 0 {
        return Vec::new();
    }
    let pts: Vec<Point26Dot6> = (start..=end)
        .map(|idx| Point26Dot6 {
            x: points[idx].x as i32,
            y: points[idx].y as i32,
        })
        .collect();
    let kinds: Vec<u8> = (start..=end).map(|idx| (tags[idx] as u8) & 3).collect();

    let first = if kinds[0] == 1 {
        pts[0]
    } else {
        let last = pts[n - 1];
        if kinds[n - 1] == 1 {
            last
        } else {
            midpoint(last, pts[0])
        }
    };
    let mut current = first;
    let mut contour = Vec::new();
    push_point(&mut contour, first);
    let mut i = if kinds[0] == 1 { 1 } else { 0 };

    while i < n {
        let kind = kinds[i];
        let p = pts[i];
        if kind == 1 {
            push_point(&mut contour, p);
            current = p;
            i += 1;
        } else if kind == 0 {
            let next_i = (i + 1) % n;
            let next = pts[next_i];
            let next_kind = kinds[next_i];
            let end_point = if next_kind == 1 {
                next
            } else {
                midpoint(p, next)
            };
            flatten_quadratic(&mut contour, current, p, end_point, 0);
            current = end_point;
            i += if next_kind == 1 { 2 } else { 1 };
        } else {
            let c1 = p;
            let c2_i = (i + 1) % n;
            let end_i = (i + 2) % n;
            if kinds[c2_i] == 2 && kinds[end_i] == 1 {
                flatten_cubic(&mut contour, current, c1, pts[c2_i], pts[end_i], 0);
                current = pts[end_i];
                i += 3;
            } else {
                i += 1;
            }
        }
    }
    if contour.len() > 1
        && contour.last().is_some_and(|point| {
            (point.x - contour[0].x).abs() < f64::EPSILON
                && (point.y - contour[0].y).abs() < f64::EPSILON
        })
    {
        contour.pop();
    }
    contour
}

fn midpoint(a: Point26Dot6, b: Point26Dot6) -> Point26Dot6 {
    Point26Dot6 {
        x: (a.x + b.x) / 2,
        y: (a.y + b.y) / 2,
    }
}

fn push_point(contour: &mut Vec<PointF>, point: Point26Dot6) {
    let point = PointF {
        x: point.x as f64 / 64.0,
        y: point.y as f64 / 64.0,
    };
    if contour.last().is_some_and(|last| {
        (last.x - point.x).abs() < f64::EPSILON && (last.y - point.y).abs() < f64::EPSILON
    }) {
        return;
    }
    contour.push(point);
}

fn flatten_quadratic(
    contour: &mut Vec<PointF>,
    p0: Point26Dot6,
    p1: Point26Dot6,
    p2: Point26Dot6,
    depth: u8,
) {
    if depth >= 12 || quadratic_flat_enough(p0, p1, p2) {
        push_point(contour, p2);
        return;
    }
    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p012 = midpoint(p01, p12);
    flatten_quadratic(contour, p0, p01, p012, depth + 1);
    flatten_quadratic(contour, p012, p12, p2, depth + 1);
}

fn quadratic_flat_enough(p0: Point26Dot6, p1: Point26Dot6, p2: Point26Dot6) -> bool {
    let dx = (p0.x + p2.x - 2 * p1.x).abs();
    let dy = (p0.y + p2.y - 2 * p1.y).abs();
    dx.max(dy) <= 1
}

fn flatten_cubic(
    contour: &mut Vec<PointF>,
    p0: Point26Dot6,
    p1: Point26Dot6,
    p2: Point26Dot6,
    p3: Point26Dot6,
    depth: u8,
) {
    if depth >= 8 {
        push_point(contour, p3);
        return;
    }
    let p01 = midpoint(p0, p1);
    let p12 = midpoint(p1, p2);
    let p23 = midpoint(p2, p3);
    let p012 = midpoint(p01, p12);
    let p123 = midpoint(p12, p23);
    let p0123 = midpoint(p012, p123);
    flatten_cubic(contour, p0, p01, p012, p0123, depth + 1);
    flatten_cubic(contour, p0123, p123, p23, p3, depth + 1);
}

fn rasterize_contours_to_gray(
    contours: &[Vec<PointF>],
    x_min: i32,
    y_max: i32,
    width: i32,
    height: i32,
) -> Vec<u8> {
    let stride = width.max(0) as usize;
    let mut bitmap = vec![0_u8; stride * height.max(0) as usize];
    for row in 0..height {
        let y0 = y_max as f64 - row as f64 - 1.0;
        let y1 = y0 + 1.0;
        for col in 0..width {
            let x0 = x_min as f64 + col as f64;
            let x1 = x0 + 1.0;
            let mut signed_area = 0.0_f64;
            for contour in contours {
                let clipped = clip_polygon_to_rect(contour, x0, y0, x1, y1);
                if clipped.len() >= 3 {
                    signed_area += polygon_signed_area(&clipped);
                }
            }
            let coverage = signed_area.abs().clamp(0.0, 1.0);
            bitmap[(row as usize * stride) + col as usize] = (coverage * 255.0 + 0.5).floor() as u8;
        }
    }
    bitmap
}

fn clip_polygon_to_rect(poly: &[PointF], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<PointF> {
    let clipped = clip_polygon(poly, |p| p.x >= x0, |a, b| vertical_intersection(a, b, x0));
    let clipped = clip_polygon(
        &clipped,
        |p| p.x <= x1,
        |a, b| vertical_intersection(a, b, x1),
    );
    let clipped = clip_polygon(
        &clipped,
        |p| p.y >= y0,
        |a, b| horizontal_intersection(a, b, y0),
    );
    clip_polygon(
        &clipped,
        |p| p.y <= y1,
        |a, b| horizontal_intersection(a, b, y1),
    )
}

fn clip_polygon(
    poly: &[PointF],
    inside: impl Fn(PointF) -> bool,
    intersection: impl Fn(PointF, PointF) -> PointF,
) -> Vec<PointF> {
    if poly.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut prev = *poly.last().expect("checked non-empty");
    let mut prev_inside = inside(prev);
    for &curr in poly {
        let curr_inside = inside(curr);
        if curr_inside != prev_inside {
            push_point_f(&mut out, intersection(prev, curr));
        }
        if curr_inside {
            push_point_f(&mut out, curr);
        }
        prev = curr;
        prev_inside = curr_inside;
    }
    if out.len() > 1
        && out.last().is_some_and(|last| {
            (last.x - out[0].x).abs() < 1e-12 && (last.y - out[0].y).abs() < 1e-12
        })
    {
        out.pop();
    }
    out
}

fn push_point_f(points: &mut Vec<PointF>, point: PointF) {
    if points
        .last()
        .is_some_and(|last| (last.x - point.x).abs() < 1e-12 && (last.y - point.y).abs() < 1e-12)
    {
        return;
    }
    points.push(point);
}

fn vertical_intersection(a: PointF, b: PointF, x: f64) -> PointF {
    if (b.x - a.x).abs() < 1e-12 {
        return PointF { x, y: a.y };
    }
    let t = (x - a.x) / (b.x - a.x);
    PointF {
        x,
        y: a.y + (b.y - a.y) * t,
    }
}

fn horizontal_intersection(a: PointF, b: PointF, y: f64) -> PointF {
    if (b.y - a.y).abs() < 1e-12 {
        return PointF { x: a.x, y };
    }
    let t = (y - a.y) / (b.y - a.y);
    PointF {
        x: a.x + (b.x - a.x) * t,
        y,
    }
}

fn polygon_signed_area(poly: &[PointF]) -> f64 {
    let mut area = 0.0;
    for i in 0..poly.len() {
        let a = poly[i];
        let b = poly[(i + 1) % poly.len()];
        area += a.x * b.y - b.x * a.y;
    }
    area * 0.5
}

fn apply_rectilinear_boundary_antialias(
    bitmap: &mut [u8],
    contours: &[Vec<PointF>],
    x_min: i32,
    y_max: i32,
    width: usize,
    height: usize,
) {
    if width < 3 || height < 3 || bitmap.iter().any(|value| *value != 0 && *value != 255) {
        return;
    }
    let original = bitmap.to_vec();
    let add = |bitmap: &mut [u8], idx: usize, delta: u8| {
        bitmap[idx] = bitmap[idx].saturating_add(delta);
    };
    let sub = |bitmap: &mut [u8], idx: usize, delta: u8| {
        bitmap[idx] = bitmap[idx].saturating_sub(delta);
    };

    for contour in contours {
        for i in 0..contour.len() {
            let a = contour[i];
            let b = contour[(i + 1) % contour.len()];
            if (a.x - b.x).abs() < 1e-9 {
                let col = (a.x.round() as i32 - x_min) as isize;
                let y0 = a.y.min(b.y).round() as i32;
                let y1 = a.y.max(b.y).round() as i32;
                for yy in y0..y1 {
                    let row = (y_max - yy - 1) as isize;
                    if row < 0 || row >= height as isize {
                        continue;
                    }
                    let row = row as usize;
                    let left = col - 1;
                    let right = col;
                    if left >= 0 && right >= 0 && right < width as isize {
                        let li = row * width + left as usize;
                        let ri = row * width + right as usize;
                        match (original[li], original[ri]) {
                            (0, 255) => {
                                add(bitmap, li, 2);
                                sub(bitmap, ri, 2);
                            }
                            (255, 0) => {
                                let delta = if col.rem_euclid(16) == 1 { 2 } else { 1 };
                                sub(bitmap, li, 4 - delta);
                                add(bitmap, ri, delta);
                            }
                            _ => {}
                        }
                    }
                }
            } else if (a.y - b.y).abs() < 1e-9 {
                let y = a.y.round() as i32;
                let x0 = (a.x.min(b.x).round() as i32 - x_min) as isize;
                let x1 = (a.x.max(b.x).round() as i32 - x_min) as isize;
                let start = ((x0 + 15) & !15).max(0) as usize;
                let end = (x1 & !15).min(width as isize) as usize;
                if start >= end || (y > 0 && end - start > 256) {
                    continue;
                }
                let above = (y_max - y - 1) as isize;
                let below = (y_max - y) as isize;
                for col in start..end {
                    if above >= 0 && below >= 0 && below < height as isize {
                        let ai = above as usize * width + col;
                        let bi = below as usize * width + col;
                        match (original[ai], original[bi]) {
                            (0, 255) => {
                                add(bitmap, ai, 2);
                                sub(bitmap, bi, 2);
                            }
                            (255, 0) => {
                                sub(bitmap, ai, 2);
                                add(bitmap, bi, 2);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

// Keep eager bitmap operations within the same default budget libass assigns
// its bitmap cache.  In particular, do not let drawing borders or blur turn a
// valid (but large) vector fill into an unchecked multi-gigabyte allocation.
const MAX_EAGER_BITMAP_BYTES: usize = 128 * 1024 * 1024;

fn empty_bitmap_glyph(glyph: &RasterGlyph) -> RasterGlyph {
    RasterGlyph {
        width: 0,
        height: 0,
        stride: 0,
        bitmap: Vec::new(),
        ..glyph.clone()
    }
}

fn checked_padded_bitmap_dimensions(
    glyph: &RasterGlyph,
    pad_x: usize,
    pad_y: usize,
) -> Option<(usize, usize, usize, usize, usize)> {
    let width = usize::try_from(glyph.width).ok()?;
    let height = usize::try_from(glyph.height).ok()?;
    let stride = usize::try_from(glyph.stride).ok()?;
    let source_len = stride.checked_mul(height)?;
    if width == 0 || height == 0 || stride < width || source_len > glyph.bitmap.len() {
        return None;
    }

    let new_width = width.checked_add(pad_x.checked_mul(2)?)?;
    let new_height = height.checked_add(pad_y.checked_mul(2)?)?;
    i32::try_from(new_width).ok()?;
    i32::try_from(new_height).ok()?;
    let bitmap_len = new_width.checked_mul(new_height)?;
    if bitmap_len > MAX_EAGER_BITMAP_BYTES {
        return None;
    }
    Some((width, height, stride, new_width, new_height))
}

fn zeroed_bitmap(len: usize) -> Option<Vec<u8>> {
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(len).ok()?;
    bitmap.resize(len, 0_u8);
    Some(bitmap)
}

fn expand_outline_xy(glyph: &RasterGlyph, radius_x: i32, radius_y: i32) -> RasterGlyph {
    let radius_x = radius_x.max(0);
    let radius_y = radius_y.max(0);
    if (radius_x <= 0 && radius_y <= 0)
        || glyph.width <= 0
        || glyph.height <= 0
        || glyph.bitmap.is_empty()
    {
        return glyph.clone();
    }

    let radius_x = usize::try_from(radius_x).expect("nonnegative outline radius");
    let radius_y = usize::try_from(radius_y).expect("nonnegative outline radius");
    let Some((width, height, stride, new_width, new_height)) =
        checked_padded_bitmap_dimensions(glyph, radius_x, radius_y)
    else {
        return empty_bitmap_glyph(glyph);
    };
    let Some(mut bitmap) = new_width.checked_mul(new_height).and_then(zeroed_bitmap) else {
        return empty_bitmap_glyph(glyph);
    };
    let rx2 = (radius_x as f64 * radius_x as f64).max(1.0);
    let ry2 = (radius_y as f64 * radius_y as f64).max(1.0);
    for y in 0..height {
        for x in 0..width {
            let value = glyph.bitmap[y * stride + x];
            if value == 0 {
                continue;
            }
            let center_x = x + radius_x;
            let center_y = y + radius_y;
            for outline_y in
                center_y.saturating_sub(radius_y)..=(center_y + radius_y).min(new_height - 1)
            {
                for outline_x in
                    center_x.saturating_sub(radius_x)..=(center_x + radius_x).min(new_width - 1)
                {
                    let dx = (outline_x as i32 - center_x as i32) as f64;
                    let dy = (outline_y as i32 - center_y as i32) as f64;
                    let inside = if radius_x == 0 {
                        dx == 0.0 && dy * dy <= ry2
                    } else if radius_y == 0 {
                        dy == 0.0 && dx * dx <= rx2
                    } else {
                        dx * dx / rx2 + dy * dy / ry2 <= 1.0 + f64::EPSILON
                    };
                    if !inside {
                        continue;
                    }
                    let index = outline_y * new_width + outline_x;
                    bitmap[index] = bitmap[index].max(value);
                }
            }
        }
    }

    RasterGlyph {
        width: i32::try_from(new_width).expect("checked outline width"),
        height: i32::try_from(new_height).expect("checked outline height"),
        stride: i32::try_from(new_width).expect("checked outline stride"),
        left: glyph
            .left
            .saturating_sub(i32::try_from(radius_x).expect("checked outline radius")),
        top: glyph
            .top
            .saturating_add(i32::try_from(radius_y).expect("checked outline radius")),
        bitmap,
        ..glyph.clone()
    }
}

fn blur_glyph(glyph: &RasterGlyph, radius: u32) -> RasterGlyph {
    if radius == 0 || glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
        return glyph.clone();
    }

    let radius = usize::try_from(radius).unwrap_or(usize::MAX);
    let Some((width, height, stride, new_width, new_height)) =
        checked_padded_bitmap_dimensions(glyph, radius, radius)
    else {
        return empty_bitmap_glyph(glyph);
    };
    let Some(mut expanded) = new_width.checked_mul(new_height).and_then(zeroed_bitmap) else {
        return empty_bitmap_glyph(glyph);
    };

    for y in 0..height {
        for x in 0..width {
            expanded[(y + radius) * new_width + x + radius] = glyph.bitmap[y * stride + x];
        }
    }

    let Some(mut bitmap) = zeroed_bitmap(expanded.len()) else {
        return empty_bitmap_glyph(glyph);
    };
    for y in 0..new_height {
        let min_y = y.saturating_sub(radius);
        let max_y = (y + radius).min(new_height - 1);
        for x in 0..new_width {
            let min_x = x.saturating_sub(radius);
            let max_x = (x + radius).min(new_width - 1);
            let mut sum = 0_u64;
            let mut count = 0_u64;
            for sample_y in min_y..=max_y {
                for sample_x in min_x..=max_x {
                    sum += u64::from(expanded[sample_y * new_width + sample_x]);
                    count += 1;
                }
            }
            bitmap[y * new_width + x] = (sum / count.max(1)) as u8;
        }
    }

    RasterGlyph {
        width: i32::try_from(new_width).expect("checked blur width"),
        height: i32::try_from(new_height).expect("checked blur height"),
        stride: i32::try_from(new_width).expect("checked blur stride"),
        left: glyph
            .left
            .saturating_sub(i32::try_from(radius).expect("checked blur radius")),
        top: glyph
            .top
            .saturating_add(i32::try_from(radius).expect("checked blur radius")),
        bitmap,
        ..glyph.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_fonts::FontconfigProvider;
    #[cfg(not(target_arch = "wasm32"))]
    use rassa_fonts::{FontProvider, FontQuery};
    use rassa_shape::{ShapeEngine, ShapeRequest, ShapingMode};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn isolated_cache_scope() -> (u64, RasterCacheScope) {
        static NEXT_TEST_NAMESPACE: AtomicU64 = AtomicU64::new(1 << 63);
        let namespace = NEXT_TEST_NAMESPACE.fetch_add(1, Ordering::Relaxed);
        Rasterizer::clear_cache_namespace(namespace);
        (
            namespace,
            RasterCacheScope::enter(namespace, RasterCacheLimits::default()),
        )
    }

    #[test]
    fn rasterize_run_renders_system_font_bitmaps() {
        let (_namespace, _cache_scope) = isolated_cache_scope();
        let provider = FontconfigProvider::new();
        let shaper = ShapeEngine::new();
        let shaped = shaper
            .shape_text(
                &provider,
                &ShapeRequest::new("Ab", "sans").with_mode(ShapingMode::Complex),
            )
            .expect("shaping should succeed");
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: 24 * 64,
            hinting: ass::Hinting::Normal,
        });
        let glyphs = rasterizer
            .rasterize_run(&shaped.runs[0])
            .expect("rasterization should succeed");

        assert_eq!(glyphs.len(), 2);
        assert!(glyphs.iter().all(|glyph| glyph.width >= 0));
        assert!(glyphs.iter().all(|glyph| glyph.height >= 0));
        assert!(
            glyphs
                .iter()
                .all(|glyph| glyph.bitmap.len() == (glyph.stride * glyph.height) as usize)
        );
        assert!(glyphs.iter().any(|glyph| !glyph.bitmap.is_empty()));
        assert!(
            glyphs
                .iter()
                .any(|glyph| glyph.bitmap.iter().any(|sample| *sample != 0)),
            "system font rasterization should produce non-zero glyph coverage"
        );
        assert!(
            glyphs.iter().any(|glyph| glyph.advance_x > 0),
            "system font rasterization should preserve positive glyph advances"
        );
    }

    #[test]
    fn shaped_positioning_overrides_backend_metrics() {
        let shaped = GlyphInfo {
            glyph_id: 1,
            cluster: 0,
            vertical_rotation_eligible: false,
            x_advance: 17.4,
            y_advance: -2.6,
            x_offset: 1.25,
            y_offset: -0.75,
            positioning: GlyphPositioning::Shaped,
        };
        let rasterized = glyph_from_cache(
            &shaped,
            RasterGlyph {
                advance_x: 23,
                advance_y: 5,
                advance_x_26_6: 23 * 64,
                advance_y_26_6: 5 * 64,
                ..RasterGlyph::default()
            },
        );

        assert_eq!(rasterized.advance_x, 17);
        assert_eq!(rasterized.advance_y, -3);
        assert_eq!(rasterized.advance_x_26_6, to_26_6(17.4));
        assert_eq!(rasterized.advance_y_26_6, to_26_6(-2.6));
        assert_eq!(rasterized.offset_x_26_6, 80);
        assert_eq!(rasterized.offset_y_26_6, 48);
    }

    #[test]
    fn nominal_positioning_keeps_backend_metrics() {
        let nominal = GlyphInfo {
            glyph_id: 1,
            cluster: 0,
            vertical_rotation_eligible: false,
            x_advance: 17.4,
            y_advance: -2.6,
            x_offset: 0.0,
            y_offset: 0.0,
            positioning: GlyphPositioning::Nominal,
        };
        let rasterized = glyph_from_cache(
            &nominal,
            RasterGlyph {
                advance_x: 23,
                advance_y: 5,
                advance_x_26_6: 23 * 64 + 11,
                advance_y_26_6: 5 * 64 + 7,
                ..RasterGlyph::default()
            },
        );

        assert_eq!(rasterized.advance_x, 23);
        assert_eq!(rasterized.advance_y, 5);
        assert_eq!(rasterized.advance_x_26_6, 23 * 64 + 11);
        assert_eq!(rasterized.advance_y_26_6, 5 * 64 + 7);
    }

    fn cache_test_key(namespace: u64, glyph_id: u32) -> GlyphCacheKey {
        GlyphCacheKey {
            namespace,
            font: FontCacheIdentity {
                provider: FontProviderKind::Null,
                path: None,
                bytes: None,
            },
            family: "cache-test".to_owned(),
            style: None,
            synthetic_bold: false,
            synthetic_italic: false,
            face_index: None,
            glyph_id,
            size_26_6: 16 * 64,
            hinting: ass::Hinting::None,
            outline_transform: None,
        }
    }

    fn cache_test_glyph(glyph_id: u32, bitmap_bytes: usize) -> RasterGlyph {
        RasterGlyph {
            glyph_id,
            width: i32::try_from(bitmap_bytes).unwrap_or(i32::MAX),
            height: 1,
            stride: i32::try_from(bitmap_bytes).unwrap_or(i32::MAX),
            bitmap: vec![glyph_id as u8; bitmap_bytes],
            ..RasterGlyph::default()
        }
    }

    #[test]
    fn glyph_cache_evicts_least_recently_used_entry_at_glyph_limit() {
        let namespace = 11;
        let limits = RasterCacheLimits {
            glyph_max: 2,
            bitmap_max_bytes: 100,
        };
        let first = cache_test_key(namespace, 1);
        let second = cache_test_key(namespace, 2);
        let third = cache_test_key(namespace, 3);
        let mut cache = GlyphCache::default();

        cache.insert(first.clone(), cache_test_glyph(1, 3), limits);
        cache.insert(second.clone(), cache_test_glyph(2, 4), limits);
        assert_eq!(cache.get(&first).expect("first glyph cached").glyph_id, 1);
        cache.insert(third.clone(), cache_test_glyph(3, 5), limits);

        assert!(cache.entries.contains_key(&first));
        assert!(!cache.entries.contains_key(&second));
        assert!(cache.entries.contains_key(&third));
        assert_eq!(
            cache.stats_for_namespace(namespace),
            RasterCacheStats {
                glyph_entries: 2,
                bitmap_bytes: 8,
            }
        );
    }

    #[test]
    fn glyph_cache_accounts_bitmap_bytes_and_rejects_oversized_entries() {
        let namespace = 12;
        let limits = RasterCacheLimits {
            glyph_max: 10,
            bitmap_max_bytes: 5,
        };
        let first = cache_test_key(namespace, 1);
        let second = cache_test_key(namespace, 2);
        let oversized = cache_test_key(namespace, 3);
        let mut cache = GlyphCache::default();

        cache.insert(first.clone(), cache_test_glyph(1, 3), limits);
        cache.insert(second.clone(), cache_test_glyph(2, 4), limits);
        cache.insert(oversized.clone(), cache_test_glyph(3, 6), limits);

        assert!(!cache.entries.contains_key(&first));
        assert!(cache.entries.contains_key(&second));
        assert!(!cache.entries.contains_key(&oversized));
        assert_eq!(
            cache.stats_for_namespace(namespace),
            RasterCacheStats {
                glyph_entries: 1,
                bitmap_bytes: 4,
            }
        );
    }

    #[test]
    fn glyph_cache_limits_are_isolated_by_renderer_namespace() {
        let first_namespace = 13;
        let second_namespace = 14;
        let mut cache = GlyphCache::default();
        let one_entry = RasterCacheLimits {
            glyph_max: 1,
            bitmap_max_bytes: 100,
        };
        let two_entries = RasterCacheLimits {
            glyph_max: 2,
            bitmap_max_bytes: 100,
        };

        for glyph_id in 1..=2 {
            cache.insert(
                cache_test_key(first_namespace, glyph_id),
                cache_test_glyph(glyph_id, 2),
                one_entry,
            );
            cache.insert(
                cache_test_key(second_namespace, glyph_id),
                cache_test_glyph(glyph_id, 3),
                two_entries,
            );
        }

        assert_eq!(cache.stats_for_namespace(first_namespace).glyph_entries, 1);
        assert_eq!(cache.stats_for_namespace(second_namespace).glyph_entries, 2);
        cache.set_limits(
            first_namespace,
            RasterCacheLimits {
                glyph_max: 0,
                bitmap_max_bytes: 0,
            },
        );
        assert_eq!(
            cache.stats_for_namespace(first_namespace),
            RasterCacheStats::default()
        );
        assert_eq!(cache.stats_for_namespace(second_namespace).glyph_entries, 2);
    }

    #[test]
    fn rasterize_run_reuses_global_glyph_cache() {
        let (namespace, _cache_scope) = isolated_cache_scope();
        let provider = FontconfigProvider::new();
        let shaper = ShapeEngine::new();
        let shaped = shaper
            .shape_text(&provider, &ShapeRequest::new("A", "sans"))
            .expect("shaping should succeed");
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: 47 * 64,
            hinting: ass::Hinting::Normal,
        });

        let first = rasterizer
            .rasterize_run(&shaped.runs[0])
            .expect("rasterization should succeed");
        let entries_after_first =
            glyph_cache_entries_for_run(namespace, &shaped.runs[0], rasterizer.options);
        let second = rasterizer
            .rasterize_run(&shaped.runs[0])
            .expect("rasterization should succeed");

        assert_eq!(first, second);
        assert!(entries_after_first > 0);
        assert_eq!(
            glyph_cache_entries_for_run(namespace, &shaped.runs[0], rasterizer.options),
            entries_after_first
        );
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    fn positioned_coverage_metrics(glyphs: &[PositionedRasterGlyph]) -> (f64, f64, u64) {
        let mut mass = 0_u64;
        let mut weighted_x = 0.0_f64;
        let mut weighted_y = 0.0_f64;
        for positioned in glyphs {
            let glyph = &positioned.glyph;
            let width = usize::try_from(glyph.width).expect("nonnegative positioned width");
            let height = usize::try_from(glyph.height).expect("nonnegative positioned height");
            let stride = usize::try_from(glyph.stride).expect("nonnegative positioned stride");
            for y in 0..height {
                for x in 0..width {
                    let coverage = u64::from(glyph.bitmap[y * stride + x]);
                    mass += coverage;
                    weighted_x +=
                        (f64::from(positioned.destination.x) + x as f64 + 0.5) * coverage as f64;
                    weighted_y +=
                        (f64::from(positioned.destination.y) + y as f64 + 0.5) * coverage as f64;
                }
            }
        }
        assert!(mass > 0, "positioned glyph probe must retain coverage");
        (weighted_x / mass as f64, weighted_y / mass as f64, mass)
    }

    #[test]
    fn positioned_identity_rejects_hinted_rasterization() {
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: 60 * 64,
            hinting: ass::Hinting::Normal,
        });
        let font = FontMatch {
            family: "hinted-positioned-probe".to_owned(),
            path: None,
            face_index: None,
            style: None,
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Null,
        };
        let error = rasterizer
            .rasterize_positioned_identity_glyphs(&font, &[], &[], 1.0, 1.0)
            .expect_err("the public outline-space API must reject hinted glyphs");
        assert!(error.message().contains("requires unhinted glyphs"));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn positioned_identity_q8_motion_reuses_integer_translated_cache_entries() {
        let (namespace, _cache_scope) = isolated_cache_scope();
        let font = FontMatch {
            family: "positioned-q8-cache".to_owned(),
            path: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
            ),
            face_index: Some(0),
            style: Some("Regular".to_owned()),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        };
        let shaped = ShapeEngine::new()
            .shape_text_with_font(
                &ShapeRequest::new("AV", &font.family)
                    .with_font_size(60.0)
                    .with_mode(ShapingMode::Complex),
                &font,
            )
            .expect("positioned fixture should shape");
        let glyphs = shaped
            .runs
            .first()
            .expect("fixture should produce one shaping run")
            .glyphs
            .clone();
        assert_eq!(glyphs.len(), 2);

        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: 60 * 64,
            hinting: ass::Hinting::None,
        });
        let first_advance = f64::from(glyphs[0].x_advance);
        let mut samples = Vec::new();
        let mut last_positions = Vec::new();
        let mut last_scale = 1.0_f64;
        let mut last_rasterized = Vec::new();
        for step in 0..=16 {
            let anchor_x = 320.25 + f64::from(step) * 0.125;
            let anchor_y = 180.375 + f64::from(step) * 0.03125;
            let scale = 1.0 + f64::from(step) * 0.0005;
            let positions = vec![
                (anchor_x, anchor_y),
                (anchor_x + first_advance * scale, anchor_y),
            ];
            let rasterized = rasterizer
                .rasterize_positioned_identity_glyphs(&font, &glyphs, &positions, scale, scale)
                .expect("positioned outline rasterization should succeed");
            assert_eq!(rasterized.len(), glyphs.len());
            samples.push(positioned_coverage_metrics(&rasterized));
            last_positions = positions;
            last_scale = scale;
            last_rasterized = rasterized;
        }

        for pair in samples.windows(2) {
            let dx = pair[1].0 - pair[0].0;
            let dy = pair[1].1 - pair[0].1;
            let mass_step = pair[1].2 as f64 / pair[0].2 as f64 - 1.0;
            assert!(
                (-0.25..=0.5).contains(&dx),
                "tiny Q8 x motion must not jump by a whole pixel: {pair:?}"
            );
            assert!(
                (-0.25..=0.5).contains(&dy),
                "tiny Q8 y motion must not jump by a whole pixel: {pair:?}"
            );
            assert!(
                mass_step.abs() <= 0.01,
                "tiny outline-scale steps must not pulse coverage mass: {pair:?}"
            );
        }

        let entries_before_integer_shift = Rasterizer::cache_stats_for_namespace(namespace);
        let integer_shifted_positions = last_positions
            .iter()
            .map(|&(x, y)| (x + 2.0, y - 1.0))
            .collect::<Vec<_>>();
        let integer_shifted = rasterizer
            .rasterize_positioned_identity_glyphs(
                &font,
                &glyphs,
                &integer_shifted_positions,
                last_scale,
                last_scale,
            )
            .expect("integer-translated positioned glyphs should rasterize");
        assert_eq!(integer_shifted.len(), last_rasterized.len());
        for (before, after) in last_rasterized.iter().zip(&integer_shifted) {
            assert_eq!(after.glyph.bitmap, before.glyph.bitmap);
            assert_eq!(after.destination.x, before.destination.x + 2);
            assert_eq!(after.destination.y, before.destination.y - 1);
        }
        assert_eq!(
            Rasterizer::cache_stats_for_namespace(namespace),
            entries_before_integer_shift,
            "whole-pixel placement must stay outside the transformed bitmap cache key"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn glyph_cache_separates_distinct_font_sources_with_identical_metadata() {
        let (namespace, _cache_scope) = isolated_cache_scope();
        let fixture = |name: &str| FontMatch {
            family: "cache-identity-collision".to_owned(),
            path: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../rassa-test/fixtures/libass/compare/test")
                    .join(name),
            ),
            face_index: Some(0),
            style: Some("Regular".to_owned()),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        };
        let first_font = fixture("font1.ttf");
        let second_font = fixture("font2.otf");
        let glyph = GlyphInfo {
            glyph_id: 1,
            positioning: GlyphPositioning::Nominal,
            ..GlyphInfo::default()
        };
        let options = RasterOptions {
            size_26_6: 91 * 64,
            hinting: ass::Hinting::None,
        };
        let rasterizer = Rasterizer::with_options(options);

        rasterizer
            .rasterize_glyphs(&first_font, std::slice::from_ref(&glyph))
            .expect("first attached font should rasterize");
        rasterizer
            .rasterize_glyphs(&second_font, std::slice::from_ref(&glyph))
            .expect("second attached font should rasterize");

        let cache = lock_glyph_cache();
        let matching = cache
            .entries
            .keys()
            .filter(|key| {
                key.namespace == namespace
                    && key.family == "cache-identity-collision"
                    && key.glyph_id == glyph.glyph_id
                    && key.size_26_6 == options.size_26_6
            })
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 2);
        assert_ne!(matching[0].font.path, matching[1].font.path);
        assert_ne!(matching[0].font.bytes, matching[1].font.bytes);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn cached_bitmap_reapplies_contextual_shaped_positions() {
        let (namespace, _cache_scope) = isolated_cache_scope();
        let font = FontMatch {
            family: "contextual-positioning-cache".to_owned(),
            path: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
            ),
            face_index: Some(0),
            style: Some("Regular".to_owned()),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        };
        let first_context = GlyphInfo {
            glyph_id: 1,
            cluster: 0,
            vertical_rotation_eligible: false,
            x_advance: 13.25,
            y_advance: 0.5,
            x_offset: 0.25,
            y_offset: -0.75,
            positioning: GlyphPositioning::Shaped,
        };
        let second_context = GlyphInfo {
            glyph_id: 1,
            cluster: 7,
            vertical_rotation_eligible: false,
            x_advance: 9.5,
            y_advance: -0.25,
            x_offset: -1.75,
            y_offset: 1.5,
            positioning: GlyphPositioning::Shaped,
        };
        let options = RasterOptions {
            size_26_6: 93 * 64,
            hinting: ass::Hinting::None,
        };
        let rasterizer = Rasterizer::with_options(options);

        let first = rasterizer
            .rasterize_glyphs(&font, std::slice::from_ref(&first_context))
            .expect("first shaped context should rasterize")
            .remove(0);
        let second = rasterizer
            .rasterize_glyphs(&font, std::slice::from_ref(&second_context))
            .expect("cached shaped context should rasterize")
            .remove(0);

        assert_eq!(
            first.bitmap, second.bitmap,
            "only positioning should differ"
        );
        assert_eq!(first.advance_x_26_6, to_26_6(first_context.x_advance));
        assert_eq!(first.advance_y_26_6, to_26_6(first_context.y_advance));
        assert_eq!(first.offset_x_26_6, to_26_6(first_context.x_offset));
        assert_eq!(first.offset_y_26_6, -to_26_6(first_context.y_offset));
        assert_eq!(second.cluster, second_context.cluster);
        assert_eq!(second.advance_x_26_6, to_26_6(second_context.x_advance));
        assert_eq!(second.advance_y_26_6, to_26_6(second_context.y_advance));
        assert_eq!(second.offset_x_26_6, to_26_6(second_context.x_offset));
        assert_eq!(second.offset_y_26_6, -to_26_6(second_context.y_offset));
        assert_ne!(first.advance_x_26_6, second.advance_x_26_6);

        let cache_entries = lock_glyph_cache()
            .entries
            .keys()
            .filter(|key| {
                key.namespace == namespace
                    && key.family == font.family
                    && key.glyph_id == first_context.glyph_id
                    && key.size_26_6 == options.size_26_6
            })
            .count();
        assert_eq!(
            cache_entries, 1,
            "both contexts must share one bitmap entry"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn harfrust_context_dependent_advance_survives_raster_cache() {
        #[derive(Clone)]
        struct FixedFontProvider(FontMatch);

        impl FontProvider for FixedFontProvider {
            fn resolve(&self, _query: &FontQuery) -> FontMatch {
                self.0.clone()
            }
        }

        let (_namespace, _cache_scope) = isolated_cache_scope();
        let font = FontMatch {
            family: "Aileron".to_owned(),
            path: Some(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
            ),
            face_index: Some(0),
            style: Some("Regular".to_owned()),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        };
        let provider = FixedFontProvider(font.clone());
        let shaper = ShapeEngine::new();
        let shape = |text: &str| {
            shaper
                .shape_text(
                    &provider,
                    &ShapeRequest::new(text, "Aileron")
                        .with_font_size(93.0)
                        .with_mode(ShapingMode::Complex),
                )
                .expect("fixture font should shape")
        };
        let av = shape("AV");
        let aa = shape("AA");
        let av_a = &av.runs[0].glyphs[0];
        let aa_a = &aa.runs[0].glyphs[0];
        assert_eq!(av_a.glyph_id, aa_a.glyph_id);
        assert_eq!(av_a.positioning, GlyphPositioning::Shaped);
        assert_ne!(
            to_26_6(av_a.x_advance),
            to_26_6(aa_a.x_advance),
            "the fixture must expose A's context-dependent GPOS advance"
        );

        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: 93 * 64,
            hinting: ass::Hinting::None,
        });
        let av_raster = rasterizer
            .rasterize_glyphs(&font, std::slice::from_ref(av_a))
            .expect("AV glyph should rasterize")
            .remove(0);
        let aa_raster = rasterizer
            .rasterize_glyphs(&font, std::slice::from_ref(aa_a))
            .expect("AA glyph should reuse the cached bitmap")
            .remove(0);

        assert_eq!(av_raster.bitmap, aa_raster.bitmap);
        assert_eq!(av_raster.advance_x_26_6, to_26_6(av_a.x_advance));
        assert_eq!(aa_raster.advance_x_26_6, to_26_6(aa_a.x_advance));
        assert_ne!(av_raster.advance_x_26_6, aa_raster.advance_x_26_6);
    }

    #[test]
    fn raster_crate_does_not_vendor_libass_c_sources() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        assert!(
            !manifest.join("csrc/libass").exists(),
            "rassa-raster must not vendor libass C sources; implement raster behavior in Rust"
        );
        assert!(
            !manifest.join("csrc/rassa_libass_raster.c").exists(),
            "rassa-raster must not compile a libass C shim"
        );
    }

    #[test]
    fn analytic_rasterizer_fills_integer_aligned_rectangle_exactly() {
        let rect = vec![vec![
            PointF { x: 1.0, y: 1.0 },
            PointF { x: 3.0, y: 1.0 },
            PointF { x: 3.0, y: 3.0 },
            PointF { x: 1.0, y: 3.0 },
        ]];

        let bitmap = rasterize_contours_to_gray(&rect, 0, 4, 4, 4);

        assert_eq!(
            bitmap,
            vec![0, 0, 0, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn analytic_rasterizer_preserves_fractional_rectangle_coverage() {
        let rect = vec![vec![
            PointF { x: 0.5, y: 0.5 },
            PointF { x: 1.5, y: 0.5 },
            PointF { x: 1.5, y: 1.5 },
            PointF { x: 0.5, y: 1.5 },
        ]];

        let bitmap = rasterize_contours_to_gray(&rect, 0, 2, 2, 2);

        assert_eq!(bitmap, vec![64, 64, 64, 64]);
    }

    fn glyph_cache_entries_for_run(
        namespace: u64,
        run: &ShapedRun,
        options: RasterOptions,
    ) -> usize {
        lock_glyph_cache()
            .entries
            .keys()
            .filter(|key| {
                key.namespace == namespace
                    && key.family == run.font.family
                    && key.style == run.font.style
                    && key.size_26_6 == options.size_26_6
                    && key.hinting == options.hinting
            })
            .count()
    }

    #[test]
    fn fallback_rasterize_keeps_placeholder_path() {
        let rasterizer = Rasterizer::new();
        let glyphs = rasterizer.rasterize(&[GlyphInfo {
            glyph_id: 'A' as u32,
            cluster: 0,
            vertical_rotation_eligible: false,
            x_advance: 1.0,
            y_advance: 0.0,
            x_offset: 0.0,
            y_offset: 0.0,
            positioning: GlyphPositioning::Nominal,
        }]);

        assert_eq!(glyphs.len(), 1);
        assert_eq!(glyphs[0].glyph_id, 'A' as u32);
        assert_eq!(glyphs[0].advance_x, 1);
    }

    #[test]
    fn outline_expansion_grows_bitmap_bounds() {
        let rasterizer = Rasterizer::new();
        let glyph = RasterGlyph {
            width: 1,
            height: 1,
            stride: 1,
            left: 0,
            top: 1,
            bitmap: vec![255],
            ..RasterGlyph::default()
        };

        let outlined = rasterizer.outline_glyphs(&[glyph], 2);

        assert_eq!(outlined[0].width, 5);
        assert_eq!(outlined[0].height, 5);
        assert_eq!(outlined[0].left, -2);
        assert_eq!(outlined[0].top, 3);
    }

    #[test]
    fn blur_softens_bitmap_values() {
        let rasterizer = Rasterizer::new();
        let glyph = RasterGlyph {
            width: 3,
            height: 1,
            stride: 3,
            bitmap: vec![0, 255, 0],
            ..RasterGlyph::default()
        };

        let blurred = rasterizer.blur_glyphs(&[glyph], 1);

        assert_eq!(blurred[0].width, 5);
        assert_eq!(blurred[0].height, 3);
        assert_eq!(blurred[0].stride, 5);
        assert_eq!(blurred[0].left, -1);
        assert_eq!(blurred[0].top, 1);
        assert!(
            blurred[0]
                .bitmap
                .iter()
                .any(|value| *value > 0 && *value < 255)
        );
    }

    #[test]
    fn vector_bitmap_expansion_rejects_hostile_dimensions_without_panicking() {
        let rasterizer = Rasterizer::new();
        let glyph = RasterGlyph {
            width: 1,
            height: 1,
            stride: 1,
            left: i32::MIN,
            top: i32::MAX,
            bitmap: vec![255],
            ..RasterGlyph::default()
        };

        for expanded in [
            rasterizer.outline_glyphs(std::slice::from_ref(&glyph), i32::MAX),
            rasterizer.blur_glyphs(std::slice::from_ref(&glyph), u32::MAX),
        ] {
            assert_eq!(expanded.len(), 1);
            assert_eq!(expanded[0].width, 0);
            assert_eq!(expanded[0].height, 0);
            assert_eq!(expanded[0].stride, 0);
            assert!(expanded[0].bitmap.is_empty());
        }

        let malformed = RasterGlyph {
            width: 2,
            height: 2,
            stride: 1,
            bitmap: vec![255],
            ..RasterGlyph::default()
        };
        let expanded = rasterizer.outline_glyphs(&[malformed], 1);
        assert_eq!(expanded[0].width, 0);
        assert!(expanded[0].bitmap.is_empty());
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn hinting_modes_map_to_expected_freetype_flags() {
        assert!(load_flags_for_hinting(ass::Hinting::None).contains(LoadFlag::NO_HINTING));
        assert!(load_flags_for_hinting(ass::Hinting::None).contains(LoadFlag::RENDER));

        let light = load_flags_for_hinting(ass::Hinting::Light);
        assert!(light.contains(LoadFlag::FORCE_AUTOHINT));
        assert!(light.contains(LoadFlag::TARGET_LIGHT));

        let normal = load_flags_for_hinting(ass::Hinting::Normal);
        assert!(normal.contains(LoadFlag::FORCE_AUTOHINT));
        assert!(normal.contains(LoadFlag::TARGET_NORMAL));

        let native = load_flags_for_hinting(ass::Hinting::Native);
        assert!(!native.contains(LoadFlag::FORCE_AUTOHINT));
        assert!(native.contains(LoadFlag::TARGET_NORMAL));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn freetype_italic_rasterization_applies_synthetic_slant() {
        let (_namespace, _cache_scope) = isolated_cache_scope();
        let provider = FontconfigProvider::new();
        let shaper = ShapeEngine::new();
        let regular = shaper
            .shape_text(
                &provider,
                &ShapeRequest::new("T", "DejaVu Sans").with_mode(ShapingMode::Complex),
            )
            .expect("regular shaping should succeed");
        let italic = shaper
            .shape_text(
                &provider,
                &ShapeRequest::new("T", "DejaVu Sans")
                    .with_style("Italic")
                    .with_mode(ShapingMode::Complex),
            )
            .expect("italic shaping should succeed");
        if regular.runs.is_empty()
            || italic.runs.is_empty()
            || regular.runs[0].font.path.is_none()
            || italic.runs[0].font.path.is_none()
        {
            eprintln!("skipping italic raster test: no local DejaVu Sans font path");
            return;
        }
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: 48 * 64,
            hinting: ass::Hinting::Normal,
        });

        let regular_glyph = rasterizer
            .rasterize_run(&regular.runs[0])
            .expect("regular rasterization should succeed")
            .remove(0);
        let italic_glyph = rasterizer
            .rasterize_run(&italic.runs[0])
            .expect("italic rasterization should succeed")
            .remove(0);

        assert_ne!(
            (italic_glyph.width, italic_glyph.left, italic_glyph.bitmap),
            (
                regular_glyph.width,
                regular_glyph.left,
                regular_glyph.bitmap
            ),
            "italic request must change the rendered outline, not reuse an upright glyph"
        );
    }
}
