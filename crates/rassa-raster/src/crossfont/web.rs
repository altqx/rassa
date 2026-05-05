//! Web/wasm rasterization backend.
//!
//! This backend intentionally avoids native font APIs. It can rasterize explicit
//! font bytes supplied by the caller and provides no system font discovery.

use std::collections::HashMap;
use std::path::Path;

use fontdue::Font;

use crate::crossfont::{
    BitmapBuffer, Error, FontDesc, FontKey, GlyphIdKey, GlyphKey, Metrics, ProportionalMetrics,
    Rasterize, RasterizedGlyph, Size,
};

struct LoadedFont {
    font: Font,
}

#[derive(Default)]
pub struct WebRasterizer {
    fonts: HashMap<FontKey, LoadedFont>,
}

impl Rasterize for WebRasterizer {
    fn new() -> Result<Self, Error> {
        Ok(Self::default())
    }

    fn metrics(&self, key: FontKey, size: Size) -> Result<Metrics, Error> {
        let font = self.fonts.get(&key).ok_or(Error::UnknownFontKey)?;
        let line = font.font.horizontal_line_metrics(size.as_px());
        Ok(Metrics {
            average_advance: font.font.metrics('0', size.as_px()).advance_width as f64,
            line_height: line
                .map(|line| line.ascent - line.descent + line.line_gap)
                .unwrap_or_else(|| size.as_px()) as f64,
            descent: line.map(|line| line.descent).unwrap_or(0.0),
            underline_position: 0.0,
            underline_thickness: 1.0,
            strikeout_position: size.as_px() / 2.0,
            strikeout_thickness: 1.0,
        })
    }

    fn load_font(&mut self, desc: &FontDesc, _size: Size) -> Result<FontKey, Error> {
        Err(Error::FontNotFound(desc.clone()))
    }

    fn load_font_path(&mut self, path: &Path, size: Size) -> Result<FontKey, Error> {
        let bytes = std::fs::read(path).map_err(|error| Error::PlatformError(error.to_string()))?;
        self.load_font_bytes(&bytes, size)
    }

    fn load_font_bytes(&mut self, bytes: &[u8], _size: Size) -> Result<FontKey, Error> {
        let font = Font::from_bytes(bytes.to_vec(), fontdue::FontSettings::default())
            .map_err(|error| Error::PlatformError(error.to_string()))?;
        let key = FontKey::next();
        self.fonts.insert(key, LoadedFont { font });
        Ok(key)
    }

    fn get_glyph(&mut self, glyph: GlyphKey) -> Result<RasterizedGlyph, Error> {
        let font = self
            .fonts
            .get(&glyph.font_key)
            .ok_or(Error::UnknownFontKey)?;
        let glyph_id = font.font.lookup_glyph_index(glyph.character) as u32;
        rasterize_fontdue_glyph(&font.font, glyph.character, glyph_id, glyph.size)
    }

    fn get_glyph_id(&mut self, glyph: GlyphIdKey) -> Result<RasterizedGlyph, Error> {
        let font = self
            .fonts
            .get(&glyph.font_key)
            .ok_or(Error::UnknownFontKey)?;
        rasterize_fontdue_glyph(&font.font, '\0', glyph.glyph_id, glyph.size)
    }

    fn glyph_id_metrics(&mut self, glyph: GlyphIdKey) -> Result<ProportionalMetrics, Error> {
        let font = self
            .fonts
            .get(&glyph.font_key)
            .ok_or(Error::UnknownFontKey)?;
        let metrics = font
            .font
            .metrics_indexed(glyph.glyph_id as u16, glyph.size.as_px());
        Ok(ProportionalMetrics {
            advance_x: metrics.advance_width,
            advance_y: metrics.advance_height,
            bounds_x: metrics.bounds.xmin,
            bounds_y: metrics.bounds.ymin,
            bounds_width: metrics.bounds.width,
            bounds_height: metrics.bounds.height,
        })
    }

    fn drop_font(&mut self, key: FontKey) -> Result<(), Error> {
        self.fonts
            .remove(&key)
            .map(|_| ())
            .ok_or(Error::UnknownFontKey)
    }

    fn evict_cache(&mut self) {
        self.fonts.clear();
    }

    fn kerning(&mut self, _left: GlyphKey, _right: GlyphKey) -> (f32, f32) {
        (0.0, 0.0)
    }
}

fn rasterize_fontdue_glyph(
    font: &Font,
    character: char,
    glyph_id: u32,
    size: Size,
) -> Result<RasterizedGlyph, Error> {
    let (metrics, bitmap) = font.rasterize_indexed(glyph_id as u16, size.as_px());
    Ok(RasterizedGlyph {
        character,
        width: metrics.width as i32,
        height: metrics.height as i32,
        top: metrics.ymin + metrics.height as i32,
        left: metrics.xmin,
        advance: (
            metrics.advance_width.round() as i32,
            metrics.advance_height.round() as i32,
        ),
        buffer: BitmapBuffer::Rgb(bitmap.into_iter().flat_map(|v| [v, v, v]).collect()),
    })
}
