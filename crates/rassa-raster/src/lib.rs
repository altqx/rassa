use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

use freetype::{Bitmap, Library, face::LoadFlag, ffi};
use rassa_core::{RassaError, RassaResult, ass};
use rassa_fonts::FontMatch;
use rassa_shape::{GlyphInfo, ShapedRun};

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
    pub advance_x: i32,
    pub advance_y: i32,
    pub pixel_mode: RasterPixelMode,
    pub bitmap: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterOptions {
    pub size_26_6: i32,
    pub hinting: ass::Hinting,
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
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct GlyphCacheKey {
    path: PathBuf,
    glyph_id: u32,
    size_26_6: i32,
    hinting: ass::Hinting,
}

static GLYPH_CACHE: OnceLock<Mutex<HashMap<GlyphCacheKey, RasterGlyph>>> = OnceLock::new();

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
                advance_x: glyph.x_advance.round() as i32,
                advance_y: glyph.y_advance.round() as i32,
                ..RasterGlyph::default()
            })
            .collect()
    }

    pub fn rasterize_glyphs(
        &self,
        font: &FontMatch,
        glyphs: &[GlyphInfo],
    ) -> RassaResult<Vec<RasterGlyph>> {
        let font_path = font
            .path
            .as_ref()
            .ok_or_else(|| RassaError::new(format!("font '{}' is unresolved", font.family)))?;
        let library = Library::init()
            .map_err(|error| RassaError::new(format!("freetype init failed: {error:?}")))?;
        let mut face = library.new_face(font_path, 0).map_err(|error| {
            RassaError::new(format!(
                "failed to load font '{}': {error:?}",
                font_path.display()
            ))
        })?;
        request_real_dim_size(&mut face, self.options.size_26_6.max(64))?;

        let mut rasterized = Vec::with_capacity(glyphs.len());
        let load_flags = load_flags_for_hinting(self.options.hinting);
        for glyph in glyphs {
            let cache_key = GlyphCacheKey {
                path: font_path.clone(),
                glyph_id: glyph.glyph_id,
                size_26_6: self.options.size_26_6,
                hinting: self.options.hinting,
            };
            if let Some(cached) = glyph_cache()
                .lock()
                .expect("glyph cache mutex poisoned")
                .get(&cache_key)
                .cloned()
            {
                rasterized.push(RasterGlyph {
                    cluster: glyph.cluster,
                    offset_x: glyph.x_offset.round() as i32,
                    offset_y: (-glyph.y_offset).round() as i32,
                    advance_x: cached.advance_x,
                    advance_y: cached.advance_y,
                    ..cached
                });
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
            let bitmap = slot.bitmap();
            let stride = bitmap.pitch().abs();
            let rendered = RasterGlyph {
                glyph_id: glyph.glyph_id,
                cluster: glyph.cluster,
                width: bitmap.width(),
                height: bitmap.rows(),
                stride,
                left: slot.bitmap_left(),
                top: slot.bitmap_top(),
                offset_x: glyph.x_offset.round() as i32,
                offset_y: (-glyph.y_offset).round() as i32,
                advance_x: (slot.advance().x >> 6) as i32,
                advance_y: (slot.advance().y >> 6) as i32,
                pixel_mode: classify_pixel_mode(&bitmap),
                bitmap: copy_bitmap_rows(&bitmap),
            };
            glyph_cache()
                .lock()
                .expect("glyph cache mutex poisoned")
                .insert(cache_key, rendered.clone());
            rasterized.push(rendered);
        }

        Ok(rasterized)
    }

    pub fn rasterize_run(&self, run: &ShapedRun) -> RassaResult<Vec<RasterGlyph>> {
        self.rasterize_glyphs(&run.font, &run.glyphs)
    }

    pub fn outline_glyphs(&self, glyphs: &[RasterGlyph], radius: i32) -> Vec<RasterGlyph> {
        glyphs
            .iter()
            .map(|glyph| expand_outline(glyph, radius))
            .collect()
    }

    pub fn blur_glyphs(&self, glyphs: &[RasterGlyph], radius: u32) -> Vec<RasterGlyph> {
        glyphs
            .iter()
            .map(|glyph| blur_glyph(glyph, radius))
            .collect()
    }

    pub fn clear_cache() {
        glyph_cache()
            .lock()
            .expect("glyph cache mutex poisoned")
            .clear();
    }

    pub fn cache_stats() -> RasterCacheStats {
        RasterCacheStats {
            glyph_entries: glyph_cache()
                .lock()
                .expect("glyph cache mutex poisoned")
                .len(),
        }
    }
}

fn glyph_cache() -> &'static Mutex<HashMap<GlyphCacheKey, RasterGlyph>> {
    GLYPH_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn request_real_dim_size(face: &mut freetype::Face, size_26_6: i32) -> RassaResult<()> {
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

fn load_flags_for_hinting(hinting: ass::Hinting) -> LoadFlag {
    let base = LoadFlag::RENDER
        | LoadFlag::NO_BITMAP
        | LoadFlag::IGNORE_GLOBAL_ADVANCE_WITH
        | LoadFlag::IGNORE_TRANSFORM;
    match hinting {
        ass::Hinting::None => base | LoadFlag::NO_HINTING,
        ass::Hinting::Light => base | LoadFlag::FORCE_AUTOHINT | LoadFlag::TARGET_LIGHT,
        ass::Hinting::Normal => base | LoadFlag::FORCE_AUTOHINT,
        ass::Hinting::Native => base,
    }
}

fn classify_pixel_mode(bitmap: &Bitmap) -> RasterPixelMode {
    match bitmap.pixel_mode() {
        Ok(freetype::bitmap::PixelMode::Mono) => RasterPixelMode::Mono,
        Ok(freetype::bitmap::PixelMode::Gray) => RasterPixelMode::Gray,
        _ => RasterPixelMode::Other,
    }
}

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

fn expand_outline(glyph: &RasterGlyph, radius: i32) -> RasterGlyph {
    if radius <= 0 || glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
        return glyph.clone();
    }

    let radius = radius as usize;
    let radius_squared = (radius * radius) as i32;
    let width = glyph.width as usize;
    let height = glyph.height as usize;
    let stride = glyph.stride as usize;
    let new_width = width + radius * 2;
    let new_height = height + radius * 2;
    let mut bitmap = vec![0_u8; new_width * new_height];

    for y in 0..height {
        for x in 0..width {
            let value = glyph.bitmap[y * stride + x];
            if value == 0 {
                continue;
            }
            let center_x = x + radius;
            let center_y = y + radius;
            for outline_y in
                center_y.saturating_sub(radius)..=(center_y + radius).min(new_height - 1)
            {
                for outline_x in
                    center_x.saturating_sub(radius)..=(center_x + radius).min(new_width - 1)
                {
                    let dx = outline_x as i32 - center_x as i32;
                    let dy = outline_y as i32 - center_y as i32;
                    if dx * dx + dy * dy > radius_squared {
                        continue;
                    }
                    let index = outline_y * new_width + outline_x;
                    bitmap[index] = bitmap[index].max(value);
                }
            }
        }
    }

    RasterGlyph {
        width: new_width as i32,
        height: new_height as i32,
        stride: new_width as i32,
        left: glyph.left - radius as i32,
        top: glyph.top + radius as i32,
        bitmap,
        ..glyph.clone()
    }
}

fn blur_glyph(glyph: &RasterGlyph, radius: u32) -> RasterGlyph {
    if radius == 0 || glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
        return glyph.clone();
    }

    let radius = radius as usize;
    let width = glyph.width as usize;
    let height = glyph.height as usize;
    let stride = glyph.stride as usize;
    let new_width = width + radius * 2;
    let new_height = height + radius * 2;
    let mut expanded = vec![0_u8; new_width * new_height];

    for y in 0..height {
        for x in 0..width {
            expanded[(y + radius) * new_width + x + radius] = glyph.bitmap[y * stride + x];
        }
    }

    let mut bitmap = vec![0_u8; expanded.len()];
    for y in 0..new_height {
        let min_y = y.saturating_sub(radius);
        let max_y = (y + radius).min(new_height - 1);
        for x in 0..new_width {
            let min_x = x.saturating_sub(radius);
            let max_x = (x + radius).min(new_width - 1);
            let mut sum = 0_u32;
            let mut count = 0_u32;
            for sample_y in min_y..=max_y {
                for sample_x in min_x..=max_x {
                    sum += u32::from(expanded[sample_y * new_width + sample_x]);
                    count += 1;
                }
            }
            bitmap[y * new_width + x] = (sum / count.max(1)) as u8;
        }
    }

    RasterGlyph {
        width: new_width as i32,
        height: new_height as i32,
        stride: new_width as i32,
        left: glyph.left - radius as i32,
        top: glyph.top + radius as i32,
        bitmap,
        ..glyph.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_fonts::FontconfigProvider;
    use rassa_shape::{ShapeEngine, ShapeRequest};

    #[test]
    fn rasterize_run_renders_system_font_bitmaps() {
        Rasterizer::clear_cache();
        let provider = FontconfigProvider::new();
        let shaper = ShapeEngine::new();
        let shaped = shaper
            .shape_text(&provider, &ShapeRequest::new("Ab", "sans"))
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
    }

    #[test]
    fn rasterize_run_reuses_global_glyph_cache() {
        Rasterizer::clear_cache();
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
        let entries_after_first = glyph_cache_entries_for_run(&shaped.runs[0], rasterizer.options);
        let second = rasterizer
            .rasterize_run(&shaped.runs[0])
            .expect("rasterization should succeed");

        assert_eq!(first, second);
        assert!(entries_after_first > 0);
        assert_eq!(
            glyph_cache_entries_for_run(&shaped.runs[0], rasterizer.options),
            entries_after_first
        );
    }

    fn glyph_cache_entries_for_run(run: &ShapedRun, options: RasterOptions) -> usize {
        let Some(path) = run.font.path.as_ref() else {
            return 0;
        };
        glyph_cache()
            .lock()
            .expect("glyph cache mutex poisoned")
            .keys()
            .filter(|key| {
                key.path == *path
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
            x_advance: 1.0,
            y_advance: 0.0,
            x_offset: 0.0,
            y_offset: 0.0,
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
}
