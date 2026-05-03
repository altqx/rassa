pub mod ass {
    pub const LIBASS_VERSION: i32 = 0x0170_4000;

    pub const VALIGN_SUB: i32 = 0;
    pub const VALIGN_CENTER: i32 = 8;
    pub const VALIGN_TOP: i32 = 4;
    pub const HALIGN_LEFT: i32 = 1;
    pub const HALIGN_CENTER: i32 = 2;
    pub const HALIGN_RIGHT: i32 = 3;
    pub const ASS_JUSTIFY_AUTO: i32 = 0;
    pub const ASS_JUSTIFY_LEFT: i32 = 1;
    pub const ASS_JUSTIFY_CENTER: i32 = 2;
    pub const ASS_JUSTIFY_RIGHT: i32 = 3;

    pub const FONT_WEIGHT_LIGHT: i32 = 300;
    pub const FONT_WEIGHT_MEDIUM: i32 = 400;
    pub const FONT_WEIGHT_BOLD: i32 = 700;
    pub const FONT_SLANT_NONE: i32 = 0;
    pub const FONT_SLANT_ITALIC: i32 = 100;
    pub const FONT_SLANT_OBLIQUE: i32 = 110;
    pub const FONT_WIDTH_CONDENSED: i32 = 75;
    pub const FONT_WIDTH_NORMAL: i32 = 100;
    pub const FONT_WIDTH_EXPANDED: i32 = 125;

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
    pub enum Hinting {
        #[default]
        None = 0,
        Light = 1,
        Normal = 2,
        Native = 3,
    }

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ShapingLevel {
        Simple = 0,
        Complex = 1,
    }

    impl Default for ShapingLevel {
        fn default() -> Self {
            Self::Complex
        }
    }

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum DefaultFontProvider {
        #[default]
        None = 0,
        Autodetect = 1,
        CoreText = 2,
        Fontconfig = 3,
        DirectWrite = 4,
    }

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Feature {
        IncompatibleExtensions = 0,
        BidiBrackets = 1,
        WholeTextLayout = 2,
        WrapUnicode = 3,
    }

    pub mod override_bits {
        pub const DEFAULT: i32 = 0;
        pub const STYLE: i32 = 1 << 0;
        pub const SELECTIVE_FONT_SCALE: i32 = 1 << 1;
        pub const FONT_SIZE: i32 = 1 << 1;
        pub const FONT_SIZE_FIELDS: i32 = 1 << 2;
        pub const FONT_NAME: i32 = 1 << 3;
        pub const COLORS: i32 = 1 << 4;
        pub const ATTRIBUTES: i32 = 1 << 5;
        pub const BORDER: i32 = 1 << 6;
        pub const ALIGNMENT: i32 = 1 << 7;
        pub const MARGINS: i32 = 1 << 8;
        pub const FULL_STYLE: i32 = 1 << 9;
        pub const JUSTIFY: i32 = 1 << 10;
        pub const BLUR: i32 = 1 << 11;
    }

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum YCbCrMatrix {
        #[default]
        Default = 0,
        Unknown = 1,
        None = 2,
        Bt601Tv = 3,
        Bt601Pc = 4,
        Bt709Tv = 5,
        Bt709Pc = 6,
        Smpte240mTv = 7,
        Smpte240mPc = 8,
        FccTv = 9,
        FccPc = 10,
    }

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum TrackType {
        #[default]
        Unknown = 0,
        Ass = 1,
        Ssa = 2,
    }

    #[repr(i32)]
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub enum ImageType {
        #[default]
        Character = 0,
        Outline = 1,
        Shadow = 2,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x_min: i32,
    pub y_min: i32,
    pub x_max: i32,
    pub y_max: i32,
}

impl Rect {
    pub fn width(self) -> i32 {
        (self.x_max - self.x_min).max(0)
    }

    pub fn height(self) -> i32 {
        (self.y_max - self.y_min).max(0)
    }

    pub fn is_empty(self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let rect = Self {
            x_min: self.x_min.max(other.x_min),
            y_min: self.y_min.max(other.y_min),
            x_max: self.x_max.min(other.x_max),
            y_max: self.y_max.min(other.y_max),
        };
        (!rect.is_empty()).then_some(rect)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margins {
    pub top: i32,
    pub bottom: i32,
    pub left: i32,
    pub right: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RgbaColor(pub u32);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImagePlane {
    pub size: Size,
    pub stride: i32,
    pub color: RgbaColor,
    pub destination: Point,
    pub kind: ass::ImageType,
    pub bitmap: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RendererConfig {
    pub frame: Size,
    pub storage: Size,
    pub margins: Margins,
    pub use_margins: bool,
    pub pixel_aspect: f64,
    pub font_scale: f64,
    pub line_spacing: f64,
    pub line_position: f64,
    pub hinting: ass::Hinting,
    pub shaping: ass::ShapingLevel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RassaError {
    message: String,
}

impl RassaError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for RassaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RassaError {}

pub type RassaResult<T> = Result<T, RassaError>;
