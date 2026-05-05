//! Compatibility layer for different font engines.
//!
//! CoreText is used on macOS.
//! DirectWrite is used on Windows.
//! FreeType is used everywhere else.

#![allow(clippy::all, clippy::if_not_else, clippy::enum_glob_use)]
#![allow(mismatched_lifetime_syntaxes, unsafe_op_in_unsafe_fn, unused_imports)]

use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(not(any(target_os = "macos", windows, target_arch = "wasm32")))]
pub mod ft;
#[cfg(not(any(target_os = "macos", windows, target_arch = "wasm32")))]
pub use ft::FreeTypeRasterizer as Rasterizer;

#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
pub use web::WebRasterizer as Rasterizer;

#[cfg(windows)]
pub mod directwrite;
#[cfg(windows)]
pub use directwrite::DirectWriteRasterizer as Rasterizer;

#[cfg(target_os = "macos")]
pub mod darwin;
#[cfg(target_os = "macos")]
pub use darwin::CoreTextRasterizer as Rasterizer;

/// Max font size in pt.
///
/// The value is picked based on `u32` max, since we use 6 digits for fract.
const MAX_FONT_PT_SIZE: f32 = 3999.;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontDesc {
    name: String,
    style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slant {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Normal,
    Bold,
}

/// Style of font.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Style {
    Specific(String),
    Description { slant: Slant, weight: Weight },
}

impl fmt::Display for Style {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Style::Specific(ref s) => f.write_str(s),
            Style::Description { slant, weight } => {
                write!(f, "slant={slant:?}, weight={weight:?}")
            }
        }
    }
}

impl FontDesc {
    pub fn new<S>(name: S, style: Style) -> FontDesc
    where
        S: Into<String>,
    {
        FontDesc {
            name: name.into(),
            style,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn style(&self) -> &Style {
        &self.style
    }
}

impl fmt::Display for FontDesc {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} - {}", self.name, self.style)
    }
}

/// Identifier for a Font for use in maps/etc.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct FontKey {
    token: u32,
}

impl FontKey {
    /// Get next font key for given size.
    ///
    /// The generated key will be globally unique.
    pub fn next() -> FontKey {
        static TOKEN: AtomicUsize = AtomicUsize::new(0);

        FontKey {
            token: TOKEN.fetch_add(1, Ordering::SeqCst) as _,
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct GlyphKey {
    pub character: char,
    pub font_key: FontKey,
    pub size: Size,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct GlyphIdKey {
    pub glyph_id: u32,
    pub font_key: FontKey,
    pub size: Size,
}

#[derive(Debug, Copy, Clone, Default, PartialEq)]
pub struct ProportionalMetrics {
    pub advance_x: f32,
    pub advance_y: f32,
    pub bounds_x: f32,
    pub bounds_y: f32,
    pub bounds_width: f32,
    pub bounds_height: f32,
}

/// Font size stored as base and fraction.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Size(u32);

impl Size {
    /// Create a new `Size` from a f32 size in points.
    ///
    /// The font size is automatically clamped to supported range of `[1.; 3999.]` pt.
    pub fn new(size: f32) -> Size {
        let size = size.clamp(1., MAX_FONT_PT_SIZE);
        Size((size * Self::factor()) as u32)
    }

    /// Create a new `Size` from px.
    ///
    /// The value will be clamped to the pt range of [`Size::new`].
    pub fn from_px(size: f32) -> Self {
        let pt = size * 72. / 96.;
        Size::new(pt)
    }

    /// Scale font size by the given amount.
    pub fn scale(self, scale: f32) -> Self {
        Self::new(self.as_pt() * scale)
    }

    /// Get size in `px`.
    pub fn as_px(self) -> f32 {
        self.as_pt() * 96. / 72.
    }

    /// Get the size in `pt`.
    pub fn as_pt(self) -> f32 {
        (f64::from(self.0) / Size::factor() as f64) as f32
    }

    /// Scale factor between font "Size" type and point size.
    #[inline]
    fn factor() -> f32 {
        1_000_000.
    }
}

#[derive(Debug, Clone)]
pub struct RasterizedGlyph {
    pub character: char,
    pub width: i32,
    pub height: i32,
    pub top: i32,
    pub left: i32,
    pub advance: (i32, i32),
    pub buffer: BitmapBuffer,
}

#[derive(Clone, Debug)]
pub enum BitmapBuffer {
    /// RGB alphamask.
    Rgb(Vec<u8>),

    /// RGBA pixels with premultiplied alpha.
    Rgba(Vec<u8>),
}

impl Default for RasterizedGlyph {
    fn default() -> RasterizedGlyph {
        RasterizedGlyph {
            character: ' ',
            width: 0,
            height: 0,
            top: 0,
            left: 0,
            advance: (0, 0),
            buffer: BitmapBuffer::Rgb(Vec::new()),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Metrics {
    pub average_advance: f64,
    pub line_height: f64,
    pub descent: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub strikeout_position: f32,
    pub strikeout_thickness: f32,
}

/// Errors occuring when using the rasterizer.
#[derive(Debug)]
pub enum Error {
    /// Unable to find a font matching the description.
    FontNotFound(FontDesc),

    /// Unable to find metrics for a font face.
    MetricsNotFound,

    /// The glyph could not be found in any font.
    MissingGlyph(RasterizedGlyph),

    /// Requested an operation with a FontKey that isn't known to the rasterizer.
    UnknownFontKey,

    /// Error from platfrom's font system.
    PlatformError(String),

    /// Requested operation is not available on this backend.
    Unsupported(&'static str),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::FontNotFound(font) => write!(f, "font {font:?} not found"),
            Error::MissingGlyph(glyph) => {
                write!(f, "glyph for character {:?} not found", glyph.character)
            }
            Error::UnknownFontKey => f.write_str("invalid font key"),
            Error::MetricsNotFound => f.write_str("metrics not found"),
            Error::PlatformError(err) => write!(f, "{err}"),
            Error::Unsupported(op) => write!(f, "operation is not supported by this backend: {op}"),
        }
    }
}

pub trait Rasterize {
    /// Create a new Rasterizer.
    fn new() -> Result<Self, Error>
    where
        Self: Sized;

    /// Get `Metrics` for the given `FontKey`.
    fn metrics(&self, _: FontKey, _: Size) -> Result<Metrics, Error>;

    /// Get proportional metrics for a pre-shaped glyph id.
    fn glyph_id_metrics(&mut self, glyph: GlyphIdKey) -> Result<ProportionalMetrics, Error> {
        let glyph = self.get_glyph_id(glyph)?;
        Ok(ProportionalMetrics {
            advance_x: glyph.advance.0 as f32,
            advance_y: glyph.advance.1 as f32,
            bounds_x: glyph.left as f32,
            bounds_y: glyph.top as f32,
            bounds_width: glyph.width as f32,
            bounds_height: glyph.height as f32,
        })
    }

    /// Load the font described by `FontDesc` and `Size`.
    fn load_font(&mut self, _: &FontDesc, _: Size) -> Result<FontKey, Error>;

    /// Load a concrete font file from disk.
    fn load_font_path(&mut self, _: &Path, _: Size) -> Result<FontKey, Error> {
        Err(Error::Unsupported("load_font_path"))
    }

    /// Load a concrete font from in-memory bytes.
    fn load_font_bytes(&mut self, _: &[u8], _: Size) -> Result<FontKey, Error> {
        Err(Error::Unsupported("load_font_bytes"))
    }

    /// Rasterize the glyph described by `GlyphKey`..
    fn get_glyph(&mut self, _: GlyphKey) -> Result<RasterizedGlyph, Error>;

    /// Rasterize a pre-shaped glyph id from a loaded face.
    fn get_glyph_id(&mut self, glyph: GlyphIdKey) -> Result<RasterizedGlyph, Error> {
        let character = char::from_u32(glyph.glyph_id).ok_or_else(|| {
            Error::PlatformError(format!(
                "glyph id {} has no Unicode fallback",
                glyph.glyph_id
            ))
        })?;
        self.get_glyph(GlyphKey {
            character,
            font_key: glyph.font_key,
            size: glyph.size,
        })
    }

    /// Drop one loaded face and backend-owned cached data for it.
    fn drop_font(&mut self, _: FontKey) -> Result<(), Error> {
        Err(Error::Unsupported("drop_font"))
    }

    /// Evict all backend-owned caches.
    fn evict_cache(&mut self) {}

    /// Kerning between two characters.
    fn kerning(&mut self, left: GlyphKey, right: GlyphKey) -> (f32, f32);
}
