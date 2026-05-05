//! Rasterization powered by DirectWrite.
#![allow(deprecated)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::sync::Arc;

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use dwrote::{
    DWRITE_GLYPH_RUN, FontCollection, FontFace, FontFallback, FontFile, FontStretch, FontStyle,
    FontWeight, GlyphOffset, GlyphRunAnalysis, TextAnalysisSource, TextAnalysisSourceMethods,
};

use winapi::shared::ntdef::{HRESULT, LOCALE_NAME_MAX_LENGTH};
use winapi::um::dwrite::{self, DWRITE_FONT_SIMULATIONS_NONE};
use winapi::um::winnls::GetUserDefaultLocaleName;

use super::{
    BitmapBuffer, Error, FontDesc, FontKey, GlyphKey, Metrics, RasterizedGlyph, Size, Slant, Style,
    Weight,
};

/// DirectWrite uses 0 for missing glyph symbols.
/// https://docs.microsoft.com/en-us/typography/opentype/spec/recom#glyph-0-the-notdef-glyph
const MISSING_GLYPH_INDEX: u16 = 0;

/// Cached DirectWrite font.
struct Font {
    face: FontFace,
    family_name: String,
    weight: FontWeight,
    style: FontStyle,
    stretch: FontStretch,
}

pub struct DirectWriteRasterizer {
    fonts: HashMap<FontKey, Font>,
    keys: HashMap<FontDesc, FontKey>,
    available_fonts: FontCollection,
    fallback_sequence: Option<FontFallback>,
}

impl DirectWriteRasterizer {
    fn rasterize_glyph(
        &self,
        face: &FontFace,
        size: Size,
        character: char,
        glyph_index: u16,
    ) -> Result<RasterizedGlyph, Error> {
        let em_size = size.as_px();

        let glyph_run = DWRITE_GLYPH_RUN {
            fontFace: unsafe { face.as_ptr() },
            fontEmSize: em_size,
            glyphCount: 1,
            glyphIndices: &glyph_index,
            glyphAdvances: &0.0,
            glyphOffsets: &GlyphOffset::default(),
            isSideways: 0,
            bidiLevel: 0,
        };

        let rendering_mode = face.get_recommended_rendering_mode_default_params(
            em_size,
            1.,
            dwrote::DWRITE_MEASURING_MODE_NATURAL,
        );

        let glyph_analysis = GlyphRunAnalysis::create(
            &glyph_run,
            1.,
            None,
            rendering_mode,
            dwrote::DWRITE_MEASURING_MODE_NATURAL,
            0.0,
            0.0,
        )?;

        let cleartype_bounds =
            glyph_analysis.get_alpha_texture_bounds(dwrote::DWRITE_TEXTURE_CLEARTYPE_3x1)?;
        let cleartype_width = cleartype_bounds.right - cleartype_bounds.left;
        let cleartype_height = cleartype_bounds.bottom - cleartype_bounds.top;
        if cleartype_width > 0 && cleartype_height > 0 {
            let cleartype = glyph_analysis
                .create_alpha_texture(dwrote::DWRITE_TEXTURE_CLEARTYPE_3x1, cleartype_bounds)?;
            if cleartype.iter().any(|sample| *sample != 0) {
                return Ok(RasterizedGlyph {
                    character,
                    width: cleartype_width,
                    height: cleartype_height,
                    top: -cleartype_bounds.top,
                    left: cleartype_bounds.left,
                    advance: (0, 0),
                    buffer: BitmapBuffer::Rgb(normalize_cleartype_layout(cleartype)),
                });
            }
        }

        // Headless Windows runners can report a ClearType rendering mode but return an
        // empty ClearType alpha texture. Build a fresh aliased analysis instead of
        // reusing the ClearType analysis, then require real non-zero coverage before
        // treating the glyph as rendered.
        let aliased_analysis = GlyphRunAnalysis::create(
            &glyph_run,
            1.,
            None,
            dwrote::DWRITE_RENDERING_MODE_ALIASED,
            dwrote::DWRITE_MEASURING_MODE_NATURAL,
            0.0,
            0.0,
        )?;
        let aliased_bounds =
            aliased_analysis.get_alpha_texture_bounds(dwrote::DWRITE_TEXTURE_ALIASED_1x1)?;
        let aliased_width = aliased_bounds.right - aliased_bounds.left;
        let aliased_height = aliased_bounds.bottom - aliased_bounds.top;
        if aliased_width > 0 && aliased_height > 0 {
            let aliased = aliased_analysis
                .create_alpha_texture(dwrote::DWRITE_TEXTURE_ALIASED_1x1, aliased_bounds)?;
            if aliased.iter().any(|sample| *sample != 0) {
                return Ok(RasterizedGlyph {
                    character,
                    width: aliased_width,
                    height: aliased_height,
                    top: -aliased_bounds.top,
                    left: aliased_bounds.left,
                    advance: (0, 0),
                    buffer: BitmapBuffer::Rgb(
                        aliased.into_iter().flat_map(|sample| [sample; 3]).collect(),
                    ),
                });
            }
        }

        // Some valid spacing glyphs (for example U+0020 shaped by glyph id) have no
        // outline coverage. Return an empty glyph so one space does not make the
        // whole run fail; higher-level smoke tests still catch runs where every
        // printable glyph has zero coverage.
        Ok(RasterizedGlyph {
            character,
            width: 0,
            height: 0,
            top: 0,
            left: 0,
            advance: (0, 0),
            buffer: BitmapBuffer::Rgb(Vec::new()),
        })
    }

    fn load_font_file(
        &mut self,
        font_file: FontFile,
        family_name: String,
    ) -> Result<FontKey, Error> {
        let face = font_file
            .create_face(0, DWRITE_FONT_SIMULATIONS_NONE)
            .map_err(Error::from)?;
        let key = FontKey::next();
        self.fonts.insert(
            key,
            Font {
                face,
                family_name,
                weight: FontWeight::Regular,
                style: FontStyle::Normal,
                stretch: FontStretch::Normal,
            },
        );
        Ok(key)
    }

    fn get_loaded_font(&self, font_key: FontKey) -> Result<&Font, Error> {
        self.fonts.get(&font_key).ok_or(Error::UnknownFontKey)
    }

    fn get_glyph_index(&self, face: &FontFace, character: char) -> u16 {
        face.get_glyph_indices(&[character as u32])
            .first()
            .copied()
            .unwrap_or(MISSING_GLYPH_INDEX)
    }

    fn get_fallback_font(&self, loaded_font: &Font, character: char) -> Option<dwrote::Font> {
        let fallback = self.fallback_sequence.as_ref()?;

        let mut buffer = [0u16; 2];
        character.encode_utf16(&mut buffer);

        let length = character.len_utf16() as u32;
        let utf16_codepoints = &buffer[..length as usize];

        let locale = get_current_locale();

        let text_analysis_source_data = TextAnalysisSourceData {
            locale: &locale,
            length,
        };
        let text_analysis_source = TextAnalysisSource::from_text(
            Box::new(text_analysis_source_data),
            Cow::Borrowed(utf16_codepoints),
        );

        let fallback_result = fallback.map_characters(
            &text_analysis_source,
            0,
            length,
            &self.available_fonts,
            Some(&loaded_font.family_name),
            loaded_font.weight,
            loaded_font.style,
            loaded_font.stretch,
        );

        fallback_result.mapped_font
    }
}

impl crate::crossfont::Rasterize for DirectWriteRasterizer {
    fn new() -> Result<DirectWriteRasterizer, Error> {
        Ok(DirectWriteRasterizer {
            fonts: HashMap::new(),
            keys: HashMap::new(),
            available_fonts: FontCollection::system(),
            fallback_sequence: FontFallback::get_system_fallback(),
        })
    }

    fn metrics(&self, key: FontKey, size: Size) -> Result<Metrics, Error> {
        let face = &self.get_loaded_font(key)?.face;
        let vmetrics = face.metrics().metrics0();

        let scale = size.as_px() / f32::from(vmetrics.designUnitsPerEm);

        let underline_position = f32::from(vmetrics.underlinePosition) * scale;
        let underline_thickness = f32::from(vmetrics.underlineThickness) * scale;

        let strikeout_position = f32::from(vmetrics.strikethroughPosition) * scale;
        let strikeout_thickness = f32::from(vmetrics.strikethroughThickness) * scale;

        let ascent = f32::from(vmetrics.ascent) * scale;
        let descent = -f32::from(vmetrics.descent) * scale;
        let line_gap = f32::from(vmetrics.lineGap) * scale;

        let line_height = f64::from(ascent - descent + line_gap);

        // Since all monospace characters have the same width, we use `!` for horizontal metrics.
        let character = '!';
        let glyph_index = self.get_glyph_index(face, character);

        let glyph_metrics = face.get_design_glyph_metrics(&[glyph_index], false);
        let hmetrics = glyph_metrics.first().ok_or(Error::MetricsNotFound)?;

        let average_advance = f64::from(hmetrics.advanceWidth) * f64::from(scale);

        Ok(Metrics {
            descent,
            average_advance,
            line_height,
            underline_position,
            underline_thickness,
            strikeout_position,
            strikeout_thickness,
        })
    }

    fn load_font(&mut self, desc: &FontDesc, _size: Size) -> Result<FontKey, Error> {
        // Fast path if face is already loaded.
        if let Some(key) = self.keys.get(desc) {
            return Ok(*key);
        }

        let family = self
            .available_fonts
            .get_font_family_by_name(&desc.name)
            .ok_or_else(|| Error::FontNotFound(desc.clone()))?;

        let font =
            match desc.style {
                Style::Description { weight, slant } => {
                    // This searches for the "best" font - should mean we don't have to worry about
                    // fallbacks if our exact desired weight/style isn't available.
                    Ok(family.get_first_matching_font(
                        weight.into(),
                        FontStretch::Normal,
                        slant.into(),
                    ))
                }
                Style::Specific(ref style) => {
                    let mut idx = 0;
                    let count = family.get_font_count();

                    loop {
                        if idx == count {
                            break Err(Error::FontNotFound(desc.clone()));
                        }

                        let font = family.get_font(idx);

                        if font.face_name() == *style {
                            break Ok(font);
                        }

                        idx += 1;
                    }
                }
            }?;

        let key = FontKey::next();
        self.keys.insert(desc.clone(), key);
        self.fonts.insert(key, font.into());

        Ok(key)
    }

    fn load_font_path(&mut self, path: &Path, _size: Size) -> Result<FontKey, Error> {
        let font_file = FontFile::new_from_path(path).ok_or_else(|| {
            Error::PlatformError(format!(
                "failed to load DirectWrite font '{}'",
                path.display()
            ))
        })?;
        self.load_font_file(font_file, path.display().to_string())
    }

    fn load_font_bytes(&mut self, bytes: &[u8], _size: Size) -> Result<FontKey, Error> {
        let font_file = FontFile::new_from_buffer(Arc::new(bytes.to_vec())).ok_or_else(|| {
            Error::PlatformError("failed to load DirectWrite font from bytes".to_owned())
        })?;
        self.load_font_file(font_file, "<memory>".to_owned())
    }

    fn get_glyph(&mut self, glyph: GlyphKey) -> Result<RasterizedGlyph, Error> {
        let loaded_font = self.get_loaded_font(glyph.font_key)?;

        let loaded_fallback_font;
        let mut font = loaded_font;
        let mut glyph_index = self.get_glyph_index(&loaded_font.face, glyph.character);
        if glyph_index == MISSING_GLYPH_INDEX {
            if let Some(fallback_font) = self.get_fallback_font(loaded_font, glyph.character) {
                loaded_fallback_font = Font::from(fallback_font);
                glyph_index = self.get_glyph_index(&loaded_fallback_font.face, glyph.character);
                font = &loaded_fallback_font;
            }
        }

        let rasterized_glyph =
            self.rasterize_glyph(&font.face, glyph.size, glyph.character, glyph_index)?;

        if glyph_index == MISSING_GLYPH_INDEX {
            Err(Error::MissingGlyph(rasterized_glyph))
        } else {
            Ok(rasterized_glyph)
        }
    }

    fn get_glyph_id(
        &mut self,
        glyph: crate::crossfont::GlyphIdKey,
    ) -> Result<RasterizedGlyph, Error> {
        let loaded_font = self.get_loaded_font(glyph.font_key)?;
        self.rasterize_glyph(
            &loaded_font.face,
            glyph.size,
            '\0',
            glyph.glyph_id.try_into().map_err(|_| {
                Error::PlatformError(format!(
                    "DirectWrite glyph id {} exceeds u16",
                    glyph.glyph_id
                ))
            })?,
        )
    }

    fn drop_font(&mut self, key: FontKey) -> Result<(), Error> {
        self.fonts.remove(&key).ok_or(Error::UnknownFontKey)?;
        self.keys.retain(|_, existing| *existing != key);
        Ok(())
    }

    fn evict_cache(&mut self) {
        self.fonts.clear();
        self.keys.clear();
    }

    fn kerning(&mut self, _left: GlyphKey, _right: GlyphKey) -> (f32, f32) {
        (0., 0.)
    }
}

impl From<dwrote::Font> for Font {
    fn from(font: dwrote::Font) -> Font {
        Font {
            face: font.create_font_face(),
            family_name: font.family_name(),
            weight: font.weight(),
            style: font.style(),
            stretch: font.stretch(),
        }
    }
}

impl From<Weight> for FontWeight {
    fn from(weight: Weight) -> FontWeight {
        match weight {
            Weight::Bold => FontWeight::Bold,
            Weight::Normal => FontWeight::Regular,
        }
    }
}

impl From<Slant> for FontStyle {
    fn from(slant: Slant) -> FontStyle {
        match slant {
            Slant::Oblique => FontStyle::Oblique,
            Slant::Italic => FontStyle::Italic,
            Slant::Normal => FontStyle::Normal,
        }
    }
}

fn get_current_locale() -> String {
    let mut buffer = vec![0u16; LOCALE_NAME_MAX_LENGTH];
    let len =
        unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) as usize };

    // `len` includes null byte, which we don't need in Rust.
    OsString::from_wide(&buffer[..len - 1])
        .into_string()
        .expect("Locale not valid unicode")
}

/// Font fallback information for dwrote's TextAnalysisSource.
struct TextAnalysisSourceData<'a> {
    locale: &'a str,
    length: u32,
}

impl TextAnalysisSourceMethods for TextAnalysisSourceData<'_> {
    fn get_locale_name(&self, _text_position: u32) -> (Cow<str>, u32) {
        (Cow::Borrowed(self.locale), self.length)
    }

    fn get_paragraph_reading_direction(&self) -> dwrite::DWRITE_READING_DIRECTION {
        dwrite::DWRITE_READING_DIRECTION_LEFT_TO_RIGHT
    }
}

impl From<HRESULT> for Error {
    fn from(hresult: HRESULT) -> Self {
        let message = format!("a DirectWrite rendering error occurred: {:X}", hresult);
        Error::PlatformError(message)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SubpixelLayout {
    Rgb,
    Bgr,
}

fn normalize_cleartype_layout(mut bytes: Vec<u8>) -> Vec<u8> {
    // DirectWrite returns ClearType samples in RGB order by default. Some LCD panels
    // use BGR subpixel geometry; normalize the texture to the user's configured or
    // auto-detected physical layout before handing it to the renderer.
    if configured_subpixel_layout() == SubpixelLayout::Bgr {
        for pixel in bytes.chunks_exact_mut(3) {
            pixel.swap(0, 2);
        }
    }
    bytes
}

fn configured_subpixel_layout() -> SubpixelLayout {
    // Keep the existing explicit override for embedders and tests. `auto` (and an
    // unset value) uses Windows' per-display ClearType registry keys.
    if let Some(layout) = std::env::var_os("RASSA_CROSSFONT_WINDOWS_SUBPIXEL")
        .and_then(|value| value.into_string().ok())
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "rgb" => Some(SubpixelLayout::Rgb),
            "bgr" => Some(SubpixelLayout::Bgr),
            "auto" => None,
            _ => None,
        })
    {
        return layout;
    }

    registry_subpixel_layout().unwrap_or(SubpixelLayout::Rgb)
}

fn registry_subpixel_layout() -> Option<SubpixelLayout> {
    // WPF/DirectWrite stores ClearType tuning under one key per display, for
    // example `DISPLAY1` and `DISPLAY2`. The rasterizer has no window or monitor
    // handle, so choose BGR if any configured display reports BGR; otherwise RGB
    // if at least one display reports RGB. Unknown/flat/missing values fall back
    // to DirectWrite's default RGB order.
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let graphics = hkcu
        .open_subkey("Software\\Microsoft\\Avalon.Graphics")
        .ok()?;

    let mut saw_rgb = false;
    for display_name in graphics.enum_keys().filter_map(Result::ok) {
        let Ok(display) = graphics.open_subkey(display_name) else {
            continue;
        };
        let Ok(pixel_structure) = display.get_value::<u32, _>("PixelStructure") else {
            continue;
        };
        match layout_from_registry_pixel_structure(pixel_structure) {
            Some(SubpixelLayout::Bgr) => return Some(SubpixelLayout::Bgr),
            Some(SubpixelLayout::Rgb) => saw_rgb = true,
            None => {}
        }
    }

    saw_rgb.then_some(SubpixelLayout::Rgb)
}

fn layout_from_registry_pixel_structure(value: u32) -> Option<SubpixelLayout> {
    match value {
        // Windows stores `0` for flat/no subpixel AA, `1` for RGB, and `2` for BGR.
        1 => Some(SubpixelLayout::Rgb),
        2 => Some(SubpixelLayout::Bgr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn registry_pixel_structure_values_map_to_layouts() {
        assert_eq!(layout_from_registry_pixel_structure(0), None);
        assert_eq!(
            layout_from_registry_pixel_structure(1),
            Some(SubpixelLayout::Rgb)
        );
        assert_eq!(
            layout_from_registry_pixel_structure(2),
            Some(SubpixelLayout::Bgr)
        );
        assert_eq!(layout_from_registry_pixel_structure(3), None);
    }

    #[test]
    fn env_override_normalizes_bgr_cleartype_bytes() {
        let _guard = ENV_LOCK.lock().expect("env test lock");
        let previous = std::env::var_os("RASSA_CROSSFONT_WINDOWS_SUBPIXEL");
        unsafe {
            std::env::set_var("RASSA_CROSSFONT_WINDOWS_SUBPIXEL", "bgr");
        }
        let normalized = normalize_cleartype_layout(vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(normalized, vec![3, 2, 1, 6, 5, 4]);
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("RASSA_CROSSFONT_WINDOWS_SUBPIXEL", value);
            } else {
                std::env::remove_var("RASSA_CROSSFONT_WINDOWS_SUBPIXEL");
            }
        }
    }
}
