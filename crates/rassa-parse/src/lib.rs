use rassa_core::{
    Point, RassaError, RassaResult, Rect,
    ass::{self, TrackType, YCbCrMatrix},
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedAttachment {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedStyle {
    pub name: String,
    pub font_name: String,
    pub font_size: f64,
    pub primary_colour: u32,
    pub secondary_colour: u32,
    pub outline_colour: u32,
    pub back_colour: u32,
    pub bold: bool,
    pub font_weight: i32,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub scale_x: f64,
    pub scale_y: f64,
    pub spacing: f64,
    pub angle: f64,
    pub border_style: i32,
    pub outline: f64,
    pub shadow: f64,
    pub alignment: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub encoding: i32,
    pub treat_fontname_as_pattern: i32,
    pub blur: f64,
    pub justify: i32,
}

impl Default for ParsedStyle {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            font_name: "Arial".to_string(),
            font_size: 18.0,
            primary_colour: 0x00ff_ffff,
            secondary_colour: 0x00ff_ff00,
            outline_colour: 0x0000_0000,
            back_colour: 0x8000_0000,
            bold: false,
            font_weight: 200,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: 1.0,
            scale_y: 1.0,
            spacing: 0.0,
            angle: 0.0,
            border_style: 1,
            outline: 2.0,
            shadow: 3.0,
            alignment: ass::VALIGN_SUB | ass::HALIGN_CENTER,
            margin_l: 20,
            margin_r: 20,
            margin_v: 20,
            encoding: 0,
            treat_fontname_as_pattern: 0,
            blur: 0.0,
            justify: ass::ASS_JUSTIFY_AUTO,
        }
    }
}

impl ParsedStyle {
    fn parsed_line_seed() -> Self {
        Self {
            name: String::new(),
            font_name: String::new(),
            font_size: 0.0,
            primary_colour: 0,
            secondary_colour: 0,
            outline_colour: 0,
            back_colour: 0,
            bold: false,
            font_weight: 400,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: 1.0,
            scale_y: 1.0,
            spacing: 0.0,
            angle: 0.0,
            border_style: 0,
            outline: 0.0,
            shadow: 0.0,
            alignment: 0,
            margin_l: 0,
            margin_r: 0,
            margin_v: 0,
            encoding: 0,
            treat_fontname_as_pattern: 0,
            blur: 0.0,
            justify: 0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedEvent {
    pub start: i64,
    pub duration: i64,
    pub read_order: i32,
    pub layer: i32,
    pub style: i32,
    pub name: String,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub effect: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSpanStyle {
    pub font_name: String,
    pub encoding: i32,
    pub font_size: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub spacing: f64,
    pub underline: bool,
    pub strike_out: bool,
    pub rotation_x: f64,
    pub rotation_y: f64,
    pub rotation_z: f64,
    pub shear_x: f64,
    pub shear_y: f64,
    pub bold: bool,
    pub font_weight: i32,
    pub italic: bool,
    pub primary_colour: u32,
    pub secondary_colour: u32,
    pub outline_colour: u32,
    pub back_colour: u32,
    pub border: f64,
    pub border_x: f64,
    pub border_y: f64,
    pub shadow: f64,
    pub shadow_x: f64,
    pub shadow_y: f64,
    pub blur: f64,
    pub be: f64,
    pub pbo: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedAnimatedStyle {
    pub font_size: Option<f64>,
    pub font_size_steps: Vec<ParsedFontSizeTransform>,
    pub scale_x: Option<f64>,
    pub scale_x_steps: Vec<ParsedScaleTransform>,
    pub scale_y: Option<f64>,
    pub scale_y_steps: Vec<ParsedScaleTransform>,
    pub spacing: Option<f64>,
    pub spacing_steps: Vec<ParsedLinearTransform>,
    pub rotation_x: Option<f64>,
    pub rotation_x_steps: Vec<ParsedLinearTransform>,
    pub rotation_y: Option<f64>,
    pub rotation_y_steps: Vec<ParsedLinearTransform>,
    pub rotation_z: Option<f64>,
    pub rotation_z_steps: Vec<ParsedLinearTransform>,
    pub shear_x: Option<f64>,
    pub shear_x_steps: Vec<ParsedLinearTransform>,
    pub shear_y: Option<f64>,
    pub shear_y_steps: Vec<ParsedLinearTransform>,
    pub primary_colour: Option<u32>,
    pub primary_colour_steps: Vec<ParsedColourTransform>,
    pub secondary_colour: Option<u32>,
    pub secondary_colour_steps: Vec<ParsedColourTransform>,
    pub outline_colour: Option<u32>,
    pub outline_colour_steps: Vec<ParsedColourTransform>,
    pub back_colour: Option<u32>,
    pub back_colour_steps: Vec<ParsedColourTransform>,
    pub border: Option<f64>,
    pub border_x: Option<f64>,
    pub border_x_steps: Vec<ParsedAxisTransform>,
    pub border_y: Option<f64>,
    pub border_y_steps: Vec<ParsedAxisTransform>,
    pub shadow: Option<f64>,
    pub shadow_x: Option<f64>,
    pub shadow_x_steps: Vec<ParsedAxisTransform>,
    pub shadow_y: Option<f64>,
    pub shadow_y_steps: Vec<ParsedAxisTransform>,
    pub blur: Option<f64>,
    pub blur_steps: Vec<ParsedLinearTransform>,
    pub be: Option<f64>,
    pub be_steps: Vec<ParsedLinearTransform>,
    /// Animated rectangular \clip target (\t interpolates rect clips; vector clips never animate).
    pub clip_rect: Option<ParsedRectF64>,
    pub clip_inverse: Option<bool>,
}

impl ParsedAnimatedStyle {
    fn is_empty(&self) -> bool {
        self.font_size.is_none()
            && self.font_size_steps.is_empty()
            && self.scale_x.is_none()
            && self.scale_x_steps.is_empty()
            && self.scale_y.is_none()
            && self.scale_y_steps.is_empty()
            && self.spacing.is_none()
            && self.spacing_steps.is_empty()
            && self.rotation_x.is_none()
            && self.rotation_x_steps.is_empty()
            && self.rotation_y.is_none()
            && self.rotation_y_steps.is_empty()
            && self.rotation_z.is_none()
            && self.rotation_z_steps.is_empty()
            && self.shear_x.is_none()
            && self.shear_x_steps.is_empty()
            && self.shear_y.is_none()
            && self.shear_y_steps.is_empty()
            && self.primary_colour.is_none()
            && self.primary_colour_steps.is_empty()
            && self.secondary_colour.is_none()
            && self.secondary_colour_steps.is_empty()
            && self.outline_colour.is_none()
            && self.outline_colour_steps.is_empty()
            && self.back_colour.is_none()
            && self.back_colour_steps.is_empty()
            && self.border.is_none()
            && self.border_x.is_none()
            && self.border_x_steps.is_empty()
            && self.border_y.is_none()
            && self.border_y_steps.is_empty()
            && self.shadow.is_none()
            && self.shadow_x.is_none()
            && self.shadow_x_steps.is_empty()
            && self.shadow_y.is_none()
            && self.shadow_y_steps.is_empty()
            && self.blur.is_none()
            && self.blur_steps.is_empty()
            && self.be.is_none()
            && self.be_steps.is_empty()
            && self.clip_rect.is_none()
            && self.clip_inverse.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParsedFontSizeTransform {
    Reset { reset: f64 },
    Absolute { value: f64, reset: f64 },
    Relative { value: f64, reset: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParsedScaleTransform {
    Reset { reset: f64 },
    Absolute { value: f64, reset: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParsedLinearTransform {
    Reset { reset: f64 },
    Absolute { value: f64, reset: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParsedAxisTransform {
    Reset { reset: f64 },
    Absolute { value: f64, reset: f64, clamp: bool },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParsedColourTransform {
    ResetRgb { reset: u32 },
    Rgb { value: u32 },
    ResetAlpha { reset: u8 },
    Alpha { value: i32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSpanTransform {
    pub start_ms: i32,
    pub end_ms: Option<i32>,
    pub accel: f64,
    pub style: ParsedAnimatedStyle,
}

impl Default for ParsedSpanStyle {
    fn default() -> Self {
        Self {
            font_name: ParsedStyle::default().font_name,
            encoding: ParsedStyle::default().encoding,
            font_size: ParsedStyle::default().font_size,
            scale_x: ParsedStyle::default().scale_x,
            scale_y: ParsedStyle::default().scale_y,
            spacing: ParsedStyle::default().spacing,
            underline: false,
            strike_out: false,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: ParsedStyle::default().angle,
            shear_x: 0.0,
            shear_y: 0.0,
            bold: false,
            font_weight: 400,
            italic: false,
            primary_colour: ParsedStyle::default().primary_colour,
            secondary_colour: ParsedStyle::default().secondary_colour,
            outline_colour: ParsedStyle::default().outline_colour,
            back_colour: ParsedStyle::default().back_colour,
            border: ParsedStyle::default().outline,
            border_x: ParsedStyle::default().outline,
            border_y: ParsedStyle::default().outline,
            shadow: ParsedStyle::default().shadow,
            shadow_x: ParsedStyle::default().shadow,
            shadow_y: ParsedStyle::default().shadow,
            blur: ParsedStyle::default().blur,
            be: 0.0,
            pbo: 0.0,
        }
    }
}

impl ParsedSpanStyle {
    fn from_style(style: &ParsedStyle) -> Self {
        Self {
            font_name: style.font_name.clone(),
            encoding: style.encoding,
            font_size: style.font_size,
            scale_x: style.scale_x,
            scale_y: style.scale_y,
            spacing: style.spacing,
            underline: style.underline,
            strike_out: style.strike_out,
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: style.angle,
            shear_x: 0.0,
            shear_y: 0.0,
            bold: style.bold,
            font_weight: style.font_weight,
            italic: style.italic,
            primary_colour: style.primary_colour,
            secondary_colour: style.secondary_colour,
            outline_colour: style.outline_colour,
            back_colour: style.back_colour,
            border: style.outline,
            border_x: style.outline,
            border_y: style.outline,
            shadow: style.shadow,
            shadow_x: style.shadow,
            shadow_y: style.shadow,
            blur: style.blur,
            be: 0.0,
            pbo: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedTextSpan {
    pub text: String,
    pub style: ParsedSpanStyle,
    pub transforms: Vec<ParsedSpanTransform>,
    pub karaoke: Option<ParsedKaraokeSpan>,
    pub drawing: Option<ParsedDrawing>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedTextLine {
    pub text: String,
    pub spans: Vec<ParsedTextSpan>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedDialogueText {
    pub lines: Vec<ParsedTextLine>,
    pub hard_override: bool,
    pub transform_disables_collision: bool,
    pub alignment: Option<i32>,
    pub position: Option<(i32, i32)>,
    pub position_exact: Option<(f64, f64)>,
    pub movement: Option<ParsedMovement>,
    pub movement_exact: Option<ParsedMovementExact>,
    pub fade: Option<ParsedFade>,
    pub clip_rect: Option<Rect>,
    pub clip_rect_exact: Option<ParsedRectF64>,
    pub vector_clip: Option<ParsedVectorClip>,
    pub inverse_clip: bool,
    pub vector_clip_inverse: bool,
    pub wrap_style: Option<i32>,
    pub origin: Option<(i32, i32)>,
    pub origin_exact: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParsedMovement {
    pub start: (i32, i32),
    pub end: (i32, i32),
    pub t1_ms: i32,
    pub t2_ms: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParsedMovementExact {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub t1_ms: i32,
    pub t2_ms: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ParsedRectF64 {
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParsedFade {
    Simple {
        fade_in_ms: i32,
        fade_out_ms: i32,
    },
    Complex {
        alpha1: i32,
        alpha2: i32,
        alpha3: i32,
        t1_ms: i32,
        t2_ms: i32,
        t3_ms: i32,
        t4_ms: i32,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParsedKaraokeMode {
    #[default]
    FillSwap,
    Sweep,
    OutlineToggle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParsedKaraokeSpan {
    pub start_ms: i32,
    pub duration_ms: i32,
    pub mode: ParsedKaraokeMode,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedVectorClip {
    pub scale: i32,
    pub polygons: Vec<Vec<Point>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedDrawing {
    pub scale: i32,
    pub polygons: Vec<Vec<Point>>,
}

/// Signed 26.6 outline domain `[-OUTLINE_MAX, OUTLINE_MAX]`; reject hostile drawings before integer conversion.
pub const LIBASS_OUTLINE_MAX_D6: i32 = (1_i32 << 28) - 1;

// Integer-pixel ceiling: a valid fractional 26.6 coordinate can round up to this.
const RASSA_OUTLINE_MAX_COORD: i64 = (LIBASS_OUTLINE_MAX_D6 as i64 + ((1_i64 << 6) - 1)) >> 6;

/// Convert to 26.6 and enforce ass_outline_add_point's outline range.
pub fn libass_drawing_coordinate_to_d6(value: f64) -> Option<i32> {
    let scaled = value * 64.0;
    if !scaled.is_finite() {
        return None;
    }
    // double_to_d6 / ass_lrint FE_TONEAREST: exact half-D6 values pick the even integer, including negatives.
    let rounded = scaled.round_ties_even();
    if rounded < -f64::from(LIBASS_OUTLINE_MAX_D6) || rounded > f64::from(LIBASS_OUTLINE_MAX_D6) {
        return None;
    }
    Some(rounded as i32)
}

/// Round a transformed outline coordinate to integer pixels; reject values outside the outline domain.
pub fn libass_outline_coordinate_from_f64(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let rounded = value.round();
    if rounded < -(RASSA_OUTLINE_MAX_COORD as f64) || rounded > RASSA_OUTLINE_MAX_COORD as f64 {
        return None;
    }
    Some(rounded as i32)
}

pub fn libass_outline_point_is_valid(point: Point) -> bool {
    let x = i64::from(point.x);
    let y = i64::from(point.y);
    (-RASSA_OUTLINE_MAX_COORD..=RASSA_OUTLINE_MAX_COORD).contains(&x)
        && (-RASSA_OUTLINE_MAX_COORD..=RASSA_OUTLINE_MAX_COORD).contains(&y)
}

impl ParsedVectorClip {
    pub fn bounds(&self) -> Option<Rect> {
        bounds_from_polygons(&self.polygons)
    }
}

impl ParsedDrawing {
    pub fn bounds(&self) -> Option<Rect> {
        bounds_from_polygons(&self.polygons)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParsedTrack {
    pub styles: Vec<ParsedStyle>,
    pub events: Vec<ParsedEvent>,
    pub attachments: Vec<ParsedAttachment>,
    pub style_format: String,
    pub event_format: String,
    pub track_type: TrackType,
    pub play_res_x: i32,
    pub play_res_y: i32,
    pub timer: f64,
    pub wrap_style: i32,
    pub scaled_border_and_shadow: bool,
    pub kerning: bool,
    pub language: String,
    pub ycbcr_matrix: YCbCrMatrix,
    pub default_style: i32,
    pub layout_res_x: i32,
    pub layout_res_y: i32,
}

impl Default for ParsedTrack {
    fn default() -> Self {
        Self {
            styles: vec![ParsedStyle::default()],
            events: Vec::new(),
            attachments: Vec::new(),
            style_format: String::new(),
            event_format: String::new(),
            track_type: TrackType::Unknown,
            play_res_x: 0,
            play_res_y: 0,
            timer: 100.0,
            wrap_style: 0,
            scaled_border_and_shadow: false,
            kerning: true,
            language: String::new(),
            ycbcr_matrix: YCbCrMatrix::Default,
            default_style: 0,
            layout_res_x: 0,
            layout_res_y: 0,
        }
    }
}

const SINFO_LANGUAGE: u16 = 1 << 0;
const SINFO_PLAYRESX: u16 = 1 << 1;
const SINFO_PLAYRESY: u16 = 1 << 2;
const SINFO_TIMER: u16 = 1 << 3;
const SINFO_WRAPSTYLE: u16 = 1 << 4;
const SINFO_SCALEDBORDER: u16 = 1 << 5;
const SINFO_COLOURMATRIX: u16 = 1 << 6;
const SINFO_KERNING: u16 = 1 << 7;
const SINFO_SCRIPTTYPE: u16 = 1 << 8;
const SINFO_LAYOUTRESX: u16 = 1 << 9;
const SINFO_LAYOUTRESY: u16 = 1 << 10;
const GENBY_FFMPEG: u16 = 1 << 11;

pub fn parse_script_bytes(bytes: &[u8]) -> RassaResult<ParsedTrack> {
    parse_script_bytes_with_codepage(bytes, None)
}

pub fn parse_script_bytes_with_codepage(
    bytes: &[u8],
    codepage: Option<&str>,
) -> RassaResult<ParsedTrack> {
    let text = decode_script_bytes_with_codepage(bytes, codepage)?;
    parse_script_text(&text)
}

pub fn parse_style_section_bytes_with_codepage(
    bytes: &[u8],
    codepage: Option<&str>,
    track_type: TrackType,
) -> RassaResult<ParsedTrack> {
    let text = decode_script_bytes_with_codepage(bytes, codepage)?;
    let section = if track_type == TrackType::Ssa {
        "[V4 Styles]\n"
    } else {
        "[V4+ Styles]\n"
    };
    parse_script_text(&format!("{section}{text}"))
}

fn decode_script_bytes_with_codepage(bytes: &[u8], codepage: Option<&str>) -> RassaResult<String> {
    if let Some(codepage) = codepage {
        let text = iconv_native::decode(bytes, codepage).map_err(|error| {
            RassaError::new(format!(
                "failed to decode subtitle data from codepage {codepage:?}: {error}"
            ))
        })?;
        return Ok(text);
    }

    Ok(match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    })
}

pub fn parse_script_text(text: &str) -> RassaResult<ParsedTrack> {
    let mut track = ParsedTrack::default();
    let mut section = String::new();
    let mut style_format: Vec<String> = Vec::new();
    let mut event_format: Vec<String> = Vec::new();
    let mut style_format_seen = false;
    let mut event_format_seen = false;
    let mut pending_font_name: Option<String> = None;
    let mut pending_font_data = String::new();
    let mut scaled_border_and_shadow_explicit = false;
    let mut script_info_flags = 0_u16;

    for raw_line in ass_text_lines(text) {
        let line = trim_ass_line_bom(raw_line);
        let line = trim_ass_leading_spaces(line);
        if line.is_empty() {
            continue;
        }
        if section == "script info"
            && line
                .strip_prefix("; Script generated by ")
                .is_some_and(|generator| generator.starts_with("FFmpeg/Lavc"))
        {
            script_info_flags |= GENBY_FFMPEG;
            continue;
        }

        if let Some(next_section) = parse_section_header(line) {
            flush_font_attachment(&mut track, &mut pending_font_name, &mut pending_font_data);
            section.clear();
            section.push_str(next_section);
            if section == "v4+ styles" {
                track.track_type = TrackType::Ass;
            } else if section == "v4 styles" {
                track.track_type = TrackType::Ssa;
            }
            continue;
        }

        if section == "fonts" {
            process_font_line(
                line,
                &mut track,
                &mut pending_font_name,
                &mut pending_font_data,
            );
            continue;
        }

        if line.starts_with(';') {
            continue;
        }

        if section == "script info" {
            let Some((key, value)) = split_once_colon_raw(line) else {
                continue;
            };
            script_info_flags |= script_info_flag(key);
            if key == "ScaledBorderAndShadow" {
                scaled_border_and_shadow_explicit = true;
            }
            apply_script_info_field(&mut track, key, value);
            continue;
        }

        let Some((key, value)) = split_once_colon_raw(line) else {
            continue;
        };

        match section.as_str() {
            "v4+ styles" | "v4 styles" => {
                if key == "Format" {
                    let value = trim_ass_leading_spaces(value);
                    style_format_seen = true;
                    track.style_format = value.to_string();
                    if !scaled_border_and_shadow_explicit
                        && !format_matches_libass_standard(
                            value,
                            &default_style_format(track.track_type),
                        )
                    {
                        track.scaled_border_and_shadow = true;
                    }
                    style_format = parse_format_fields(value);
                } else if key == "Style" {
                    if !style_format_seen {
                        style_format = default_style_format(track.track_type);
                        if track.style_format.is_empty() {
                            track.style_format = style_format.join(", ");
                        }
                    }
                    if let Some(style) = parse_style_line(value, &style_format, track.track_type) {
                        if style.name == "Default" {
                            track.default_style = track.styles.len() as i32;
                        }
                        track.styles.push(style);
                    }
                }
            }
            "events" => {
                if key == "Format" {
                    let value = trim_ass_leading_spaces(value);
                    event_format_seen = true;
                    track.event_format = value.to_string();
                    if !scaled_border_and_shadow_explicit
                        && !format_matches_libass_standard(
                            value,
                            &default_event_format(track.track_type),
                        )
                    {
                        track.scaled_border_and_shadow = true;
                    }
                    if detect_legacy_ffmpeg_subs(&track, script_info_flags) {
                        track.scaled_border_and_shadow = true;
                    }
                    event_format = parse_format_fields(value);
                } else if key == "Dialogue" {
                    let value = trim_ass_leading_spaces(value);
                    if !event_format_seen {
                        event_format = default_event_format(track.track_type);
                        if track.event_format.is_empty() {
                            track.event_format = event_format.join(", ");
                        }
                    }
                    if let Some(event) = parse_event_line(
                        value,
                        &event_format,
                        track.events.len() as i32,
                        &track.styles,
                        track.default_style,
                    ) {
                        track.events.push(event);
                    }
                }
            }
            _ => {}
        }
    }

    flush_font_attachment(&mut track, &mut pending_font_name, &mut pending_font_data);

    if track.styles.is_empty() {
        track.styles.push(ParsedStyle::default());
    }

    if !style_format_seen && track.style_format.is_empty() {
        track.style_format = default_style_format(track.track_type).join(", ");
    }
    if !event_format_seen && track.event_format.is_empty() {
        track.event_format = default_event_format(track.track_type).join(", ");
    }
    apply_play_res_fallback(&mut track);

    Ok(track)
}

fn script_info_flag(key: &str) -> u16 {
    match key {
        "PlayResX" => SINFO_PLAYRESX,
        "PlayResY" => SINFO_PLAYRESY,
        "Timer" => SINFO_TIMER,
        "WrapStyle" => SINFO_WRAPSTYLE,
        "ScaledBorderAndShadow" => SINFO_SCALEDBORDER,
        "Kerning" => SINFO_KERNING,
        "Language" => SINFO_LANGUAGE,
        "LayoutResX" => SINFO_LAYOUTRESX,
        "LayoutResY" => SINFO_LAYOUTRESY,
        "YCbCr Matrix" => SINFO_COLOURMATRIX,
        "ScriptType" => SINFO_SCRIPTTYPE,
        _ => 0,
    }
}

fn detect_legacy_ffmpeg_subs(track: &ParsedTrack, script_info_flags: u16) -> bool {
    script_info_flags == (SINFO_SCRIPTTYPE | SINFO_PLAYRESX | SINFO_PLAYRESY | GENBY_FFMPEG)
        && track.styles.len() == 2
        && track.styles[1].name.starts_with("Default")
}

fn apply_play_res_fallback(track: &mut ParsedTrack) {
    if track.play_res_x > 0 && track.play_res_y > 0 {
        return;
    }
    if track.play_res_x <= 0 && track.play_res_y <= 0 {
        track.play_res_x = 384;
        track.play_res_y = 288;
    } else if track.play_res_y <= 0 {
        track.play_res_y = if track.play_res_x == 1280 {
            1024
        } else {
            let play_res_x = track.play_res_x as u32;
            ((play_res_x - 1) - (play_res_x - 1) / 4).max(1) as i32
        };
    } else if track.play_res_x <= 0 {
        track.play_res_x = if track.play_res_y == 1024 {
            1280
        } else {
            let derived = i64::from(track.play_res_y) + i64::from(track.play_res_y) / 3;
            derived.min(i64::from(i32::MAX)) as i32
        };
    }
}

fn trim_ass_leading_spaces(value: &str) -> &str {
    value.trim_start_matches([' ', '\t'])
}

fn trim_ass_trailing_spaces(value: &str) -> &str {
    value.trim_end_matches([' ', '\t'])
}

fn trim_ass_c_leading_spaces(value: &str) -> &str {
    value.trim_start_matches([' ', '\t', '\n', '\r', '\u{000b}', '\u{000c}'])
}

fn next_format_token<'a>(input: &mut &'a str) -> Option<&'a str> {
    *input = trim_ass_leading_spaces(input);
    if input.is_empty() {
        return None;
    }

    if let Some((head, tail)) = input.split_once(',') {
        *input = tail;
        Some(trim_ass_trailing_spaces(head))
    } else {
        let token = trim_ass_trailing_spaces(input);
        *input = "";
        Some(token)
    }
}

fn process_font_line(
    line: &str,
    track: &mut ParsedTrack,
    pending_font_name: &mut Option<String>,
    pending_font_data: &mut String,
) {
    if let Some(name) = line.strip_prefix("fontname:") {
        flush_font_attachment(track, pending_font_name, pending_font_data);
        *pending_font_name = Some(trim_ass_leading_spaces(name).to_string());
        return;
    }

    if pending_font_name.is_some() {
        pending_font_data.push_str(line);
    }
}

fn flush_font_attachment(
    track: &mut ParsedTrack,
    pending_font_name: &mut Option<String>,
    pending_font_data: &mut String,
) {
    let Some(name) = pending_font_name.take() else {
        pending_font_data.clear();
        return;
    };

    let encoded = std::mem::take(pending_font_data);
    if let Some(data) = decode_embedded_font(&encoded) {
        track.attachments.push(ParsedAttachment { name, data });
    }
}

fn decode_embedded_font(encoded: &str) -> Option<Vec<u8>> {
    if encoded.is_empty() {
        return Some(Vec::new());
    }
    if encoded.len() % 4 == 1 {
        return None;
    }

    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(encoded.len() / 4 * 3 + encoded.len() % 4);
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        decode_chars(&bytes[offset..offset + 4], &mut decoded);
        offset += 4;
    }
    match bytes.len() - offset {
        0 => {}
        2 => decode_chars(&bytes[offset..offset + 2], &mut decoded),
        3 => decode_chars(&bytes[offset..offset + 3], &mut decoded),
        _ => return None,
    }

    Some(decoded)
}

fn decode_chars(src: &[u8], dst: &mut Vec<u8>) {
    let mut value = 0_u32;
    for (index, byte) in src.iter().enumerate() {
        value |= u32::from(byte.wrapping_sub(33) & 63) << (6 * (3 - index));
    }

    dst.push((value >> 16) as u8);
    if src.len() >= 3 {
        dst.push(((value >> 8) & 0xFF) as u8);
    }
    if src.len() >= 4 {
        dst.push((value & 0xFF) as u8);
    }
}

pub fn parse_dialogue_text(
    text: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
) -> ParsedDialogueText {
    parse_dialogue_text_with_wrap_style(text, base_style, styles, 0)
}

pub fn parse_dialogue_text_with_wrap_style(
    text: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
    inherited_wrap_style: i32,
) -> ParsedDialogueText {
    let mut parsed = ParsedDialogueText::default();
    let mut current_wrap_style = inherited_wrap_style;
    let mut current_style = ParsedSpanStyle::from_style(base_style);
    let mut active_reset_style = current_style.clone();
    let mut active_reset_alignment = base_style.alignment;
    let mut active_line = ParsedTextLine::default();
    let mut buffer = String::new();
    let mut pending_karaoke = None;
    let mut deferred_karaoke = None;
    let mut karaoke_cursor_ms = 0;
    let mut drawing_scale = 0;
    let mut current_transforms = Vec::new();
    let inherited_wrap_style = current_wrap_style;
    let mut vector_clip_claimed = false;
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '{' => {
                let remaining: String = characters.clone().collect();
                if !remaining.contains('}') {
                    if block_has_libass_hard_override(&remaining) {
                        parsed.hard_override = true;
                    }
                    buffer.push(character);
                    continue;
                }

                if drawing_scale > 0 {
                    flush_span_for_run_break(
                        &mut buffer,
                        &current_style,
                        &mut pending_karaoke,
                        &mut deferred_karaoke,
                        drawing_scale,
                        &current_transforms,
                        &mut active_line,
                    );
                }

                let mut tag_block = String::new();
                for next in characters.by_ref() {
                    if next == '}' {
                        break;
                    }
                    tag_block.push(next);
                }
                apply_override_block(
                    &tag_block,
                    base_style,
                    styles,
                    &mut current_style,
                    &mut active_reset_style,
                    &mut active_reset_alignment,
                    &mut parsed,
                    &mut buffer,
                    &mut active_line,
                    &mut pending_karaoke,
                    &mut deferred_karaoke,
                    &mut karaoke_cursor_ms,
                    &mut drawing_scale,
                    &mut current_transforms,
                    &mut current_wrap_style,
                    inherited_wrap_style,
                    &mut vector_clip_claimed,
                );
            }
            '\\' if drawing_scale > 0 => buffer.push(character),
            '\\' => match characters.peek().copied() {
                Some('N') => {
                    characters.next();
                    flush_span_for_run_break(
                        &mut buffer,
                        &current_style,
                        &mut pending_karaoke,
                        &mut deferred_karaoke,
                        drawing_scale,
                        &current_transforms,
                        &mut active_line,
                    );
                    push_line(
                        &mut parsed,
                        &mut active_line,
                        &current_style,
                        &current_transforms,
                    );
                }
                Some('n') => {
                    characters.next();
                    if drawing_scale == 0 && current_wrap_style == 2 {
                        flush_span_for_run_break(
                            &mut buffer,
                            &current_style,
                            &mut pending_karaoke,
                            &mut deferred_karaoke,
                            drawing_scale,
                            &current_transforms,
                            &mut active_line,
                        );
                        push_line(
                            &mut parsed,
                            &mut active_line,
                            &current_style,
                            &current_transforms,
                        );
                    } else {
                        buffer.push(' ');
                    }
                }
                Some('h') => {
                    characters.next();
                    buffer.push('\u{00A0}');
                }
                Some('{') => {
                    characters.next();
                    buffer.push('{');
                }
                Some('}') => {
                    characters.next();
                    buffer.push('}');
                }
                Some(next) => {
                    characters.next();
                    buffer.push('\\');
                    buffer.push(next);
                }
                None => buffer.push(character),
            },
            '\n' if drawing_scale > 0 => buffer.push(character),
            '\n' => {
                flush_span_for_run_break(
                    &mut buffer,
                    &current_style,
                    &mut pending_karaoke,
                    &mut deferred_karaoke,
                    drawing_scale,
                    &current_transforms,
                    &mut active_line,
                );
                push_line(
                    &mut parsed,
                    &mut active_line,
                    &current_style,
                    &current_transforms,
                );
            }
            '\r' if drawing_scale > 0 => buffer.push(character),
            '\r' => {}
            '\t' if drawing_scale > 0 => buffer.push(character),
            '\t' => buffer.push(' '),
            _ => buffer.push(character),
        }
    }

    flush_span(
        &mut buffer,
        &current_style,
        pending_karaoke,
        drawing_scale,
        &current_transforms,
        &mut active_line,
    );
    push_line(
        &mut parsed,
        &mut active_line,
        &current_style,
        &current_transforms,
    );
    if parsed.lines.is_empty() {
        parsed.lines.push(ParsedTextLine::default());
    }
    parsed
}

struct AssTextLines<'a> {
    remaining: &'a str,
}

fn ass_text_lines(text: &str) -> AssTextLines<'_> {
    AssTextLines { remaining: text }
}

impl<'a> Iterator for AssTextLines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }

        let line_end = self
            .remaining
            .find(['\r', '\n'])
            .unwrap_or(self.remaining.len());
        let line = &self.remaining[..line_end];
        if line_end == self.remaining.len() {
            self.remaining = "";
        } else {
            let separator_len = self.remaining[line_end..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0);
            self.remaining = &self.remaining[line_end + separator_len..];
        }

        Some(line)
    }
}

fn trim_ass_line_bom(mut line: &str) -> &str {
    while let Some(rest) = line.strip_prefix('\u{feff}') {
        line = rest;
    }
    line
}

fn parse_section_header(line: &str) -> Option<&'static str> {
    if line
        .get(..13)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[Script Info]"))
    {
        Some("script info")
    } else if line
        .get(..11)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[V4 Styles]"))
    {
        Some("v4 styles")
    } else if line
        .get(..12)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[V4+ Styles]"))
    {
        Some("v4+ styles")
    } else if line
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[Events]"))
    {
        Some("events")
    } else if line
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("[Fonts]"))
    {
        Some("fonts")
    } else {
        None
    }
}

fn split_once_colon_raw(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key, trim_ass_leading_spaces(value)))
}

fn parse_format_fields(value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut remainder = value;
    while let Some(field) = next_format_token(&mut remainder) {
        fields.push(field.to_string());
    }
    fields
}

fn format_matches_libass_standard(value: &str, standard: &[String]) -> bool {
    let fields = parse_format_fields(value);
    fields.len() == standard.len()
        && fields
            .iter()
            .zip(standard)
            .all(|(field, standard)| format_token_matches_libass(field, standard))
}

fn format_token_matches_libass(left: &str, right: &str) -> bool {
    fn alias(token: &str) -> &str {
        if token.eq_ignore_ascii_case("Actor") {
            "Name"
        } else {
            token
        }
    }

    alias(left).eq_ignore_ascii_case(alias(right))
}

fn default_style_format(track_type: TrackType) -> Vec<String> {
    let fields = match track_type {
        TrackType::Ssa => &[
            "Name",
            "Fontname",
            "Fontsize",
            "PrimaryColour",
            "SecondaryColour",
            "TertiaryColour",
            "BackColour",
            "Bold",
            "Italic",
            "BorderStyle",
            "Outline",
            "Shadow",
            "Alignment",
            "MarginL",
            "MarginR",
            "MarginV",
            "AlphaLevel",
            "Encoding",
        ][..],
        _ => &[
            "Name",
            "Fontname",
            "Fontsize",
            "PrimaryColour",
            "SecondaryColour",
            "OutlineColour",
            "BackColour",
            "Bold",
            "Italic",
            "Underline",
            "StrikeOut",
            "ScaleX",
            "ScaleY",
            "Spacing",
            "Angle",
            "BorderStyle",
            "Outline",
            "Shadow",
            "Alignment",
            "MarginL",
            "MarginR",
            "MarginV",
            "Encoding",
        ][..],
    };
    fields.iter().copied().map(str::to_string).collect()
}

fn default_event_format(track_type: TrackType) -> Vec<String> {
    let fields = match track_type {
        TrackType::Ssa => &[
            "Marked", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect",
            "Text",
        ][..],
        _ => &[
            "Layer", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect",
            "Text",
        ][..],
    };
    fields.iter().copied().map(str::to_string).collect()
}

fn parse_style_line(value: &str, format: &[String], track_type: TrackType) -> Option<ParsedStyle> {
    let fields = split_fields(value, format.len());
    let mut style = ParsedStyle::parsed_line_seed();
    let mut ssa_alpha = 0;
    let mut font_name_seen = false;
    for (key, raw_value) in format.iter().zip(fields) {
        let lowered = key.to_ascii_lowercase();
        match lowered.as_str() {
            "name" => {
                let name = raw_value.trim_start_matches('*');
                style.name = if name.is_empty() {
                    "Default".to_string()
                } else {
                    name.to_string()
                };
            }
            "fontname" => {
                font_name_seen = true;
                style.font_name = raw_value.to_string();
            }
            "fontsize" => style.font_size = parse_style_f64_arg(raw_value),
            "primarycolour" | "primarycolor" => {
                style.primary_colour = parse_style_color_arg(raw_value)
            }
            "secondarycolour" | "secondarycolor" => {
                style.secondary_colour = parse_style_color_arg(raw_value)
            }
            "outlinecolour" | "outlinecolor" => {
                style.outline_colour = parse_style_color_arg(raw_value)
            }
            "backcolour" | "backcolor" => {
                style.back_colour = parse_style_color_arg(raw_value);
                if track_type == TrackType::Ssa {
                    style.outline_colour = style.back_colour;
                }
            }
            "bold" => {
                style.font_weight = parse_bold_weight(raw_value, style.font_weight);
                style.bold = bold_weight_is_active(style.font_weight);
            }
            "italic" => style.italic = parse_style_bool(raw_value),
            "underline" => style.underline = parse_style_bool(raw_value),
            "strikeout" => style.strike_out = parse_style_bool(raw_value),
            "scalex" => style.scale_x = parse_style_scale(raw_value),
            "scaley" => style.scale_y = parse_style_scale(raw_value),
            "spacing" => style.spacing = parse_style_f64_arg(raw_value).max(0.0),
            "angle" => style.angle = parse_style_f64_arg(raw_value),
            "borderstyle" => style.border_style = parse_style_i32_arg(raw_value),
            "outline" => style.outline = parse_style_f64_arg(raw_value).max(0.0),
            "shadow" => style.shadow = parse_style_f64_arg(raw_value).max(0.0),
            "alignment" => {
                style.alignment =
                    style_alignment_from_style(parse_style_i32_arg(raw_value), track_type);
            }
            "marginl" => style.margin_l = parse_style_i32_arg(raw_value),
            "marginr" => style.margin_r = parse_style_i32_arg(raw_value),
            "marginv" => style.margin_v = parse_style_i32_arg(raw_value),
            "alphalevel" => ssa_alpha = parse_style_i32_arg(raw_value),
            "encoding" => style.encoding = parse_style_i32_arg(raw_value),
            "treat_fontname_as_pattern" => {
                style.treat_fontname_as_pattern = parse_style_i32_arg(raw_value)
            }
            "blur" => style.blur = parse_style_f64_arg(raw_value),
            "justify" => style.justify = parse_style_i32_arg(raw_value),
            _ => {}
        }
    }

    if track_type == TrackType::Ssa {
        let alpha = ssa_alpha.clamp(0, 0xff) as u8;
        style.primary_colour = with_alpha(style.primary_colour, alpha);
        style.secondary_colour = with_alpha(style.secondary_colour, alpha);
        style.outline_colour = with_alpha(style.outline_colour, alpha);
        style.back_colour = with_alpha(style.back_colour, 0x80);
    }
    if style.name.is_empty() {
        style.name = "Default".to_string();
    }
    if !font_name_seen {
        style.font_name = "Arial".to_string();
    }

    Some(style)
}

fn parse_event_line(
    value: &str,
    format: &[String],
    read_order: i32,
    styles: &[ParsedStyle],
    default_style: i32,
) -> Option<ParsedEvent> {
    let mut event = ParsedEvent {
        read_order,
        ..ParsedEvent::default()
    };
    let mut end = 0_i64;
    let mut remainder = value;

    for key in format {
        let lowered = key.to_ascii_lowercase();
        if lowered == "text" {
            event.text = remainder.trim_end_matches(['\r', '\t', ' ']).to_string();
            event.duration = end - event.start;
            return Some(event);
        }

        let raw_value = next_value_token(&mut remainder)?;
        match lowered.as_str() {
            "layer" => event.layer = parse_header_i32_arg(raw_value),
            "start" => event.start = parse_timestamp(raw_value).unwrap_or(event.start),
            "end" | "duration" => end = parse_timestamp(raw_value).unwrap_or(end),
            "style" => event.style = parse_style_reference(raw_value, styles, default_style),
            "name" | "actor" => event.name = raw_value.to_string(),
            "marginl" => event.margin_l = parse_header_i32_arg(raw_value),
            "marginr" => event.margin_r = parse_header_i32_arg(raw_value),
            "marginv" => event.margin_v = parse_header_i32_arg(raw_value),
            "effect" => event.effect = raw_value.to_string(),
            _ => {}
        }
    }

    None
}

fn split_fields(input: &str, field_count: usize) -> Vec<&str> {
    let mut fields = Vec::with_capacity(field_count);
    let mut remainder = input;
    for _ in 0..field_count {
        let Some(token) = next_value_token(&mut remainder) else {
            break;
        };
        fields.push(token);
    }
    fields
}

fn next_value_token<'a>(input: &mut &'a str) -> Option<&'a str> {
    *input = trim_ass_leading_spaces(input);
    if input.is_empty() {
        return None;
    }

    if let Some((head, tail)) = input.split_once(',') {
        *input = tail;
        Some(head)
    } else {
        let token = *input;
        *input = "";
        Some(token)
    }
}

fn apply_script_info_field(track: &mut ParsedTrack, key: &str, value: &str) {
    match key {
        "PlayResX" => track.play_res_x = parse_header_i32_arg(value),
        "PlayResY" => track.play_res_y = parse_header_i32_arg(value),
        "Timer" => track.timer = parse_header_f64_arg(value),
        "WrapStyle" => track.wrap_style = parse_header_i32_arg(value),
        "ScaledBorderAndShadow" => track.scaled_border_and_shadow = parse_bool(value),
        "Kerning" => track.kerning = parse_bool(value),
        "Language" => track.language = parse_language(value),
        "LayoutResX" => track.layout_res_x = parse_header_i32_arg(value),
        "LayoutResY" => track.layout_res_y = parse_header_i32_arg(value),
        "YCbCr Matrix" => track.ycbcr_matrix = parse_matrix(value),
        "ScriptType" => parse_script_type(track, value),
        _ => {}
    }
}

fn parse_script_type(track: &mut ParsedTrack, value: &str) {
    let trimmed = trim_ass_trailing_spaces(value);
    if trimmed.len() < 4 {
        return;
    }

    let (version, track_type) = if let Some(version) = trimmed.strip_suffix('+') {
        (version, TrackType::Ass)
    } else {
        (trimmed, TrackType::Ssa)
    };
    if version.ends_with("4.00") {
        track.track_type = track_type;
    }
}

fn parse_bool(value: &str) -> bool {
    let trimmed = trim_ass_leading_spaces(value);
    if trimmed.to_ascii_lowercase().starts_with("yes") {
        return true;
    }
    parse_i32_decimal_prefix(trimmed).is_some_and(|parsed| parsed > 0)
}

fn parse_language(value: &str) -> String {
    let trimmed = trim_ass_c_leading_spaces(value);
    let mut end = trimmed.len().min(2);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_string()
}

fn parse_style_bool(value: &str) -> bool {
    parse_style_i32_arg(value) != 0
}

fn parse_bold_weight(value: &str, _fallback: i32) -> i32 {
    match parse_style_i32_arg(value) {
        0 => 400,
        _ => 700,
    }
}

fn parse_override_bold_weight(value: &str, fallback: i32) -> i32 {
    let Some(arg) = first_override_arg(value) else {
        return fallback;
    };

    match parse_i32_decimal_prefix(arg).unwrap_or(0) {
        0 => 400,
        1 => 700,
        weight if weight >= 100 => weight,
        _ => fallback,
    }
}

fn bold_weight_is_active(weight: i32) -> bool {
    weight >= 700
}

fn parse_f64(value: &str, fallback: f64) -> f64 {
    parse_drawing_number(value).unwrap_or(fallback)
}

fn parse_header_i32_arg(value: &str) -> i32 {
    parse_style_i32_arg(value)
}

fn parse_header_f64_arg(value: &str) -> f64 {
    parse_style_f64_arg(value)
}

fn parse_style_f64_arg(value: &str) -> f64 {
    parse_drawing_number(value).unwrap_or(0.0)
}

fn parse_style_scale(value: &str) -> f64 {
    (parse_style_f64_arg(value) / 100.0).max(0.0)
}

fn parse_style_color_arg(value: &str) -> u32 {
    parse_style_i32_arg(value) as u32
}

fn parse_style_i32_arg(value: &str) -> i32 {
    let value = trim_ass_leading_spaces(value);
    let (base, digits) = value
        .strip_prefix("&H")
        .or_else(|| value.strip_prefix("&h"))
        .or_else(|| value.strip_prefix("0x"))
        .or_else(|| value.strip_prefix("0X"))
        .map_or((10, value), |digits| (16, digits));
    parse_u32_modulo_prefix(digits, base) as i32
}

fn parse_u32_modulo_prefix(value: &str, base: u32) -> u32 {
    let value = trim_ass_leading_spaces(value);
    let (negative, rest) = match value.as_bytes().first().copied() {
        Some(b'+') => (false, &value[1..]),
        Some(b'-') => (true, &value[1..]),
        _ => (false, value),
    };
    let rest = if base == 16 {
        rest.strip_prefix("0x")
            .or_else(|| rest.strip_prefix("0X"))
            .unwrap_or(rest)
    } else {
        rest
    };

    let mut parsed = 0_u32;
    let mut found_digit = false;
    for byte in rest.bytes() {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a' + 10),
            b'A'..=b'F' => u32::from(byte - b'A' + 10),
            _ => break,
        };
        if digit >= base {
            break;
        }
        parsed = parsed.wrapping_mul(base).wrapping_add(digit);
        found_digit = true;
    }

    if !found_digit {
        return 0;
    }
    if negative {
        0_u32.wrapping_sub(parsed)
    } else {
        parsed
    }
}

fn first_override_arg(value: &str) -> Option<&str> {
    let trimmed = trim_ass_trailing_spaces(trim_ass_leading_spaces(value));
    if trimmed.is_empty() {
        return None;
    }

    if let Some((before_parentheses, inside_parentheses)) = split_first_parenthesized_args(trimmed)
    {
        if let Some(arg) = inside_parentheses
            .split(',')
            .map(|part| trim_ass_trailing_spaces(trim_ass_leading_spaces(part)))
            .find(|part| !part.is_empty())
        {
            return Some(arg);
        }

        let fallback = trim_ass_trailing_spaces(trim_ass_leading_spaces(before_parentheses));
        return (!fallback.is_empty()).then_some(fallback);
    }

    Some(trimmed)
}

fn first_colour_override_arg(value: &str) -> Option<&str> {
    let trimmed = trim_ass_trailing_spaces(value);
    if trimmed.is_empty() {
        return None;
    }

    if let Some((before_parentheses, inside_parentheses)) = split_first_parenthesized_args(trimmed)
    {
        if let Some(arg) = inside_parentheses
            .split(',')
            .map(|part| trim_ass_trailing_spaces(trim_ass_leading_spaces(part)))
            .find(|part| !part.is_empty())
        {
            return Some(arg);
        }

        let fallback = trim_ass_trailing_spaces(before_parentheses);
        return (!fallback.is_empty()).then_some(fallback);
    }

    Some(trimmed)
}

fn first_reset_style_arg(value: &str) -> Option<&str> {
    let trimmed = trim_ass_trailing_spaces(value);
    if trimmed.is_empty() {
        return None;
    }

    if let Some((before_parentheses, inside_parentheses)) = split_first_parenthesized_args(trimmed)
    {
        if let Some(arg) = inside_parentheses
            .split(',')
            .map(|part| trim_ass_trailing_spaces(trim_ass_leading_spaces(part)))
            .find(|part| !part.is_empty())
        {
            return Some(arg);
        }

        let fallback = trim_ass_trailing_spaces(before_parentheses);
        return (!fallback.is_empty()).then_some(fallback);
    }

    Some(trimmed)
}

fn parse_override_f64_arg(value: &str, missing_fallback: f64) -> f64 {
    first_override_arg(value).map_or(missing_fallback, |arg| {
        parse_drawing_number(arg).unwrap_or(0.0)
    })
}

fn parse_override_scale(value: &str, missing_fallback: f64) -> f64 {
    (parse_override_f64_arg(value, missing_fallback * 100.0) / 100.0).max(0.0)
}

fn parse_transform_f64_target(value: &str, missing_fallback: f64) -> f64 {
    first_override_arg(value).map_or(missing_fallback, |arg| {
        parse_drawing_number(arg).unwrap_or(0.0)
    })
}

fn parse_transform_scale_target(value: &str, missing_fallback: f64) -> f64 {
    first_override_arg(value).map_or(missing_fallback, |arg| {
        parse_drawing_number(arg).unwrap_or(0.0) / 100.0
    })
}

fn parse_be_override(value: &str) -> f64 {
    libass_be_value(parse_transform_be_target(value))
}

fn parse_transform_blur_target(value: &str) -> f64 {
    first_override_arg(value).map_or(0.0, |arg| parse_drawing_number(arg).unwrap_or(0.0))
}

fn parse_transform_be_target(value: &str) -> f64 {
    first_override_arg(value).map_or(0.0, |arg| parse_drawing_number(arg).unwrap_or(0.0))
}

fn libass_be_value(raw: f64) -> f64 {
    let shifted = raw + 0.5;
    if shifted.is_nan() || shifted <= f64::from(i32::MIN) || shifted >= f64::from(i32::MAX) + 1.0 {
        return 0.0;
    }

    shifted.trunc().clamp(0.0, 127.0)
}

fn parse_i32_decimal_prefix(value: &str) -> Option<i32> {
    let value = trim_ass_c_leading_spaces(value);
    if value.is_empty() {
        return None;
    }

    let (sign, rest) = match value.as_bytes()[0] {
        b'+' => (1_i64, &value[1..]),
        b'-' => (-1_i64, &value[1..]),
        _ => (1_i64, value),
    };
    let mut parsed = 0_i128;
    let mut found_digit = false;
    for byte in rest.bytes() {
        let digit = match byte {
            b'0'..=b'9' => i128::from(byte - b'0'),
            _ => break,
        };
        parsed = parsed.saturating_mul(10).saturating_add(digit);
        found_digit = true;
    }
    if !found_digit {
        return None;
    }

    Some(
        parsed
            .saturating_mul(i128::from(sign))
            .clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32,
    )
}

fn parse_override_i32_arg(value: &str) -> Option<i32> {
    first_override_arg(value).map(|arg| parse_i32_decimal_prefix(arg).unwrap_or(0))
}

fn parenthesized_args(value: &str) -> Option<&str> {
    let value = trim_ass_leading_spaces(value);
    let (_, inside) = split_first_parenthesized_args(value)?;
    Some(inside)
}

fn split_first_parenthesized_args(value: &str) -> Option<(&str, &str)> {
    let open = value.find('(')?;
    let inside = &value[open + 1..];
    let end = inside.find(')').unwrap_or(inside.len());
    Some((&value[..open], &inside[..end]))
}

fn split_complex_args(inside: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0;

    while start <= inside.len() {
        while let Some(character) = inside.get(start..).and_then(|rest| rest.chars().next()) {
            if character != ' ' && character != '\t' {
                break;
            }
            start += character.len_utf8();
        }
        if start >= inside.len() {
            break;
        }

        let rest = &inside[start..];
        let mut delimiter = None;
        for (offset, character) in rest.char_indices() {
            if character == ',' || character == '\\' {
                delimiter = Some((start + offset, character));
                break;
            }
        }

        match delimiter {
            Some((index, ',')) => {
                push_complex_arg(&mut args, &inside[start..index]);
                start = index + 1;
            }
            Some((index, '\\')) => {
                push_complex_arg(&mut args, &inside[start..]);
                debug_assert!(index >= start);
                break;
            }
            _ => {
                push_complex_arg(&mut args, &inside[start..]);
                break;
            }
        }
    }

    args
}

fn push_complex_arg<'a>(args: &mut Vec<&'a str>, arg: &'a str) {
    let arg = trim_ass_trailing_spaces(arg);
    if !arg.is_empty() {
        args.push(arg);
    }
}

fn parse_complex_f64_arg(value: &str) -> f64 {
    parse_drawing_number(value).unwrap_or(0.0)
}

fn parse_complex_i32_arg(value: &str) -> i32 {
    parse_i32_decimal_prefix(value).unwrap_or(0)
}

fn apply_color_override(value: &str, current: u32, base: u32) -> u32 {
    if first_colour_override_arg(value).is_none() {
        return (current & 0xFF00_0000) | (base & 0x00FF_FFFF);
    }
    let rgb = parse_override_rgb(value).unwrap_or(0);
    (current & 0xFF00_0000) | rgb
}

fn parse_timestamp(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    let Some((hours, index)) = parse_timestamp_i64(bytes, 0) else {
        return Some(0);
    };
    let Some(index) = consume_timestamp_literal(bytes, index, b':') else {
        return Some(0);
    };
    let Some((minutes, index)) = parse_timestamp_i64(bytes, index) else {
        return Some(0);
    };
    let Some(index) = consume_timestamp_literal(bytes, index, b':') else {
        return Some(0);
    };
    let Some((seconds, index)) = parse_timestamp_i64(bytes, index) else {
        return Some(0);
    };
    let Some(index) = consume_timestamp_literal(bytes, index, b'.') else {
        return Some(0);
    };
    let Some((centiseconds, _)) = parse_timestamp_i64(bytes, index) else {
        return Some(0);
    };

    Some((((hours * 60 + minutes) * 60) + seconds) * 1000 + centiseconds * 10)
}

fn parse_timestamp_i64(bytes: &[u8], mut index: usize) -> Option<(i64, usize)> {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }

    let mut sign = 1_i64;
    match bytes.get(index).copied() {
        Some(b'+') => index += 1,
        Some(b'-') => {
            sign = -1;
            index += 1;
        }
        _ => {}
    }

    let start = index;
    let mut value = 0_i64;
    while let Some(byte) = bytes.get(index).copied() {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'));
        index += 1;
    }

    (index > start).then_some((value.saturating_mul(sign), index))
}

fn consume_timestamp_literal(bytes: &[u8], index: usize, literal: u8) -> Option<usize> {
    bytes
        .get(index)
        .is_some_and(|byte| *byte == literal)
        .then_some(index + 1)
}

fn parse_style_reference(value: &str, styles: &[ParsedStyle], default_style: i32) -> i32 {
    let style_name = value.trim_start_matches('*');
    let style_name = if style_name.eq_ignore_ascii_case("Default") {
        "Default"
    } else {
        style_name
    };

    styles
        .iter()
        .enumerate()
        .rev()
        .find(|(_, style)| style.name == style_name)
        .map(|(index, _)| index as i32)
        .unwrap_or(default_style)
}

#[allow(clippy::too_many_arguments)]
fn apply_override_block(
    block: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
    current_style: &mut ParsedSpanStyle,
    active_reset_style: &mut ParsedSpanStyle,
    active_reset_alignment: &mut i32,
    parsed: &mut ParsedDialogueText,
    buffer: &mut String,
    active_line: &mut ParsedTextLine,
    pending_karaoke: &mut Option<ParsedKaraokeSpan>,
    deferred_karaoke: &mut Option<ParsedKaraokeSpan>,
    karaoke_cursor_ms: &mut i32,
    drawing_scale: &mut i32,
    current_transforms: &mut Vec<ParsedSpanTransform>,
    current_wrap_style: &mut i32,
    inherited_wrap_style: i32,
    vector_clip_claimed: &mut bool,
) {
    if block_has_libass_hard_override(block) {
        parsed.hard_override = true;
    }

    for raw_tag in split_override_tags(block) {
        let tag = trim_ass_tag(raw_tag);
        if tag.is_empty() {
            continue;
        }

        let previous = current_style.clone();
        let previous_transforms = current_transforms.clone();
        if let Some(rest) = tag.strip_prefix("fn") {
            current_style.font_name = parse_font_name_override(rest, &active_reset_style.font_name);
        } else if let Some(rest) = tag.strip_prefix("fe") {
            current_style.encoding =
                parse_override_i32_arg(rest).unwrap_or(active_reset_style.encoding);
        } else if let Some(rest) = tag.strip_prefix("kt") {
            let pending_karaoke_is_unconsumed = buffer.is_empty() && pending_karaoke.is_some();
            apply_karaoke_timing_reset(
                pending_karaoke,
                deferred_karaoke,
                karaoke_cursor_ms,
                rest,
                pending_karaoke_is_unconsumed,
            );
        } else if let Some((rest, mode)) = tag
            .strip_prefix("kf")
            .map(|rest| (rest, ParsedKaraokeMode::Sweep))
            .or_else(|| {
                tag.strip_prefix("ko")
                    .map(|rest| (rest, ParsedKaraokeMode::OutlineToggle))
            })
            .or_else(|| {
                tag.strip_prefix('K')
                    .map(|rest| (rest, ParsedKaraokeMode::Sweep))
            })
            .or_else(|| {
                tag.strip_prefix('k')
                    .map(|rest| (rest, ParsedKaraokeMode::FillSwap))
            })
        {
            if let Some(duration_ms) = parse_karaoke_duration(rest) {
                if duration_ms == 0
                    && !buffer.is_empty()
                    && pending_karaoke.is_some_and(|karaoke| karaoke.mode == mode)
                {
                    *deferred_karaoke = Some(ParsedKaraokeSpan {
                        start_ms: *karaoke_cursor_ms,
                        duration_ms,
                        mode,
                    });
                    *karaoke_cursor_ms = (*karaoke_cursor_ms).wrapping_add(duration_ms);
                    continue;
                }
                flush_span_before_karaoke_tag(
                    buffer,
                    &previous,
                    pending_karaoke,
                    deferred_karaoke,
                    *drawing_scale,
                    &previous_transforms,
                    active_line,
                );
                *pending_karaoke = Some(ParsedKaraokeSpan {
                    start_ms: *karaoke_cursor_ms,
                    duration_ms,
                    mode,
                });
                *karaoke_cursor_ms = (*karaoke_cursor_ms).wrapping_add(duration_ms);
            }
        } else if let Some(rest) = tag.strip_prefix("fscx") {
            current_style.scale_x = parse_override_scale(rest, active_reset_style.scale_x);
        } else if let Some(rest) = tag.strip_prefix("fscy") {
            current_style.scale_y = parse_override_scale(rest, active_reset_style.scale_y);
        } else if tag.strip_prefix("fsc").is_some() {
            current_style.scale_x = active_reset_style.scale_x;
            current_style.scale_y = active_reset_style.scale_y;
        } else if let Some(rest) = tag.strip_prefix("fsp") {
            current_style.spacing = parse_override_f64_arg(rest, active_reset_style.spacing);
        } else if let Some(rest) = tag.strip_prefix("frx") {
            current_style.rotation_x = parse_override_f64_arg(rest, 0.0);
        } else if let Some(rest) = tag.strip_prefix("fry") {
            current_style.rotation_y = parse_override_f64_arg(rest, 0.0);
        } else if let Some(rest) = tag.strip_prefix("frz").or_else(|| tag.strip_prefix("fr")) {
            current_style.rotation_z = parse_override_f64_arg(rest, active_reset_style.rotation_z);
        } else if let Some(rest) = tag.strip_prefix("fax") {
            current_style.shear_x = parse_override_f64_arg(rest, 0.0);
        } else if let Some(rest) = tag.strip_prefix("fay") {
            current_style.shear_y = parse_override_f64_arg(rest, 0.0);
        } else if let Some(rest) = tag.strip_prefix("fs") {
            current_style.font_size = parse_font_size_override(
                rest,
                current_style.font_size,
                active_reset_style.font_size,
            );
        } else if let Some(rest) = tag.strip_prefix("iclip") {
            if let Some(rect) = parse_rect_clip(rest) {
                parsed.clip_rect = Some(rect);
                parsed.clip_rect_exact = parse_rect_clip_exact(rest);
                parsed.inverse_clip = true;
            } else if let Some(rect) = parse_rect_clip_exact(rest) {
                parsed.clip_rect = None;
                parsed.clip_rect_exact = Some(rect);
                parsed.inverse_clip = true;
            } else if !*vector_clip_claimed && vector_clip_args(rest).is_some() {
                *vector_clip_claimed = true;
                if let Some(vector) = parse_vector_clip(rest) {
                    if parsed.clip_rect.is_none() && parsed.clip_rect_exact.is_none() {
                        parsed.inverse_clip = true;
                    }
                    parsed.vector_clip = Some(vector);
                    parsed.vector_clip_inverse = true;
                }
            }
        } else if let Some(rest) = tag.strip_prefix("move") {
            if parsed.position.is_none()
                && parsed.position_exact.is_none()
                && parsed.movement.is_none()
                && parsed.movement_exact.is_none()
            {
                parsed.movement = parse_move(rest);
                parsed.movement_exact = parse_move_exact(rest);
            }
        } else if let Some(rest) = tag.strip_prefix("fade") {
            if parsed.fade.is_none() {
                parsed.fade = parse_fade(rest);
            }
        } else if let Some(rest) = tag.strip_prefix("fad") {
            if parsed.fade.is_none() {
                parsed.fade = parse_fad(rest);
            }
        } else if let Some(rest) = tag.strip_prefix("clip") {
            if let Some(rect) = parse_rect_clip(rest) {
                parsed.clip_rect = Some(rect);
                parsed.clip_rect_exact = parse_rect_clip_exact(rest);
                parsed.inverse_clip = false;
            } else if let Some(rect) = parse_rect_clip_exact(rest) {
                parsed.clip_rect = None;
                parsed.clip_rect_exact = Some(rect);
                parsed.inverse_clip = false;
            } else if !*vector_clip_claimed && vector_clip_args(rest).is_some() {
                *vector_clip_claimed = true;
                if let Some(vector) = parse_vector_clip(rest) {
                    if parsed.clip_rect.is_none() && parsed.clip_rect_exact.is_none() {
                        parsed.inverse_clip = false;
                    }
                    parsed.vector_clip = Some(vector);
                    parsed.vector_clip_inverse = false;
                }
            }
        } else if let Some(rest) = tag.strip_prefix("1c").or_else(|| tag.strip_prefix('c')) {
            current_style.primary_colour = apply_color_override(
                rest,
                current_style.primary_colour,
                active_reset_style.primary_colour,
            );
        } else if let Some(rest) = tag.strip_prefix("2c") {
            current_style.secondary_colour = apply_color_override(
                rest,
                current_style.secondary_colour,
                active_reset_style.secondary_colour,
            );
        } else if let Some(rest) = tag.strip_prefix("3c") {
            current_style.outline_colour = apply_color_override(
                rest,
                current_style.outline_colour,
                active_reset_style.outline_colour,
            );
        } else if let Some(rest) = tag.strip_prefix("4c") {
            current_style.back_colour = apply_color_override(
                rest,
                current_style.back_colour,
                active_reset_style.back_colour,
            );
        } else if let Some(rest) = tag.strip_prefix("alpha") {
            if let Some(alpha) = parse_alpha_tag(rest) {
                current_style.primary_colour = with_alpha(current_style.primary_colour, alpha);
                current_style.secondary_colour = with_alpha(current_style.secondary_colour, alpha);
                current_style.outline_colour = with_alpha(current_style.outline_colour, alpha);
                current_style.back_colour = with_alpha(current_style.back_colour, alpha);
            } else {
                current_style.primary_colour = with_alpha(
                    current_style.primary_colour,
                    alpha_of(active_reset_style.primary_colour),
                );
                current_style.secondary_colour = with_alpha(
                    current_style.secondary_colour,
                    alpha_of(active_reset_style.secondary_colour),
                );
                current_style.outline_colour = with_alpha(
                    current_style.outline_colour,
                    alpha_of(active_reset_style.outline_colour),
                );
                current_style.back_colour = with_alpha(
                    current_style.back_colour,
                    alpha_of(active_reset_style.back_colour),
                );
            }
        } else if let Some(rest) = tag.strip_prefix("1a") {
            let alpha = parse_alpha_tag(rest)
                .unwrap_or_else(|| alpha_of(active_reset_style.primary_colour));
            current_style.primary_colour = with_alpha(current_style.primary_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("2a") {
            let alpha = parse_alpha_tag(rest)
                .unwrap_or_else(|| alpha_of(active_reset_style.secondary_colour));
            current_style.secondary_colour = with_alpha(current_style.secondary_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("3a") {
            let alpha = parse_alpha_tag(rest)
                .unwrap_or_else(|| alpha_of(active_reset_style.outline_colour));
            current_style.outline_colour = with_alpha(current_style.outline_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("4a") {
            let alpha =
                parse_alpha_tag(rest).unwrap_or_else(|| alpha_of(active_reset_style.back_colour));
            current_style.back_colour = with_alpha(current_style.back_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("xbord") {
            current_style.border_x =
                parse_override_f64_arg(rest, active_reset_style.border_x).max(0.0);
        } else if let Some(rest) = tag.strip_prefix("ybord") {
            current_style.border_y =
                parse_override_f64_arg(rest, active_reset_style.border_y).max(0.0);
        } else if let Some(rest) = tag.strip_prefix("bord") {
            current_style.border = parse_override_f64_arg(rest, active_reset_style.border).max(0.0);
            current_style.border_x = current_style.border;
            current_style.border_y = current_style.border;
        } else if let Some(rest) = tag.strip_prefix("xshad") {
            current_style.shadow_x = parse_override_f64_arg(rest, active_reset_style.shadow_x);
        } else if let Some(rest) = tag.strip_prefix("yshad") {
            current_style.shadow_y = parse_override_f64_arg(rest, active_reset_style.shadow_y);
        } else if let Some(rest) = tag.strip_prefix("shad") {
            current_style.shadow = parse_override_f64_arg(rest, active_reset_style.shadow).max(0.0);
            current_style.shadow_x = current_style.shadow;
            current_style.shadow_y = current_style.shadow;
        } else if let Some(rest) = tag.strip_prefix("blur") {
            current_style.blur = parse_override_f64_arg(rest, 0.0).clamp(0.0, 100.0);
        } else if let Some(rest) = tag.strip_prefix("be") {
            current_style.be = parse_be_override(rest);
        } else if let Some(rest) = tag.strip_prefix('t') {
            parsed.transform_disables_collision = true;
            apply_transform_immediate_tags(
                rest,
                base_style,
                styles,
                current_style,
                active_reset_style,
                active_reset_alignment,
                parsed,
                buffer,
                active_line,
                pending_karaoke,
                karaoke_cursor_ms,
                drawing_scale,
                current_transforms,
                current_wrap_style,
                inherited_wrap_style,
                vector_clip_claimed,
                deferred_karaoke,
            );
            current_transforms.extend(parse_transforms(rest, current_style, active_reset_style));
        } else if let Some(rest) = tag.strip_prefix('u') {
            current_style.underline = parse_override_bool(rest, active_reset_style.underline);
        } else if let Some(rest) = tag.strip_prefix('s') {
            current_style.strike_out = parse_override_bool(rest, active_reset_style.strike_out);
        } else if let Some(rest) = tag.strip_prefix('b') {
            current_style.font_weight =
                parse_override_bold_weight(rest, active_reset_style.font_weight);
            current_style.bold = bold_weight_is_active(current_style.font_weight);
        } else if let Some(rest) = tag.strip_prefix('i') {
            current_style.italic = parse_override_bool(rest, active_reset_style.italic);
        } else if let Some(rest) = tag.strip_prefix("an") {
            if parsed.alignment.is_none() {
                let value = parse_override_i32_arg(rest).unwrap_or(0);
                parsed.alignment =
                    Some(alignment_from_an(value).unwrap_or(*active_reset_alignment));
            }
        } else if let Some(rest) = tag.strip_prefix('a') {
            if parsed.alignment.is_none() {
                let value = parse_override_i32_arg(rest).unwrap_or(0);
                parsed.alignment =
                    Some(alignment_from_legacy_a(value).unwrap_or(*active_reset_alignment));
            }
        } else if let Some(rest) = tag.strip_prefix('q') {
            let value = parse_override_i32_arg(rest).unwrap_or(inherited_wrap_style);
            let value = if (0..=3).contains(&value) {
                value
            } else {
                inherited_wrap_style
            };
            parsed.wrap_style = Some(value);
            *current_wrap_style = value;
        } else if let Some(rest) = tag.strip_prefix("org") {
            if parsed.origin.is_none() && parsed.origin_exact.is_none() {
                parsed.origin = parse_pos(rest);
                parsed.origin_exact = parse_pos_exact(rest);
            }
        } else if let Some(rest) = tag.strip_prefix("pos") {
            if parsed.position.is_none()
                && parsed.position_exact.is_none()
                && parsed.movement.is_none()
                && parsed.movement_exact.is_none()
            {
                parsed.position = parse_pos(rest);
                parsed.position_exact = parse_pos_exact(rest);
            }
        } else if let Some(rest) = tag.strip_prefix("pbo") {
            current_style.pbo = parse_override_f64_arg(rest, 0.0);
        } else if let Some(rest) = tag.strip_prefix('p') {
            flush_span_for_run_break(
                buffer,
                &previous,
                pending_karaoke,
                deferred_karaoke,
                *drawing_scale,
                &previous_transforms,
                active_line,
            );
            *drawing_scale = parse_override_i32_arg(rest).unwrap_or(0).max(0);
        } else if let Some(rest) = tag.strip_prefix('r') {
            let reset_style = resolve_reset_style(rest, base_style, styles);
            let preserved_pbo = current_style.pbo;
            *active_reset_style = reset_style.clone();
            *current_style = reset_style;
            current_style.pbo = preserved_pbo;
            *active_reset_alignment = resolve_reset_alignment(rest, base_style, styles);
            current_transforms.clear();
        }

        suppress_transform_fields_for_override(tag, current_style, current_transforms);

        if *current_style != previous || *current_transforms != previous_transforms {
            flush_span_for_run_break(
                buffer,
                &previous,
                pending_karaoke,
                deferred_karaoke,
                *drawing_scale,
                &previous_transforms,
                active_line,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_transform_immediate_tags(
    value: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
    current_style: &mut ParsedSpanStyle,
    active_reset_style: &mut ParsedSpanStyle,
    active_reset_alignment: &mut i32,
    parsed: &mut ParsedDialogueText,
    buffer: &mut String,
    active_line: &mut ParsedTextLine,
    pending_karaoke: &mut Option<ParsedKaraokeSpan>,
    karaoke_cursor_ms: &mut i32,
    drawing_scale: &mut i32,
    current_transforms: &[ParsedSpanTransform],
    current_wrap_style: &mut i32,
    inherited_wrap_style: i32,
    vector_clip_claimed: &mut bool,
    deferred_karaoke: &mut Option<ParsedKaraokeSpan>,
) {
    let Some(inside) = parenthesized_args(value) else {
        return;
    };
    let Some(tag_start) = inside.find('\\') else {
        return;
    };

    for raw_tag in split_override_tags(&inside[tag_start..]) {
        let tag = trim_ass_tag(raw_tag);
        if let Some(rest) = tag.strip_prefix("iclip") {
            if !*vector_clip_claimed && vector_clip_args(rest).is_some() {
                *vector_clip_claimed = true;
                if let Some(vector) = parse_vector_clip(rest) {
                    if parsed.clip_rect.is_none() && parsed.clip_rect_exact.is_none() {
                        parsed.inverse_clip = true;
                    }
                    parsed.vector_clip = Some(vector);
                    parsed.vector_clip_inverse = true;
                }
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("clip") {
            if !*vector_clip_claimed && vector_clip_args(rest).is_some() {
                *vector_clip_claimed = true;
                if let Some(vector) = parse_vector_clip(rest) {
                    if parsed.clip_rect.is_none() && parsed.clip_rect_exact.is_none() {
                        parsed.inverse_clip = false;
                    }
                    parsed.vector_clip = Some(vector);
                    parsed.vector_clip_inverse = false;
                }
            }
            continue;
        }
        if transform_tag_animates_style(tag) {
            continue;
        }
        if let Some(rest) = tag.strip_prefix("kt") {
            let pending_karaoke_is_unconsumed = buffer.is_empty() && pending_karaoke.is_some();
            apply_karaoke_timing_reset(
                pending_karaoke,
                deferred_karaoke,
                karaoke_cursor_ms,
                rest,
                pending_karaoke_is_unconsumed,
            );
            continue;
        }
        if let Some((rest, mode)) = tag
            .strip_prefix("kf")
            .map(|rest| (rest, ParsedKaraokeMode::Sweep))
            .or_else(|| {
                tag.strip_prefix("ko")
                    .map(|rest| (rest, ParsedKaraokeMode::OutlineToggle))
            })
            .or_else(|| {
                tag.strip_prefix('K')
                    .map(|rest| (rest, ParsedKaraokeMode::Sweep))
            })
            .or_else(|| {
                tag.strip_prefix('k')
                    .map(|rest| (rest, ParsedKaraokeMode::FillSwap))
            })
        {
            if let Some(duration_ms) = parse_karaoke_duration(rest) {
                if duration_ms == 0
                    && !buffer.is_empty()
                    && pending_karaoke.is_some_and(|karaoke| karaoke.mode == mode)
                {
                    *deferred_karaoke = Some(ParsedKaraokeSpan {
                        start_ms: *karaoke_cursor_ms,
                        duration_ms,
                        mode,
                    });
                    *karaoke_cursor_ms = (*karaoke_cursor_ms).wrapping_add(duration_ms);
                    continue;
                }
                flush_span_before_karaoke_tag(
                    buffer,
                    current_style,
                    pending_karaoke,
                    deferred_karaoke,
                    *drawing_scale,
                    current_transforms,
                    active_line,
                );
                *pending_karaoke = Some(ParsedKaraokeSpan {
                    start_ms: *karaoke_cursor_ms,
                    duration_ms,
                    mode,
                });
                *karaoke_cursor_ms = (*karaoke_cursor_ms).wrapping_add(duration_ms);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("fn") {
            current_style.font_name = parse_font_name_override(rest, &active_reset_style.font_name);
            continue;
        }
        if let Some(rest) = tag.strip_prefix("pbo") {
            current_style.pbo = parse_override_f64_arg(rest, 0.0);
            continue;
        }
        if let Some(rest) = tag.strip_prefix("fe") {
            current_style.encoding =
                parse_override_i32_arg(rest).unwrap_or(active_reset_style.encoding);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('u') {
            current_style.underline = parse_override_bool(rest, active_reset_style.underline);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('s') {
            current_style.strike_out = parse_override_bool(rest, active_reset_style.strike_out);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('b') {
            current_style.font_weight =
                parse_override_bold_weight(rest, active_reset_style.font_weight);
            current_style.bold = bold_weight_is_active(current_style.font_weight);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('i') {
            current_style.italic = parse_override_bool(rest, active_reset_style.italic);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('q') {
            let value = parse_override_i32_arg(rest).unwrap_or(inherited_wrap_style);
            let value = if (0..=3).contains(&value) {
                value
            } else {
                inherited_wrap_style
            };
            parsed.wrap_style = Some(value);
            *current_wrap_style = value;
            continue;
        }
        if let Some(rest) = tag.strip_prefix("an") {
            if parsed.alignment.is_none() {
                let value = parse_override_i32_arg(rest).unwrap_or(0);
                parsed.alignment =
                    Some(alignment_from_an(value).unwrap_or(*active_reset_alignment));
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix('a') {
            if parsed.alignment.is_none() {
                let value = parse_override_i32_arg(rest).unwrap_or(0);
                parsed.alignment =
                    Some(alignment_from_legacy_a(value).unwrap_or(*active_reset_alignment));
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("org") {
            if parsed.origin.is_none() && parsed.origin_exact.is_none() {
                parsed.origin = parse_pos(rest);
                parsed.origin_exact = parse_pos_exact(rest);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("pos") {
            if parsed.position.is_none()
                && parsed.position_exact.is_none()
                && parsed.movement.is_none()
                && parsed.movement_exact.is_none()
            {
                parsed.position = parse_pos(rest);
                parsed.position_exact = parse_pos_exact(rest);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("move") {
            if parsed.position.is_none()
                && parsed.position_exact.is_none()
                && parsed.movement.is_none()
                && parsed.movement_exact.is_none()
            {
                parsed.movement = parse_move(rest);
                parsed.movement_exact = parse_move_exact(rest);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("fade") {
            if parsed.fade.is_none() {
                parsed.fade = parse_fade(rest);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("fad") {
            if parsed.fade.is_none() {
                parsed.fade = parse_fad(rest);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix('p') {
            flush_span_for_run_break(
                buffer,
                current_style,
                pending_karaoke,
                deferred_karaoke,
                *drawing_scale,
                current_transforms,
                active_line,
            );
            *drawing_scale = parse_override_i32_arg(rest).unwrap_or(0).max(0);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('r') {
            let reset_style = resolve_reset_style(rest, base_style, styles);
            let preserved_pbo = current_style.pbo;
            *active_reset_style = reset_style.clone();
            *current_style = reset_style;
            current_style.pbo = preserved_pbo;
            *active_reset_alignment = resolve_reset_alignment(rest, base_style, styles);
            continue;
        }
        if let Some(rest) = tag.strip_prefix('t') {
            apply_transform_immediate_tags(
                rest,
                base_style,
                styles,
                current_style,
                active_reset_style,
                active_reset_alignment,
                parsed,
                buffer,
                active_line,
                pending_karaoke,
                karaoke_cursor_ms,
                drawing_scale,
                current_transforms,
                current_wrap_style,
                inherited_wrap_style,
                vector_clip_claimed,
                deferred_karaoke,
            );
        }
    }
}

fn transform_tag_animates_style(tag: &str) -> bool {
    strip_primary_colour_tag(tag).is_some()
        || tag.strip_prefix("2c").is_some()
        || tag.strip_prefix("3c").is_some()
        || tag.strip_prefix("4c").is_some()
        || tag.strip_prefix("alpha").is_some()
        || tag.strip_prefix("1a").is_some()
        || tag.strip_prefix("2a").is_some()
        || tag.strip_prefix("3a").is_some()
        || tag.strip_prefix("4a").is_some()
        || tag.strip_prefix("fscx").is_some()
        || tag.strip_prefix("fscy").is_some()
        || tag.strip_prefix("fsc").is_some()
        || tag.strip_prefix("fsp").is_some()
        || tag.strip_prefix("frx").is_some()
        || tag.strip_prefix("fry").is_some()
        || tag
            .strip_prefix("frz")
            .or_else(|| tag.strip_prefix("fr"))
            .is_some()
        || tag.strip_prefix("fax").is_some()
        || tag.strip_prefix("fay").is_some()
        || tag.strip_prefix("fs").is_some()
        || tag.strip_prefix("xbord").is_some()
        || tag.strip_prefix("ybord").is_some()
        || tag.strip_prefix("bord").is_some()
        || tag.strip_prefix("xshad").is_some()
        || tag.strip_prefix("yshad").is_some()
        || tag.strip_prefix("shad").is_some()
        || tag.strip_prefix("blur").is_some()
        || tag.strip_prefix("be").is_some()
}

fn suppress_transform_fields_for_override(
    tag: &str,
    current_style: &ParsedSpanStyle,
    current_transforms: &mut Vec<ParsedSpanTransform>,
) {
    if current_transforms.is_empty() || tag.strip_prefix('t').is_some() {
        return;
    }

    for transform in current_transforms.iter_mut() {
        let style = &mut transform.style;
        if tag.strip_prefix("iclip").is_some() || tag.strip_prefix("clip").is_some() {
            style.clip_rect = None;
            style.clip_inverse = None;
        } else if strip_primary_colour_tag(tag).is_some() {
            suppress_transform_rgb(&mut style.primary_colour, current_style.primary_colour);
            suppress_transform_rgb_steps(&mut style.primary_colour_steps);
        } else if tag.strip_prefix("2c").is_some() {
            suppress_transform_rgb(&mut style.secondary_colour, current_style.secondary_colour);
            suppress_transform_rgb_steps(&mut style.secondary_colour_steps);
        } else if tag.strip_prefix("3c").is_some() {
            suppress_transform_rgb(&mut style.outline_colour, current_style.outline_colour);
            suppress_transform_rgb_steps(&mut style.outline_colour_steps);
        } else if tag.strip_prefix("4c").is_some() {
            suppress_transform_rgb(&mut style.back_colour, current_style.back_colour);
            suppress_transform_rgb_steps(&mut style.back_colour_steps);
        } else if tag.strip_prefix("alpha").is_some() {
            suppress_transform_alpha(&mut style.primary_colour, current_style.primary_colour);
            suppress_transform_alpha(&mut style.secondary_colour, current_style.secondary_colour);
            suppress_transform_alpha(&mut style.outline_colour, current_style.outline_colour);
            suppress_transform_alpha(&mut style.back_colour, current_style.back_colour);
            suppress_transform_alpha_steps(&mut style.primary_colour_steps);
            suppress_transform_alpha_steps(&mut style.secondary_colour_steps);
            suppress_transform_alpha_steps(&mut style.outline_colour_steps);
            suppress_transform_alpha_steps(&mut style.back_colour_steps);
        } else if tag.strip_prefix("1a").is_some() {
            suppress_transform_alpha(&mut style.primary_colour, current_style.primary_colour);
            suppress_transform_alpha_steps(&mut style.primary_colour_steps);
        } else if tag.strip_prefix("2a").is_some() {
            suppress_transform_alpha(&mut style.secondary_colour, current_style.secondary_colour);
            suppress_transform_alpha_steps(&mut style.secondary_colour_steps);
        } else if tag.strip_prefix("3a").is_some() {
            suppress_transform_alpha(&mut style.outline_colour, current_style.outline_colour);
            suppress_transform_alpha_steps(&mut style.outline_colour_steps);
        } else if tag.strip_prefix("4a").is_some() {
            suppress_transform_alpha(&mut style.back_colour, current_style.back_colour);
            suppress_transform_alpha_steps(&mut style.back_colour_steps);
        } else if tag.strip_prefix("fscx").is_some() {
            style.scale_x = None;
            style.scale_x_steps.clear();
        } else if tag.strip_prefix("fscy").is_some() {
            style.scale_y = None;
            style.scale_y_steps.clear();
        } else if tag.strip_prefix("fsc").is_some() {
            style.scale_x = None;
            style.scale_x_steps.clear();
            style.scale_y = None;
            style.scale_y_steps.clear();
        } else if tag.strip_prefix("fsp").is_some() {
            style.spacing = None;
            style.spacing_steps.clear();
        } else if tag.strip_prefix("frx").is_some() {
            style.rotation_x = None;
            style.rotation_x_steps.clear();
        } else if tag.strip_prefix("fry").is_some() {
            style.rotation_y = None;
            style.rotation_y_steps.clear();
        } else if tag
            .strip_prefix("frz")
            .or_else(|| tag.strip_prefix("fr"))
            .is_some()
        {
            style.rotation_z = None;
            style.rotation_z_steps.clear();
        } else if tag.strip_prefix("fax").is_some() {
            style.shear_x = None;
            style.shear_x_steps.clear();
        } else if tag.strip_prefix("fay").is_some() {
            style.shear_y = None;
            style.shear_y_steps.clear();
        } else if strip_font_size_tag(tag).is_some() {
            style.font_size = None;
            style.font_size_steps.clear();
        } else if tag.strip_prefix("xbord").is_some() {
            style.border = None;
            style.border_x = None;
            style.border_x_steps.clear();
        } else if tag.strip_prefix("ybord").is_some() {
            style.border = None;
            style.border_y = None;
            style.border_y_steps.clear();
        } else if tag.strip_prefix("bord").is_some() {
            style.border = None;
            style.border_x = None;
            style.border_x_steps.clear();
            style.border_y = None;
            style.border_y_steps.clear();
        } else if tag.strip_prefix("xshad").is_some() {
            style.shadow = None;
            style.shadow_x = None;
            style.shadow_x_steps.clear();
        } else if tag.strip_prefix("yshad").is_some() {
            style.shadow = None;
            style.shadow_y = None;
            style.shadow_y_steps.clear();
        } else if tag.strip_prefix("shad").is_some() {
            style.shadow = None;
            style.shadow_x = None;
            style.shadow_x_steps.clear();
            style.shadow_y = None;
            style.shadow_y_steps.clear();
        } else if tag.strip_prefix("blur").is_some() {
            style.blur = None;
            style.blur_steps.clear();
        } else if tag.strip_prefix("be").is_some() {
            style.be = None;
            style.be_steps.clear();
        }
    }

    current_transforms.retain(|transform| !transform.style.is_empty());
}

fn suppress_transform_rgb(target: &mut Option<u32>, current: u32) {
    if let Some(colour) = *target {
        let adjusted = (colour & 0xFF00_0000) | (current & 0x00FF_FFFF);
        *target = (adjusted != current).then_some(adjusted);
    }
}

fn suppress_transform_alpha(target: &mut Option<u32>, current: u32) {
    if let Some(colour) = *target {
        let adjusted = (colour & 0x00FF_FFFF) | (current & 0xFF00_0000);
        *target = (adjusted != current).then_some(adjusted);
    }
}

fn suppress_transform_rgb_steps(steps: &mut Vec<ParsedColourTransform>) {
    steps.retain(|step| {
        !matches!(
            step,
            ParsedColourTransform::ResetRgb { .. } | ParsedColourTransform::Rgb { .. }
        )
    });
}

fn suppress_transform_alpha_steps(steps: &mut Vec<ParsedColourTransform>) {
    steps.retain(|step| {
        !matches!(
            step,
            ParsedColourTransform::ResetAlpha { .. } | ParsedColourTransform::Alpha { .. }
        )
    });
}

fn parse_transforms(
    value: &str,
    current_style: &ParsedSpanStyle,
    reset_style: &ParsedSpanStyle,
) -> Vec<ParsedSpanTransform> {
    let Some(inside) = parenthesized_args(value) else {
        return Vec::new();
    };
    let inside = trim_ass_tag(inside);
    let Some(tag_start) = inside.find('\\') else {
        return Vec::new();
    };
    let tags_part = &inside[tag_start..];
    let mut transforms = Vec::new();
    if let Some(transform) = parse_transform(value, current_style, reset_style) {
        transforms.push(transform);
    }
    for raw_tag in split_override_tags(tags_part) {
        let tag = trim_ass_tag(raw_tag);
        if let Some(rest) = tag.strip_prefix('t') {
            transforms.extend(parse_transforms(rest, current_style, reset_style));
        }
    }
    transforms
}

fn parse_transform_ms(value: &str, fallback: i32) -> i32 {
    let Some(parsed) = parse_drawing_number(value) else {
        return fallback;
    };
    libass_dtoi32(parsed)
}

fn libass_dtoi32(value: f64) -> i32 {
    if value.is_nan() || value <= f64::from(i32::MIN) || value >= f64::from(i32::MAX) + 1.0 {
        i32::MIN
    } else {
        value as i32
    }
}

fn parse_font_size_transform_step(tag: &str, reset: f64) -> Option<ParsedFontSizeTransform> {
    let rest = strip_font_size_tag(tag)?;
    let Some(arg) = first_override_arg(rest) else {
        return Some(ParsedFontSizeTransform::Reset { reset });
    };
    let value = parse_drawing_number(arg).unwrap_or(0.0);
    if arg.starts_with(['+', '-']) {
        Some(ParsedFontSizeTransform::Relative { value, reset })
    } else {
        Some(ParsedFontSizeTransform::Absolute { value, reset })
    }
}

fn strip_font_size_tag(tag: &str) -> Option<&str> {
    if tag.starts_with("fsc") || tag.starts_with("fsp") {
        return None;
    }
    tag.strip_prefix("fs")
}

fn font_size_transform_steps_are_needed(steps: &[ParsedFontSizeTransform]) -> bool {
    steps.len() > 1
        || steps.iter().any(|step| match *step {
            ParsedFontSizeTransform::Reset { .. } => true,
            ParsedFontSizeTransform::Absolute { value, .. } => value <= 0.0,
            ParsedFontSizeTransform::Relative { value, .. } => value <= -10.0,
        })
}

fn parse_scale_transform_step(rest: &str, reset: f64) -> ParsedScaleTransform {
    let Some(arg) = first_override_arg(rest) else {
        return ParsedScaleTransform::Reset { reset };
    };
    ParsedScaleTransform::Absolute {
        value: parse_drawing_number(arg).unwrap_or(0.0) / 100.0,
        reset,
    }
}

fn parse_scale_transform_steps(
    tag: &str,
    reset_style: &ParsedSpanStyle,
) -> (Option<ParsedScaleTransform>, Option<ParsedScaleTransform>) {
    if let Some(rest) = tag.strip_prefix("fscx") {
        return (
            Some(parse_scale_transform_step(rest, reset_style.scale_x)),
            None,
        );
    }
    if let Some(rest) = tag.strip_prefix("fscy") {
        return (
            None,
            Some(parse_scale_transform_step(rest, reset_style.scale_y)),
        );
    }
    if tag.strip_prefix("fsc").is_some() {
        return (
            Some(ParsedScaleTransform::Reset {
                reset: reset_style.scale_x,
            }),
            Some(ParsedScaleTransform::Reset {
                reset: reset_style.scale_y,
            }),
        );
    }
    (None, None)
}

fn scale_transform_steps_are_needed(steps: &[ParsedScaleTransform]) -> bool {
    steps.len() > 1
        || steps
            .iter()
            .any(|step| matches!(step, ParsedScaleTransform::Reset { .. }))
}

fn parse_linear_transform_step(rest: &str, reset: f64) -> ParsedLinearTransform {
    let Some(arg) = first_override_arg(rest) else {
        return ParsedLinearTransform::Reset { reset };
    };
    ParsedLinearTransform::Absolute {
        value: parse_drawing_number(arg).unwrap_or(0.0),
        reset,
    }
}

fn linear_transform_steps_are_needed(steps: &[ParsedLinearTransform]) -> bool {
    steps.len() > 1
        || steps
            .iter()
            .any(|step| matches!(step, ParsedLinearTransform::Reset { .. }))
}

fn parse_axis_transform_step(rest: &str, reset: f64, clamp: bool) -> ParsedAxisTransform {
    let Some(arg) = first_override_arg(rest) else {
        return ParsedAxisTransform::Reset { reset };
    };
    ParsedAxisTransform::Absolute {
        value: parse_drawing_number(arg).unwrap_or(0.0),
        reset,
        clamp,
    }
}

fn axis_transform_steps_are_needed(steps: &[ParsedAxisTransform]) -> bool {
    steps.len() > 1
        || steps
            .iter()
            .any(|step| matches!(step, ParsedAxisTransform::Reset { .. }))
}

fn axis_pair_transform_steps_are_needed(
    x_steps: &[ParsedAxisTransform],
    y_steps: &[ParsedAxisTransform],
) -> bool {
    axis_transform_steps_are_needed(x_steps) || axis_transform_steps_are_needed(y_steps)
}

fn parse_colour_rgb_transform_step(rest: &str, reset: u32) -> ParsedColourTransform {
    let Some(arg) = first_colour_override_arg(rest) else {
        return ParsedColourTransform::ResetRgb { reset };
    };
    let trimmed = arg.trim_start_matches(['&', 'H']);
    let value = parse_hex_i32_clamped_prefix(trimmed).unwrap_or(0) as u32 & 0x00FF_FFFF;
    ParsedColourTransform::Rgb { value }
}

fn parse_colour_alpha_transform_step(rest: &str, reset: u32) -> ParsedColourTransform {
    let Some(arg) = first_colour_override_arg(rest) else {
        return ParsedColourTransform::ResetAlpha {
            reset: alpha_of(reset),
        };
    };
    let trimmed = arg.trim_start_matches(['&', 'H']);
    let value = parse_hex_i32_clamped_prefix(trimmed).unwrap_or(0);
    ParsedColourTransform::Alpha { value }
}

fn colour_transform_steps_are_needed(steps: &[ParsedColourTransform]) -> bool {
    steps.len() > 1
        || steps.iter().any(|step| {
            matches!(
                step,
                ParsedColourTransform::ResetRgb { .. } | ParsedColourTransform::ResetAlpha { .. }
            ) || matches!(
                step,
                ParsedColourTransform::Alpha { value } if !(0..=0xFF).contains(value)
            )
        })
}

struct AxisPairTransformSteps {
    border_x: Option<ParsedAxisTransform>,
    border_y: Option<ParsedAxisTransform>,
    shadow_x: Option<ParsedAxisTransform>,
    shadow_y: Option<ParsedAxisTransform>,
}

fn parse_axis_pair_transform_steps(
    tag: &str,
    reset_style: &ParsedSpanStyle,
) -> AxisPairTransformSteps {
    let mut steps = AxisPairTransformSteps {
        border_x: None,
        border_y: None,
        shadow_x: None,
        shadow_y: None,
    };

    if let Some(rest) = tag.strip_prefix("xbord") {
        steps.border_x = Some(parse_axis_transform_step(rest, reset_style.border, true));
    } else if let Some(rest) = tag.strip_prefix("ybord") {
        steps.border_y = Some(parse_axis_transform_step(rest, reset_style.border, true));
    } else if let Some(rest) = tag.strip_prefix("bord") {
        steps.border_x = Some(parse_axis_transform_step(rest, reset_style.border, true));
        steps.border_y = Some(parse_axis_transform_step(rest, reset_style.border, true));
    } else if let Some(rest) = tag.strip_prefix("xshad") {
        steps.shadow_x = Some(parse_axis_transform_step(rest, reset_style.shadow, false));
    } else if let Some(rest) = tag.strip_prefix("yshad") {
        steps.shadow_y = Some(parse_axis_transform_step(rest, reset_style.shadow, false));
    } else if let Some(rest) = tag.strip_prefix("shad") {
        steps.shadow_x = Some(parse_axis_transform_step(rest, reset_style.shadow, true));
        steps.shadow_y = Some(parse_axis_transform_step(rest, reset_style.shadow, true));
    }

    steps
}

struct LinearTransformSteps {
    rotation_x: Option<ParsedLinearTransform>,
    rotation_y: Option<ParsedLinearTransform>,
    rotation_z: Option<ParsedLinearTransform>,
    shear_x: Option<ParsedLinearTransform>,
    shear_y: Option<ParsedLinearTransform>,
}

fn parse_geometry_linear_transform_steps(
    tag: &str,
    reset_style: &ParsedSpanStyle,
) -> LinearTransformSteps {
    let mut steps = LinearTransformSteps {
        rotation_x: None,
        rotation_y: None,
        rotation_z: None,
        shear_x: None,
        shear_y: None,
    };

    if let Some(rest) = tag.strip_prefix("frx") {
        steps.rotation_x = Some(parse_linear_transform_step(rest, 0.0));
    } else if let Some(rest) = tag.strip_prefix("fry") {
        steps.rotation_y = Some(parse_linear_transform_step(rest, 0.0));
    } else if let Some(rest) = tag.strip_prefix("frz").or_else(|| tag.strip_prefix("fr")) {
        steps.rotation_z = Some(parse_linear_transform_step(rest, reset_style.rotation_z));
    } else if let Some(rest) = tag.strip_prefix("fax") {
        steps.shear_x = Some(parse_linear_transform_step(rest, 0.0));
    } else if let Some(rest) = tag.strip_prefix("fay") {
        steps.shear_y = Some(parse_linear_transform_step(rest, 0.0));
    }

    steps
}

fn parse_transform(
    value: &str,
    current_style: &ParsedSpanStyle,
    reset_style: &ParsedSpanStyle,
) -> Option<ParsedSpanTransform> {
    let inside = trim_ass_tag(parenthesized_args(value)?);
    let tag_start = inside.find('\\')?;
    let (timing_part, tags_part) = inside.split_at(tag_start);
    let params = timing_part
        .split(',')
        .map(trim_ass_tag)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let (start_ms, end_ms, accel) = match params.as_slice() {
        [] => (0, None, 1.0),
        [accel] => (0, None, parse_f64(accel, 1.0)),
        [start, end] => {
            let start = parse_transform_ms(start, 0);
            let end = parse_transform_ms(end, 0);
            (start, Some(end), 1.0)
        }
        [start, end, accel, ..] => {
            if params.len() > 3 {
                return None;
            }
            let start = parse_complex_i32_arg(start);
            let end = parse_complex_i32_arg(end);
            (start, Some(end), parse_f64(accel, 1.0))
        }
    };

    let mut target_style = current_style.clone();
    let mut explicit_style = ParsedAnimatedStyle::default();
    let mut animated_clip = None;
    let mut animated_clip_inverse = None;
    let mut font_size_steps = Vec::new();
    let mut primary_colour_steps = Vec::new();
    let mut secondary_colour_steps = Vec::new();
    let mut outline_colour_steps = Vec::new();
    let mut back_colour_steps = Vec::new();
    let mut scale_x_steps = Vec::new();
    let mut scale_y_steps = Vec::new();
    let mut spacing_steps = Vec::new();
    let mut rotation_x_steps = Vec::new();
    let mut rotation_y_steps = Vec::new();
    let mut rotation_z_steps = Vec::new();
    let mut shear_x_steps = Vec::new();
    let mut shear_y_steps = Vec::new();
    let mut border_x_steps = Vec::new();
    let mut border_y_steps = Vec::new();
    let mut shadow_x_steps = Vec::new();
    let mut shadow_y_steps = Vec::new();
    let mut blur_steps = Vec::new();
    let mut be_steps = Vec::new();
    for raw_tag in split_override_tags(tags_part) {
        let tag = trim_ass_tag(raw_tag);
        if let Some(rest) = tag.strip_prefix("iclip") {
            if let Some(rect) = parse_rect_clip(rest) {
                animated_clip = Some(rect_to_f64(rect));
                animated_clip_inverse = Some(true);
            }
            continue;
        }
        if let Some(rest) = tag.strip_prefix("clip") {
            if let Some(rect) = parse_rect_clip(rest) {
                animated_clip = Some(rect_to_f64(rect));
                animated_clip_inverse = Some(false);
            }
            continue;
        }
        if let Some(step) = parse_font_size_transform_step(tag, reset_style.font_size) {
            font_size_steps.push(step);
        }
        if let Some(rest) = strip_primary_colour_tag(tag) {
            primary_colour_steps.push(parse_colour_rgb_transform_step(
                rest,
                reset_style.primary_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("2c") {
            secondary_colour_steps.push(parse_colour_rgb_transform_step(
                rest,
                reset_style.secondary_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("3c") {
            outline_colour_steps.push(parse_colour_rgb_transform_step(
                rest,
                reset_style.outline_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("4c") {
            back_colour_steps.push(parse_colour_rgb_transform_step(
                rest,
                reset_style.back_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("alpha") {
            primary_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.primary_colour,
            ));
            secondary_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.secondary_colour,
            ));
            outline_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.outline_colour,
            ));
            back_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.back_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("1a") {
            primary_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.primary_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("2a") {
            secondary_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.secondary_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("3a") {
            outline_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.outline_colour,
            ));
        } else if let Some(rest) = tag.strip_prefix("4a") {
            back_colour_steps.push(parse_colour_alpha_transform_step(
                rest,
                reset_style.back_colour,
            ));
        }
        let (scale_x_step, scale_y_step) = parse_scale_transform_steps(tag, reset_style);
        if let Some(step) = scale_x_step {
            scale_x_steps.push(step);
        }
        if let Some(step) = scale_y_step {
            scale_y_steps.push(step);
        }
        if let Some(rest) = tag.strip_prefix("fsp") {
            spacing_steps.push(parse_linear_transform_step(rest, reset_style.spacing));
        }
        let linear_steps = parse_geometry_linear_transform_steps(tag, reset_style);
        if let Some(step) = linear_steps.rotation_x {
            rotation_x_steps.push(step);
        }
        if let Some(step) = linear_steps.rotation_y {
            rotation_y_steps.push(step);
        }
        if let Some(step) = linear_steps.rotation_z {
            rotation_z_steps.push(step);
        }
        if let Some(step) = linear_steps.shear_x {
            shear_x_steps.push(step);
        }
        if let Some(step) = linear_steps.shear_y {
            shear_y_steps.push(step);
        }
        let axis_steps = parse_axis_pair_transform_steps(tag, reset_style);
        if let Some(step) = axis_steps.border_x {
            border_x_steps.push(step);
        }
        if let Some(step) = axis_steps.border_y {
            border_y_steps.push(step);
        }
        if let Some(step) = axis_steps.shadow_x {
            shadow_x_steps.push(step);
        }
        if let Some(step) = axis_steps.shadow_y {
            shadow_y_steps.push(step);
        }
        if let Some(rest) = tag.strip_prefix("blur") {
            blur_steps.push(parse_linear_transform_step(rest, 0.0));
        }
        if let Some(rest) = tag.strip_prefix("be") {
            be_steps.push(parse_linear_transform_step(rest, 0.0));
        }
        apply_transform_tag(tag, &mut target_style, reset_style);
        if tag.strip_prefix('r').is_some() {
            explicit_style = ParsedAnimatedStyle::default();
            font_size_steps.clear();
            primary_colour_steps.clear();
            secondary_colour_steps.clear();
            outline_colour_steps.clear();
            back_colour_steps.clear();
            scale_x_steps.clear();
            scale_y_steps.clear();
            spacing_steps.clear();
            rotation_x_steps.clear();
            rotation_y_steps.clear();
            rotation_z_steps.clear();
            shear_x_steps.clear();
            shear_y_steps.clear();
            border_x_steps.clear();
            border_y_steps.clear();
            shadow_x_steps.clear();
            shadow_y_steps.clear();
            blur_steps.clear();
            be_steps.clear();
            continue;
        }
        record_explicit_transform_tag(tag, &target_style, &mut explicit_style);
    }

    let mut animated = diff_animated_style(current_style, &target_style);
    merge_explicit_transform_style(&mut animated, explicit_style);
    if font_size_transform_steps_are_needed(&font_size_steps) {
        animated.font_size = None;
        animated.font_size_steps = font_size_steps;
    }
    if colour_transform_steps_are_needed(&primary_colour_steps) {
        animated.primary_colour = None;
        animated.primary_colour_steps = primary_colour_steps;
    }
    if colour_transform_steps_are_needed(&secondary_colour_steps) {
        animated.secondary_colour = None;
        animated.secondary_colour_steps = secondary_colour_steps;
    }
    if colour_transform_steps_are_needed(&outline_colour_steps) {
        animated.outline_colour = None;
        animated.outline_colour_steps = outline_colour_steps;
    }
    if colour_transform_steps_are_needed(&back_colour_steps) {
        animated.back_colour = None;
        animated.back_colour_steps = back_colour_steps;
    }
    if scale_transform_steps_are_needed(&scale_x_steps) {
        animated.scale_x = None;
        animated.scale_x_steps = scale_x_steps;
    }
    if scale_transform_steps_are_needed(&scale_y_steps) {
        animated.scale_y = None;
        animated.scale_y_steps = scale_y_steps;
    }
    if linear_transform_steps_are_needed(&spacing_steps) {
        animated.spacing = None;
        animated.spacing_steps = spacing_steps;
    }
    if linear_transform_steps_are_needed(&rotation_x_steps) {
        animated.rotation_x = None;
        animated.rotation_x_steps = rotation_x_steps;
    }
    if linear_transform_steps_are_needed(&rotation_y_steps) {
        animated.rotation_y = None;
        animated.rotation_y_steps = rotation_y_steps;
    }
    if linear_transform_steps_are_needed(&rotation_z_steps) {
        animated.rotation_z = None;
        animated.rotation_z_steps = rotation_z_steps;
    }
    if linear_transform_steps_are_needed(&shear_x_steps) {
        animated.shear_x = None;
        animated.shear_x_steps = shear_x_steps;
    }
    if linear_transform_steps_are_needed(&shear_y_steps) {
        animated.shear_y = None;
        animated.shear_y_steps = shear_y_steps;
    }
    if axis_pair_transform_steps_are_needed(&border_x_steps, &border_y_steps) {
        animated.border = None;
        animated.border_x = None;
        animated.border_y = None;
        animated.border_x_steps = border_x_steps;
        animated.border_y_steps = border_y_steps;
    }
    if axis_pair_transform_steps_are_needed(&shadow_x_steps, &shadow_y_steps) {
        animated.shadow = None;
        animated.shadow_x = None;
        animated.shadow_y = None;
        animated.shadow_x_steps = shadow_x_steps;
        animated.shadow_y_steps = shadow_y_steps;
    }
    if linear_transform_steps_are_needed(&blur_steps) {
        animated.blur = None;
        animated.blur_steps = blur_steps;
    }
    if linear_transform_steps_are_needed(&be_steps) {
        animated.be = None;
        animated.be_steps = be_steps;
    }
    animated.clip_rect = animated_clip;
    animated.clip_inverse = animated_clip_inverse;
    (!animated.is_empty()).then_some(ParsedSpanTransform {
        start_ms,
        end_ms,
        accel,
        style: animated,
    })
}

fn merge_explicit_transform_style(target: &mut ParsedAnimatedStyle, explicit: ParsedAnimatedStyle) {
    if explicit.font_size.is_some() {
        target.font_size = explicit.font_size;
    }
    if !explicit.font_size_steps.is_empty() {
        target.font_size_steps = explicit.font_size_steps;
    }
    if explicit.scale_x.is_some() {
        target.scale_x = explicit.scale_x;
    }
    if !explicit.scale_x_steps.is_empty() {
        target.scale_x_steps = explicit.scale_x_steps;
    }
    if explicit.scale_y.is_some() {
        target.scale_y = explicit.scale_y;
    }
    if !explicit.scale_y_steps.is_empty() {
        target.scale_y_steps = explicit.scale_y_steps;
    }
    if explicit.spacing.is_some() {
        target.spacing = explicit.spacing;
    }
    if !explicit.spacing_steps.is_empty() {
        target.spacing_steps = explicit.spacing_steps;
    }
    if explicit.rotation_x.is_some() {
        target.rotation_x = explicit.rotation_x;
    }
    if !explicit.rotation_x_steps.is_empty() {
        target.rotation_x_steps = explicit.rotation_x_steps;
    }
    if explicit.rotation_y.is_some() {
        target.rotation_y = explicit.rotation_y;
    }
    if !explicit.rotation_y_steps.is_empty() {
        target.rotation_y_steps = explicit.rotation_y_steps;
    }
    if explicit.rotation_z.is_some() {
        target.rotation_z = explicit.rotation_z;
    }
    if !explicit.rotation_z_steps.is_empty() {
        target.rotation_z_steps = explicit.rotation_z_steps;
    }
    if explicit.shear_x.is_some() {
        target.shear_x = explicit.shear_x;
    }
    if !explicit.shear_x_steps.is_empty() {
        target.shear_x_steps = explicit.shear_x_steps;
    }
    if explicit.shear_y.is_some() {
        target.shear_y = explicit.shear_y;
    }
    if !explicit.shear_y_steps.is_empty() {
        target.shear_y_steps = explicit.shear_y_steps;
    }
    if explicit.primary_colour.is_some() {
        target.primary_colour = explicit.primary_colour;
    }
    if !explicit.primary_colour_steps.is_empty() {
        target.primary_colour_steps = explicit.primary_colour_steps;
    }
    if explicit.secondary_colour.is_some() {
        target.secondary_colour = explicit.secondary_colour;
    }
    if !explicit.secondary_colour_steps.is_empty() {
        target.secondary_colour_steps = explicit.secondary_colour_steps;
    }
    if explicit.outline_colour.is_some() {
        target.outline_colour = explicit.outline_colour;
    }
    if !explicit.outline_colour_steps.is_empty() {
        target.outline_colour_steps = explicit.outline_colour_steps;
    }
    if explicit.back_colour.is_some() {
        target.back_colour = explicit.back_colour;
    }
    if !explicit.back_colour_steps.is_empty() {
        target.back_colour_steps = explicit.back_colour_steps;
    }
    if explicit.border.is_some() {
        target.border = explicit.border;
    }
    if explicit.border_x.is_some() {
        target.border_x = explicit.border_x;
    }
    if !explicit.border_x_steps.is_empty() {
        target.border_x_steps = explicit.border_x_steps;
    }
    if explicit.border_y.is_some() {
        target.border_y = explicit.border_y;
    }
    if !explicit.border_y_steps.is_empty() {
        target.border_y_steps = explicit.border_y_steps;
    }
    if explicit.shadow.is_some() {
        target.shadow = explicit.shadow;
    }
    if explicit.shadow_x.is_some() {
        target.shadow_x = explicit.shadow_x;
    }
    if !explicit.shadow_x_steps.is_empty() {
        target.shadow_x_steps = explicit.shadow_x_steps;
    }
    if explicit.shadow_y.is_some() {
        target.shadow_y = explicit.shadow_y;
    }
    if !explicit.shadow_y_steps.is_empty() {
        target.shadow_y_steps = explicit.shadow_y_steps;
    }
    if explicit.blur.is_some() {
        target.blur = explicit.blur;
    }
    if !explicit.blur_steps.is_empty() {
        target.blur_steps = explicit.blur_steps;
    }
    if explicit.be.is_some() {
        target.be = explicit.be;
    }
    if !explicit.be_steps.is_empty() {
        target.be_steps = explicit.be_steps;
    }
}

fn record_explicit_transform_tag(
    tag: &str,
    style: &ParsedSpanStyle,
    animated: &mut ParsedAnimatedStyle,
) {
    if strip_primary_colour_tag(tag).is_some() {
        animated.primary_colour = Some(style.primary_colour);
    } else if tag.strip_prefix("2c").is_some() {
        animated.secondary_colour = Some(style.secondary_colour);
    } else if tag.strip_prefix("3c").is_some() {
        animated.outline_colour = Some(style.outline_colour);
    } else if tag.strip_prefix("4c").is_some() {
        animated.back_colour = Some(style.back_colour);
    } else if tag.strip_prefix("alpha").is_some() {
        animated.primary_colour = Some(style.primary_colour);
        animated.secondary_colour = Some(style.secondary_colour);
        animated.outline_colour = Some(style.outline_colour);
        animated.back_colour = Some(style.back_colour);
    } else if tag.strip_prefix("1a").is_some() {
        animated.primary_colour = Some(style.primary_colour);
    } else if tag.strip_prefix("2a").is_some() {
        animated.secondary_colour = Some(style.secondary_colour);
    } else if tag.strip_prefix("3a").is_some() {
        animated.outline_colour = Some(style.outline_colour);
    } else if tag.strip_prefix("4a").is_some() {
        animated.back_colour = Some(style.back_colour);
    } else if tag.strip_prefix("fscx").is_some() {
        animated.scale_x = Some(style.scale_x);
    } else if tag.strip_prefix("fscy").is_some() {
        animated.scale_y = Some(style.scale_y);
    } else if tag.strip_prefix("fsc").is_some() {
        animated.scale_x = Some(style.scale_x);
        animated.scale_y = Some(style.scale_y);
    } else if tag.strip_prefix("fsp").is_some() {
        animated.spacing = Some(style.spacing);
    } else if tag.strip_prefix("frx").is_some() {
        animated.rotation_x = Some(style.rotation_x);
    } else if tag.strip_prefix("fry").is_some() {
        animated.rotation_y = Some(style.rotation_y);
    } else if tag
        .strip_prefix("frz")
        .or_else(|| tag.strip_prefix("fr"))
        .is_some()
    {
        animated.rotation_z = Some(style.rotation_z);
    } else if tag.strip_prefix("fax").is_some() {
        animated.shear_x = Some(style.shear_x);
    } else if tag.strip_prefix("fay").is_some() {
        animated.shear_y = Some(style.shear_y);
    } else if strip_font_size_tag(tag).is_some() {
        animated.font_size = Some(style.font_size);
    } else if tag.strip_prefix("xbord").is_some() {
        animated.border_x = Some(style.border_x);
    } else if tag.strip_prefix("ybord").is_some() {
        animated.border_y = Some(style.border_y);
    } else if tag.strip_prefix("bord").is_some() {
        animated.border = Some(style.border);
        animated.border_x = Some(style.border_x);
        animated.border_y = Some(style.border_y);
    } else if tag.strip_prefix("xshad").is_some() {
        animated.shadow_x = Some(style.shadow_x);
    } else if tag.strip_prefix("yshad").is_some() {
        animated.shadow_y = Some(style.shadow_y);
    } else if tag.strip_prefix("shad").is_some() {
        animated.shadow = Some(style.shadow);
        animated.shadow_x = Some(style.shadow_x);
        animated.shadow_y = Some(style.shadow_y);
    } else if tag.strip_prefix("blur").is_some() {
        animated.blur = Some(style.blur);
    } else if tag.strip_prefix("be").is_some() {
        animated.be = Some(style.be);
    }
}

fn split_override_tags(block: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let bytes = block.as_bytes();
    let mut cursor = 0;

    while let Some(offset) = bytes[cursor..].iter().position(|byte| *byte == b'\\') {
        let slash = cursor + offset;
        let mut tag_start = slash + 1;
        tag_start = skip_ass_tag_spaces(bytes, tag_start);

        let mut end = tag_start;
        while end < bytes.len() && bytes[end] != b'(' && bytes[end] != b'\\' {
            end += 1;
        }
        if end == tag_start {
            cursor = end;
            continue;
        }

        if end < bytes.len() && bytes[end] == b'(' {
            end = consume_libass_parenthesized_tag(bytes, end);
        }

        let tag = trim_ass_tag(&block[tag_start..end]);
        if !tag.is_empty() {
            tags.push(tag);
        }

        cursor = end;
    }

    tags
}

fn skip_ass_tag_spaces(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
        index += 1;
    }
    index
}

fn consume_libass_parenthesized_tag(bytes: &[u8], mut index: usize) -> usize {
    index += 1;
    loop {
        if index < bytes.len() {
            index = skip_ass_tag_spaces(bytes, index);
        }

        let mut end = index;
        while end < bytes.len() && bytes[end] != b',' && bytes[end] != b'\\' && bytes[end] != b')' {
            end += 1;
        }

        if end < bytes.len() && bytes[end] == b',' {
            index = end + 1;
            continue;
        }

        if end < bytes.len() && bytes[end] == b'\\' {
            end = bytes[end..]
                .iter()
                .position(|byte| *byte == b')')
                .map_or(bytes.len(), |offset| end + offset);
        }

        index = end;
        if index < bytes.len() {
            index += 1;
        }
        return index;
    }
}

fn trim_ass_tag(value: &str) -> &str {
    trim_ass_trailing_spaces(trim_ass_leading_spaces(value))
}

fn block_has_libass_hard_override(block: &str) -> bool {
    let bytes = block.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && libass_hard_override_tag_at(&block[index + 1..]) {
            return true;
        }
        index += 1;
    }
    false
}

/// Raw scan matching libass ass_event_has_hard_overrides (backslash tags inside override blocks).
pub fn dialogue_has_libass_hard_override(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            index += 2;
            continue;
        }
        if bytes[index] != b'{' {
            index += 1;
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index] != b'}' {
            if bytes[index] == b'\\'
                && libass_hard_override_tag_at(text.get(index + 1..).unwrap_or_default())
            {
                return true;
            }
            index += 1;
        }
    }
    false
}

fn libass_hard_override_tag_at(value: &str) -> bool {
    value.starts_with("pos")
        || value.starts_with("move")
        || value.starts_with("clip")
        || value.starts_with("iclip")
        || value.starts_with("org")
        || value.starts_with("pbo")
        || value.starts_with('p')
}

fn apply_transform_tag(tag: &str, style: &mut ParsedSpanStyle, reset_style: &ParsedSpanStyle) {
    if let Some(rest) = strip_primary_colour_tag(tag) {
        style.primary_colour =
            apply_color_override(rest, style.primary_colour, reset_style.primary_colour);
    } else if let Some(rest) = tag.strip_prefix("2c") {
        style.secondary_colour =
            apply_color_override(rest, style.secondary_colour, reset_style.secondary_colour);
    } else if let Some(rest) = tag.strip_prefix("3c") {
        style.outline_colour =
            apply_color_override(rest, style.outline_colour, reset_style.outline_colour);
    } else if let Some(rest) = tag.strip_prefix("4c") {
        style.back_colour = apply_color_override(rest, style.back_colour, reset_style.back_colour);
    } else if let Some(rest) = tag.strip_prefix("alpha") {
        if let Some(alpha) = parse_alpha_tag(rest) {
            style.primary_colour = with_alpha(style.primary_colour, alpha);
            style.secondary_colour = with_alpha(style.secondary_colour, alpha);
            style.outline_colour = with_alpha(style.outline_colour, alpha);
            style.back_colour = with_alpha(style.back_colour, alpha);
        } else {
            style.primary_colour =
                with_alpha(style.primary_colour, alpha_of(reset_style.primary_colour));
            style.secondary_colour = with_alpha(
                style.secondary_colour,
                alpha_of(reset_style.secondary_colour),
            );
            style.outline_colour =
                with_alpha(style.outline_colour, alpha_of(reset_style.outline_colour));
            style.back_colour = with_alpha(style.back_colour, alpha_of(reset_style.back_colour));
        }
    } else if let Some(rest) = tag.strip_prefix("1a") {
        style.primary_colour = with_alpha(
            style.primary_colour,
            parse_alpha_tag(rest).unwrap_or_else(|| alpha_of(reset_style.primary_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("2a") {
        style.secondary_colour = with_alpha(
            style.secondary_colour,
            parse_alpha_tag(rest).unwrap_or_else(|| alpha_of(reset_style.secondary_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("3a") {
        style.outline_colour = with_alpha(
            style.outline_colour,
            parse_alpha_tag(rest).unwrap_or_else(|| alpha_of(reset_style.outline_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("4a") {
        style.back_colour = with_alpha(
            style.back_colour,
            parse_alpha_tag(rest).unwrap_or_else(|| alpha_of(reset_style.back_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("fscx") {
        style.scale_x = parse_transform_scale_target(rest, reset_style.scale_x);
    } else if let Some(rest) = tag.strip_prefix("fscy") {
        style.scale_y = parse_transform_scale_target(rest, reset_style.scale_y);
    } else if tag.strip_prefix("fsc").is_some() {
        style.scale_x = reset_style.scale_x;
        style.scale_y = reset_style.scale_y;
    } else if let Some(rest) = tag.strip_prefix("fsp") {
        style.spacing = parse_override_f64_arg(rest, reset_style.spacing);
    } else if let Some(rest) = tag.strip_prefix("frx") {
        style.rotation_x = parse_override_f64_arg(rest, 0.0);
    } else if let Some(rest) = tag.strip_prefix("fry") {
        style.rotation_y = parse_override_f64_arg(rest, 0.0);
    } else if let Some(rest) = tag.strip_prefix("frz").or_else(|| tag.strip_prefix("fr")) {
        style.rotation_z = parse_override_f64_arg(rest, reset_style.rotation_z);
    } else if let Some(rest) = tag.strip_prefix("fax") {
        style.shear_x = parse_override_f64_arg(rest, 0.0);
    } else if let Some(rest) = tag.strip_prefix("fay") {
        style.shear_y = parse_override_f64_arg(rest, 0.0);
    } else if let Some(rest) = strip_font_size_tag(tag) {
        style.font_size = parse_font_size_override(rest, style.font_size, reset_style.font_size);
    } else if let Some(rest) = tag.strip_prefix("xbord") {
        style.border_x = parse_transform_f64_target(rest, reset_style.border_x);
    } else if let Some(rest) = tag.strip_prefix("ybord") {
        style.border_y = parse_transform_f64_target(rest, reset_style.border_y);
    } else if let Some(rest) = tag.strip_prefix("bord") {
        style.border = parse_transform_f64_target(rest, reset_style.border);
        style.border_x = style.border;
        style.border_y = style.border;
    } else if let Some(rest) = tag.strip_prefix("xshad") {
        style.shadow_x = parse_override_f64_arg(rest, reset_style.shadow_x);
    } else if let Some(rest) = tag.strip_prefix("yshad") {
        style.shadow_y = parse_override_f64_arg(rest, reset_style.shadow_y);
    } else if let Some(rest) = tag.strip_prefix("shad") {
        style.shadow = parse_transform_f64_target(rest, reset_style.shadow);
        style.shadow_x = style.shadow;
        style.shadow_y = style.shadow;
    } else if let Some(rest) = tag.strip_prefix("blur") {
        style.blur = parse_transform_blur_target(rest);
    } else if let Some(rest) = tag.strip_prefix("be") {
        style.be = parse_transform_be_target(rest);
    } else if tag.strip_prefix('r').is_some() {
        let preserved_pbo = style.pbo;
        *style = reset_style.clone();
        style.pbo = preserved_pbo;
    }
}

fn strip_primary_colour_tag(tag: &str) -> Option<&str> {
    if let Some(rest) = tag.strip_prefix("1c") {
        return Some(rest);
    }

    if tag.starts_with("clip") {
        return None;
    }

    let rest = tag.strip_prefix('c')?;
    Some(rest)
}

fn diff_animated_style(base: &ParsedSpanStyle, target: &ParsedSpanStyle) -> ParsedAnimatedStyle {
    ParsedAnimatedStyle {
        font_size: ((target.font_size - base.font_size).abs() > f64::EPSILON)
            .then_some(target.font_size),
        font_size_steps: Vec::new(),
        scale_x: ((target.scale_x - base.scale_x).abs() > f64::EPSILON).then_some(target.scale_x),
        scale_x_steps: Vec::new(),
        scale_y: ((target.scale_y - base.scale_y).abs() > f64::EPSILON).then_some(target.scale_y),
        scale_y_steps: Vec::new(),
        spacing: ((target.spacing - base.spacing).abs() > f64::EPSILON).then_some(target.spacing),
        spacing_steps: Vec::new(),
        rotation_x: ((target.rotation_x - base.rotation_x).abs() > f64::EPSILON)
            .then_some(target.rotation_x),
        rotation_x_steps: Vec::new(),
        rotation_y: ((target.rotation_y - base.rotation_y).abs() > f64::EPSILON)
            .then_some(target.rotation_y),
        rotation_y_steps: Vec::new(),
        rotation_z: ((target.rotation_z - base.rotation_z).abs() > f64::EPSILON)
            .then_some(target.rotation_z),
        rotation_z_steps: Vec::new(),
        shear_x: ((target.shear_x - base.shear_x).abs() > f64::EPSILON).then_some(target.shear_x),
        shear_x_steps: Vec::new(),
        shear_y: ((target.shear_y - base.shear_y).abs() > f64::EPSILON).then_some(target.shear_y),
        shear_y_steps: Vec::new(),
        primary_colour: (target.primary_colour != base.primary_colour)
            .then_some(target.primary_colour),
        primary_colour_steps: Vec::new(),
        secondary_colour: (target.secondary_colour != base.secondary_colour)
            .then_some(target.secondary_colour),
        secondary_colour_steps: Vec::new(),
        outline_colour: (target.outline_colour != base.outline_colour)
            .then_some(target.outline_colour),
        outline_colour_steps: Vec::new(),
        back_colour: (target.back_colour != base.back_colour).then_some(target.back_colour),
        back_colour_steps: Vec::new(),
        border: ((target.border - base.border).abs() > f64::EPSILON).then_some(target.border),
        border_x: ((target.border_x - base.border_x).abs() > f64::EPSILON)
            .then_some(target.border_x),
        border_x_steps: Vec::new(),
        border_y: ((target.border_y - base.border_y).abs() > f64::EPSILON)
            .then_some(target.border_y),
        border_y_steps: Vec::new(),
        shadow: ((target.shadow - base.shadow).abs() > f64::EPSILON).then_some(target.shadow),
        shadow_x: ((target.shadow_x - base.shadow_x).abs() > f64::EPSILON)
            .then_some(target.shadow_x),
        shadow_x_steps: Vec::new(),
        shadow_y: ((target.shadow_y - base.shadow_y).abs() > f64::EPSILON)
            .then_some(target.shadow_y),
        shadow_y_steps: Vec::new(),
        blur: ((target.blur - base.blur).abs() > f64::EPSILON).then_some(target.blur),
        blur_steps: Vec::new(),
        be: ((target.be - base.be).abs() > f64::EPSILON).then_some(target.be),
        be_steps: Vec::new(),
        clip_rect: None,
        clip_inverse: None,
    }
}

fn parse_font_size_override(value: &str, current: f64, base: f64) -> f64 {
    let Some(arg) = first_override_arg(value) else {
        return base;
    };

    let parsed = parse_drawing_number(arg).unwrap_or(0.0);
    let resolved = if arg.starts_with(['+', '-']) {
        current * (1.0 + parsed / 10.0)
    } else {
        parsed
    };

    if resolved > 0.0 { resolved } else { base }
}

fn parse_karaoke_duration(value: &str) -> Option<i32> {
    Some(parse_karaoke_duration_with_default(value, 1000))
}

fn parse_karaoke_duration_with_default(value: &str, missing_fallback: i32) -> i32 {
    first_override_arg(value).map_or(missing_fallback, |arg| {
        libass_dtoi32(parse_drawing_number(arg).unwrap_or(0.0) * 10.0)
    })
}

fn apply_karaoke_timing_reset(
    pending_karaoke: &mut Option<ParsedKaraokeSpan>,
    deferred_karaoke: &mut Option<ParsedKaraokeSpan>,
    karaoke_cursor_ms: &mut i32,
    value: &str,
    pending_karaoke_is_unconsumed: bool,
) {
    let start_ms = parse_karaoke_duration_with_default(value, 0);
    *karaoke_cursor_ms = start_ms;
    let Some(karaoke) = *pending_karaoke else {
        *deferred_karaoke = None;
        return;
    };

    if pending_karaoke_is_unconsumed {
        if let Some(karaoke) = pending_karaoke.as_mut() {
            karaoke.start_ms = start_ms;
            karaoke.duration_ms = 0;
        }
    } else {
        *deferred_karaoke = Some(ParsedKaraokeSpan {
            start_ms,
            duration_ms: 0,
            mode: karaoke.mode,
        });
    }
}

fn parse_font_name_override(value: &str, fallback: &str) -> String {
    let value = trim_ass_trailing_spaces(value);
    if value.is_empty() {
        return fallback.to_string();
    }

    if let Some((before_parentheses, inside_parentheses)) = split_first_parenthesized_args(value) {
        if let Some(arg) = inside_parentheses
            .split(',')
            .map(|part| trim_ass_trailing_spaces(trim_ass_leading_spaces(part)))
            .find(|part| !part.is_empty())
        {
            return if arg == "0" {
                fallback.to_string()
            } else {
                arg.to_string()
            };
        }

        let fallback_arg = trim_ass_trailing_spaces(before_parentheses);
        if fallback_arg.is_empty() {
            return fallback.to_string();
        }
        return if fallback_arg == "0" {
            fallback.to_string()
        } else {
            trim_ass_leading_spaces(fallback_arg).to_string()
        };
    }

    if value == "0" {
        fallback.to_string()
    } else {
        trim_ass_leading_spaces(value).to_string()
    }
}

fn parse_override_rgb(value: &str) -> Option<u32> {
    let trimmed = first_colour_override_arg(value)?;
    let trimmed = trimmed.trim_start_matches(['&', 'H']);
    parse_hex_i32_clamped_prefix(trimmed).map(|color| (color as u32) & 0x00FF_FFFF)
}

fn parse_alpha_tag(value: &str) -> Option<u8> {
    let trimmed = first_colour_override_arg(value)?;
    let trimmed = trimmed.trim_start_matches(['&', 'H']);
    Some(parse_hex_i32_clamped_prefix(trimmed).unwrap_or(0) as u8)
}

fn parse_hex_i32_clamped_prefix(value: &str) -> Option<i32> {
    let value = trim_ass_c_leading_spaces(value);
    if value.is_empty() {
        return None;
    }

    let (sign, rest) = match value.as_bytes()[0] {
        b'+' => (1_i128, &value[1..]),
        b'-' => (-1_i128, &value[1..]),
        _ => (1_i128, value),
    };
    let digits = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .unwrap_or(rest);
    let end = digits
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_hexdigit())
        .map(|(index, character)| index + character.len_utf8())
        .last()?;

    let mut parsed = 0_i128;
    for byte in digits[..end].bytes() {
        let digit = match byte {
            b'0'..=b'9' => i128::from(byte - b'0'),
            b'a'..=b'f' => i128::from(byte - b'a' + 10),
            b'A'..=b'F' => i128::from(byte - b'A' + 10),
            _ => unreachable!("hex digit filter allowed only hex bytes"),
        };
        parsed = parsed.saturating_mul(16).saturating_add(digit);
    }

    Some(
        parsed
            .saturating_mul(sign)
            .clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32,
    )
}

fn alpha_of(color: u32) -> u8 {
    ((color >> 24) & 0xFF) as u8
}

fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color & 0x00FF_FFFF) | (u32::from(alpha) << 24)
}

fn parse_override_bool(value: &str, fallback: bool) -> bool {
    let Some(arg) = first_override_arg(value) else {
        return fallback;
    };

    match parse_i32_decimal_prefix(arg).unwrap_or(0) {
        0 => false,
        1 => true,
        _ => fallback,
    }
}

fn alignment_from_an(value: i32) -> Option<i32> {
    Some(match value {
        1 => ass::VALIGN_SUB | ass::HALIGN_LEFT,
        2 => ass::VALIGN_SUB | ass::HALIGN_CENTER,
        3 => ass::VALIGN_SUB | ass::HALIGN_RIGHT,
        4 => ass::VALIGN_CENTER | ass::HALIGN_LEFT,
        5 => ass::VALIGN_CENTER | ass::HALIGN_CENTER,
        6 => ass::VALIGN_CENTER | ass::HALIGN_RIGHT,
        7 => ass::VALIGN_TOP | ass::HALIGN_LEFT,
        8 => ass::VALIGN_TOP | ass::HALIGN_CENTER,
        9 => ass::VALIGN_TOP | ass::HALIGN_RIGHT,
        _ => return None,
    })
}

fn alignment_from_legacy_a(value: i32) -> Option<i32> {
    if !(1..=11).contains(&value) {
        return None;
    }

    let value = if value & 0x3 == 0 { 5 } else { value };
    let halign = match value & 0x3 {
        1 => ass::HALIGN_LEFT,
        2 => ass::HALIGN_CENTER,
        3 => ass::HALIGN_RIGHT,
        _ => return None,
    };
    let valign = if value & 0x4 != 0 {
        ass::VALIGN_TOP
    } else if value & 0x8 != 0 {
        ass::VALIGN_CENTER
    } else {
        ass::VALIGN_SUB
    };
    Some(valign | halign)
}

fn style_alignment_from_style(value: i32, track_type: TrackType) -> i32 {
    if track_type == TrackType::Ssa {
        match value {
            8 => 3,
            4 => 11,
            _ => value,
        }
    } else {
        style_alignment_from_numpad(value)
    }
}

fn style_alignment_from_numpad(value: i32) -> i32 {
    let value = value.checked_abs().unwrap_or(2);
    if value == 0 {
        return 0;
    }
    let halign = ((value - 1) % 3) + 1;
    let valign = if value <= 3 {
        ass::VALIGN_SUB
    } else if value <= 6 {
        ass::VALIGN_CENTER
    } else {
        ass::VALIGN_TOP
    };
    valign | halign
}

fn parse_pos(value: &str) -> Option<(i32, i32)> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    let [x, y] = parts.as_slice() else {
        return None;
    };
    let x = x.parse::<i32>().ok()?;
    let y = y.parse::<i32>().ok()?;
    Some((x, y))
}

fn parse_pos_exact(value: &str) -> Option<(f64, f64)> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    let [x, y] = parts.as_slice() else {
        return None;
    };
    let x = parse_complex_f64_arg(x);
    let y = parse_complex_f64_arg(y);
    Some((x, y))
}

fn parse_rect_clip(value: &str) -> Option<Rect> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    if parts.len() != 4 {
        return None;
    }
    let x_min = parse_complex_i32_arg(parts[0]);
    let y_min = parse_complex_i32_arg(parts[1]);
    let x_max = parse_complex_i32_arg(parts[2]);
    let y_max = parse_complex_i32_arg(parts[3]);
    Some(Rect {
        x_min,
        y_min,
        x_max,
        y_max,
    })
}

fn parse_rect_clip_exact(value: &str) -> Option<ParsedRectF64> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    if parts.len() != 4 {
        return None;
    }
    let x_min = parse_complex_f64_arg(parts[0]);
    let y_min = parse_complex_f64_arg(parts[1]);
    let x_max = parse_complex_f64_arg(parts[2]);
    let y_max = parse_complex_f64_arg(parts[3]);
    Some(ParsedRectF64 {
        x_min,
        y_min,
        x_max,
        y_max,
    })
}

fn rect_to_f64(rect: Rect) -> ParsedRectF64 {
    ParsedRectF64 {
        x_min: f64::from(rect.x_min),
        y_min: f64::from(rect.y_min),
        x_max: f64::from(rect.x_max),
        y_max: f64::from(rect.y_max),
    }
}

fn parse_vector_clip(value: &str) -> Option<ParsedVectorClip> {
    let (scale, drawing) = vector_clip_args(value)?;

    if libass_drawing_scale_base(scale) <= 0 {
        return Some(ParsedVectorClip {
            scale,
            polygons: Vec::new(),
        });
    }

    let polygons = match parse_drawing_polygons_checked(drawing, scale) {
        DrawingParseOutcome::Parsed(polygons) => polygons.unwrap_or_default(),
        // Out-of-range outline construction leaves both regular and inverse vector clips unapplied.
        DrawingParseOutcome::InvalidOutline => return None,
    };

    Some(ParsedVectorClip { scale, polygons })
}

/// First claimed vector clip in 26.6; the dialogue parser's ParsedVectorClip stays integer for API compatibility.
pub fn parse_dialogue_vector_clip_d6(text: &str) -> Option<ParsedVectorClip> {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while let Some(open_offset) = bytes[cursor..].iter().position(|byte| *byte == b'{') {
        let block_start = cursor.checked_add(open_offset)?.checked_add(1)?;
        let Some(close_offset) = bytes[block_start..].iter().position(|byte| *byte == b'}') else {
            break;
        };
        let block_end = block_start.checked_add(close_offset)?;
        if let Some(claim) =
            vector_clip_d6_claim_in_override_block(&text[block_start..block_end], false)
        {
            return claim;
        }
        cursor = block_end.checked_add(1)?;
    }
    None
}

fn vector_clip_d6_claim_in_override_block(
    block: &str,
    inside_transform: bool,
) -> Option<Option<ParsedVectorClip>> {
    for raw_tag in split_override_tags(block) {
        let tag = trim_ass_tag(raw_tag);
        let clip_rest = tag
            .strip_prefix("iclip")
            .or_else(|| tag.strip_prefix("clip"));
        if let Some(rest) = clip_rest {
            if vector_clip_args(rest).is_some() {
                return Some(parse_vector_clip_d6(rest));
            }
            continue;
        }
        // One \t layer only, matching apply_transform_immediate_tags; nested \t(\t(\clip)) is ignored.
        // Recursion would also let hostile nesting exhaust the stack.
        if !inside_transform {
            let Some(rest) = tag.strip_prefix('t') else {
                continue;
            };
            let Some(inside) = parenthesized_args(rest) else {
                continue;
            };
            let Some(tag_start) = inside.find('\\') else {
                continue;
            };
            if let Some(claim) = vector_clip_d6_claim_in_override_block(&inside[tag_start..], true)
            {
                return Some(claim);
            }
        }
    }
    None
}

fn parse_vector_clip_d6(value: &str) -> Option<ParsedVectorClip> {
    let (scale, drawing) = vector_clip_args(value)?;
    let polygons =
        match parse_drawing_polygons_checked_with_mode(drawing, DrawingCoordinateMode::FixedD6) {
            DrawingParseOutcome::Parsed(polygons) => polygons.unwrap_or_default(),
            DrawingParseOutcome::InvalidOutline => return None,
        };
    Some(ParsedVectorClip { scale, polygons })
}

fn vector_clip_args(value: &str) -> Option<(i32, &str)> {
    let inside = trim_ass_tag(parenthesized_args(value)?);
    if inside.is_empty() {
        return None;
    }

    let parts = split_complex_args(inside);
    match parts.as_slice() {
        [drawing] => Some((1, *drawing)),
        // Explicit vector-clip scales clamp to at least 1; unlike \p, zero is not a leave-drawing-mode.
        [scale, drawing] => Some((parse_override_i32_arg(scale).unwrap_or(1).max(1), *drawing)),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DrawingParseOutcome {
    Parsed(Option<Vec<Vec<Point>>>),
    InvalidOutline,
}

fn parse_drawing_polygons(drawing: &str, scale: i32) -> Option<Vec<Vec<Point>>> {
    match parse_drawing_polygons_checked(drawing, scale) {
        DrawingParseOutcome::Parsed(polygons) => polygons,
        // Keep a drawing-mode span even if the outline is rejected, so its command text is not shaped as text.
        DrawingParseOutcome::InvalidOutline => Some(Vec::new()),
    }
}

/// Parse a drawing in libass 26.6 space; ParsedDrawing stays integer for API compatibility.
pub fn parse_drawing_polygons_d6(drawing: &str, scale: i32) -> Option<Vec<Vec<Point>>> {
    if drawing.is_empty() {
        return None;
    }
    if libass_drawing_scale_base(scale) <= 0 {
        return Some(Vec::new());
    }
    match parse_drawing_polygons_checked_with_mode(drawing, DrawingCoordinateMode::FixedD6) {
        DrawingParseOutcome::Parsed(polygons) => polygons,
        DrawingParseOutcome::InvalidOutline => Some(Vec::new()),
    }
}

/// Tokenizer bbox in 26.6: control points count even if the curve never reaches them.
pub fn parse_drawing_bbox_d6(drawing: &str, scale: i32) -> Option<Rect> {
    if drawing.is_empty() || libass_drawing_scale_base(scale) <= 0 {
        return None;
    }
    // Parser is the validity oracle: an out-of-domain point only invalidates a drawing once a visible segment consumes it.
    match parse_drawing_polygons_checked_with_mode(drawing, DrawingCoordinateMode::FixedD6) {
        DrawingParseOutcome::InvalidOutline => return None,
        DrawingParseOutcome::Parsed(_) => {}
    }

    let mode = DrawingCoordinateMode::FixedD6;
    let mut cursor = DrawingCursor::new(drawing);
    let mut bounds: Option<Rect> = None;
    let mut points = 0_usize;
    let mut root_seen = false;
    let mut move_seen = false;
    let mut spline_start: Option<[Point; 3]> = None;

    let add = |bounds: &mut Option<Rect>, point: Point| {
        if !mode.point_is_valid(point) {
            return;
        }
        if let Some(bounds) = bounds {
            bounds.x_min = bounds.x_min.min(point.x);
            bounds.y_min = bounds.y_min.min(point.y);
            bounds.x_max = bounds.x_max.max(point.x);
            bounds.y_max = bounds.y_max.max(point.y);
        } else {
            *bounds = Some(Rect {
                x_min: point.x,
                y_min: point.y,
                x_max: point.x,
                y_max: point.y,
            });
        }
    };

    while let Some(command) = cursor.next_char() {
        match command {
            'm' => {
                move_seen = true;
                if !root_seen {
                    let Some(point) = cursor.parse_point(mode) else {
                        continue;
                    };
                    root_seen = true;
                    points = 1;
                    add(&mut bounds, point);
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    add(&mut bounds, batch[0]);
                    points += 1;
                });
            }
            'n' => {
                if !root_seen {
                    let Some(point) = cursor.parse_point(mode) else {
                        continue;
                    };
                    if !move_seen {
                        return None;
                    }
                    root_seen = true;
                    points = 1;
                    add(&mut bounds, point);
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    add(&mut bounds, batch[0]);
                    points += 1;
                });
            }
            'l' => {
                if !root_seen {
                    continue;
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    add(&mut bounds, batch[0]);
                    points += 1;
                });
            }
            'b' => {
                if !root_seen {
                    continue;
                }
                cursor.parse_many_points(mode, 3, |batch| {
                    for &point in batch {
                        add(&mut bounds, point);
                    }
                    points += 3;
                });
            }
            's' => {
                if !root_seen {
                    continue;
                }
                let Some(batch) = cursor.parse_exact_points::<3>(mode) else {
                    spline_start = None;
                    continue;
                };
                for point in batch {
                    add(&mut bounds, point);
                }
                spline_start = Some(batch);
                points += 3;
                cursor.parse_many_points(mode, 1, |batch| {
                    add(&mut bounds, batch[0]);
                    points += 1;
                });
            }
            'p' => {
                if points < 3 {
                    continue;
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    add(&mut bounds, batch[0]);
                    points += 1;
                });
            }
            'c' => {
                // Closing a B-spline reuses its first three tokens, already in the union box.
                spline_start = None;
            }
            _ => {}
        }
    }

    let _ = spline_start;
    bounds
}

/// Constructed-outline cbox in 26.6 (B-spline converted; move-only points ignored).
pub fn parse_drawing_outline_cbox_d6(drawing: &str, scale: i32) -> Option<Rect> {
    if drawing.is_empty() || libass_drawing_scale_base(scale) <= 0 {
        return None;
    }
    match parse_drawing_polygons_checked_with_mode(drawing, DrawingCoordinateMode::FixedD6) {
        DrawingParseOutcome::InvalidOutline => return None,
        DrawingParseOutcome::Parsed(_) => {}
    }

    let mode = DrawingCoordinateMode::FixedD6;
    let mut cursor = DrawingCursor::new(drawing);
    let mut bounds: Option<Rect> = None;
    let mut history = Vec::new();
    let mut spline_close_points: Option<[Point; 3]> = None;
    let mut points = 0_usize;
    let mut root_seen = false;
    let mut move_seen = false;
    let mut started = false;
    let mut pen = Point { x: 0, y: 0 };

    let add = |bounds: &mut Option<Rect>, point: Point| {
        if let Some(bounds) = bounds {
            bounds.x_min = bounds.x_min.min(point.x);
            bounds.y_min = bounds.y_min.min(point.y);
            bounds.x_max = bounds.x_max.max(point.x);
            bounds.y_max = bounds.y_max.max(point.y);
        } else {
            *bounds = Some(Rect {
                x_min: point.x,
                y_min: point.y,
                x_max: point.x,
                y_max: point.y,
            });
        }
    };
    let add_line = |bounds: &mut Option<Rect>, started: &mut bool, from: Point, to: Point| {
        if !*started {
            add(bounds, from);
        }
        add(bounds, to);
        *started = true;
    };
    let spline_controls = |p: [Point; 4]| -> Option<[Point; 4]> {
        let x01 = (i64::from(p[1].x) - i64::from(p[0].x)) / 3;
        let y01 = (i64::from(p[1].y) - i64::from(p[0].y)) / 3;
        let x12 = (i64::from(p[2].x) - i64::from(p[1].x)) / 3;
        let y12 = (i64::from(p[2].y) - i64::from(p[1].y)) / 3;
        let x23 = (i64::from(p[3].x) - i64::from(p[2].x)) / 3;
        let y23 = (i64::from(p[3].y) - i64::from(p[2].y)) / 3;
        let point = |x: i64, y: i64| {
            let point = Point {
                x: i32::try_from(x).ok()?,
                y: i32::try_from(y).ok()?,
            };
            mode.point_is_valid(point).then_some(point)
        };
        Some([
            point(
                i64::from(p[1].x) + ((x12 - x01) >> 1),
                i64::from(p[1].y) + ((y12 - y01) >> 1),
            )?,
            point(i64::from(p[1].x) + x12, i64::from(p[1].y) + y12)?,
            point(i64::from(p[2].x) - x12, i64::from(p[2].y) - y12)?,
            point(
                i64::from(p[2].x) + ((x23 - x12) >> 1),
                i64::from(p[2].y) + ((y23 - y12) >> 1),
            )?,
        ])
    };
    let add_curve = |bounds: &mut Option<Rect>, started: &mut bool, controls: [Point; 4]| {
        if !*started {
            add(bounds, controls[0]);
        }
        for &point in &controls[1..] {
            add(bounds, point);
        }
        *started = true;
    };

    while let Some(command) = cursor.next_char() {
        match command {
            'm' => {
                move_seen = true;
                if !root_seen {
                    let Some(point) = cursor.parse_point(mode) else {
                        continue;
                    };
                    root_seen = true;
                    points = 1;
                    pen = point;
                    history.push(point);
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    started = false;
                    pen = batch[0];
                    history.push(pen);
                    points += 1;
                });
            }
            'n' => {
                if !root_seen {
                    let Some(point) = cursor.parse_point(mode) else {
                        continue;
                    };
                    if !move_seen {
                        return None;
                    }
                    root_seen = true;
                    points = 1;
                    pen = point;
                    history.push(point);
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    pen = batch[0];
                    history.push(pen);
                    points += 1;
                });
            }
            'l' => {
                if !root_seen {
                    continue;
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    add_line(&mut bounds, &mut started, pen, batch[0]);
                    pen = batch[0];
                    history.push(pen);
                    points += 1;
                });
            }
            'b' => {
                if !root_seen {
                    continue;
                }
                cursor.parse_many_points(mode, 3, |batch| {
                    let controls = [pen, batch[0], batch[1], batch[2]];
                    add_curve(&mut bounds, &mut started, controls);
                    pen = batch[2];
                    history.extend_from_slice(batch);
                    points += 3;
                });
            }
            's' => {
                if !root_seen {
                    continue;
                }
                let spline_start = pen;
                let Some(batch) = cursor.parse_exact_points::<3>(mode) else {
                    spline_close_points = None;
                    continue;
                };
                let controls = spline_controls([spline_start, batch[0], batch[1], batch[2]])?;
                add_curve(&mut bounds, &mut started, controls);
                spline_close_points = Some([spline_start, batch[0], batch[1]]);
                pen = batch[2];
                history.extend_from_slice(&batch);
                points += 3;
                cursor.parse_many_points(mode, 1, |batch| {
                    let len = history.len();
                    if len >= 3
                        && let Some(controls) = spline_controls([
                            history[len - 3],
                            history[len - 2],
                            history[len - 1],
                            batch[0],
                        ])
                    {
                        add_curve(&mut bounds, &mut started, controls);
                    }
                    pen = batch[0];
                    history.push(pen);
                    points += 1;
                });
            }
            'p' => {
                if points < 3 {
                    continue;
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    let len = history.len();
                    if len >= 3
                        && let Some(controls) = spline_controls([
                            history[len - 3],
                            history[len - 2],
                            history[len - 1],
                            batch[0],
                        ])
                    {
                        add_curve(&mut bounds, &mut started, controls);
                    }
                    pen = batch[0];
                    history.push(pen);
                    points += 1;
                });
            }
            'c' => {
                if let Some(close_points) = spline_close_points.take() {
                    for point in close_points {
                        let len = history.len();
                        if len >= 3
                            && let Some(controls) = spline_controls([
                                history[len - 3],
                                history[len - 2],
                                history[len - 1],
                                point,
                            ])
                        {
                            add_curve(&mut bounds, &mut started, controls);
                        }
                        pen = point;
                        history.push(point);
                    }
                }
            }
            _ => {}
        }
    }
    bounds
}

fn parse_drawing_polygons_checked(drawing: &str, scale: i32) -> DrawingParseOutcome {
    if drawing.is_empty() {
        return DrawingParseOutcome::Parsed(None);
    }
    if libass_drawing_scale_base(scale) <= 0 {
        return DrawingParseOutcome::Parsed(Some(Vec::new()));
    }

    parse_drawing_polygons_checked_with_mode(drawing, DrawingCoordinateMode::ScaledInteger(scale))
}

#[derive(Clone, Copy)]
enum DrawingCoordinateMode {
    ScaledInteger(i32),
    FixedD6,
}

impl DrawingCoordinateMode {
    fn point(self, x: f64, y: f64) -> Point {
        match self {
            Self::ScaledInteger(scale) => scale_drawing_point(x, y, scale),
            Self::FixedD6 => match (
                libass_drawing_coordinate_to_d6(x),
                libass_drawing_coordinate_to_d6(y),
            ) {
                (Some(x), Some(y)) => Point { x, y },
                _ => Point {
                    x: i32::MIN,
                    y: i32::MIN,
                },
            },
        }
    }

    fn point_is_valid(self, point: Point) -> bool {
        match self {
            Self::ScaledInteger(_) => libass_outline_point_is_valid(point),
            Self::FixedD6 => {
                (-LIBASS_OUTLINE_MAX_D6..=LIBASS_OUTLINE_MAX_D6).contains(&point.x)
                    && (-LIBASS_OUTLINE_MAX_D6..=LIBASS_OUTLINE_MAX_D6).contains(&point.y)
            }
        }
    }

    fn coordinate_from_f64(self, value: f64) -> Option<i32> {
        match self {
            Self::ScaledInteger(_) => libass_outline_coordinate_from_f64(value),
            Self::FixedD6 => {
                if !value.is_finite() {
                    return None;
                }
                let rounded = value.round();
                if rounded < -f64::from(LIBASS_OUTLINE_MAX_D6)
                    || rounded > f64::from(LIBASS_OUTLINE_MAX_D6)
                {
                    return None;
                }
                Some(rounded as i32)
            }
        }
    }
}

fn parse_drawing_polygons_checked_with_mode(
    drawing: &str,
    mode: DrawingCoordinateMode,
) -> DrawingParseOutcome {
    let mut cursor = DrawingCursor::new(drawing);
    let mut polygons = Vec::new();
    let mut current = Vec::new();
    let mut history = Vec::new();
    let mut spline_close_points: Option<[Point; 3]> = None;
    let mut points = 0usize;
    let mut root_seen = false;
    let mut move_seen = false;
    let mut started = false;
    let mut valid_outline = true;
    let mut pen = Point { x: 0, y: 0 };

    while let Some(command) = cursor.next_char() {
        match command {
            'm' => {
                move_seen = true;
                if !root_seen {
                    let Some(point) = cursor.parse_point(mode) else {
                        continue;
                    };
                    root_seen = true;
                    points = 1;
                    pen = point;
                    history.push(point);
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    let point = batch[0];
                    close_current_contour(&mut polygons, &mut current, &mut started);
                    pen = point;
                    history.push(point);
                    points += 1;
                });
            }
            'n' => {
                if !root_seen {
                    let Some(point) = cursor.parse_point(mode) else {
                        continue;
                    };
                    if !move_seen {
                        return DrawingParseOutcome::Parsed(None);
                    }
                    root_seen = true;
                    points = 1;
                    pen = point;
                    history.push(point);
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    pen = batch[0];
                    history.push(pen);
                    points += 1;
                });
            }
            'l' => {
                if !root_seen {
                    continue;
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    let point = batch[0];
                    valid_outline &= add_line_segment(&mut current, &mut started, pen, point, mode);
                    pen = point;
                    history.push(point);
                    points += 1;
                });
            }
            'b' => {
                if !root_seen {
                    continue;
                }
                cursor.parse_many_points(mode, 3, |batch| {
                    let start = *history.last().expect("drawing root exists before bezier");
                    valid_outline &= add_cubic_segment(
                        &mut current,
                        &mut started,
                        start,
                        batch[0],
                        batch[1],
                        batch[2],
                        mode,
                    );
                    pen = batch[2];
                    history.extend_from_slice(batch);
                    points += 3;
                });
            }
            's' => {
                if !root_seen {
                    continue;
                }
                let spline_start = *history.last().expect("drawing root exists before spline");
                let Some(batch) = cursor.parse_exact_points::<3>(mode) else {
                    spline_close_points = None;
                    continue;
                };
                valid_outline &= add_spline_segment(
                    &mut current,
                    &mut started,
                    spline_start,
                    batch[0],
                    batch[1],
                    batch[2],
                    mode,
                );
                spline_close_points = Some([spline_start, batch[0], batch[1]]);
                pen = batch[2];
                history.extend_from_slice(&batch);
                points += 3;
                cursor.parse_many_points(mode, 1, |batch| {
                    valid_outline &=
                        add_extend_spline(&mut current, &mut started, &history, batch[0], mode);
                    pen = batch[0];
                    history.push(batch[0]);
                    points += 1;
                });
            }
            'p' => {
                if points < 3 {
                    continue;
                }
                cursor.parse_many_points(mode, 1, |batch| {
                    valid_outline &=
                        add_extend_spline(&mut current, &mut started, &history, batch[0], mode);
                    pen = batch[0];
                    history.push(batch[0]);
                    points += 1;
                });
            }
            'c' => {
                if let Some(close_points) = spline_close_points.take() {
                    for point in close_points {
                        valid_outline &=
                            add_extend_spline(&mut current, &mut started, &history, point, mode);
                        pen = point;
                        history.push(point);
                    }
                }
            }
            _ => {}
        }
    }

    close_current_contour(&mut polygons, &mut current, &mut started);

    if !valid_outline {
        DrawingParseOutcome::InvalidOutline
    } else {
        DrawingParseOutcome::Parsed(Some(polygons))
    }
}

struct DrawingCursor<'a> {
    text: &'a str,
    index: usize,
}

impl<'a> DrawingCursor<'a> {
    fn new(text: &'a str) -> Self {
        Self { text, index: 0 }
    }

    fn next_char(&mut self) -> Option<char> {
        let character = self.text.get(self.index..)?.chars().next()?;
        self.index += character.len_utf8();
        Some(character)
    }

    fn parse_point(&mut self, mode: DrawingCoordinateMode) -> Option<Point> {
        let x = self.parse_number()?;
        let y = self.parse_number()?;
        Some(mode.point(x, y))
    }

    fn parse_exact_points<const N: usize>(
        &mut self,
        mode: DrawingCoordinateMode,
    ) -> Option<[Point; N]> {
        let mut points = Vec::with_capacity(N);
        for _ in 0..N {
            let point = self.parse_point(mode)?;
            points.push(point);
        }
        points.try_into().ok()
    }

    fn parse_many_points(
        &mut self,
        mode: DrawingCoordinateMode,
        batch_size: usize,
        mut append_batch: impl FnMut(&[Point]),
    ) {
        debug_assert!(batch_size > 0);
        let mut batch = Vec::with_capacity(batch_size);
        while let Some(point) = self.parse_point(mode) {
            batch.push(point);
            if batch.len() == batch_size {
                append_batch(&batch);
                batch.clear();
            }
        }
    }

    fn parse_number(&mut self) -> Option<f64> {
        let (number, consumed) = parse_drawing_number_prefix(self.text.get(self.index..)?)?;
        self.index += consumed;
        Some(number)
    }
}

fn close_current_contour(
    polygons: &mut Vec<Vec<Point>>,
    current: &mut Vec<Point>,
    started: &mut bool,
) {
    if current.len() >= 3 {
        polygons.push(std::mem::take(current));
    } else {
        current.clear();
    }
    *started = false;
}

fn parse_drawing_number(token: &str) -> Option<f64> {
    parse_drawing_number_prefix(token).map(|(number, _)| number)
}

fn parse_drawing_number_prefix(token: &str) -> Option<(f64, usize)> {
    let start = token
        .char_indices()
        .find_map(|(index, character)| (!is_ass_c_space(character)).then_some(index))?;

    let mut index = start;
    if let Some(character) = token.get(index..)?.chars().next()
        && (character == '+' || character == '-')
    {
        index += character.len_utf8();
    }

    let mut seen_digit = false;
    let mut seen_dot = false;
    while let Some(character) = token.get(index..)?.chars().next() {
        if character.is_ascii_digit() {
            seen_digit = true;
            index += character.len_utf8();
        } else if character == '.' && !seen_dot {
            seen_dot = true;
            index += character.len_utf8();
        } else {
            break;
        }
    }

    if !seen_digit {
        return None;
    }

    let mantissa_end = index;
    let mut parse_end = mantissa_end;
    if let Some(character) = token.get(index..)?.chars().next()
        && (character == 'e' || character == 'E')
    {
        index += character.len_utf8();
        if let Some(character) = token.get(index..)?.chars().next()
            && (character == '+' || character == '-')
        {
            index += character.len_utf8();
        }
        let exponent_start = index;
        while let Some(character) = token.get(index..)?.chars().next() {
            if !character.is_ascii_digit() {
                break;
            }
            index += character.len_utf8();
        }
        if index > exponent_start {
            parse_end = index;
        }
    }

    let parsed = parse_ass_number_prefix(&token[start..parse_end])?;
    Some((parsed, index))
}

fn parse_ass_number_prefix(number: &str) -> Option<f64> {
    number.parse::<f64>().ok().or_else(|| {
        let sign_len = number
            .starts_with('+')
            .then_some(1)
            .or_else(|| number.starts_with('-').then_some(1))
            .unwrap_or(0);
        number[sign_len..].starts_with('.').then(|| {
            let mut normalized = String::with_capacity(number.len() + 1);
            normalized.push_str(&number[..sign_len]);
            normalized.push('0');
            normalized.push_str(&number[sign_len..]);
            normalized.parse::<f64>().ok()
        })?
    })
}

fn is_ass_c_space(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}'
    )
}

fn add_line_segment(
    current: &mut Vec<Point>,
    started: &mut bool,
    from: Point,
    to: Point,
    mode: DrawingCoordinateMode,
) -> bool {
    if !mode.point_is_valid(from) || !mode.point_is_valid(to) {
        return false;
    }
    if !*started {
        current.push(from);
    }
    current.push(to);
    *started = true;
    true
}

fn add_cubic_segment(
    current: &mut Vec<Point>,
    started: &mut bool,
    start: Point,
    control1: Point,
    control2: Point,
    end: Point,
    mode: DrawingCoordinateMode,
) -> bool {
    if [start, control1, control2, end]
        .into_iter()
        .any(|point| !mode.point_is_valid(point))
    {
        return false;
    }
    if !*started {
        current.push(start);
    }
    let Some(points) = approximate_cubic_bezier(start, control1, control2, end, 16, mode) else {
        return false;
    };
    current.extend(points);
    *started = true;
    true
}

fn add_spline_segment(
    current: &mut Vec<Point>,
    started: &mut bool,
    previous: Point,
    point1: Point,
    point2: Point,
    point3: Point,
    mode: DrawingCoordinateMode,
) -> bool {
    if [previous, point1, point2, point3]
        .into_iter()
        .any(|point| !mode.point_is_valid(point))
    {
        return false;
    }
    if !*started {
        current.push(previous);
    }
    let Some(points) = approximate_spline_segment(previous, point1, point2, point3, 16, mode)
    else {
        return false;
    };
    current.extend(points);
    *started = true;
    true
}

fn add_extend_spline(
    current: &mut Vec<Point>,
    started: &mut bool,
    history: &[Point],
    point: Point,
    mode: DrawingCoordinateMode,
) -> bool {
    let len = history.len();
    if len < 3 {
        return true;
    }
    add_spline_segment(
        current,
        started,
        history[len - 3],
        history[len - 2],
        history[len - 1],
        point,
        mode,
    )
}

fn approximate_cubic_bezier(
    start: Point,
    control1: Point,
    control2: Point,
    end: Point,
    segments: usize,
    mode: DrawingCoordinateMode,
) -> Option<Vec<Point>> {
    let segments = segments.max(1);
    let mut points = Vec::with_capacity(segments);
    for step in 1..=segments {
        let t = step as f64 / segments as f64;
        let one_minus_t = 1.0 - t;
        let x = one_minus_t.powi(3) * f64::from(start.x)
            + 3.0 * one_minus_t.powi(2) * t * f64::from(control1.x)
            + 3.0 * one_minus_t * t.powi(2) * f64::from(control2.x)
            + t.powi(3) * f64::from(end.x);
        let y = one_minus_t.powi(3) * f64::from(start.y)
            + 3.0 * one_minus_t.powi(2) * t * f64::from(control1.y)
            + 3.0 * one_minus_t * t.powi(2) * f64::from(control2.y)
            + t.powi(3) * f64::from(end.y);
        let point = Point {
            x: mode.coordinate_from_f64(x)?,
            y: mode.coordinate_from_f64(y)?,
        };
        if points.last().copied() != Some(point) {
            points.push(point);
        }
    }
    Some(points)
}

fn approximate_spline_segment(
    previous: Point,
    point1: Point,
    point2: Point,
    point3: Point,
    segments: usize,
    mode: DrawingCoordinateMode,
) -> Option<Vec<Point>> {
    let x01 = (i64::from(point1.x) - i64::from(previous.x)) / 3;
    let y01 = (i64::from(point1.y) - i64::from(previous.y)) / 3;
    let x12 = (i64::from(point2.x) - i64::from(point1.x)) / 3;
    let y12 = (i64::from(point2.y) - i64::from(point1.y)) / 3;
    let x23 = (i64::from(point3.x) - i64::from(point2.x)) / 3;
    let y23 = (i64::from(point3.y) - i64::from(point2.y)) / 3;

    let point_from_i64 = |x: i64, y: i64| {
        let point = Point {
            x: i32::try_from(x).ok()?,
            y: i32::try_from(y).ok()?,
        };
        mode.point_is_valid(point).then_some(point)
    };
    let start = point_from_i64(
        i64::from(point1.x) + ((x12 - x01) >> 1),
        i64::from(point1.y) + ((y12 - y01) >> 1),
    )?;
    let control1 = point_from_i64(i64::from(point1.x) + x12, i64::from(point1.y) + y12)?;
    let control2 = point_from_i64(i64::from(point2.x) - x12, i64::from(point2.y) - y12)?;
    let end = point_from_i64(
        i64::from(point2.x) + ((x23 - x12) >> 1),
        i64::from(point2.y) + ((y23 - y12) >> 1),
    )?;

    approximate_cubic_bezier(start, control1, control2, end, segments, mode)
}

fn scale_drawing_point(x: f64, y: f64, scale: i32) -> Point {
    // Out-of-range tokens become INT32_MIN; unused moves stay valid, but a line/curve using them invalidates the drawing.
    if libass_drawing_coordinate_to_d6(x).is_none() || libass_drawing_coordinate_to_d6(y).is_none()
    {
        return Point {
            x: i32::MIN,
            y: i32::MIN,
        };
    }
    let factor = libass_drawing_scale_base(scale).max(1);
    let Some(x) = libass_outline_coordinate_from_f64(x.round() / f64::from(factor)) else {
        return Point {
            x: i32::MIN,
            y: i32::MIN,
        };
    };
    let Some(y) = libass_outline_coordinate_from_f64(y.round() / f64::from(factor)) else {
        return Point {
            x: i32::MIN,
            y: i32::MIN,
        };
    };
    Point { x, y }
}

pub fn libass_drawing_scale_base(scale: i32) -> i32 {
    let shift = scale.wrapping_sub(1) & 31;
    (1_u32 << shift as u32) as i32
}

fn bounds_from_polygons(polygons: &[Vec<Point>]) -> Option<Rect> {
    let mut points = polygons.iter().flat_map(|polygon| polygon.iter().copied());
    let first = points.next()?;
    if !libass_outline_point_is_valid(first) {
        return None;
    }
    let mut x_min = first.x;
    let mut y_min = first.y;
    let mut x_max = first.x;
    let mut y_max = first.y;
    for point in points {
        if !libass_outline_point_is_valid(point) {
            return None;
        }
        x_min = x_min.min(point.x);
        y_min = y_min.min(point.y);
        x_max = x_max.max(point.x);
        y_max = y_max.max(point.y);
    }
    Some(Rect {
        x_min,
        y_min,
        x_max: x_max.checked_add(1)?,
        y_max: y_max.checked_add(1)?,
    })
}

fn parse_move(value: &str) -> Option<ParsedMovement> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    let (x1, y1, x2, y2, t1_ms, t2_ms) = match parts.as_slice() {
        [x1, y1, x2, y2] => (
            x1.parse::<i32>().ok()?,
            y1.parse::<i32>().ok()?,
            x2.parse::<i32>().ok()?,
            y2.parse::<i32>().ok()?,
            0,
            0,
        ),
        [x1, y1, x2, y2, t1, t2] => {
            let mut t1_ms = parse_complex_i32_arg(t1);
            let mut t2_ms = parse_complex_i32_arg(t2);
            if t1_ms > t2_ms {
                std::mem::swap(&mut t1_ms, &mut t2_ms);
            }
            (
                x1.parse::<i32>().ok()?,
                y1.parse::<i32>().ok()?,
                x2.parse::<i32>().ok()?,
                y2.parse::<i32>().ok()?,
                t1_ms,
                t2_ms,
            )
        }
        _ => return None,
    };

    Some(ParsedMovement {
        start: (x1, y1),
        end: (x2, y2),
        t1_ms,
        t2_ms,
    })
}

fn parse_move_exact(value: &str) -> Option<ParsedMovementExact> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    let (x1, y1, x2, y2, t1_ms, t2_ms) = match parts.as_slice() {
        [x1, y1, x2, y2] => (
            parse_complex_f64_arg(x1),
            parse_complex_f64_arg(y1),
            parse_complex_f64_arg(x2),
            parse_complex_f64_arg(y2),
            0,
            0,
        ),
        [x1, y1, x2, y2, t1, t2] => {
            let mut t1_ms = parse_complex_i32_arg(t1);
            let mut t2_ms = parse_complex_i32_arg(t2);
            if t1_ms > t2_ms {
                std::mem::swap(&mut t1_ms, &mut t2_ms);
            }
            (
                parse_complex_f64_arg(x1),
                parse_complex_f64_arg(y1),
                parse_complex_f64_arg(x2),
                parse_complex_f64_arg(y2),
                t1_ms,
                t2_ms,
            )
        }
        _ => return None,
    };

    Some(ParsedMovementExact {
        start: (x1, y1),
        end: (x2, y2),
        t1_ms,
        t2_ms,
    })
}

fn parse_fad(value: &str) -> Option<ParsedFade> {
    parse_fade_tag(value)
}

fn parse_fade(value: &str) -> Option<ParsedFade> {
    parse_fade_tag(value)
}

fn parse_fade_tag(value: &str) -> Option<ParsedFade> {
    let inside = parenthesized_args(value)?;
    let parts = split_complex_args(inside);
    match parts.as_slice() {
        [fade_in, fade_out] => Some(ParsedFade::Simple {
            fade_in_ms: parse_complex_i32_arg(fade_in),
            fade_out_ms: parse_complex_i32_arg(fade_out),
        }),
        [a1, a2, a3, t1, t2, t3, t4] => Some(ParsedFade::Complex {
            alpha1: parse_complex_i32_arg(a1),
            alpha2: parse_complex_i32_arg(a2),
            alpha3: parse_complex_i32_arg(a3),
            t1_ms: parse_complex_i32_arg(t1),
            t2_ms: parse_complex_i32_arg(t2),
            t3_ms: parse_complex_i32_arg(t3),
            t4_ms: parse_complex_i32_arg(t4),
        }),
        _ => None,
    }
}

fn resolve_reset_style(
    value: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
) -> ParsedSpanStyle {
    let Some(name) = first_reset_style_arg(value) else {
        return ParsedSpanStyle::from_style(base_style);
    };

    styles
        .iter()
        .rev()
        .find(|style| style.name == name)
        .map(ParsedSpanStyle::from_style)
        .unwrap_or_else(|| ParsedSpanStyle::from_style(base_style))
}

fn resolve_reset_alignment(value: &str, base_style: &ParsedStyle, styles: &[ParsedStyle]) -> i32 {
    let Some(name) = first_reset_style_arg(value) else {
        return base_style.alignment;
    };

    styles
        .iter()
        .rev()
        .find(|style| style.name == name)
        .map(|style| style.alignment)
        .unwrap_or(base_style.alignment)
}

fn flush_span(
    buffer: &mut String,
    style: &ParsedSpanStyle,
    karaoke: Option<ParsedKaraokeSpan>,
    drawing_scale: i32,
    transforms: &[ParsedSpanTransform],
    line: &mut ParsedTextLine,
) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    let drawing = (drawing_scale > 0)
        .then(|| parse_drawing_polygons(&text, drawing_scale))
        .flatten()
        .map(|polygons| ParsedDrawing {
            scale: drawing_scale,
            polygons,
        });
    line.text.push_str(&text);
    line.spans.push(ParsedTextSpan {
        text,
        style: style.clone(),
        transforms: transforms.to_vec(),
        karaoke,
        drawing,
    });
}

fn flush_span_for_run_break(
    buffer: &mut String,
    style: &ParsedSpanStyle,
    pending_karaoke: &mut Option<ParsedKaraokeSpan>,
    deferred_karaoke: &mut Option<ParsedKaraokeSpan>,
    drawing_scale: i32,
    transforms: &[ParsedSpanTransform],
    line: &mut ParsedTextLine,
) {
    let had_text = !buffer.is_empty();
    flush_span(
        buffer,
        style,
        *pending_karaoke,
        drawing_scale,
        transforms,
        line,
    );
    if had_text {
        // Every real style/drawing run is a karaoke-word boundary; missing tags inherit mode with zero duration.
        *pending_karaoke = deferred_karaoke.take().or_else(|| {
            pending_karaoke.map(|karaoke| ParsedKaraokeSpan {
                start_ms: karaoke.start_ms.wrapping_add(karaoke.duration_ms),
                duration_ms: 0,
                mode: karaoke.mode,
            })
        });
    }
}

fn flush_span_before_karaoke_tag(
    buffer: &mut String,
    style: &ParsedSpanStyle,
    pending_karaoke: &mut Option<ParsedKaraokeSpan>,
    deferred_karaoke: &mut Option<ParsedKaraokeSpan>,
    drawing_scale: i32,
    transforms: &[ParsedSpanTransform],
    line: &mut ParsedTextLine,
) {
    flush_span(
        buffer,
        style,
        *pending_karaoke,
        drawing_scale,
        transforms,
        line,
    );
    *deferred_karaoke = None;
}

fn push_line(
    parsed: &mut ParsedDialogueText,
    line: &mut ParsedTextLine,
    style: &ParsedSpanStyle,
    transforms: &[ParsedSpanTransform],
) {
    if line.spans.is_empty() {
        // Explicit newlines stay as metric-only glyph records; empty lines still contribute half ascent/descent.
        line.spans.push(ParsedTextSpan {
            text: String::new(),
            style: style.clone(),
            transforms: transforms.to_vec(),
            karaoke: None,
            drawing: None,
        });
    }
    parsed.lines.push(std::mem::take(line));
}

fn parse_matrix(value: &str) -> YCbCrMatrix {
    let value = trim_ass_trailing_spaces(trim_ass_leading_spaces(value));
    match value.to_ascii_lowercase().as_str() {
        "none" => YCbCrMatrix::None,
        "tv.601" => YCbCrMatrix::Bt601Tv,
        "pc.601" => YCbCrMatrix::Bt601Pc,
        "tv.709" => YCbCrMatrix::Bt709Tv,
        "pc.709" => YCbCrMatrix::Bt709Pc,
        "tv.240m" => YCbCrMatrix::Smpte240mTv,
        "pc.240m" => YCbCrMatrix::Smpte240mPc,
        "tv.fcc" => YCbCrMatrix::FccTv,
        "pc.fcc" => YCbCrMatrix::FccPc,
        "" => YCbCrMatrix::Default,
        _ => YCbCrMatrix::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLE_FORMAT: &str = "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding";
    const EVENT_FORMAT: &str =
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";

    #[test]
    fn invalid_present_script_info_numbers_parse_as_zero_like_libass() {
        let input = format!(
            "[Script Info]\nPlayResX: bad\nPlayResY: \nTimer: nope\nWrapStyle: bad\nScaledBorderAndShadow: \nKerning: bad\nLayoutResX: bad\nLayoutResY: &HFFFFFFFF\n\n[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.play_res_x, 384);
        assert_eq!(track.play_res_y, 288);
        assert_eq!(track.timer, 0.0);
        assert_eq!(track.wrap_style, 0);
        assert!(!track.scaled_border_and_shadow);
        assert!(!track.kerning);
        assert_eq!(track.layout_res_x, 0);
        assert_eq!(track.layout_res_y, -1);
    }

    #[test]
    fn script_info_keys_are_case_and_space_sensitive_like_libass() {
        let input = format!(
            "[Script Info]\nplayresx: 1280\nPlayResY : 720\nPlayResY: 480\n\n[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.play_res_x, 640);
        assert_eq!(track.play_res_y, 480);
    }

    #[test]
    fn play_res_defaults_follow_libass_lazy_init() {
        let both_missing = parse_script_text("[Script Info]\n").expect("script should parse");
        let x_1280 =
            parse_script_text("[Script Info]\nPlayResX: 1280").expect("script should parse");
        let x_only =
            parse_script_text("[Script Info]\nPlayResX: 640").expect("script should parse");
        let y_1024 =
            parse_script_text("[Script Info]\nPlayResY: 1024").expect("script should parse");
        let y_only =
            parse_script_text("[Script Info]\nPlayResY: 480").expect("script should parse");

        assert_eq!(
            (both_missing.play_res_x, both_missing.play_res_y),
            (384, 288)
        );
        assert_eq!((x_1280.play_res_x, x_1280.play_res_y), (1280, 1024));
        assert_eq!((x_only.play_res_x, x_only.play_res_y), (640, 480));
        assert_eq!((y_1024.play_res_x, y_1024.play_res_y), (1280, 1024));
        assert_eq!((y_only.play_res_x, y_only.play_res_y), (640, 480));
    }

    #[test]
    fn script_type_header_updates_track_type_like_libass() {
        let input = "[Script Info]\nScriptType: v4.00+";
        let ass = parse_script_text(input).expect("script should parse");
        let input = "[Script Info]\nScriptType: anything4.00";
        let ssa = parse_script_text(input).expect("script should parse");

        assert_eq!(ass.track_type, TrackType::Ass);
        assert_eq!(ssa.track_type, TrackType::Ssa);
    }

    #[test]
    fn section_headers_accept_trailing_text_like_libass() {
        let input = format!(
            "[Script Info] trailing\nPlayResX: 640\n[V4+ Styles] trailing\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n[Events] trailing\n{EVENT_FORMAT}\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Text"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.play_res_x, 640);
        assert_eq!(track.track_type, TrackType::Ass);
        assert_eq!(track.styles.len(), 2);
        assert_eq!(track.events.len(), 1);
    }

    #[test]
    fn cr_only_line_endings_split_like_libass() {
        let input = format!(
            "[Script Info]\rPlayResX: 640\r[V4+ Styles]\r{STYLE_FORMAT}\rStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\r[Events]\r{EVENT_FORMAT}\rDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Text"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.play_res_x, 640);
        assert_eq!(track.track_type, TrackType::Ass);
        assert_eq!(track.styles.len(), 2);
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].text, "Text");
    }

    #[test]
    fn style_and_event_control_lines_match_exactly_like_libass() {
        let input = format!(
            "[V4+ Styles]\nformat: Name, Fontname, Fontsize\nStyle : Spaced,Arial,30\nStyle: Exact,Arial,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\nstyle: Lower,Arial,40\n\n[Events]\n{EVENT_FORMAT}\nDialogue : 0,0:00:00.00,0:00:01.00,Exact,,0,0,0,,Spaced\nComment: 0,0:00:00.00,0:00:01.00,Exact,,0,0,0,,Comment\nDialogue: 0,0:00:00.00,0:00:01.00,Exact,,0,0,0,,Exact\ndialogue: 0,0:00:00.00,0:00:01.00,Exact,,0,0,0,,Lower"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.styles.len(), 2);
        assert_eq!(track.styles[1].name, "Exact");
        assert_eq!(track.styles[1].font_size, 24.0);
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].text, "Exact");
    }

    #[test]
    fn event_text_trims_only_libass_trailing_ascii_whitespace() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\n{EVENT_FORMAT}\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,  Text\u{00a0} \t "
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.events[0].text, "  Text\u{00a0}");
    }

    #[test]
    fn style_and_event_values_preserve_libass_token_spacing() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
Style: Sign ,Arial ,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,8,20,20,20,1\n\n\
[Events]\n{EVENT_FORMAT}\n\
Dialogue: 0,0:00:00.00,0:00:01.00, Sign , Actor  ,0,0,0, Effect  ,Exact\n\
Dialogue: 0,0:00:00.00,0:00:01.00,Sign,,0,0,0,,Fallback"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.styles[2].name, "Sign ");
        assert_eq!(track.styles[2].font_name, "Arial ");
        assert_eq!(track.events[0].style, 2);
        assert_eq!(track.events[0].name, "Actor  ");
        assert_eq!(track.events[0].effect, "Effect  ");
        assert_eq!(track.events[1].style, 1);
    }

    #[test]
    fn event_text_format_field_is_terminal_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n\
[Events]\nFormat: Layer, Text, Start, End, Style\n\
Dialogue: 5,  Text,0:00:10.00,0:00:11.00,Missing"
        );
        let track = parse_script_text(&input).expect("script should parse");
        let event = &track.events[0];

        assert_eq!(event.layer, 5);
        assert_eq!(event.start, 0);
        assert_eq!(event.duration, 0);
        assert_eq!(event.text, "  Text,0:00:10.00,0:00:11.00,Missing");
    }

    #[test]
    fn event_text_first_field_skips_dialogue_prefix_spaces_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n\
[Events]\nFormat: Text, Start, End, Style\n\
Dialogue:   Leading text,0:00:10.00,0:00:11.00,Missing"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(
            track.events[0].text,
            "Leading text,0:00:10.00,0:00:11.00,Missing"
        );
    }

    #[test]
    fn default_style_lookup_tracks_libass_default_style() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Other,Arial,30,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n\
[Events]\n{EVENT_FORMAT}\n\
Dialogue: 0,0:00:00.00,0:00:01.00,Missing,,0,0,0,,Fallback\n\
Dialogue: 0,0:00:00.00,0:00:01.00,default,,0,0,0,,Lowercase\n\
Dialogue: 0,0:00:00.00,0:00:01.00,*Default,,0,0,0,,Starred"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.default_style, 2);
        assert_eq!(track.events[0].style, 2);
        assert_eq!(track.events[1].style, 2);
        assert_eq!(track.events[2].style, 2);
    }

    #[test]
    fn empty_style_line_still_creates_style_like_libass() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize\nStyle:";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(track.styles.len(), 2);
        assert_eq!(style.name, "Default");
        assert_eq!(style.font_name, "Arial");
        assert_eq!(style.font_size, 0.0);
        assert_eq!(style.primary_colour, 0);
        assert_eq!(style.scale_x, 1.0);
        assert_eq!(style.scale_y, 1.0);
        assert_eq!(style.shadow, 0.0);
    }

    #[test]
    fn explicit_blank_format_lines_do_not_fall_back_like_libass() {
        let input = "[V4+ Styles]\n\
Format:\n\
Style: Named,Arial,30\n\n\
[Events]\n\
Format:\n\
Dialogue: 0,0:00:00.00,0:00:01.00,Named,,0,0,0,,Text";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(track.style_format, "");
        assert_eq!(track.event_format, "");
        assert_eq!(track.styles.len(), 2);
        assert_eq!(style.name, "Default");
        assert_eq!(style.font_name, "Arial");
        assert_eq!(style.font_size, 0.0);
        assert!(track.events.is_empty());
        assert!(track.scaled_border_and_shadow);
    }

    #[test]
    fn format_empty_fields_consume_values_like_libass() {
        let input = "[V4+ Styles]\n\
Format: Name, , Fontname, Fontsize\n\
Style: Default,ignored,Arial,34\n\n\
[Events]\n\
Format: Layer, , Text\n\
Dialogue: 7,ignored,Visible";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.styles[1].font_name, "Arial");
        assert_eq!(track.styles[1].font_size, 34.0);
        assert_eq!(track.events[0].layer, 7);
        assert_eq!(track.events[0].text, "Visible");
    }

    #[test]
    fn no_style_fallback_uses_libass_builtin_default() {
        let track = parse_script_text("[Script Info]\n").expect("script should parse");
        let style = &track.styles[0];

        assert_eq!(style.font_name, "Arial");
        assert_eq!(style.font_size, 18.0);
        assert_eq!(style.primary_colour, 0x00FF_FFFF);
        assert_eq!(style.secondary_colour, 0x00FF_FF00);
        assert_eq!(style.back_colour, 0x8000_0000);
        assert_eq!(style.font_weight, 200);
        assert_eq!(style.shadow, 3.0);
        assert_eq!(
            (style.margin_l, style.margin_r, style.margin_v),
            (20, 20, 20)
        );
        assert_eq!(style.encoding, 0);
    }

    #[test]
    fn parsed_style_missing_fields_start_zero_like_libass() {
        let input = "[V4+ Styles]\n\
Format: Name, Fontsize\n\
Style: Sparse,22";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(style.name, "Sparse");
        assert_eq!(style.font_name, "Arial");
        assert_eq!(style.font_size, 22.0);
        assert_eq!(style.primary_colour, 0);
        assert_eq!(style.secondary_colour, 0);
        assert_eq!(style.outline_colour, 0);
        assert_eq!(style.back_colour, 0);
        assert_eq!(style.border_style, 0);
        assert_eq!(style.outline, 0.0);
        assert_eq!(style.shadow, 0.0);
        assert_eq!(style.alignment, 0);
        assert_eq!((style.margin_l, style.margin_r, style.margin_v), (0, 0, 0));
        assert_eq!(style.encoding, 0);
    }

    #[test]
    fn format_tokens_trim_only_space_and_tab_like_libass() {
        let input = "[V4+ Styles]\n\
Format: Name,\u{00a0}Fontname, Fontsize\n\
Style: Default,Ignored,34";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.styles[1].name, "Default");
        assert_eq!(track.styles[1].font_name, "Arial");
        assert_eq!(track.styles[1].font_size, 34.0);
    }

    #[test]
    fn script_language_keeps_two_byte_prefix_like_libass() {
        let input = format!(
            "[Script Info]\nLanguage:  english\n\n[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.language, "en");
    }

    #[test]
    fn ycbcr_matrix_accepts_only_libass_header_names() {
        let input = format!(
            "[Script Info]\nYCbCr Matrix: bt601(tv)\n\n[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
        );
        let alias = parse_script_text(&input).expect("script should parse");
        let input = format!(
            "[Script Info]\nYCbCr Matrix: tv.601\n\n[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
        );
        let canonical = parse_script_text(&input).expect("script should parse");

        assert_eq!(alias.ycbcr_matrix, YCbCrMatrix::Unknown);
        assert_eq!(canonical.ycbcr_matrix, YCbCrMatrix::Bt601Tv);
    }

    #[test]
    fn script_info_string_whitespace_uses_libass_ascii_sets() {
        let input = "[Script Info]\n\
ScriptType: v4.00+\u{00a0}\n\
YCbCr Matrix: tv.601\u{00a0}\n\
Language:\u{00a0}english\n\
ScaledBorderAndShadow:\u{00a0}yes\n";
        let nbsp = parse_script_text(input).expect("script should parse");
        let input = "[Script Info]\n\
ScriptType:\tv4.00+\t\n\
YCbCr Matrix:\ttv.601\t\n\
Language:\tenglish\n\
ScaledBorderAndShadow:\tyes\n";
        let ascii = parse_script_text(input).expect("script should parse");

        assert_eq!(nbsp.track_type, TrackType::Unknown);
        assert_eq!(nbsp.ycbcr_matrix, YCbCrMatrix::Unknown);
        assert_eq!(nbsp.language, "\u{00a0}");
        assert!(!nbsp.scaled_border_and_shadow);
        assert_eq!(ascii.track_type, TrackType::Ass);
        assert_eq!(ascii.ycbcr_matrix, YCbCrMatrix::Bt601Tv);
        assert_eq!(ascii.language, "en");
        assert!(ascii.scaled_border_and_shadow);
    }

    #[test]
    fn scaled_border_default_follows_libass_custom_format_rules() {
        let standard = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n\
[Events]\nFormat: Layer, Start, End, Style, Actor, MarginL, MarginR, MarginV, Effect, Text\n\
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Text"
        );
        let custom = "[V4+ Styles]\n\
Format: Name, Fontname, Fontsize\n\
Style: Default,Arial,20";
        let explicit_no = "[Script Info]\n\
ScaledBorderAndShadow: no\n\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize\n\
Style: Default,Arial,20";

        assert!(
            !parse_script_text(&standard)
                .expect("script should parse")
                .scaled_border_and_shadow
        );
        assert!(
            parse_script_text(custom)
                .expect("script should parse")
                .scaled_border_and_shadow
        );
        assert!(
            !parse_script_text(explicit_no)
                .expect("script should parse")
                .scaled_border_and_shadow
        );
    }

    #[test]
    fn scaled_border_detects_legacy_ffmpeg_subs_like_libass() {
        let legacy = format!(
            "[Script Info]\n\
ScriptType: v4.00+\n\
PlayResX: 384\n\
PlayResY: 288\n\
; Script generated by FFmpeg/Lavc58.0\n\n\
[V4+ Styles]\n\
{STYLE_FORMAT}\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n\
[Events]\n\
{EVENT_FORMAT}"
        );
        let modified = legacy.replace("PlayResY: 288\n", "PlayResY: 288\nWrapStyle: 0\n");

        assert!(
            parse_script_text(&legacy)
                .expect("script should parse")
                .scaled_border_and_shadow
        );
        assert!(
            !parse_script_text(&modified)
                .expect("script should parse")
                .scaled_border_and_shadow
        );
    }

    #[test]
    fn numeric_fields_do_not_trim_unicode_whitespace_like_libass() {
        let input = "[Script Info]\n\
PlayResX:\u{00a0}1280\n\
Timer:\u{00a0}2.5\n\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, MarginL\n\
Style: Default,Arial,\u{00a0}42,\u{00a0}100";
        let nbsp = parse_script_text(input).expect("script should parse");
        let input = "[Script Info]\n\
PlayResX:\t1280\n\
Timer:\t2.5\n\n\
[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, MarginL\n\
Style: Default,Arial,\t42,\t100";
        let ascii = parse_script_text(input).expect("script should parse");

        assert_eq!(nbsp.play_res_x, 384);
        assert_eq!(nbsp.play_res_y, 288);
        assert_eq!(nbsp.timer, 0.0);
        assert_eq!(nbsp.styles[1].font_size, 0.0);
        assert_eq!(nbsp.styles[1].margin_l, 0);
        assert_eq!(ascii.play_res_x, 1280);
        assert_eq!(ascii.play_res_y, 1024);
        assert_eq!(ascii.timer, 2.5);
        assert_eq!(ascii.styles[1].font_size, 42.0);
        assert_eq!(ascii.styles[1].margin_l, 100);
    }

    #[test]
    fn invalid_present_event_integer_fields_parse_as_zero_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\n{EVENT_FORMAT}\nDialogue: bad,0:00:01.00,0:00:03.00,Default,,bad,&HFFFFFFFF,,fx,Text"
        );
        let track = parse_script_text(&input).expect("script should parse");
        let event = &track.events[0];

        assert_eq!(event.layer, 0);
        assert_eq!(event.margin_l, 0);
        assert_eq!(event.margin_r, -1);
        assert_eq!(event.margin_v, 0);
    }

    #[test]
    fn event_timecodes_use_libass_sscanf_fraction_rules() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\n{EVENT_FORMAT}\nDialogue: 0,0:00:01.123,0:00:03.5,Default,,,,,,Text"
        );
        let track = parse_script_text(&input).expect("script should parse");
        let event = &track.events[0];

        assert_eq!(event.start, 2230);
        assert_eq!(event.duration, 820);
    }

    #[test]
    fn malformed_event_timecodes_return_zero_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\n{EVENT_FORMAT}\nDialogue: 0,0:00:01,bad,Default,,,,,,Text"
        );
        let track = parse_script_text(&input).expect("script should parse");
        let event = &track.events[0];

        assert_eq!(event.start, 0);
        assert_eq!(event.duration, 0);
    }

    #[test]
    fn parses_p_drawing_with_numeric_token_followed_by_plain_text_suffix() {
        let input = "[Script Info]\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.50,Default,,0000,0000,0000,,{\\p1}m 0 0 l 10 0 10 10 0 10I'm";
        let track = parse_script_text(input).expect("script should parse");
        let parsed = parse_dialogue_text_with_wrap_style(
            &track.events[0].text,
            &track.styles[0],
            &track.styles,
            track.wrap_style,
        );

        assert!(
            parsed.lines[0].spans[0].drawing.is_some(),
            "libass accepts the numeric prefix of a drawing coordinate even when generator text is appended to the same token"
        );
    }

    #[test]
    fn drawing_parser_keeps_valid_prefix_after_unknown_tokens_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\p1}ignored m 0 0 l 10 0 10 10 0 10 lyric",
            &base_style,
            &[],
        );

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(
            drawing.bounds(),
            Some(Rect {
                x_min: 0,
                y_min: 0,
                x_max: 11,
                y_max: 11,
            })
        );
    }

    #[test]
    fn drawing_parser_scans_commands_inside_junk_tokens_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}xm0 0l10 0 10 10 0 10", &base_style, &[]);

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(
            drawing.bounds(),
            Some(Rect {
                x_min: 0,
                y_min: 0,
                x_max: 11,
                y_max: 11,
            })
        );
    }

    #[test]
    fn drawing_parser_keeps_commands_lowercase_only_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}M 0 0 L 10 0 10 10 0 10", &base_style, &[]);

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert!(
            drawing.polygons.is_empty(),
            "libass ignores uppercase drawing commands"
        );
    }

    #[test]
    fn drawing_parser_rejects_leading_n_command_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}n 0 0 l 10 0 10 10 0 10", &base_style, &[]);

        assert!(
            parsed.lines[0].spans[0].drawing.is_none(),
            "libass rejects drawings whose first valid command is n"
        );
    }

    #[test]
    fn drawing_parser_accepts_n_after_invalid_m_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed =
            parse_dialogue_text("{\\p1}m invalid n 0 0 l 10 0 10 10 0 10", &base_style, &[]);

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(
            drawing.bounds(),
            Some(Rect {
                x_min: 0,
                y_min: 0,
                x_max: 11,
                y_max: 11,
            })
        );
    }

    #[test]
    fn repeated_move_points_update_pen_without_drawing_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}m 0 0 20 20 l 30 20 30 30", &base_style, &[]);

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(
            drawing.bounds(),
            Some(Rect {
                x_min: 20,
                y_min: 20,
                x_max: 31,
                y_max: 31,
            })
        );
    }

    #[test]
    fn parses_basic_ass_script() {
        let input = "[Script Info]\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.50,Default,,0000,0000,0000,,Hello, world!";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.play_res_x, 1280);
        assert_eq!(track.play_res_y, 720);
        assert_eq!(track.styles.len(), 2);
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].start, 1000);
        assert_eq!(track.events[0].duration, 2500);
        assert_eq!(track.events[0].style, 1);
        assert_eq!(track.events[0].text, "Hello, world!");
        assert_eq!(
            track.styles[1].alignment,
            ass::VALIGN_SUB | ass::HALIGN_CENTER
        );
    }

    #[test]
    fn decodes_legacy_codepage_bytes_before_parsing() {
        let mut input = b"[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n".to_vec();
        input.extend_from_slice(&[
            68, 105, 97, 108, 111, 103, 117, 101, 58, 32, 48, 44, 48, 58, 48, 48, 58, 48, 48, 46,
            48, 48, 44, 48, 58, 48, 48, 58, 48, 49, 46, 48, 48, 44, 68, 101, 102, 97, 117, 108,
            116, 44, 44, 48, 44, 48, 44, 48, 44, 44, 147, 250, 150, 123, 140, 234,
        ]);

        let track = parse_script_bytes_with_codepage(&input, Some("SHIFT_JIS"))
            .expect("Shift-JIS script should parse");

        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].text, "日本語");
    }

    #[test]
    fn normalizes_style_alignment_numbers_to_libass_bits() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Mid,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,10,10,10,1";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(
            track.styles[1].alignment,
            ass::VALIGN_CENTER | ass::HALIGN_CENTER
        );
    }

    #[test]
    fn resolves_event_style_by_name() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\nStyle: Sign,DejaVu Sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,8,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Sign,,0000,0000,0000,,Visible text";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.styles.len(), 3);
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].style, 2);
    }

    #[test]
    fn resolves_styles_by_exact_last_match_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\
Style: Sign,Old,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,8,20,20,20,1\n\
Style: sign,Lower,30,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,8,20,20,20,1\n\
Style: Sign,New,40,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,8,20,20,20,1\n\n\
[Events]\n{EVENT_FORMAT}\n\
Dialogue: 0,0:00:00.00,0:00:01.00,Sign,,0000,0000,0000,,Upper\n\
Dialogue: 0,0:00:00.00,0:00:01.00,sign,,0000,0000,0000,,Lower\n\
Dialogue: 0,0:00:00.00,0:00:01.00,SIGN,,0000,0000,0000,,Fallback"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.events[0].style, 4);
        assert_eq!(track.events[1].style, 3);
        assert_eq!(track.events[2].style, 1);

        let parsed = parse_dialogue_text(
            "{\\rsign}lower{\\rSign}upper{\\rSIGN}fallback",
            &track.styles[1],
            &track.styles,
        );
        assert_eq!(parsed.lines[0].spans[0].style.font_size, 30.0);
        assert_eq!(parsed.lines[0].spans[1].style.font_size, 40.0);
        assert_eq!(parsed.lines[0].spans[2].style.font_size, 20.0);
    }

    #[test]
    fn defaults_empty_style_name_but_preserves_empty_font_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: , ,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,0,10,10,10,1"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.styles[1].name, "Default");
        assert_eq!(track.styles[1].font_name, "");
        assert_eq!(track.styles[1].alignment, 0);
    }

    #[test]
    fn clamps_style_spacing_and_booleanizes_negative_flags_like_libass() {
        let input = format!(
            "[V4+ Styles]\n{STYLE_FORMAT}\n\
Style: Default,Arial,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,-1,-1,-2,-3,100,100,-5,0,1,2,2,2,10,10,10,1"
        );
        let track = parse_script_text(&input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
        assert!(style.italic);
        assert!(style.underline);
        assert!(style.strike_out);
        assert_eq!(style.spacing, 0.0);
    }

    #[test]
    fn invalid_present_style_numbers_parse_as_zero_like_libass() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding, Blur, Justify\nStyle: Bad,Arial,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad,bad";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(style.font_size, 0.0);
        assert_eq!(style.primary_colour, 0);
        assert_eq!(style.secondary_colour, 0);
        assert_eq!(style.outline_colour, 0);
        assert_eq!(style.back_colour, 0);
        assert_eq!(style.font_weight, 400);
        assert!(!style.bold);
        assert!(!style.italic);
        assert!(!style.underline);
        assert!(!style.strike_out);
        assert_eq!(style.scale_x, 0.0);
        assert_eq!(style.scale_y, 0.0);
        assert_eq!(style.spacing, 0.0);
        assert_eq!(style.angle, 0.0);
        assert_eq!(style.border_style, 0);
        assert_eq!(style.outline, 0.0);
        assert_eq!(style.shadow, 0.0);
        assert_eq!(style.alignment, 0);
        assert_eq!(style.margin_l, 0);
        assert_eq!(style.margin_r, 0);
        assert_eq!(style.margin_v, 0);
        assert_eq!(style.encoding, 0);
        assert_eq!(style.blur, 0.0);
        assert_eq!(style.justify, 0);
    }

    #[test]
    fn style_integer_fields_parse_hex_modulo_like_libass() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Hex,Arial,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,&HFFFFFFFF,0,0,0,100,100,0,0,1,2,2,2,&HFFFFFFFF,0,0,1";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
        assert_eq!(style.margin_l, -1);
    }

    #[test]
    fn style_colours_parse_modulo_like_libass() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour\nStyle: Overflow,Arial,28,4294967295,4294967296,&H100000001,&H100000000";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];

        assert_eq!(style.primary_colour, 0xFFFF_FFFF);
        assert_eq!(style.secondary_colour, 0);
        assert_eq!(style.outline_colour, 1);
        assert_eq!(style.back_colour, 0);
    }

    #[test]
    fn v4_style_alignment_uses_ssa_legacy_quirks() {
        let input = "[V4 Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\nStyle: Four,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,1,2,2,4,10,10,10,0,1\nStyle: Eight,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,1,2,2,8,10,10,10,0,1";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.track_type, TrackType::Ssa);
        assert_eq!(track.styles[1].alignment, 11);
        assert_eq!(track.styles[2].alignment, 3);
    }

    #[test]
    fn v4_styles_section_switches_back_to_ssa_like_libass() {
        let input = "[V4+ Styles]\n\
Format: Name, Fontname, Fontsize, Alignment\n\
Style: AssStyle,Arial,20,8\n\n\
[V4 Styles]\n\
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding\n\
Style: SsaStyle,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,1,2,2,8,10,10,10,0,1\n\n\
[Events]\n\
Dialogue: Marked=0,0:00:00.00,0:00:01.00,SsaStyle,,0,0,0,,Text";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.track_type, TrackType::Ssa);
        assert!(track.style_format.contains("AlphaLevel"));
        assert!(track.event_format.starts_with("Marked, Start, End"));
        assert_eq!(track.styles[2].alignment, 3);
        assert_eq!(track.events[0].style, 2);
    }

    #[test]
    fn ssa_missing_format_uses_libass_fallbacks_and_alpha() {
        let input = "[V4 Styles]\n\
Style: Ssa,Arial,20,&H00112233,&H00445566,&H00000000,&H00778899,0,0,1,2,3,4,11,12,13,64,1\n\n\
[Events]\n\
Dialogue: 7,0:00:01.00,0:00:03.00,Ssa,Actor,21,22,23,fx,Text";
        let track = parse_script_text(input).expect("script should parse");
        let style = &track.styles[1];
        let event = &track.events[0];

        assert_eq!(track.track_type, TrackType::Ssa);
        assert!(track.style_format.starts_with("Name, Fontname, Fontsize"));
        assert!(track.style_format.contains("TertiaryColour"));
        assert!(track.event_format.starts_with("Marked, Start, End"));
        assert_eq!(style.border_style, 1);
        assert_eq!(style.outline, 2.0);
        assert_eq!(style.shadow, 3.0);
        assert_eq!(style.alignment, 11);
        assert_eq!(style.margin_l, 11);
        assert_eq!(style.margin_r, 12);
        assert_eq!(style.margin_v, 13);
        assert_eq!(style.primary_colour, 0x4011_2233);
        assert_eq!(style.secondary_colour, 0x4044_5566);
        assert_eq!(style.outline_colour, 0x4077_8899);
        assert_eq!(style.back_colour, 0x8077_8899);
        assert_eq!(event.layer, 0);
        assert_eq!(event.start, 1000);
        assert_eq!(event.duration, 2000);
        assert_eq!(event.style, 1);
        assert_eq!(event.name, "Actor");
        assert_eq!(event.margin_l, 21);
        assert_eq!(event.margin_r, 22);
        assert_eq!(event.margin_v, 23);
        assert_eq!(event.effect, "fx");
        assert_eq!(event.text, "Text");
    }

    #[test]
    fn parses_dialogue_overrides_into_spans_and_event_metadata() {
        let base_style = ParsedStyle {
            font_name: "Arial".to_string(),
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_name: "DejaVu Sans".to_string(),
            font_size: 28.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fnLiberation Sans\\fs32\\fscx150\\fscy75\\fsp3\\an7}Hello{\\rAlt} world\\N{\\pos(120,48)}again",
            &base_style,
            &[base_style.clone(), alt_style.clone()],
        );

        assert_eq!(parsed.alignment, Some(ass::VALIGN_TOP | ass::HALIGN_LEFT));
        assert_eq!(parsed.position, Some((120, 48)));
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].spans.len(), 2);
        assert_eq!(parsed.lines[0].spans[0].style.font_name, "Liberation Sans");
        assert_eq!(parsed.lines[0].spans[0].style.font_size, 32.0);
        assert_eq!(parsed.lines[0].spans[0].style.scale_x, 1.5);
        assert_eq!(parsed.lines[0].spans[0].style.scale_y, 0.75);
        assert_eq!(parsed.lines[0].spans[0].style.spacing, 3.0);
        assert_eq!(parsed.lines[0].spans[1].style.font_name, "DejaVu Sans");
        assert_eq!(parsed.lines[1].text, "again");
    }

    #[test]
    fn fn_zero_reset_keeps_libass_leading_space_quirk() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            ..ParsedStyle::default()
        };
        let reset = parse_dialogue_text("{\\fn0}Reset", &base_style, &[]);
        let spaced = parse_dialogue_text("{\\fn 0}Literal", &base_style, &[]);
        let parenthesized = parse_dialogue_text("{\\fn( 0 )}Reset", &base_style, &[]);

        assert_eq!(reset.lines[0].spans[0].style.font_name, "Base");
        assert_eq!(spaced.lines[0].spans[0].style.font_name, "0");
        assert_eq!(parenthesized.lines[0].spans[0].style.font_name, "Base");
    }

    #[test]
    fn transform_fn_zero_reset_keeps_libass_leading_space_quirk() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("{\\t(0,100,\\fn 0)}Text", &base_style, &[]);

        assert_eq!(parsed.lines[0].spans[0].style.font_name, "0");
    }

    #[test]
    fn first_alignment_override_wins_and_invalid_values_reset_to_base() {
        let base_style = ParsedStyle {
            alignment: ass::VALIGN_SUB | ass::HALIGN_RIGHT,
            ..ParsedStyle::default()
        };

        let first = parse_dialogue_text("{\\an7\\an2}first", &base_style, &[]);
        assert_eq!(first.alignment, Some(ass::VALIGN_TOP | ass::HALIGN_LEFT));

        let legacy_a4 = parse_dialogue_text("{\\a4}legacy", &base_style, &[]);
        let legacy_a8 = parse_dialogue_text("{\\a8}legacy", &base_style, &[]);
        assert_eq!(
            legacy_a4.alignment,
            Some(ass::VALIGN_TOP | ass::HALIGN_LEFT)
        );
        assert_eq!(
            legacy_a8.alignment,
            Some(ass::VALIGN_TOP | ass::HALIGN_LEFT)
        );

        let invalid = parse_dialogue_text("{\\an12\\an7}invalid", &base_style, &[]);
        assert_eq!(invalid.alignment, Some(base_style.alignment));
    }

    #[test]
    fn fe_override_updates_span_encoding() {
        let base_style = ParsedStyle {
            encoding: 1,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("{\\fe128}encoded", &base_style, &[]);

        assert_eq!(parsed.lines[0].spans[0].style.encoding, 128);
    }

    #[test]
    fn numeric_bold_preserves_weight_and_matches_libass_thresholds() {
        let style = ParsedStyle::default();
        for (tag, expected_bold, expected_weight) in [
            ("0", false, 400),
            ("1", true, 700),
            ("100", false, 100),
            ("400", false, 400),
            ("500", false, 500),
            ("700", true, 700),
            ("900", true, 900),
        ] {
            let parsed = parse_dialogue_text(&format!("{{\\b{tag}}}bold"), &style, &[]);
            let span_style = &parsed.lines[0].spans[0].style;
            assert_eq!(
                span_style.bold, expected_bold,
                "unexpected bold state for \\b{tag}"
            );
            assert_eq!(
                span_style.font_weight, expected_weight,
                "unexpected preserved font weight for \\b{tag}"
            );
        }
    }

    #[test]
    fn parse_text_preserves_unknown_literal_backslash_escapes() {
        let style = ParsedStyle::default();
        let parsed = parse_dialogue_text("animated \\t and drawing \\p", &style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(
            parsed.lines[0].spans[0].text,
            "animated \\t and drawing \\p"
        );
    }

    #[test]
    fn escaped_braces_and_tabs_parse_like_libass_text_chars() {
        let style = ParsedStyle::default();
        let parsed = parse_dialogue_text("literal \\{tag\\}\tgap", &style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(parsed.lines[0].spans[0].text, "literal {tag} gap");
    }

    #[test]
    fn unmatched_open_brace_is_literal_text_like_libass() {
        let style = ParsedStyle::default();
        let parsed = parse_dialogue_text("literal {\\b1 no close", &style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(parsed.lines[0].spans[0].text, "literal {\\b1 no close");
        assert!(!parsed.lines[0].spans[0].style.bold);
        assert!(!parsed.hard_override);

        let hard = parse_dialogue_text("literal {\\pos(10,20) no close", &style, &[]);
        assert_eq!(
            hard.lines[0].spans[0].text,
            "literal {\\pos(10,20) no close"
        );
        assert_eq!(hard.position_exact, None);
        assert!(hard.hard_override);
    }

    #[test]
    fn override_alpha_tags_update_ass_alpha_byte() {
        let style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\alpha&H40&\\1a&H00&\\3a&H20&\\4a&H80&}alpha",
            &style,
            &[],
        );
        let span_style = &parsed.lines[0].spans[0].style;

        assert_eq!((span_style.primary_colour >> 24) & 0xff, 0x00);
        assert_eq!((span_style.secondary_colour >> 24) & 0xff, 0x40);
        assert_eq!((span_style.outline_colour >> 24) & 0xff, 0x20);
        assert_eq!((span_style.back_colour >> 24) & 0xff, 0x80);
    }

    #[test]
    fn color_overrides_preserve_alpha_and_empty_args_reset_base_channels() {
        let style = ParsedStyle {
            primary_colour: 0x2212_3456,
            secondary_colour: 0x3344_5566,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("{\\1a&H80&\\1c&H112233&\\2a&H40&\\2c}alpha", &style, &[]);
        let span_style = &parsed.lines[0].spans[0].style;

        assert_eq!(span_style.primary_colour, 0x8011_2233);
        assert_eq!(span_style.secondary_colour, 0x4044_5566);
    }

    #[test]
    fn colour_tags_preserve_simple_leading_space_before_hex_prefix_like_libass() {
        let style = ParsedStyle {
            primary_colour: 0x8012_3456,
            secondary_colour: 0x4056_789a,
            ..ParsedStyle::default()
        };
        let simple = parse_dialogue_text("{\\c &H112233&\\alpha &H7F&}simple", &style, &[]);
        let parenthesized =
            parse_dialogue_text("{\\c( &H112233& )\\alpha( &H7F& )}paren", &style, &[]);

        assert_eq!(simple.lines[0].spans[0].style.primary_colour, 0x0000_0000);
        assert_eq!(simple.lines[0].spans[0].style.secondary_colour, 0x0056_789a);
        assert_eq!(
            parenthesized.lines[0].spans[0].style.primary_colour,
            0x7f11_2233
        );
        assert_eq!(
            parenthesized.lines[0].spans[0].style.secondary_colour,
            0x7f56_789a
        );
    }

    #[test]
    fn color_and_alpha_overrides_clamp_hex_like_libass() {
        let style = ParsedStyle {
            primary_colour: 0x2212_3456,
            secondary_colour: 0x3344_5566,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\1c&H100000001&\\1a&H100000000&\\2c&H-1&}clamp",
            &style,
            &[],
        );
        let span_style = &parsed.lines[0].spans[0].style;

        assert_eq!(span_style.primary_colour, 0xFFFF_FFFF);
        assert_eq!(span_style.secondary_colour, 0x33FF_FFFF);
    }

    #[test]
    fn parses_rectangular_clip_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\clip(10,20,30,40)}Clip", &base_style, &[]);
        let inverse = parse_dialogue_text("{\\iclip(1,2,3,4)}Clip", &base_style, &[]);

        assert_eq!(
            parsed.clip_rect,
            Some(Rect {
                x_min: 10,
                y_min: 20,
                x_max: 30,
                y_max: 40
            })
        );
        assert_eq!(
            parsed.clip_rect_exact,
            Some(ParsedRectF64 {
                x_min: 10.0,
                y_min: 20.0,
                x_max: 30.0,
                y_max: 40.0,
            })
        );
        assert!(!parsed.inverse_clip);
        assert_eq!(
            inverse.clip_rect,
            Some(Rect {
                x_min: 1,
                y_min: 2,
                x_max: 3,
                y_max: 4
            })
        );
        assert_eq!(
            inverse.clip_rect_exact,
            Some(ParsedRectF64 {
                x_min: 1.0,
                y_min: 2.0,
                x_max: 3.0,
                y_max: 4.0,
            })
        );
        assert!(inverse.inverse_clip);
    }

    #[test]
    fn decimal_position_origin_move_preserve_exact_coordinates_but_clip_truncates_like_libass() {
        let base_style = ParsedStyle::default();
        let positioned =
            parse_dialogue_text("{\\pos(10.25,20.75)\\org(4.5,8.125)}Pos", &base_style, &[]);
        let moved = parse_dialogue_text(
            "{\\move(1.5,2.25,30.75,40.125,900,100)}Move",
            &base_style,
            &[],
        );
        let clipped = parse_dialogue_text("{\\clip(1.5,2.5,30.25,40.75)}Clip", &base_style, &[]);

        assert_eq!(positioned.position_exact, Some((10.25, 20.75)));
        assert_eq!(positioned.origin_exact, Some((4.5, 8.125)));
        assert_eq!(positioned.position, None);
        assert_eq!(positioned.origin, None);
        assert_eq!(
            moved.movement_exact,
            Some(ParsedMovementExact {
                start: (1.5, 2.25),
                end: (30.75, 40.125),
                t1_ms: 100,
                t2_ms: 900,
            })
        );
        assert_eq!(moved.movement, None);
        assert_eq!(
            clipped.clip_rect_exact,
            Some(ParsedRectF64 {
                x_min: 1.5,
                y_min: 2.5,
                x_max: 30.25,
                y_max: 40.75,
            })
        );
        assert_eq!(
            clipped.clip_rect,
            Some(Rect {
                x_min: 1,
                y_min: 2,
                x_max: 30,
                y_max: 40,
            })
        );
    }

    #[test]
    fn invalid_present_complex_tag_arguments_parse_as_zero_like_libass() {
        let base_style = ParsedStyle::default();
        let positioned = parse_dialogue_text("{\\pos(abc,20)\\org(4,zz)}Pos", &base_style, &[]);
        let moved = parse_dialogue_text("{\\move(abc,2,30,zz,foo,100)}Move", &base_style, &[]);
        let clipped = parse_dialogue_text("{\\clip(abc,2,30,zz)}Clip", &base_style, &[]);
        let faded = parse_dialogue_text("{\\fad(foo,200)}Fade", &base_style, &[]);

        assert_eq!(positioned.position_exact, Some((0.0, 20.0)));
        assert_eq!(positioned.origin_exact, Some((4.0, 0.0)));
        assert_eq!(positioned.position, None);
        assert_eq!(
            moved.movement_exact,
            Some(ParsedMovementExact {
                start: (0.0, 2.0),
                end: (30.0, 0.0),
                t1_ms: 0,
                t2_ms: 100,
            })
        );
        assert_eq!(moved.movement, None);
        assert_eq!(
            clipped.clip_rect,
            Some(Rect {
                x_min: 0,
                y_min: 2,
                x_max: 30,
                y_max: 0,
            })
        );
        assert_eq!(
            clipped.clip_rect_exact,
            Some(ParsedRectF64 {
                x_min: 0.0,
                y_min: 2.0,
                x_max: 30.0,
                y_max: 0.0,
            })
        );
        assert_eq!(
            faded.fade,
            Some(ParsedFade::Simple {
                fade_in_ms: 0,
                fade_out_ms: 200,
            })
        );
    }

    #[test]
    fn huge_complex_integer_arguments_clamp_like_libass() {
        let base_style = ParsedStyle::default();
        let clipped = parse_dialogue_text(
            "{\\clip(999999999999999999999,-999999999999999999999,1,2)}Clip",
            &base_style,
            &[],
        );

        assert_eq!(
            clipped.clip_rect,
            Some(Rect {
                x_min: i32::MAX,
                y_min: i32::MIN,
                x_max: 1,
                y_max: 2,
            })
        );
    }

    #[test]
    fn override_arguments_do_not_trim_unicode_whitespace_like_libass() {
        let base_style = ParsedStyle {
            italic: false,
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fs\u{00a0}40\\i\u{00a0}1\\pos(\u{00a0}10,20)}Text",
            &base_style,
            &[],
        );
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_size, 20.0);
        assert!(!style.italic);
        assert_eq!(parsed.position_exact, Some((0.0, 20.0)));
    }

    #[test]
    fn empty_complex_tag_arguments_are_missing_not_zero_like_libass() {
        let base_style = ParsedStyle::default();
        let positioned = parse_dialogue_text("{\\pos(,20)}Pos", &base_style, &[]);
        let clipped = parse_dialogue_text("{\\clip(,2,30,40)}Clip", &base_style, &[]);
        let faded = parse_dialogue_text("{\\fad(,200)}Fade", &base_style, &[]);

        assert_eq!(positioned.position_exact, None);
        assert_eq!(positioned.position, None);
        assert_eq!(clipped.clip_rect, None);
        assert_eq!(clipped.clip_rect_exact, None);
        assert_eq!(clipped.vector_clip, None);
        assert_eq!(faded.fade, None);
    }

    #[test]
    fn complex_tag_backslash_tail_keeps_following_commas_in_one_arg_like_libass() {
        let base_style = ParsedStyle::default();
        let moved = parse_dialogue_text("{\\move(1,2,3,4,\\bord5,6)}Move", &base_style, &[]);

        assert_eq!(moved.movement, None);
        assert_eq!(moved.movement_exact, None);
        assert_eq!(moved.lines[0].spans[0].style.border, base_style.outline);
    }

    #[test]
    fn vector_clip_rejects_extra_nonempty_arguments_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed =
            parse_dialogue_text("{\\clip(1,m 0 0,l 10 0 10 10 0 10)}Clip", &base_style, &[]);

        assert_eq!(parsed.vector_clip, None);
        assert_eq!(parsed.clip_rect, None);
        assert_eq!(parsed.clip_rect_exact, None);
    }

    #[test]
    fn origin_override_is_first_wins_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\org(10,20)\\org(30,40)}Origin", &base_style, &[]);

        assert_eq!(parsed.origin, Some((10, 20)));
        assert_eq!(parsed.origin_exact, Some((10.0, 20.0)));
    }

    #[test]
    fn invalid_boolean_overrides_fall_back_like_libass() {
        let base_style = ParsedStyle {
            italic: true,
            underline: true,
            strike_out: true,
            font_weight: 400,
            bold: false,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("{\\b-1\\i2\\u-1\\s3}Flags", &base_style, &[]);
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_weight, 400);
        assert!(!style.bold);
        assert!(style.italic);
        assert!(style.underline);
        assert!(style.strike_out);
    }

    #[test]
    fn invalid_present_override_arguments_parse_as_zero_like_libass() {
        let base_style = ParsedStyle {
            encoding: 7,
            primary_colour: 0x8001_0203,
            italic: true,
            underline: true,
            strike_out: true,
            font_weight: 700,
            bold: true,
            scale_x: 1.5,
            spacing: 4.0,
            angle: 12.0,
            outline: 5.0,
            shadow: 6.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\feabc\\fscxabc\\fspabc\\frzabc\\faxabc\\xbordabc\\xshadabc\\qabc\\czz\\alphazz\\babc\\iabc\\uabc\\sabc}Bad",
            &base_style,
            &[],
        );
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.encoding, 0);
        assert_eq!(style.scale_x, 0.0);
        assert_eq!(style.spacing, 0.0);
        assert_eq!(style.rotation_z, 0.0);
        assert_eq!(style.shear_x, 0.0);
        assert_eq!(style.border_x, 0.0);
        assert_eq!(style.shadow_x, 0.0);
        assert_eq!(style.primary_colour, 0);
        assert_eq!(style.font_weight, 400);
        assert!(!style.bold);
        assert!(!style.italic);
        assert!(!style.underline);
        assert!(!style.strike_out);
        assert_eq!(parsed.wrap_style, Some(0));
    }

    #[test]
    fn invalid_present_karaoke_duration_still_creates_zero_length_span_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\kabc}Karaoke", &base_style, &[]);
        let karaoke = parsed.lines[0].spans[0]
            .karaoke
            .expect("invalid-present karaoke tag still starts a karaoke span");

        assert_eq!(karaoke.start_ms, 0);
        assert_eq!(karaoke.duration_ms, 0);
    }

    #[test]
    fn numeric_prefix_accepts_bare_exponent_marker_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\blur1e\\fsp2e+\\fscx150e-}Num", &base_style, &[]);
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.blur, 1.0);
        assert_eq!(style.spacing, 2.0);
        assert_eq!(style.scale_x, 1.5);
    }

    #[test]
    fn be_override_rounds_with_libass_vsfilter_half_bias() {
        let base_style = ParsedStyle::default();
        let be_for = |tag: &str| {
            parse_dialogue_text(&format!("{{\\be{tag}}}Blur"), &base_style, &[]).lines[0].spans[0]
                .style
                .be
        };

        assert_eq!(be_for("0.49"), 0.0);
        assert_eq!(be_for("0.5"), 1.0);
        assert_eq!(be_for("1.6"), 2.0);
        assert_eq!(be_for("200"), 127.0);
        assert_eq!(be_for("-1"), 0.0);
    }

    #[test]
    fn static_blur_clamps_but_transform_blur_keeps_raw_target_like_libass() {
        let base_style = ParsedStyle::default();
        let static_blur = parse_dialogue_text("{\\blur200}Blur", &base_style, &[]).lines[0].spans
            [0]
        .style
        .blur;
        let animated_blur =
            parse_dialogue_text("{\\t(0,100,\\blur200)}Blur", &base_style, &[]).lines[0].spans[0]
                .transforms[0]
                .style
                .blur;

        assert_eq!(static_blur, 100.0);
        assert_eq!(animated_blur, Some(200.0));
    }

    #[test]
    fn static_nonnegative_tags_clamp_but_transform_targets_keep_raw_values_like_libass() {
        let base_style = ParsedStyle::default();
        let static_style =
            parse_dialogue_text("{\\fscx-50\\bord-3\\shad-2}Clamp", &base_style, &[]).lines[0]
                .spans[0]
                .style
                .clone();
        let animated = parse_dialogue_text(
            "{\\t(0,100,\\fscx-50\\bord-3\\shad-2)}Clamp",
            &base_style,
            &[],
        )
        .lines[0]
            .spans[0]
            .transforms[0]
            .style
            .clone();

        assert_eq!(static_style.scale_x, 0.0);
        assert_eq!(static_style.border, 0.0);
        assert_eq!(static_style.shadow, 0.0);
        assert_eq!(animated.scale_x, Some(-0.5));
        assert_eq!(animated.border, Some(-3.0));
        assert_eq!(animated.shadow, Some(-2.0));
    }

    #[test]
    fn transform_invalid_present_arguments_animate_to_zero_like_libass() {
        let base_style = ParsedStyle {
            primary_colour: 0x8001_0203,
            scale_x: 1.5,
            spacing: 4.0,
            angle: 12.0,
            outline: 5.0,
            shadow: 6.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\fscxabc\\fspabc\\frzabc\\faxabc\\xbordabc\\xshadabc\\czz\\alphazz)}Bad",
            &base_style,
            &[],
        );

        let transform = &parsed.lines[0].spans[0].transforms[0].style;
        assert_eq!(transform.scale_x, Some(0.0));
        assert_eq!(transform.spacing, Some(0.0));
        assert_eq!(transform.rotation_z, Some(0.0));
        assert_eq!(transform.shear_x, Some(0.0));
        assert_eq!(transform.border_x, Some(0.0));
        assert_eq!(transform.shadow_x, Some(0.0));
        assert_eq!(transform.primary_colour, None);
        assert_eq!(
            transform.primary_colour_steps,
            vec![
                ParsedColourTransform::Rgb { value: 0 },
                ParsedColourTransform::Alpha { value: 0 },
            ]
        );
    }

    #[test]
    fn transform_alpha_targets_keep_raw_int_until_render_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\1a&H100&\\1a&H80&\\2a&H-1&\\2a&H00&)}Alpha",
            &base_style,
            &[],
        );

        assert_eq!(
            parsed.lines[0].spans[0].transforms[0]
                .style
                .primary_colour_steps,
            vec![
                ParsedColourTransform::Alpha { value: 0x100 },
                ParsedColourTransform::Alpha { value: 0x80 },
            ]
        );
        assert_eq!(
            parsed.lines[0].spans[0].transforms[0]
                .style
                .secondary_colour_steps,
            vec![
                ParsedColourTransform::Alpha { value: -1 },
                ParsedColourTransform::Alpha { value: 0 },
            ]
        );
    }

    #[test]
    fn single_transform_alpha_target_keeps_out_of_byte_range_value_like_libass() {
        let parsed = parse_dialogue_text("{\\1a0\\t(\\1a1FF)}Alpha", &ParsedStyle::default(), &[]);
        let transform = &parsed.lines[0].spans[0].transforms[0].style;

        assert_eq!(transform.primary_colour, None);
        assert_eq!(
            transform.primary_colour_steps,
            vec![ParsedColourTransform::Alpha { value: 0x1FF }]
        );
    }

    #[test]
    fn negative_letter_spacing_is_preserved_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\fsp-2.5}Spacing", &base_style, &[]);

        assert_eq!(parsed.lines[0].spans[0].style.spacing, -2.5);
    }

    #[test]
    fn hard_override_flag_tracks_libass_event_scan_tags() {
        let base_style = ParsedStyle::default();

        assert!(!parse_dialogue_text("{\\blur1}Text", &base_style, &[]).hard_override);
        assert!(parse_dialogue_text("{\\p0}Text", &base_style, &[]).hard_override);
        assert!(parse_dialogue_text("{\\t(0,100,\\p0)}Text", &base_style, &[]).hard_override);
        assert!(parse_dialogue_text("{\\org(10,20)}Text", &base_style, &[]).hard_override);
        assert!(parse_dialogue_text("{\\foo(\\pos(10,20))}Text", &base_style, &[]).hard_override);
        assert!(!parse_dialogue_text("{\\ pos(10,20)}Text", &base_style, &[]).hard_override);
    }

    #[test]
    fn transform_tag_disables_collision_without_becoming_hard_override_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\t}Text", &base_style, &[]);

        assert!(!parsed.hard_override);
        assert!(parsed.transform_disables_collision);
        assert!(parsed.lines[0].spans[0].transforms.is_empty());
    }

    #[test]
    fn parenthesized_simple_override_arguments_parse_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            primary_colour: 0x2000_0000,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fn(Alt Font)\\fs(30)\\fscx(150)\\fsp(4)\\frz(45)\\fax(0.25)\\bord(3)\\xshad(-2)\\blur(1.5)\\be(1.6)\\pbo(7)\\c(&H112233&)\\alpha(&H80&)\\b(1)\\i(0)\\u(1)\\s(1)\\q(2)\\an(7)}Text",
            &base_style,
            &[],
        );
        let legacy = parse_dialogue_text("{\\a(5)}Text", &base_style, &[]);
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_name, "Alt Font");
        assert_eq!(style.font_size, 30.0);
        assert_eq!(style.scale_x, 1.5);
        assert_eq!(style.spacing, 4.0);
        assert_eq!(style.rotation_z, 45.0);
        assert_eq!(style.shear_x, 0.25);
        assert_eq!(style.border, 3.0);
        assert_eq!(style.border_x, 3.0);
        assert_eq!(style.border_y, 3.0);
        assert_eq!(style.shadow_x, -2.0);
        assert_eq!(style.blur, 1.5);
        assert_eq!(style.be, 2.0);
        assert_eq!(style.pbo, 7.0);
        assert_eq!(style.primary_colour, 0x8011_2233);
        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
        assert!(!style.italic);
        assert!(style.underline);
        assert!(style.strike_out);
        assert_eq!(parsed.wrap_style, Some(2));
        assert_eq!(parsed.alignment, Some(ass::VALIGN_TOP | ass::HALIGN_LEFT));
        assert_eq!(legacy.alignment, Some(ass::VALIGN_TOP | ass::HALIGN_LEFT));
    }

    #[test]
    fn parenthesized_simple_override_args_beat_tag_name_tail_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            scale_x: 1.25,
            primary_colour: 0x2000_0000,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fnjunk(Alt Font)\\fsjunk(30)\\fscxjunk(150)\\blurjunk(2.5)\\cjunk(&H112233&)\\alphajunk(&H80&)\\qjunk(2)}Text",
            &base_style,
            &[],
        );
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_name, "Alt Font");
        assert_eq!(style.font_size, 30.0);
        assert_eq!(style.scale_x, 1.5);
        assert_eq!(style.blur, 2.5);
        assert_eq!(style.primary_colour, 0x8011_2233);
        assert_eq!(parsed.wrap_style, Some(2));
    }

    #[test]
    fn empty_parenthesized_simple_override_args_fall_back_to_tail_like_libass() {
        let base_style = ParsedStyle {
            scale_x: 1.25,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("{\\fscxjunk()}Text", &base_style, &[]);

        assert_eq!(parsed.lines[0].spans[0].style.scale_x, 0.0);
    }

    #[test]
    fn complex_override_tags_ignore_tag_name_tail_before_parentheses_like_libass() {
        let base_style = ParsedStyle::default();
        let positioned = parse_dialogue_text("{\\posjunk(10,20)}Text", &base_style, &[]);
        let clipped = parse_dialogue_text("{\\clipjunk(1,2,30,40)}Text", &base_style, &[]);
        let transformed =
            parse_dialogue_text("{\\tjunk(0,100,\\blurjunk(5))}Text", &base_style, &[]);

        assert_eq!(positioned.position, Some((10, 20)));
        assert_eq!(positioned.position_exact, Some((10.0, 20.0)));
        assert_eq!(
            clipped.clip_rect,
            Some(Rect {
                x_min: 1,
                y_min: 2,
                x_max: 30,
                y_max: 40,
            })
        );
        assert_eq!(
            clipped.clip_rect_exact,
            Some(rect_to_f64(clipped.clip_rect.unwrap()))
        );
        assert_eq!(
            transformed.lines[0].spans[0].transforms[0].style.blur,
            Some(5.0)
        );
    }

    #[test]
    fn fsc_prefix_resets_both_scale_axes_like_libass() {
        let base_style = ParsedStyle {
            scale_x: 1.2,
            scale_y: 0.8,
            ..ParsedStyle::default()
        };

        let suffixed = parse_dialogue_text("{\\fscx150\\fscy50\\fsc100}Text", &base_style, &[]);
        let parenthesized = parse_dialogue_text("{\\fscx150\\fscy50\\fsc()}Text", &base_style, &[]);
        let transformed = parse_dialogue_text(
            "{\\fscx150\\fscy50\\t(0,100,\\fsc())}Text",
            &base_style,
            &[],
        );

        assert_eq!(suffixed.lines[0].spans[0].style.scale_x, 1.2);
        assert_eq!(suffixed.lines[0].spans[0].style.scale_y, 0.8);
        assert_eq!(parenthesized.lines[0].spans[0].style.scale_x, 1.2);
        assert_eq!(parenthesized.lines[0].spans[0].style.scale_y, 0.8);
        assert_eq!(
            transformed.lines[0].spans[0].transforms[0].style.scale_x,
            None
        );
        assert_eq!(
            transformed.lines[0].spans[0].transforms[0]
                .style
                .scale_x_steps,
            vec![ParsedScaleTransform::Reset { reset: 1.2 }]
        );
        assert_eq!(
            transformed.lines[0].spans[0].transforms[0].style.scale_y,
            None
        );
        assert_eq!(
            transformed.lines[0].spans[0].transforms[0]
                .style
                .scale_y_steps,
            vec![ParsedScaleTransform::Reset { reset: 0.8 }]
        );
    }

    #[test]
    fn parenthesized_arguments_stop_at_first_closing_paren_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fn(Alt Font)ignored\\pos(10,20)ignored}Text",
            &base_style,
            &[],
        );

        let span = &parsed.lines[0].spans[0];
        assert_eq!(span.style.font_name, "Alt Font");
        assert_eq!(parsed.position, Some((10, 20)));
        assert_eq!(parsed.position_exact, Some((10.0, 20.0)));
    }

    #[test]
    fn empty_parenthesized_simple_override_arguments_are_missing_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            font_size: 24.0,
            outline: 5.0,
            primary_colour: 0x4011_2233,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fn(Other)\\fn()\\fs(99)\\fs()\\bord(9)\\bord()\\c&H000000&\\c()\\k()}Text",
            &base_style,
            &[],
        );
        let span = &parsed.lines[0].spans[0];

        assert_eq!(span.style.font_name, "Base");
        assert_eq!(span.style.font_size, 24.0);
        assert_eq!(span.style.border, 5.0);
        assert_eq!(span.style.primary_colour, 0x4011_2233);
        assert_eq!(
            span.karaoke
                .expect("empty \\k() still starts karaoke")
                .duration_ms,
            1000
        );
    }

    #[test]
    fn parenthesized_reset_style_argument_matches_libass() {
        let base_style = ParsedStyle {
            name: "Default".to_string(),
            font_name: "Base".to_string(),
            font_size: 24.0,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_name: "Alt Font".to_string(),
            font_size: 36.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fs20\\r(Alt)}Alt{\\r()}Base",
            &base_style,
            &[base_style.clone(), alt_style],
        );

        assert_eq!(parsed.lines[0].spans[0].style.font_name, "Alt Font");
        assert_eq!(parsed.lines[0].spans[0].style.font_size, 36.0);
        assert_eq!(parsed.lines[0].spans[1].style.font_name, "Base");
        assert_eq!(parsed.lines[0].spans[1].style.font_size, 24.0);
    }

    #[test]
    fn reset_style_lookup_preserves_unparenthesized_leading_space_like_libass() {
        let base_style = ParsedStyle {
            name: "Default".to_string(),
            font_name: "Base".to_string(),
            font_size: 24.0,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_name: "Alt Font".to_string(),
            font_size: 36.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fs20\\r Alt}Base{\\r (Alt)}Alt",
            &base_style,
            &[base_style.clone(), alt_style],
        );

        assert_eq!(parsed.lines[0].spans[0].style.font_name, "Base");
        assert_eq!(parsed.lines[0].spans[0].style.font_size, 24.0);
        assert_eq!(parsed.lines[0].spans[1].style.font_name, "Alt Font");
        assert_eq!(parsed.lines[0].spans[1].style.font_size, 36.0);
    }

    #[test]
    fn transform_parenthesized_child_tags_close_outer_transform_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\bord(4)\\blur(2)\\alpha(&H80&)\\fscx(150))}Text",
            &base_style,
            &[],
        );
        let span = &parsed.lines[0].spans[0];
        let transform = &span.transforms[0].style;

        assert_eq!(transform.border, Some(4.0));
        assert_eq!(transform.border_x, Some(4.0));
        assert_eq!(transform.border_y, Some(4.0));
        assert_eq!(transform.blur, None);
        assert_eq!(transform.primary_colour, None);
        assert_eq!(transform.scale_x, None);
        assert_eq!(span.style.blur, 2.0);
        assert_eq!(span.style.primary_colour, 0x80FF_FFFF);
        assert_eq!(span.style.scale_x, 1.5);
    }

    #[test]
    fn parses_vector_clip_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\clip(m 0 0 l 10 0 10 10 0 10)}Clip", &base_style, &[]);

        assert!(parsed.clip_rect.is_none());
        assert_eq!(
            parsed.vector_clip,
            Some(ParsedVectorClip {
                scale: 1,
                polygons: vec![vec![
                    Point { x: 0, y: 0 },
                    Point { x: 10, y: 0 },
                    Point { x: 10, y: 10 },
                    Point { x: 0, y: 10 },
                ]],
            })
        );
        assert!(!parsed.inverse_clip);
        assert!(!parsed.vector_clip_inverse);
    }

    #[test]
    fn vector_clip_is_first_wins_and_separate_from_rect_clip_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\iclip(m 0 0 l 10 0 10 10 0 10)\\clip(m 20 20 l 30 20 30 30 20 30)\\clip(1,2,3,4)}Clip",
            &base_style,
            &[],
        );

        assert_eq!(
            parsed.clip_rect,
            Some(Rect {
                x_min: 1,
                y_min: 2,
                x_max: 3,
                y_max: 4,
            })
        );
        assert!(!parsed.inverse_clip);
        assert_eq!(
            parsed.vector_clip,
            Some(ParsedVectorClip {
                scale: 1,
                polygons: vec![vec![
                    Point { x: 0, y: 0 },
                    Point { x: 10, y: 0 },
                    Point { x: 10, y: 10 },
                    Point { x: 0, y: 10 },
                ]],
            })
        );
        assert!(parsed.vector_clip_inverse);
    }

    #[test]
    fn invalid_first_vector_clip_still_blocks_later_vector_clip_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\clip(not drawing)\\iclip(m 0 0 l 10 0 10 10 0 10)\\clip(1,2,3,4)}Clip",
            &base_style,
            &[],
        );

        assert_eq!(
            parsed.clip_rect,
            Some(Rect {
                x_min: 1,
                y_min: 2,
                x_max: 3,
                y_max: 4,
            })
        );
        assert!(!parsed.inverse_clip);
        assert_eq!(
            parsed.vector_clip,
            Some(ParsedVectorClip {
                scale: 1,
                polygons: Vec::new(),
            })
        );
    }

    #[test]
    fn transform_invalid_vector_clip_claim_blocks_later_vector_clip_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(\\clip(not drawing))\\clip(m 0 0 l 10 0 10 10 0 10)}Clip",
            &base_style,
            &[],
        );

        assert_eq!(
            parsed.vector_clip,
            Some(ParsedVectorClip {
                scale: 1,
                polygons: Vec::new(),
            })
        );
    }

    #[test]
    fn nonpositive_vector_clip_scales_clamp_to_one_like_current_libass() {
        let base_style = ParsedStyle::default();
        for scale in ["abc", "0", "-1", "-2147483648"] {
            let text = format!("{{\\clip({scale},m 0 0 l 10 0 10 10 0 10)}}Clip");
            let parsed = parse_dialogue_text(&text, &base_style, &[]);
            let clip = parsed
                .vector_clip
                .expect("a present vector-clip scale keeps the drawing argument");
            assert_eq!(clip.scale, 1, "scale argument {scale:?}");
            assert!(!clip.polygons.is_empty(), "scale argument {scale:?}");
        }
    }

    #[test]
    fn parses_decimal_vector_clip_points_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\clip(m 581.33 606.67 l 562.67 525.33 625.33 517.33)}Clip",
            &base_style,
            &[],
        );

        assert_eq!(
            parsed.vector_clip,
            Some(ParsedVectorClip {
                scale: 1,
                polygons: vec![vec![
                    Point { x: 581, y: 607 },
                    Point { x: 563, y: 525 },
                    Point { x: 625, y: 517 },
                ]],
            })
        );
    }

    #[test]
    fn retains_decimal_vector_clip_points_in_d6_for_rendering() {
        let clip = parse_dialogue_vector_clip_d6(
            "{\\clip(m 0.838 1.25 l 10.5 1.25 10.5 9.75 0.838 9.75)}Clip",
        )
        .expect("vector clip");

        assert_eq!(clip.scale, 1);
        assert_eq!(
            clip.polygons,
            vec![vec![
                Point { x: 54, y: 80 },
                Point { x: 672, y: 80 },
                Point { x: 672, y: 624 },
                Point { x: 54, y: 624 },
            ]]
        );
    }

    #[test]
    fn exact_vector_clip_scan_obeys_transform_and_first_claim_semantics() {
        let clip = parse_dialogue_vector_clip_d6(
            "{\\t(0,1000,\\iclip(m 0.5 0 l 10.5 0 10.5 10 0.5 10))\\clip(m 20 20 l 30 20 30 30 20 30)}Clip",
        )
        .expect("transform-side-effect vector clip");
        assert_eq!(clip.polygons[0][0], Point { x: 32, y: 0 });
        assert_eq!(clip.polygons[0][1], Point { x: 672, y: 0 });

        let nested = parse_dialogue_vector_clip_d6(
            "{\\t(0,1000,\\t(0,500,\\clip(m 0.5 0 l 10.5 0 10.5 10 0.5 10)))\\clip(m 20.5 20 l 30.5 20 30.5 30 20.5 30)}Clip",
        )
        .expect("the first clip understood by the normal parser");
        assert_eq!(nested.polygons[0][0], Point { x: 1312, y: 1280 });
    }

    #[test]
    fn drawing_outline_range_rejects_int32_min_and_master_regression_coordinate() {
        let base_style = ParsedStyle::default();
        for coordinate in ["-2147483648", "-33554432", "4194304", "1e999"] {
            let drawing_text = format!("{{\\p1}}m 0 0 l {coordinate} 0 0 10");
            let parsed = parse_dialogue_text(&drawing_text, &base_style, &[]);
            let drawing = parsed.lines[0].spans[0]
                .drawing
                .as_ref()
                .expect("an invalid outline must remain a drawing-mode span");
            assert!(
                drawing.polygons.is_empty(),
                "out-of-range coordinate {coordinate:?} must invalidate the outline"
            );

            for tag in ["clip", "iclip"] {
                let clip_text = format!("{{\\{tag}(m 0 0 l {coordinate} 0 0 10)}}visible text");
                let clipped = parse_dialogue_text(&clip_text, &base_style, &[]);
                assert!(
                    clipped.vector_clip.is_none(),
                    "an invalid {tag} outline must be left unapplied for {coordinate:?}"
                );
            }
        }

        let boundary =
            parse_dialogue_text("{\\p1}m 0 0 l 4194303 0 4194303 1 0 1", &base_style, &[]);
        assert!(
            !boundary.lines[0].spans[0]
                .drawing
                .as_ref()
                .expect("boundary drawing")
                .polygons
                .is_empty(),
            "the last whole-pixel coordinate within libass OUTLINE_MAX stays valid"
        );

        // Range check applies only when a token is added to an outline; an unused hostile move is a valid empty clip.
        for tag in ["clip", "iclip"] {
            let parsed = parse_dialogue_text(
                &format!("{{\\{tag}(m -33554432 0)}}visible text"),
                &base_style,
                &[],
            );
            assert!(
                parsed
                    .vector_clip
                    .as_ref()
                    .is_some_and(|clip| clip.polygons.is_empty()),
                "an unused invalid move remains a syntactically valid empty {tag}"
            );
        }
    }

    #[test]
    fn enormous_spline_coordinates_and_programmatic_bounds_are_rejected_safely() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\p1}m 0 0 s -33554432 0 10 10 20 20 p 30 30 c",
            &base_style,
            &[],
        );
        assert!(
            parsed.lines[0].spans[0]
                .drawing
                .as_ref()
                .expect("spline remains a drawing span")
                .polygons
                .is_empty()
        );

        let hostile = ParsedDrawing {
            scale: 1,
            polygons: vec![vec![
                Point { x: i32::MIN, y: 0 },
                Point { x: 0, y: 0 },
                Point { x: 0, y: 1 },
            ]],
        };
        assert_eq!(hostile.bounds(), None);
    }

    #[test]
    fn parses_move_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\move(10,20,110,220,50,150)}Move", &base_style, &[]);

        assert_eq!(
            parsed.movement,
            Some(ParsedMovement {
                start: (10, 20),
                end: (110, 220),
                t1_ms: 50,
                t2_ms: 150,
            })
        );
        assert!(parsed.position.is_none());
    }

    #[test]
    fn parses_fad_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\fad(120,240)}Fade", &base_style, &[]);

        assert_eq!(
            parsed.fade,
            Some(ParsedFade::Simple {
                fade_in_ms: 120,
                fade_out_ms: 240,
            })
        );
    }

    #[test]
    fn parses_full_fade_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\fade(10,20,30,40,50,60,70)}Fade", &base_style, &[]);

        assert_eq!(
            parsed.fade,
            Some(ParsedFade::Complex {
                alpha1: 10,
                alpha2: 20,
                alpha3: 30,
                t1_ms: 40,
                t2_ms: 50,
                t3_ms: 60,
                t4_ms: 70,
            })
        );
    }

    #[test]
    fn fade_parsers_accept_libass_argument_counts_and_first_tag_wins() {
        let base_style = ParsedStyle::default();
        let fade_two = parse_dialogue_text("{\\fade(120,240)}Fade", &base_style, &[]);
        assert_eq!(
            fade_two.fade,
            Some(ParsedFade::Simple {
                fade_in_ms: 120,
                fade_out_ms: 240,
            })
        );

        let fad_seven = parse_dialogue_text("{\\fad(1,2,3,4,5,6,7)}Fade", &base_style, &[]);
        assert_eq!(
            fad_seven.fade,
            Some(ParsedFade::Complex {
                alpha1: 1,
                alpha2: 2,
                alpha3: 3,
                t1_ms: 4,
                t2_ms: 5,
                t3_ms: 6,
                t4_ms: 7,
            })
        );

        let first_wins = parse_dialogue_text("{\\fad(10,20)\\fade(1,2)}Fade", &base_style, &[]);
        assert_eq!(
            first_wins.fade,
            Some(ParsedFade::Simple {
                fade_in_ms: 10,
                fade_out_ms: 20,
            })
        );
    }

    #[test]
    fn parses_karaoke_spans() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k10}Ka{\\K20}ra{\\ko30}oke", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 3);
        assert_eq!(
            parsed.lines[0].spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            parsed.lines[0].spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 100,
                duration_ms: 200,
                mode: ParsedKaraokeMode::Sweep,
            })
        );
        assert_eq!(
            parsed.lines[0].spans[2].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 300,
                duration_ms: 300,
                mode: ParsedKaraokeMode::OutlineToggle,
            })
        );
    }

    #[test]
    fn parses_kt_karaoke_timing_reset() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k10}A{\\kt50\\k10}B", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 2);
        assert_eq!(
            parsed.lines[0].spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            parsed.lines[0].spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 500,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
    }

    #[test]
    fn kt_retimes_unconsumed_karaoke_word_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k10\\kt50}A{\\K20\\kt30}B", &base_style, &[]);

        let spans = &parsed.lines[0].spans;
        assert_eq!(spans.len(), 2);
        assert_eq!(
            spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 500,
                duration_ms: 0,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 300,
                duration_ms: 0,
                mode: ParsedKaraokeMode::Sweep,
            })
        );
    }

    #[test]
    fn parses_karaoke_empty_decimal_and_negative_durations_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k}A{\\k1.5}B{\\k-2}C{\\k1e999}D", &base_style, &[]);

        assert_eq!(parsed.lines[0].spans.len(), 4);
        assert_eq!(
            parsed.lines[0].spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 1000,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            parsed.lines[0].spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 1000,
                duration_ms: 15,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            parsed.lines[0].spans[2].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 1015,
                duration_ms: -20,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            parsed.lines[0].spans[3].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 995,
                duration_ms: i32::MIN,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
    }

    #[test]
    fn karaoke_cursor_wraps_on_i32_overflow_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k1e999}A{\\k1e999}B{\\k10}C", &base_style, &[]);

        let spans = &parsed.lines[0].spans;
        assert_eq!(spans.len(), 3);
        assert_eq!(
            spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: i32::MIN,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: i32::MIN,
                duration_ms: i32::MIN,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            spans[2].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
    }

    #[test]
    fn parses_font_size_relative_and_scale_reset_overrides() {
        let base_style = ParsedStyle {
            font_size: 20.0,
            scale_x: 1.2,
            scale_y: 0.8,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fs+5}Bigger{\\fs-2}Smaller{\\fs0}Reset{\\fscx150\\fscy50}Scaled{\\fsc}Base",
            &base_style,
            &[],
        );

        assert_eq!(parsed.lines[0].spans[0].style.font_size, 30.0);
        assert_eq!(parsed.lines[0].spans[1].style.font_size, 24.0);
        assert_eq!(parsed.lines[0].spans[2].style.font_size, 20.0);
        assert_eq!(parsed.lines[0].spans[3].style.scale_x, 1.5);
        assert_eq!(parsed.lines[0].spans[3].style.scale_y, 0.5);
        assert_eq!(parsed.lines[0].spans[4].style.scale_x, 1.2);
        assert_eq!(parsed.lines[0].spans[4].style.scale_y, 0.8);
    }

    #[test]
    fn parses_backslash_n_breaks_only_with_wrap_style_two_like_libass() {
        let base_style = ParsedStyle::default();
        let normal = parse_dialogue_text("one\\ntwo", &base_style, &[]);
        assert_eq!(normal.lines.len(), 1);
        assert_eq!(normal.lines[0].spans[0].text, "one two");

        let q2 = parse_dialogue_text("{\\q2}one\\ntwo", &base_style, &[]);
        assert_eq!(q2.lines.len(), 2);
        assert_eq!(q2.lines[0].spans[0].text, "one");
        assert_eq!(q2.lines[1].spans[0].text, "two");

        for wrap_style in [0, 1, 3] {
            let parsed =
                parse_dialogue_text_with_wrap_style("one\\ntwo", &base_style, &[], wrap_style);
            assert_eq!(parsed.lines.len(), 1);
            assert_eq!(parsed.lines[0].spans[0].text, "one two");
        }

        let reset = parse_dialogue_text_with_wrap_style("{\\q2\\q4}one\\ntwo", &base_style, &[], 1);
        assert_eq!(reset.wrap_style, Some(1));
        assert_eq!(reset.lines.len(), 1);

        let invalid_inherited =
            parse_dialogue_text_with_wrap_style("{\\q2\\q4}one\\ntwo", &base_style, &[], 7);
        assert_eq!(invalid_inherited.wrap_style, Some(7));
        assert_eq!(invalid_inherited.lines.len(), 1);
    }

    #[test]
    fn explicit_empty_lines_preserve_current_style_for_half_height_metrics() {
        let base_style = ParsedStyle {
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("A\\N{\\fs80}\\N\\NB", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 4);
        assert_eq!(parsed.lines[0].text, "A");
        assert_eq!(parsed.lines[1].text, "");
        assert_eq!(parsed.lines[2].text, "");
        assert_eq!(parsed.lines[3].text, "B");
        assert_eq!(parsed.lines[1].spans.len(), 1);
        assert_eq!(parsed.lines[1].spans[0].style.font_size, 80.0);
        assert_eq!(parsed.lines[2].spans[0].style.font_size, 80.0);
    }

    #[test]
    fn drawing_mode_treats_newline_escapes_as_path_whitespace() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}m 0 0 l 10 0\\N l 10 10 l 0 10", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing should continue across \\N like libass");
        assert_eq!(drawing.polygons.len(), 1);
        assert_eq!(drawing.bounds().expect("bounds").x_max, 11);
        assert_eq!(drawing.bounds().expect("bounds").y_max, 11);
    }

    #[test]
    fn drawing_mode_keeps_backslash_escapes_raw_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}m 0 0 l 10 0\\N10 10 0 10", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(parsed.lines[0].spans[0].text, "m 0 0 l 10 0\\N10 10 0 10");
    }

    #[test]
    fn drawing_mode_keeps_raw_ascii_whitespace_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed =
            parse_dialogue_text("{\\p1}m\t0\t0\nl\t10\t0\t10\t10\t0\t10", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(
            parsed.lines[0].spans[0].text,
            "m\t0\t0\nl\t10\t0\t10\t10\t0\t10"
        );
        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing should keep parsing across raw whitespace");
        assert_eq!(drawing.polygons.len(), 1);
    }

    #[test]
    fn drawing_mode_keeps_unmatched_open_brace_as_path_text_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}m 0 0 l 10 0 { l 10 10 l 0 10", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(
            parsed.lines[0].spans[0].text,
            "m 0 0 l 10 0 { l 10 10 l 0 10"
        );
        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("unmatched brace should stay in drawing text");
        assert_eq!(drawing.polygons.len(), 1);
        assert_eq!(drawing.bounds().expect("bounds").x_max, 11);
        assert_eq!(drawing.bounds().expect("bounds").y_max, 11);
    }

    #[test]
    fn drawing_mode_splits_object_at_override_block_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\p1}m 0 0 l 10 0 10 10 0 10{\\pos(20,20)}m 20 20 l 30 20 30 30 20 30",
            &base_style,
            &[],
        );

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 2);
        assert_eq!(parsed.lines[0].spans[0].text, "m 0 0 l 10 0 10 10 0 10");
        assert_eq!(parsed.lines[0].spans[1].text, "m 20 20 l 30 20 30 30 20 30");
        assert!(parsed.lines[0].spans[0].drawing.is_some());
        assert!(parsed.lines[0].spans[1].drawing.is_some());
        assert_eq!(parsed.position_exact, Some((20.0, 20.0)));
    }

    #[test]
    fn drawing_scale_with_nonpositive_libass_base_collapses_geometry() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p32}m 0 0 l 64 0 64 64 0 64", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span is still an object replacement");
        assert_eq!(drawing.scale, 32);
        assert!(
            drawing.polygons.is_empty(),
            "libass lshiftwrapi makes \\p32 use a non-positive scale base"
        );
    }

    #[test]
    fn parses_drawing_spans_in_p_mode() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}m 0 0 l 10 0 10 10 0 10", &base_style, &[]);

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(drawing.scale, 1);
        assert_eq!(drawing.polygons.len(), 1);
        assert_eq!(
            drawing.bounds(),
            Some(Rect {
                x_min: 0,
                y_min: 0,
                x_max: 11,
                y_max: 11
            })
        );
    }

    #[test]
    fn parses_bezier_drawing_spans_in_p_mode() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\p1}m 0 0 b 10 0 10 10 0 10", &base_style, &[]);

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(drawing.polygons.len(), 1);
        assert!(drawing.polygons[0].len() > 4);
        assert_eq!(
            drawing.polygons[0].first().copied(),
            Some(Point { x: 0, y: 0 })
        );
        assert_eq!(
            drawing.polygons[0].last().copied(),
            Some(Point { x: 0, y: 10 })
        );
    }

    #[test]
    fn drawing_bbox_retains_raw_curve_control_points_like_libass() {
        let text = "m 0 0 b 0 100 10 100 10 0";
        let cbox = parse_drawing_bbox_d6(text, 1).expect("raw drawing bbox");
        assert_eq!(
            cbox,
            Rect {
                x_min: 0,
                y_min: 0,
                x_max: 640,
                y_max: 6_400,
            }
        );

        let flattened = parse_drawing_polygons_d6(text, 1).expect("flattened drawing");
        let flattened_y_max = flattened
            .iter()
            .flatten()
            .map(|point| point.y)
            .max()
            .expect("curve samples");
        assert_eq!(flattened_y_max, 4_800);
        assert_ne!(flattened_y_max, cbox.y_max);
    }

    #[test]
    fn drawing_outline_cbox_uses_converted_spline_controls_like_libass() {
        let text = "m 0 0 s 0 100 10 100 10 0";
        assert_eq!(
            parse_drawing_bbox_d6(text, 1),
            Some(Rect {
                x_min: 0,
                y_min: 0,
                x_max: 640,
                y_max: 6_400,
            })
        );
        assert_eq!(
            parse_drawing_outline_cbox_d6(text, 1),
            Some(Rect {
                x_min: 106,
                y_min: 5_333,
                x_max: 533,
                y_max: 6_400,
            })
        );
    }

    #[test]
    fn drawing_d6_conversion_uses_nearest_even_at_half_steps() {
        assert_eq!(libass_drawing_coordinate_to_d6(1.0 / 128.0), Some(0));
        assert_eq!(libass_drawing_coordinate_to_d6(3.0 / 128.0), Some(2));
        assert_eq!(libass_drawing_coordinate_to_d6(-1.0 / 128.0), Some(0));
        assert_eq!(libass_drawing_coordinate_to_d6(-3.0 / 128.0), Some(-2));
    }

    #[test]
    fn parses_spline_drawing_spans_in_p_mode() {
        let base_style = ParsedStyle::default();
        let parsed =
            parse_dialogue_text("{\\p1}m 0 0 s 10 0 10 10 0 10 p -5 5 c", &base_style, &[]);

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(drawing.polygons.len(), 1);
        assert!(drawing.polygons[0].len() > 8);
    }

    #[test]
    fn parses_non_closing_move_drawing_spans_in_p_mode() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\p1}m 0 0 l 10 0 10 10 0 10 n 20 20 l 30 20 30 30 20 30",
            &base_style,
            &[],
        );

        let drawing = parsed.lines[0].spans[0]
            .drawing
            .as_ref()
            .expect("drawing span");
        assert_eq!(drawing.polygons.len(), 1);
        assert_eq!(
            drawing.polygons[0].first().copied(),
            Some(Point { x: 0, y: 0 })
        );
        assert_eq!(
            drawing.polygons[0].last().copied(),
            Some(Point { x: 20, y: 30 })
        );
    }

    #[test]
    fn parses_timed_transform_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(100,300,2,\\1c&H112233&\\fs48\\fscx150\\fscy50\\fsp4\\bord6\\blur2\\be0.6)}Text",
            &base_style,
            &[],
        );

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].start_ms, 100);
        assert_eq!(transforms[0].end_ms, Some(300));
        assert_eq!(transforms[0].accel, 2.0);
        assert_eq!(transforms[0].style.font_size, Some(48.0));
        assert_eq!(transforms[0].style.scale_x, Some(1.5));
        assert_eq!(transforms[0].style.scale_y, Some(0.5));
        assert_eq!(transforms[0].style.spacing, Some(4.0));
        assert_eq!(transforms[0].style.primary_colour, Some(0x0011_2233));
        assert_eq!(transforms[0].style.border, Some(6.0));
        assert_eq!(transforms[0].style.blur, Some(2.0));
        assert_eq!(transforms[0].style.be, Some(0.6));
    }

    #[test]
    fn parses_decimal_transform_timings_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(53.571428571429,107.14285714286,\\frz4)}Text",
            &base_style,
            &[],
        );

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].start_ms, 53);
        assert_eq!(transforms[0].end_ms, Some(107));
        assert_eq!(transforms[0].style.rotation_z, Some(4.0));
    }

    #[test]
    fn transform_timing_conversions_follow_libass_argument_count() {
        let base_style = ParsedStyle::default();
        let two_arg_float = parse_dialogue_text("{\\t(1e999,2e999,\\frz4)}Text", &base_style, &[]);
        let three_arg_integer =
            parse_dialogue_text("{\\t(1e3,2e3,1,\\frz4)}Text", &base_style, &[]);

        let transform = &two_arg_float.lines[0].spans[0].transforms[0];
        assert_eq!(transform.start_ms, i32::MIN);
        assert_eq!(transform.end_ms, Some(i32::MIN));
        assert_eq!(transform.style.rotation_z, Some(4.0));

        let transform = &three_arg_integer.lines[0].spans[0].transforms[0];
        assert_eq!(transform.start_ms, 1);
        assert_eq!(transform.end_ms, Some(2));
        assert_eq!(transform.accel, 1.0);
        assert_eq!(transform.style.rotation_z, Some(4.0));
    }

    #[test]
    fn transform_preserves_negative_times_zero_accel_and_unclosed_args() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\t(-100,100,0,\\fscx10\\bord-3}Text", &base_style, &[]);

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 1);
        assert_eq!(transforms[0].start_ms, -100);
        assert_eq!(transforms[0].end_ms, Some(100));
        assert_eq!(transforms[0].accel, 0.0);
        assert_eq!(transforms[0].style.scale_x, Some(0.1));
        assert_eq!(transforms[0].style.border, Some(-3.0));
    }

    #[test]
    fn transform_rejects_extra_timing_arguments_like_libass() {
        let base_style = ParsedStyle {
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text("{\\t(0,100,1,2,\\fs40)}Text", &base_style, &[]);

        let span = &parsed.lines[0].spans[0];
        assert!(span.transforms.is_empty());
        assert_eq!(span.style.font_size, 20.0);
    }

    #[test]
    fn transform_font_size_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle {
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let repeated = parse_dialogue_text("{\\t(0,100,\\fs+5\\fs+5)}Text", &base_style, &[]);
        let reset = parse_dialogue_text("{\\fs40\\t(100,200,\\fs)}Text", &base_style, &[]);

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.font_size, None);
        assert_eq!(
            repeated_style.font_size_steps,
            vec![
                ParsedFontSizeTransform::Relative {
                    value: 5.0,
                    reset: 20.0,
                },
                ParsedFontSizeTransform::Relative {
                    value: 5.0,
                    reset: 20.0,
                },
            ]
        );

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.font_size, None);
        assert_eq!(
            reset_style.font_size_steps,
            vec![ParsedFontSizeTransform::Reset { reset: 20.0 }]
        );
    }

    #[test]
    fn transform_scale_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle {
            scale_x: 1.2,
            scale_y: 0.8,
            ..ParsedStyle::default()
        };
        let repeated = parse_dialogue_text(
            "{\\t(0,100,\\fscx200\\fscx300\\fscy50\\fscy25)}Text",
            &base_style,
            &[],
        );
        let reset = parse_dialogue_text(
            "{\\fscx400\\fscy500\\t(100,200,\\fsc)}Text",
            &base_style,
            &[],
        );

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.scale_x, None);
        assert_eq!(
            repeated_style.scale_x_steps,
            vec![
                ParsedScaleTransform::Absolute {
                    value: 2.0,
                    reset: 1.2,
                },
                ParsedScaleTransform::Absolute {
                    value: 3.0,
                    reset: 1.2,
                },
            ]
        );
        assert_eq!(repeated_style.scale_y, None);
        assert_eq!(
            repeated_style.scale_y_steps,
            vec![
                ParsedScaleTransform::Absolute {
                    value: 0.5,
                    reset: 0.8,
                },
                ParsedScaleTransform::Absolute {
                    value: 0.25,
                    reset: 0.8,
                },
            ]
        );

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.scale_x, None);
        assert_eq!(
            reset_style.scale_x_steps,
            vec![ParsedScaleTransform::Reset { reset: 1.2 }]
        );
        assert_eq!(reset_style.scale_y, None);
        assert_eq!(
            reset_style.scale_y_steps,
            vec![ParsedScaleTransform::Reset { reset: 0.8 }]
        );
    }

    #[test]
    fn transform_spacing_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle {
            spacing: 4.0,
            ..ParsedStyle::default()
        };
        let repeated = parse_dialogue_text("{\\t(0,100,\\fsp8\\fsp12)}Text", &base_style, &[]);
        let reset = parse_dialogue_text("{\\fsp20\\t(100,200,\\fsp)}Text", &base_style, &[]);

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.spacing, None);
        assert_eq!(
            repeated_style.spacing_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 8.0,
                    reset: 4.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 12.0,
                    reset: 4.0,
                },
            ]
        );

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.spacing, None);
        assert_eq!(
            reset_style.spacing_steps,
            vec![ParsedLinearTransform::Reset { reset: 4.0 }]
        );
    }

    #[test]
    fn transform_rotation_shear_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle {
            angle: 15.0,
            ..ParsedStyle::default()
        };
        let repeated = parse_dialogue_text(
            "{\\t(0,100,\\frx10\\frx20\\fry-5\\fry5\\frz30\\frz60\\fax0.2\\fax0.3\\fay0.4\\fay0.5)}Text",
            &base_style,
            &[],
        );
        let reset = parse_dialogue_text(
            "{\\frx9\\fry8\\frz90\\fax0.2\\fay0.3\\t(100,200,\\frx\\fry\\frz\\fax\\fay)}Text",
            &base_style,
            &[],
        );

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.rotation_x, None);
        assert_eq!(
            repeated_style.rotation_x_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 10.0,
                    reset: 0.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 20.0,
                    reset: 0.0,
                },
            ]
        );
        assert_eq!(repeated_style.rotation_y, None);
        assert_eq!(
            repeated_style.rotation_y_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: -5.0,
                    reset: 0.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 5.0,
                    reset: 0.0,
                },
            ]
        );
        assert_eq!(repeated_style.rotation_z, None);
        assert_eq!(
            repeated_style.rotation_z_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 30.0,
                    reset: 15.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 60.0,
                    reset: 15.0,
                },
            ]
        );
        assert_eq!(repeated_style.shear_x, None);
        assert_eq!(
            repeated_style.shear_x_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 0.2,
                    reset: 0.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 0.3,
                    reset: 0.0,
                },
            ]
        );
        assert_eq!(repeated_style.shear_y, None);
        assert_eq!(
            repeated_style.shear_y_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 0.4,
                    reset: 0.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 0.5,
                    reset: 0.0,
                },
            ]
        );

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.rotation_x, None);
        assert_eq!(
            reset_style.rotation_x_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(reset_style.rotation_y, None);
        assert_eq!(
            reset_style.rotation_y_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(reset_style.rotation_z, None);
        assert_eq!(
            reset_style.rotation_z_steps,
            vec![ParsedLinearTransform::Reset { reset: 15.0 }]
        );
        assert_eq!(reset_style.shear_x, None);
        assert_eq!(
            reset_style.shear_x_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(reset_style.shear_y, None);
        assert_eq!(
            reset_style.shear_y_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
    }

    #[test]
    fn transform_blur_be_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle::default();
        let repeated = parse_dialogue_text(
            "{\\t(0,100,\\blur20\\blur40\\be2\\be4)}Text",
            &base_style,
            &[],
        );
        let reset = parse_dialogue_text(
            "{\\blur5\\be6\\t(100,200,\\blur\\be)}Text",
            &base_style,
            &[],
        );

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.blur, None);
        assert_eq!(
            repeated_style.blur_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 20.0,
                    reset: 0.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 40.0,
                    reset: 0.0,
                },
            ]
        );
        assert_eq!(repeated_style.be, None);
        assert_eq!(
            repeated_style.be_steps,
            vec![
                ParsedLinearTransform::Absolute {
                    value: 2.0,
                    reset: 0.0,
                },
                ParsedLinearTransform::Absolute {
                    value: 4.0,
                    reset: 0.0,
                },
            ]
        );

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.blur, None);
        assert_eq!(
            reset_style.blur_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(reset_style.be, None);
        assert_eq!(
            reset_style.be_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
    }

    #[test]
    fn transform_border_shadow_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle {
            outline: 2.0,
            shadow: 3.0,
            ..ParsedStyle::default()
        };
        let repeated = parse_dialogue_text(
            "{\\t(0,100,\\bord8\\xbord-20\\ybord6\\shad-5\\xshad-2\\yshad7)}Text",
            &base_style,
            &[],
        );
        let reset = parse_dialogue_text(
            "{\\bord9\\shad10\\t(100,200,\\bord\\shad)}Text",
            &base_style,
            &[],
        );

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.border, None);
        assert_eq!(repeated_style.border_x, None);
        assert_eq!(
            repeated_style.border_x_steps,
            vec![
                ParsedAxisTransform::Absolute {
                    value: 8.0,
                    reset: 2.0,
                    clamp: true,
                },
                ParsedAxisTransform::Absolute {
                    value: -20.0,
                    reset: 2.0,
                    clamp: true,
                },
            ]
        );
        assert_eq!(repeated_style.border_y, None);
        assert_eq!(
            repeated_style.border_y_steps,
            vec![
                ParsedAxisTransform::Absolute {
                    value: 8.0,
                    reset: 2.0,
                    clamp: true,
                },
                ParsedAxisTransform::Absolute {
                    value: 6.0,
                    reset: 2.0,
                    clamp: true,
                },
            ]
        );
        assert_eq!(repeated_style.shadow, None);
        assert_eq!(repeated_style.shadow_x, None);
        assert_eq!(
            repeated_style.shadow_x_steps,
            vec![
                ParsedAxisTransform::Absolute {
                    value: -5.0,
                    reset: 3.0,
                    clamp: true,
                },
                ParsedAxisTransform::Absolute {
                    value: -2.0,
                    reset: 3.0,
                    clamp: false,
                },
            ]
        );
        assert_eq!(repeated_style.shadow_y, None);
        assert_eq!(
            repeated_style.shadow_y_steps,
            vec![
                ParsedAxisTransform::Absolute {
                    value: -5.0,
                    reset: 3.0,
                    clamp: true,
                },
                ParsedAxisTransform::Absolute {
                    value: 7.0,
                    reset: 3.0,
                    clamp: false,
                },
            ]
        );

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.border, None);
        assert_eq!(reset_style.border_x, None);
        assert_eq!(
            reset_style.border_x_steps,
            vec![ParsedAxisTransform::Reset { reset: 2.0 }]
        );
        assert_eq!(reset_style.border_y, None);
        assert_eq!(
            reset_style.border_y_steps,
            vec![ParsedAxisTransform::Reset { reset: 2.0 }]
        );
        assert_eq!(reset_style.shadow, None);
        assert_eq!(reset_style.shadow_x, None);
        assert_eq!(
            reset_style.shadow_x_steps,
            vec![ParsedAxisTransform::Reset { reset: 3.0 }]
        );
        assert_eq!(reset_style.shadow_y, None);
        assert_eq!(
            reset_style.shadow_y_steps,
            vec![ParsedAxisTransform::Reset { reset: 3.0 }]
        );
    }

    #[test]
    fn transform_colour_steps_preserve_ordered_libass_semantics() {
        let base_style = ParsedStyle {
            primary_colour: 0x2010_2030,
            secondary_colour: 0x3040_5060,
            outline_colour: 0x5060_7080,
            back_colour: 0x7080_90a0,
            ..ParsedStyle::default()
        };
        let repeated = parse_dialogue_text(
            "{\\t(0,100,\\1c&H000000&\\1c&HFFFFFF&\\1a&H80&\\1a&H40&\\alpha&H20&)}Text",
            &base_style,
            &[],
        );
        let reset = parse_dialogue_text(
            "{\\1c&H000000&\\1a&HFF&\\t(100,200,\\1c\\1a\\alpha)}Text",
            &base_style,
            &[],
        );

        let repeated_style = &repeated.lines[0].spans[0].transforms[0].style;
        assert_eq!(repeated_style.primary_colour, None);
        assert_eq!(
            repeated_style.primary_colour_steps,
            vec![
                ParsedColourTransform::Rgb { value: 0x000000 },
                ParsedColourTransform::Rgb { value: 0xFFFFFF },
                ParsedColourTransform::Alpha { value: 0x80 },
                ParsedColourTransform::Alpha { value: 0x40 },
                ParsedColourTransform::Alpha { value: 0x20 },
            ]
        );
        assert_eq!(repeated_style.secondary_colour, Some(0x2040_5060));
        assert!(repeated_style.secondary_colour_steps.is_empty());
        assert_eq!(repeated_style.outline_colour, Some(0x2060_7080));
        assert!(repeated_style.outline_colour_steps.is_empty());
        assert_eq!(repeated_style.back_colour, Some(0x2080_90a0));
        assert!(repeated_style.back_colour_steps.is_empty());

        let reset_style = &reset.lines[0].spans[0].transforms[0].style;
        assert_eq!(reset_style.primary_colour, None);
        assert_eq!(
            reset_style.primary_colour_steps,
            vec![
                ParsedColourTransform::ResetRgb { reset: 0x2010_2030 },
                ParsedColourTransform::ResetAlpha { reset: 0x20 },
                ParsedColourTransform::ResetAlpha { reset: 0x20 },
            ]
        );
        assert_eq!(reset_style.secondary_colour, None);
        assert_eq!(
            reset_style.secondary_colour_steps,
            vec![ParsedColourTransform::ResetAlpha { reset: 0x30 }]
        );
        assert_eq!(reset_style.outline_colour, None);
        assert_eq!(
            reset_style.outline_colour_steps,
            vec![ParsedColourTransform::ResetAlpha { reset: 0x50 }]
        );
        assert_eq!(reset_style.back_colour, None);
        assert_eq!(
            reset_style.back_colour_steps,
            vec![ParsedColourTransform::ResetAlpha { reset: 0x70 }]
        );
    }

    #[test]
    fn transform_records_explicit_target_equal_to_static_style() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\frz0\\t(0,100,\\frz4)\\t(0,200,\\frz0)}Text",
            &base_style,
            &[],
        );

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 2);
        assert_eq!(transforms[0].style.rotation_z, Some(4.0));
        assert_eq!(transforms[1].style.rotation_z, Some(0.0));
    }

    #[test]
    fn transform_immediate_tags_apply_like_libass_recursive_parser() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            italic: true,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\t(1000,2000,\\fnAlt\\fe2\\b1\\i0\\u1\\s1\\q3\\an7\\pos(10,20)\\fad(100,200)\\pbo5)}Side",
            &base_style,
            &[],
        );
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_name, "Alt");
        assert_eq!(style.encoding, 2);
        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
        assert!(!style.italic);
        assert!(style.underline);
        assert!(style.strike_out);
        assert_eq!(style.pbo, 5.0);
        assert_eq!(parsed.wrap_style, Some(3));
        assert_eq!(parsed.alignment, Some(ass::VALIGN_TOP | ass::HALIGN_LEFT));
        assert_eq!(parsed.position_exact, Some((10.0, 20.0)));
        assert_eq!(
            parsed.fade,
            Some(ParsedFade::Simple {
                fade_in_ms: 100,
                fade_out_ms: 200,
            })
        );
        assert!(parsed.lines[0].spans[0].transforms.is_empty());
    }

    #[test]
    fn transform_p_tag_switches_drawing_mode_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "Text{\\t(0,100,\\p1)}m 0 0 l 10 0 10 10 0 10",
            &base_style,
            &[],
        );
        let spans = &parsed.lines[0].spans;

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "Text");
        assert!(spans[0].drawing.is_none());
        assert_eq!(spans[1].text, "m 0 0 l 10 0 10 10 0 10");
        assert!(
            spans[1].drawing.is_some(),
            "libass applies \\p inside \\t immediately, so following text is drawing data"
        );
    }

    #[test]
    fn transform_karaoke_tags_apply_like_libass_recursive_parser() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\k10)}Ka{\\t(0,100,\\kt)\\t(0,100,\\K20)}Ra",
            &base_style,
            &[],
        );
        let spans = &parsed.lines[0].spans;

        assert_eq!(spans.len(), 2);
        assert_eq!(
            spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 200,
                mode: ParsedKaraokeMode::Sweep,
            })
        );
    }

    #[test]
    fn transform_kt_retimes_unconsumed_karaoke_word_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\t(0,100,\\k10\\kt50)}Ka", &base_style, &[]);
        let spans = &parsed.lines[0].spans;

        assert_eq!(
            spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 500,
                duration_ms: 0,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
    }

    #[test]
    fn kt_after_text_defers_until_next_run_break_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\K100}A{\\kt50}B{\\b1}C", &base_style, &[]);
        let spans = &parsed.lines[0].spans;

        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "AB");
        assert_eq!(
            spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 1000,
                mode: ParsedKaraokeMode::Sweep,
            })
        );
        assert_eq!(spans[1].text, "C");
        assert_eq!(
            spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 500,
                duration_ms: 0,
                mode: ParsedKaraokeMode::Sweep,
            })
        );
    }

    #[test]
    fn transform_reset_tag_applies_immediately_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base".to_string(),
            font_weight: 400,
            bold: false,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_name: "AltFont".to_string(),
            font_weight: 700,
            bold: true,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fnTemp\\b1\\t(0,100,\\rAlt)\\fn0}Reset",
            &base_style,
            &[alt_style],
        );
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_name, "AltFont");
        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
    }

    #[test]
    fn transform_reset_tag_cancels_earlier_animated_targets_like_libass() {
        let base_style = ParsedStyle {
            name: "Default".to_string(),
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_size: 30.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\fs40\\rAlt)}Reset",
            &base_style,
            &[base_style.clone(), alt_style.clone()],
        );
        let span = &parsed.lines[0].spans[0];

        assert_eq!(span.style.font_size, 30.0);
        assert!(span.transforms.is_empty());

        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\fs40\\rAlt\\bord7)}Reset",
            &base_style,
            &[base_style.clone(), alt_style],
        );
        let span = &parsed.lines[0].spans[0];

        assert_eq!(span.style.font_size, 30.0);
        assert_eq!(span.transforms.len(), 1);
        assert_eq!(span.transforms[0].style.font_size, None);
        assert_eq!(span.transforms[0].style.border, Some(7.0));
    }

    #[test]
    fn transform_animatable_tags_do_not_trigger_immediate_prefix_handlers() {
        let base_style = ParsedStyle {
            bold: true,
            font_weight: 700,
            strike_out: true,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\blur3\\bord4\\shad5\\alpha&H80&)}Anim",
            &base_style,
            &[],
        );
        let style = &parsed.lines[0].spans[0].style;
        let transform = &parsed.lines[0].spans[0].transforms[0].style;

        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
        assert!(style.strike_out);
        assert_eq!(transform.blur, Some(3.0));
        assert_eq!(transform.border, Some(4.0));
        assert_eq!(transform.shadow, Some(5.0));
        assert_eq!(
            transform.primary_colour,
            Some(with_alpha(base_style.primary_colour, 0x80))
        );
    }

    #[test]
    fn style_run_break_starts_implicit_karaoke_word_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k50}ab{\\b1}cd", &base_style, &[]);

        let spans = &parsed.lines[0].spans;
        assert_eq!(spans.len(), 2);
        let first = spans[0].karaoke.expect("first span carries karaoke");
        let second = spans[1]
            .karaoke
            .expect("style run inherits the karaoke mode");
        assert_eq!(first.start_ms, 0);
        assert_eq!(first.duration_ms, 500);
        assert_eq!(second.start_ms, 500);
        assert_eq!(second.duration_ms, 0);
        assert_eq!(second.mode, first.mode);
    }

    #[test]
    fn fast_hard_override_scan_matches_libass_block_and_escape_rules() {
        assert!(dialogue_has_libass_hard_override("{\\pos(1,2)}text"));
        assert!(dialogue_has_libass_hard_override("{\\clip(0,0,1,1)"));
        assert!(dialogue_has_libass_hard_override("{\\pbo2}text"));
        assert!(!dialogue_has_libass_hard_override("\\pos(1,2) text"));
        assert!(!dialogue_has_libass_hard_override("{pos(1,2)}text"));
        assert!(!dialogue_has_libass_hard_override("\\{\\pos(1,2)}text"));
        assert!(!dialogue_has_libass_hard_override("{\\bord2}text"));
    }

    #[test]
    fn official_runsplit_fixture_advances_each_implicit_karaoke_word() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\k162}hodie{\\i1}que{\\r} |{\\k118}{\\board1\\c&HFF9920&}cael{\\b1\\u1}um{\\r} |{\\k24}est |{\\k156}{\\board1\\c&HFF9920&}candid{\\b1\\u1}um",
            &base_style,
            &[],
        );

        let final_word = parsed.lines[0]
            .spans
            .iter()
            .rev()
            .find(|span| span.text == "um")
            .and_then(|span| span.karaoke)
            .expect("final implicit karaoke word");
        assert_eq!(
            final_word,
            ParsedKaraokeSpan {
                start_ms: 4600,
                duration_ms: 0,
                mode: ParsedKaraokeMode::FillSwap,
            }
        );
    }

    #[test]
    fn zero_duration_karaoke_without_run_break_stays_in_current_word_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k100}A{\\k0}B{\\k50}C", &base_style, &[]);

        let spans = &parsed.lines[0].spans;
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].text, "AB");
        assert_eq!(spans[1].text, "C");
        let ab = spans[0].karaoke.expect("AB karaoke");
        let c = spans[1].karaoke.expect("C karaoke");
        assert_eq!((ab.start_ms, ab.duration_ms), (0, 1000));
        assert_eq!((c.start_ms, c.duration_ms), (1000, 500));
    }

    #[test]
    fn zero_duration_karaoke_applies_when_style_breaks_before_next_glyph_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k100}A{\\k0\\b1}B", &base_style, &[]);

        let spans = &parsed.lines[0].spans;
        assert_eq!(spans.len(), 2);
        let a = spans[0].karaoke.expect("A karaoke");
        let b = spans[1].karaoke.expect("B karaoke");
        assert_eq!((a.start_ms, a.duration_ms), (0, 1000));
        assert_eq!((b.start_ms, b.duration_ms), (1000, 0));
    }

    #[test]
    fn transform_clip_tag_is_not_misclassified_as_colour() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\t(0,100,\\clip(0,0,10,10))}Text", &base_style, &[]);

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(
            transforms.len(),
            1,
            "an animated rect clip parses as a transform target"
        );
        let style = &transforms[0].style;
        assert_eq!(
            style.clip_rect,
            Some(ParsedRectF64 {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 10.0,
                y_max: 10.0,
            }),
            "the \\t carries the clip rect target"
        );
        assert!(
            style.primary_colour.is_none(),
            "\\clip inside \\t must not be misclassified as a \\c colour reset"
        );
    }

    #[test]
    fn transform_iclip_tag_preserves_inverse_mode() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\t(0,100,\\iclip(0,0,10,10))}Text", &base_style, &[]);

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 1);
        let style = &transforms[0].style;
        assert_eq!(
            style.clip_rect,
            Some(ParsedRectF64 {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 10.0,
                y_max: 10.0,
            })
        );
        assert_eq!(style.clip_inverse, Some(true));
        assert!(style.primary_colour.is_none());
    }

    #[test]
    fn transform_rect_clip_coordinates_use_libass_integer_arguments() {
        let base_style = ParsedStyle::default();
        let parsed =
            parse_dialogue_text("{\\t(0,100,\\clip(1.9,abc,30.9,zz))}Text", &base_style, &[]);

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 1);
        let style = &transforms[0].style;
        assert_eq!(
            style.clip_rect,
            Some(ParsedRectF64 {
                x_min: 1.0,
                y_min: 0.0,
                x_max: 30.0,
                y_max: 0.0,
            })
        );
        assert_eq!(style.clip_inverse, Some(false));
    }

    #[test]
    fn transform_vector_clip_tags_apply_like_libass_side_effects() {
        let base_style = ParsedStyle::default();
        let clipped = parse_dialogue_text(
            "{\\t(0,100,\\clip(m 0 0 l 10 0 10 10 0 10))}Text",
            &base_style,
            &[],
        );
        let inverse = parse_dialogue_text(
            "{\\t(0,100,\\iclip(m 0 0 l 10 0 10 10 0 10))}Text",
            &base_style,
            &[],
        );

        assert!(clipped.vector_clip.is_some());
        assert!(!clipped.inverse_clip);
        assert!(clipped.lines[0].spans[0].transforms.is_empty());
        assert!(inverse.vector_clip.is_some());
        assert!(inverse.inverse_clip);
        assert!(inverse.lines[0].spans[0].transforms.is_empty());
    }

    #[test]
    fn parses_z_rotation_overrides_and_transforms() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\frz15\\t(0,1000,\\frz45)}Text", &base_style, &[]);

        let span = &parsed.lines[0].spans[0];
        assert_eq!(span.style.rotation_z, 15.0);
        assert_eq!(span.transforms.len(), 1);
        assert_eq!(span.transforms[0].style.rotation_z, Some(45.0));
    }

    #[test]
    fn parses_nested_transform_tags_in_libass_order() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\frz4\\t(100,200,\\frz-4\\t(200,300,\\frz4)))}Text",
            &base_style,
            &[],
        );

        let transforms = &parsed.lines[0].spans[0].transforms;
        assert_eq!(transforms.len(), 3);
        assert_eq!(transforms[0].start_ms, 0);
        assert_eq!(transforms[0].end_ms, Some(100));
        assert_eq!(transforms[0].style.rotation_z, Some(4.0));
        assert_eq!(transforms[1].start_ms, 100);
        assert_eq!(transforms[1].end_ms, Some(200));
        assert_eq!(transforms[1].style.rotation_z, Some(-4.0));
        assert_eq!(transforms[2].start_ms, 200);
        assert_eq!(transforms[2].end_ms, Some(300));
        assert_eq!(transforms[2].style.rotation_z, Some(4.0));
    }

    #[test]
    fn later_override_removes_same_field_from_active_transform() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(1000,3000,\\1c&H0000FF&\\frz45\\bord8)\\1c&H00FF00&\\frz15}Text",
            &base_style,
            &[],
        );

        let span = &parsed.lines[0].spans[0];
        assert_eq!(span.style.primary_colour, 0x0000_ff00);
        assert_eq!(span.style.rotation_z, 15.0);
        assert_eq!(span.transforms.len(), 1);
        assert_eq!(span.transforms[0].style.primary_colour, None);
        assert_eq!(span.transforms[0].style.rotation_z, None);
        assert_eq!(span.transforms[0].style.border, Some(8.0));
    }

    #[test]
    fn later_colour_and_alpha_overrides_suppress_only_their_channels_like_libass() {
        let base_style = ParsedStyle {
            primary_colour: 0x0011_2233,
            secondary_colour: 0x0022_3344,
            ..ParsedStyle::default()
        };
        let colour_after_alpha = parse_dialogue_text(
            "{\\t(0,100,\\alpha&H80&)\\1c&H445566&}Text",
            &base_style,
            &[],
        );
        let alpha_after_colour =
            parse_dialogue_text("{\\t(0,100,\\1c&H445566&)\\1a&H80&}Text", &base_style, &[]);

        let style = &colour_after_alpha.lines[0].spans[0].transforms[0].style;
        assert_eq!(style.primary_colour, Some(0x8044_5566));
        assert_eq!(style.secondary_colour, Some(0x8022_3344));

        let style = &alpha_after_colour.lines[0].spans[0].transforms[0].style;
        assert_eq!(style.primary_colour, Some(0x8044_5566));
    }

    #[test]
    fn later_static_clip_removes_animated_clip_target_like_libass() {
        let base_style = ParsedStyle {
            font_size: 20.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\fs40\\clip(0,0,100,100))\\clip(10,10,20,20)}Text",
            &base_style,
            &[],
        );
        let span = &parsed.lines[0].spans[0];

        assert_eq!(
            parsed.clip_rect,
            Some(Rect {
                x_min: 10,
                y_min: 10,
                x_max: 20,
                y_max: 20,
            })
        );
        assert_eq!(span.transforms.len(), 1);
        assert_eq!(span.transforms[0].style.clip_rect, None);
        assert_eq!(span.transforms[0].style.clip_inverse, None);
        assert_eq!(span.transforms[0].style.font_size, Some(40.0));
    }

    #[test]
    fn later_axis_overrides_remove_aggregate_transform_axis_like_libass() {
        let base_style = ParsedStyle {
            outline: 2.0,
            shadow: 3.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\t(0,100,\\bord8\\shad9)\\xbord4\\yshad5}Text",
            &base_style,
            &[],
        );
        let transform = &parsed.lines[0].spans[0].transforms[0].style;

        assert_eq!(transform.border, None);
        assert_eq!(transform.border_x, None);
        assert_eq!(transform.border_y, Some(8.0));
        assert_eq!(transform.shadow, None);
        assert_eq!(transform.shadow_x, Some(9.0));
        assert_eq!(transform.shadow_y, None);
    }

    #[test]
    fn bare_override_tags_reset_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base Font".to_string(),
            blur: 5.0,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\fnOther\\fn0\\fax0.25\\fay-0.5\\blur3\\be4\\pbo7\\fax\\fay\\blur\\be\\pbo\\p1\\p}Reset",
            &base_style,
            &[],
        );
        let span = &parsed.lines[0].spans[0];

        assert_eq!(span.text, "Reset");
        assert!(span.drawing.is_none(), "bare \\p resets drawing mode");
        assert_eq!(span.style.font_name, "Base Font");
        assert_eq!(span.style.shear_x, 0.0);
        assert_eq!(span.style.shear_y, 0.0);
        assert_eq!(span.style.blur, 0.0);
        assert_eq!(span.style.be, 0.0);
        assert_eq!(span.style.pbo, 0.0);
    }

    #[test]
    fn bare_override_resets_after_r_use_active_style_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base Font".to_string(),
            font_size: 20.0,
            primary_colour: 0x8011_2233,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_name: "Alt Font".to_string(),
            font_size: 30.0,
            scale_x: 1.5,
            spacing: 2.0,
            angle: 15.0,
            outline: 5.0,
            shadow: 6.0,
            primary_colour: 0x4066_7788,
            font_weight: 700,
            bold: true,
            italic: true,
            underline: true,
            strike_out: true,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\rAlt\\fnTemp\\fn0\\fs99\\fs\\fscx999\\fscx\\fsp9\\fsp\\frz99\\frz\\bord9\\bord\\shad9\\shad\\1c&H000000&\\1c\\1a&HFF&\\1a\\b0\\b\\i0\\i\\u0\\u\\s0\\s}Reset",
            &base_style,
            &[alt_style],
        );
        let style = &parsed.lines[0].spans[0].style;

        assert_eq!(style.font_name, "Alt Font");
        assert_eq!(style.font_size, 30.0);
        assert_eq!(style.scale_x, 1.5);
        assert_eq!(style.spacing, 2.0);
        assert_eq!(style.rotation_z, 15.0);
        assert_eq!(style.border, 5.0);
        assert_eq!(style.shadow, 6.0);
        assert_eq!(style.primary_colour, 0x4066_7788);
        assert_eq!(style.font_weight, 700);
        assert!(style.bold);
        assert!(style.italic);
        assert!(style.underline);
        assert!(style.strike_out);
    }

    #[test]
    fn reset_tag_preserves_pbo_like_libass() {
        let base_style = ParsedStyle {
            font_name: "Base Font".to_string(),
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_name: "Alt Font".to_string(),
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\pbo7\\fnTemp\\rAlt}Reset",
            &base_style,
            &[base_style.clone(), alt_style],
        );
        let transformed = parse_dialogue_text("{\\t(0,100,\\pbo7\\r)}Reset", &base_style, &[]);

        let span = &parsed.lines[0].spans[0];
        assert_eq!(span.style.font_name, "Alt Font");
        assert_eq!(span.style.pbo, 7.0);
        assert_eq!(transformed.lines[0].spans[0].style.pbo, 7.0);
    }

    #[test]
    fn bare_kt_resets_karaoke_clock_like_libass() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text("{\\k10}A{\\kt\\k10}B", &base_style, &[]);
        let spans = &parsed.lines[0].spans;

        assert_eq!(
            spans[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            spans[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
    }

    #[test]
    fn transform_bare_tags_reset_to_active_style_like_libass() {
        let base_style = ParsedStyle {
            name: "Default".to_string(),
            font_size: 20.0,
            scale_x: 1.0,
            scale_y: 1.0,
            spacing: 0.0,
            angle: 10.0,
            outline: 2.0,
            shadow: 3.0,
            primary_colour: 0x2211_2233,
            ..ParsedStyle::default()
        };
        let alt_style = ParsedStyle {
            name: "Alt".to_string(),
            font_size: 30.0,
            scale_x: 1.5,
            scale_y: 0.5,
            spacing: 4.0,
            angle: 15.0,
            outline: 5.0,
            shadow: 6.0,
            primary_colour: 0x3322_3344,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\rAlt\\fs40\\fscx400\\fscy500\\fsp8\\frx9\\fry8\\frz90\\fax0.2\\fay0.3\\bord7\\xshad8\\yshad9\\blur3\\be5\\1a&H80&\\1c&H445566&\\t(0,100,\\fs\\fscx\\fscy\\fsp\\frx\\fry\\frz\\fax\\fay\\bord\\xshad\\yshad\\blur\\be\\1c\\1a)}T",
            &base_style,
            &[base_style.clone(), alt_style],
        );
        let transform = &parsed.lines[0].spans[0].transforms[0].style;

        assert_eq!(transform.font_size, None);
        assert_eq!(
            transform.font_size_steps,
            vec![ParsedFontSizeTransform::Reset { reset: 30.0 }]
        );
        assert_eq!(transform.scale_x, None);
        assert_eq!(
            transform.scale_x_steps,
            vec![ParsedScaleTransform::Reset { reset: 1.5 }]
        );
        assert_eq!(transform.scale_y, None);
        assert_eq!(
            transform.scale_y_steps,
            vec![ParsedScaleTransform::Reset { reset: 0.5 }]
        );
        assert_eq!(transform.spacing, None);
        assert_eq!(
            transform.spacing_steps,
            vec![ParsedLinearTransform::Reset { reset: 4.0 }]
        );
        assert_eq!(transform.rotation_x, None);
        assert_eq!(
            transform.rotation_x_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(transform.rotation_y, None);
        assert_eq!(
            transform.rotation_y_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(transform.rotation_z, None);
        assert_eq!(
            transform.rotation_z_steps,
            vec![ParsedLinearTransform::Reset { reset: 15.0 }]
        );
        assert_eq!(transform.shear_x, None);
        assert_eq!(
            transform.shear_x_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(transform.shear_y, None);
        assert_eq!(
            transform.shear_y_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(transform.border, None);
        assert_eq!(transform.border_x, None);
        assert_eq!(
            transform.border_x_steps,
            vec![ParsedAxisTransform::Reset { reset: 5.0 }]
        );
        assert_eq!(transform.border_y, None);
        assert_eq!(
            transform.border_y_steps,
            vec![ParsedAxisTransform::Reset { reset: 5.0 }]
        );
        assert_eq!(transform.shadow, None);
        assert_eq!(transform.shadow_x, None);
        assert_eq!(
            transform.shadow_x_steps,
            vec![ParsedAxisTransform::Reset { reset: 6.0 }]
        );
        assert_eq!(transform.shadow_y, None);
        assert_eq!(
            transform.shadow_y_steps,
            vec![ParsedAxisTransform::Reset { reset: 6.0 }]
        );
        assert_eq!(transform.blur, None);
        assert_eq!(
            transform.blur_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(transform.be, None);
        assert_eq!(
            transform.be_steps,
            vec![ParsedLinearTransform::Reset { reset: 0.0 }]
        );
        assert_eq!(transform.primary_colour, None);
        assert_eq!(
            transform.primary_colour_steps,
            vec![
                ParsedColourTransform::ResetRgb { reset: 0x3322_3344 },
                ParsedColourTransform::ResetAlpha { reset: 0x33 },
            ]
        );
    }

    #[test]
    fn parses_color_and_shadow_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\1c&H112233&\\4c&H445566&\\1a&H80&\\shad3.5\\blur1.5}Color",
            &base_style,
            &[],
        );

        assert_eq!(parsed.lines.len(), 1);
        assert_eq!(parsed.lines[0].spans.len(), 1);
        assert_eq!(parsed.lines[0].spans[0].style.primary_colour, 0x8011_2233);
        assert_eq!(parsed.lines[0].spans[0].style.back_colour, 0x8044_5566);
        assert_eq!(parsed.lines[0].spans[0].style.shadow, 3.5);
        assert_eq!(parsed.lines[0].spans[0].style.blur, 1.5);
    }

    #[test]
    fn parses_missing_override_metadata_tags() {
        let base_style = ParsedStyle {
            underline: false,
            strike_out: false,
            ..ParsedStyle::default()
        };
        let parsed = parse_dialogue_text(
            "{\\u1\\s1\\a10\\q2\\org(320,240)\\frx12\\fry-8\\fax0.25\\fay-0.5\\xbord3\\ybord4\\xshad5\\yshad-6\\be2\\pbo7}Meta",
            &base_style,
            &[],
        );

        assert_eq!(
            parsed.alignment,
            Some(ass::VALIGN_CENTER | ass::HALIGN_CENTER)
        );
        assert_eq!(parsed.wrap_style, Some(2));
        assert_eq!(parsed.origin, Some((320, 240)));
        let style = &parsed.lines[0].spans[0].style;
        assert!(style.underline);
        assert!(style.strike_out);
        assert_eq!(style.rotation_x, 12.0);
        assert_eq!(style.rotation_y, -8.0);
        assert_eq!(style.shear_x, 0.25);
        assert_eq!(style.shear_y, -0.5);
        assert_eq!(style.border_x, 3.0);
        assert_eq!(style.border_y, 4.0);
        assert_eq!(style.shadow_x, 5.0);
        assert_eq!(style.shadow_y, -6.0);
        assert_eq!(style.be, 2.0);
        assert_eq!(style.pbo, 7.0);
    }

    #[test]
    fn parses_font_attachments_from_fonts_section() {
        let encoded = encode_font_bytes(b"ABC");
        let input = format!(
            "[Fonts]\nfontname: DemoFont.ttf\n{encoded}\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1"
        );
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.attachments.len(), 1);
        assert_eq!(track.attachments[0].name, "DemoFont.ttf");
        assert_eq!(track.attachments[0].data, b"ABC");
    }

    #[test]
    fn font_attachments_preserve_libass_fontname_spacing() {
        let encoded = encode_font_bytes(b"A");
        let input = format!("[Fonts]\nfontname:\tDemoFont.ttf \n{encoded}");
        let track = parse_script_text(&input).expect("script should parse");

        assert_eq!(track.attachments.len(), 1);
        assert_eq!(track.attachments[0].name, "DemoFont.ttf ");
    }

    #[test]
    fn font_attachment_payload_lines_are_not_trimmed_like_libass() {
        let encoded = encode_font_bytes(b"ABC");
        let valid = format!("[Fonts]\nfontname: Valid.ttf\n{encoded}");
        let bad_size = format!("[Fonts]\nfontname: Bad.ttf\n{encoded} ");
        let valid = parse_script_text(&valid).expect("script should parse");
        let bad_size = parse_script_text(&bad_size).expect("script should parse");

        assert_eq!(valid.attachments.len(), 1);
        assert_eq!(valid.attachments[0].data, b"ABC");
        assert!(
            bad_size.attachments.is_empty(),
            "libass counts the trailing space in encoded font data, making this size invalid"
        );
    }

    #[test]
    fn font_attachment_payload_keeps_semicolon_lines_like_libass() {
        let input = "[Fonts]\nfontname: Semi.ttf\n;!";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.attachments.len(), 1);
        assert_eq!(track.attachments[0].name, "Semi.ttf");
        assert_eq!(track.attachments[0].data, vec![104]);
    }

    #[test]
    fn font_attachment_decoder_wraps_bytes_below_bang_like_libass() {
        let input = "[Fonts]\nfontname: Space.ttf\n; ";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.attachments.len(), 1);
        assert_eq!(track.attachments[0].name, "Space.ttf");
        assert_eq!(track.attachments[0].data, vec![107]);
    }

    fn encode_font_bytes(bytes: &[u8]) -> String {
        let mut encoded = String::new();
        for chunk in bytes.chunks(3) {
            let value = match chunk.len() {
                1 => u32::from(chunk[0]) << 16,
                2 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8),
                _ => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]),
            };
            let output_len = match chunk.len() {
                1 => 2,
                2 => 3,
                _ => 4,
            };
            for shift_index in 0..output_len {
                let shift = 6 * (3 - shift_index);
                let six_bits = ((value >> shift) & 63) as u8;
                encoded.push(char::from(six_bits + 33));
            }
        }
        encoded
    }
}
