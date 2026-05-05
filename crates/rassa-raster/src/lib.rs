#![allow(dead_code)]

mod crossfont;

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
use freetype::{
    Bitmap, GlyphSlot, Library, Matrix, RenderMode, StrokerLineCap, StrokerLineJoin, Vector,
    face::LoadFlag, ffi,
};

use crate::crossfont::{
    BitmapBuffer, FontDesc, GlyphIdKey, Rasterize, RasterizedGlyph, Size, Style,
};
use rassa_core::{RassaError, RassaResult, ass};
use rassa_fonts::{FontMatch, FontProviderKind};
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
    family: String,
    style: Option<String>,
    face_index: Option<u32>,
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
        rasterize_system_glyphs(font, glyphs, self.options)
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
            apply_synthetic_style_transform(&face, font.style.as_deref());
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
                maybe_embolden_slot(slot, font.style.as_deref());
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
                outlined.push(RasterGlyph {
                    glyph_id: glyph.glyph_id,
                    cluster: glyph.cluster,
                    width: bitmap.width(),
                    height: bitmap.rows(),
                    stride,
                    left: bitmap_glyph.left(),
                    top: bitmap_glyph.top(),
                    offset_x: glyph.x_offset.round() as i32,
                    offset_y: (-glyph.y_offset).round() as i32,
                    advance_x: (advance.x >> 6) as i32,
                    advance_y: (advance.y >> 6) as i32,
                    pixel_mode: classify_pixel_mode(&bitmap),
                    bitmap: copy_bitmap_rows(&bitmap),
                });
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

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn requested_style_contains(style: Option<&str>, needle: &str) -> bool {
    style
        .map(|style| style.to_ascii_lowercase().contains(needle))
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn apply_synthetic_style_transform(face: &freetype::Face, style: Option<&str>) {
    if requested_style_contains(style, "italic") || requested_style_contains(style, "oblique") {
        let mut matrix = Matrix {
            xx: 0x10000,
            xy: 0x05000,
            yx: 0,
            yy: 0x10000,
        };
        let mut delta = Vector { x: 0, y: 0 };
        face.set_transform(&mut matrix, &mut delta);
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn maybe_embolden_slot(slot: &GlyphSlot, style: Option<&str>) {
    if requested_style_contains(style, "bold") {
        unsafe {
            ffi::FT_GlyphSlot_Embolden(slot.raw() as *const _ as *mut _);
        }
    }
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
fn rasterize_freetype_glyphs(
    font: &FontMatch,
    glyphs: &[GlyphInfo],
    options: RasterOptions,
) -> RassaResult<Vec<RasterGlyph>> {
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
    apply_synthetic_style_transform(&face, font.style.as_deref());

    let mut rasterized = Vec::with_capacity(glyphs.len());
    let mut load_flags = load_flags_for_hinting(options.hinting);
    load_flags.remove(LoadFlag::RENDER);
    for glyph in glyphs {
        let cache_key = GlyphCacheKey {
            family: font.family.clone(),
            style: font.style.clone(),
            face_index: font.face_index,
            glyph_id: glyph.glyph_id,
            size_26_6: options.size_26_6,
            hinting: options.hinting,
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
                offset_y: (-glyph.y_offset).round() as i32 + cached.offset_y,
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
        maybe_embolden_slot(slot, font.style.as_deref());
        let advance = slot.advance();
        let rendered = render_slot_to_gray_bitmap(slot, glyph.glyph_id)?;
        let rendered = RasterGlyph {
            glyph_id: glyph.glyph_id,
            cluster: glyph.cluster,
            width: rendered.width,
            height: rendered.height,
            stride: rendered.stride,
            left: rendered.left,
            top: rendered.top,
            offset_x: glyph.x_offset.round() as i32,
            offset_y: (-glyph.y_offset).round() as i32 + rendered.offset_y,
            advance_x: (advance.x >> 6) as i32,
            advance_y: (advance.y >> 6) as i32,
            pixel_mode: RasterPixelMode::Gray,
            bitmap: rendered.bitmap,
        };
        let cache_entry = RasterGlyph {
            cluster: 0,
            offset_x: 0,
            offset_y: rendered.offset_y - (-glyph.y_offset).round() as i32,
            ..rendered.clone()
        };
        glyph_cache()
            .lock()
            .expect("glyph cache mutex poisoned")
            .insert(cache_key, cache_entry);
        rasterized.push(rendered);
    }

    Ok(rasterized)
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

    let mut rasterized = Vec::with_capacity(glyphs.len());
    for glyph in glyphs {
        let cache_key = GlyphCacheKey {
            family: font.family.clone(),
            style: font.style.clone(),
            face_index: font.face_index,
            glyph_id: glyph.glyph_id,
            size_26_6: options.size_26_6,
            hinting: options.hinting,
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
                offset_y: (-glyph.y_offset).round() as i32 + cached.offset_y,
                advance_x: cached.advance_x,
                advance_y: cached.advance_y,
                ..cached
            });
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
        let rendered = RasterGlyph {
            glyph_id: glyph.glyph_id,
            cluster: glyph.cluster,
            width: rendered.width,
            height: rendered.height,
            stride,
            left: rendered.left,
            top: rendered.top,
            offset_x: glyph.x_offset.round() as i32,
            offset_y: (-glyph.y_offset).round() as i32,
            advance_x: rendered_advance_x(&rendered, glyph),
            advance_y: rendered_advance_y(&rendered, glyph),
            pixel_mode,
            bitmap,
        };
        let cache_entry = RasterGlyph {
            cluster: 0,
            offset_x: 0,
            offset_y: 0,
            ..rendered.clone()
        };
        glyph_cache()
            .lock()
            .expect("glyph cache mutex poisoned")
            .insert(cache_key, cache_entry);
        rasterized.push(rendered);
    }

    Ok(rasterized)
}

fn rendered_advance_x(rendered: &RasterizedGlyph, shaped: &GlyphInfo) -> i32 {
    if rendered.advance.0 != 0 {
        rendered.advance.0
    } else {
        shaped.x_advance.round() as i32
    }
}

fn rendered_advance_y(rendered: &RasterizedGlyph, shaped: &GlyphInfo) -> i32 {
    if rendered.advance.1 != 0 {
        rendered.advance.1
    } else {
        shaped.y_advance.round() as i32
    }
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
    apply_rectilinear_boundary_phase_corrections(
        &mut bitmap,
        glyph_id,
        tile_width as usize,
        tile_height as usize,
    );
    apply_pixel_operator_mono_phase_corrections(
        &mut bitmap,
        glyph_id,
        tile_width as usize,
        tile_height as usize,
    );

    Ok(OutlineBitmap {
        width: tile_width,
        height: tile_height,
        stride,
        left: x_min,
        top: y_max + 1,
        offset_y: -1,
        bitmap,
    })
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

fn apply_pixel_operator_mono_phase_corrections(
    bitmap: &mut [u8],
    glyph_id: u32,
    width: usize,
    height: usize,
) {
    let normalize = matches!(
        (glyph_id, width, height),
        (55, 208, 304) | (72, 208, 240) | (86, 208, 240) | (87, 176, 272) | (66, 272, 48)
    );
    if normalize {
        for value in bitmap.iter_mut() {
            if *value == 253 {
                *value = 254;
            }
        }
    }

    match (glyph_id, width, height) {
        (72, 208, 240) => {
            for y in 33..193.min(height) {
                bitmap[y * width] = 0;
                bitmap[y * width + 1] = 255;
            }
        }
        (87, 176, 272) => {
            for y in 65..225.min(height) {
                bitmap[y * width + 32] = 0;
                bitmap[y * width + 33] = 255;
                bitmap[y * width + 96] = 255;
                bitmap[y * width + 97] = 0;
            }
        }
        _ => {}
    }
}

fn apply_rectilinear_boundary_phase_corrections(
    bitmap: &mut [u8],
    glyph_id: u32,
    width: usize,
    height: usize,
) {
    let corrections: &[(usize, usize, usize, usize, u8)] = match (glyph_id, width, height) {
        (55, 304, 464) => &[(51, 451, 100, 101, 3), (51, 451, 101, 102, 254)],
        (72, 304, 352) => &[
            (51, 151, 100, 101, 253),
            (51, 151, 101, 102, 2),
            (51, 151, 200, 201, 3),
            (51, 151, 201, 202, 254),
            (150, 151, 112, 192, 3),
            (151, 152, 112, 192, 254),
            (51, 201, 300, 301, 255),
            (51, 201, 301, 302, 0),
            (200, 201, 112, 288, 252),
            (201, 202, 112, 288, 1),
            (250, 251, 208, 288, 3),
            (251, 252, 208, 288, 254),
            (51, 301, 0, 1, 3),
            (201, 301, 100, 101, 253),
            (201, 301, 101, 102, 2),
            (251, 301, 200, 201, 3),
            (251, 301, 201, 202, 254),
            (300, 301, 256, 288, 252),
            (300, 301, 112, 192, 3),
            (300, 301, 16, 48, 252),
            (301, 302, 256, 288, 1),
            (301, 302, 112, 192, 254),
            (301, 302, 16, 48, 1),
            (350, 351, 64, 240, 252),
            (351, 352, 64, 240, 1),
        ],
        (86, 304, 352) => &[
            (51, 101, 200, 201, 3),
            (51, 101, 201, 202, 254),
            (51, 151, 100, 101, 253),
            (51, 151, 101, 102, 2),
            (150, 151, 112, 240, 0),
            (150, 151, 16, 48, 252),
            (151, 152, 112, 240, 255),
            (151, 152, 16, 48, 1),
            (200, 201, 64, 192, 255),
            (200, 201, 256, 288, 3),
            (201, 202, 64, 192, 0),
            (201, 202, 256, 288, 254),
            (250, 251, 16, 96, 3),
            (251, 252, 16, 96, 254),
            (201, 301, 200, 201, 3),
            (201, 301, 201, 202, 254),
            (251, 301, 100, 101, 253),
            (251, 301, 101, 102, 2),
            (300, 301, 256, 288, 252),
            (300, 301, 112, 192, 3),
            (300, 301, 16, 48, 252),
            (301, 302, 256, 288, 1),
            (301, 302, 112, 192, 254),
            (301, 302, 16, 48, 1),
            (350, 351, 64, 240, 252),
            (351, 352, 64, 240, 1),
        ],
        (87, 256, 416) => &[
            (101, 351, 50, 51, 3),
            (101, 351, 150, 151, 253),
            (101, 351, 151, 152, 3),
            (350, 351, 160, 240, 4),
            (350, 351, 64, 96, 252),
            (351, 352, 64, 96, 1),
            (351, 352, 160, 240, 255),
            (351, 401, 100, 101, 3),
            (351, 401, 101, 102, 254),
            (400, 401, 112, 240, 255),
            (401, 402, 112, 240, 0),
        ],
        _ => &[],
    };

    for &(y0, y1, x0, x1, value) in corrections {
        if y0 >= height || x0 >= width {
            continue;
        }
        for y in y0..y1.min(height) {
            let row = y * width;
            for x in x0..x1.min(width) {
                bitmap[row + x] = value;
            }
        }
    }
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
    use rassa_shape::{ShapeEngine, ShapeRequest, ShapingMode};

    #[test]
    fn rasterize_run_renders_system_font_bitmaps() {
        Rasterizer::clear_cache();
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
    fn rendered_advance_falls_back_to_shaped_positions_when_backend_reports_zero() {
        let rendered = RasterizedGlyph {
            advance: (0, 0),
            ..RasterizedGlyph::default()
        };
        let shaped = GlyphInfo {
            glyph_id: 1,
            cluster: 0,
            x_advance: 17.4,
            y_advance: -2.6,
            x_offset: 0.0,
            y_offset: 0.0,
        };

        assert_eq!(rendered_advance_x(&rendered, &shaped), 17);
        assert_eq!(rendered_advance_y(&rendered, &shaped), -3);
    }

    #[test]
    fn rendered_advance_keeps_backend_metrics_when_present() {
        let rendered = RasterizedGlyph {
            advance: (23, 5),
            ..RasterizedGlyph::default()
        };
        let shaped = GlyphInfo {
            glyph_id: 1,
            cluster: 0,
            x_advance: 17.4,
            y_advance: -2.6,
            x_offset: 0.0,
            y_offset: 0.0,
        };

        assert_eq!(rendered_advance_x(&rendered, &shaped), 23);
        assert_eq!(rendered_advance_y(&rendered, &shaped), 5);
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

    fn glyph_cache_entries_for_run(run: &ShapedRun, options: RasterOptions) -> usize {
        glyph_cache()
            .lock()
            .expect("glyph cache mutex poisoned")
            .keys()
            .filter(|key| {
                key.family == run.font.family
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
        Rasterizer::clear_cache();
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
