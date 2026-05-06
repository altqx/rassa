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
            font_size: 20.0,
            primary_colour: 0x0000_00ff,
            secondary_colour: 0x0000_ffff,
            outline_colour: 0x0000_0000,
            back_colour: 0x0000_0000,
            bold: false,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: 1.0,
            scale_y: 1.0,
            spacing: 0.0,
            angle: 0.0,
            border_style: 1,
            outline: 2.0,
            shadow: 2.0,
            alignment: ass::VALIGN_SUB | ass::HALIGN_CENTER,
            margin_l: 10,
            margin_r: 10,
            margin_v: 10,
            encoding: 1,
            treat_fontname_as_pattern: 0,
            blur: 0.0,
            justify: ass::ASS_JUSTIFY_AUTO,
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
    pub scale_x: Option<f64>,
    pub scale_y: Option<f64>,
    pub spacing: Option<f64>,
    pub rotation_x: Option<f64>,
    pub rotation_y: Option<f64>,
    pub rotation_z: Option<f64>,
    pub shear_x: Option<f64>,
    pub shear_y: Option<f64>,
    pub primary_colour: Option<u32>,
    pub secondary_colour: Option<u32>,
    pub outline_colour: Option<u32>,
    pub back_colour: Option<u32>,
    pub border: Option<f64>,
    pub border_x: Option<f64>,
    pub border_y: Option<f64>,
    pub shadow: Option<f64>,
    pub shadow_x: Option<f64>,
    pub shadow_y: Option<f64>,
    pub blur: Option<f64>,
    pub be: Option<f64>,
}

impl ParsedAnimatedStyle {
    fn is_empty(&self) -> bool {
        self.font_size.is_none()
            && self.scale_x.is_none()
            && self.scale_y.is_none()
            && self.spacing.is_none()
            && self.rotation_x.is_none()
            && self.rotation_y.is_none()
            && self.rotation_z.is_none()
            && self.shear_x.is_none()
            && self.shear_y.is_none()
            && self.primary_colour.is_none()
            && self.secondary_colour.is_none()
            && self.outline_colour.is_none()
            && self.back_colour.is_none()
            && self.border.is_none()
            && self.border_x.is_none()
            && self.border_y.is_none()
            && self.shadow.is_none()
            && self.shadow_x.is_none()
            && self.shadow_y.is_none()
            && self.blur.is_none()
            && self.be.is_none()
    }

    fn clear_colours(&mut self) {
        self.primary_colour = None;
        self.secondary_colour = None;
        self.outline_colour = None;
        self.back_colour = None;
    }
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
    pub alignment: Option<i32>,
    pub position: Option<(i32, i32)>,
    pub movement: Option<ParsedMovement>,
    pub fade: Option<ParsedFade>,
    pub clip_rect: Option<Rect>,
    pub vector_clip: Option<ParsedVectorClip>,
    pub inverse_clip: bool,
    pub wrap_style: Option<i32>,
    pub origin: Option<(i32, i32)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParsedMovement {
    pub start: (i32, i32),
    pub end: (i32, i32),
    pub t1_ms: i32,
    pub t2_ms: i32,
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
            styles: Vec::new(),
            events: Vec::new(),
            attachments: Vec::new(),
            style_format: String::new(),
            event_format: String::new(),
            track_type: TrackType::Unknown,
            play_res_x: 384,
            play_res_y: 288,
            timer: 100.0,
            wrap_style: 0,
            scaled_border_and_shadow: true,
            kerning: true,
            language: String::new(),
            ycbcr_matrix: YCbCrMatrix::Default,
            default_style: 0,
            layout_res_x: 0,
            layout_res_y: 0,
        }
    }
}

pub fn parse_script_bytes(bytes: &[u8]) -> RassaResult<ParsedTrack> {
    parse_script_bytes_with_codepage(bytes, None)
}

pub fn parse_script_bytes_with_codepage(
    bytes: &[u8],
    codepage: Option<&str>,
) -> RassaResult<ParsedTrack> {
    if let Some(codepage) = codepage.filter(|value| !value.trim().is_empty()) {
        let text = iconv_native::decode(bytes, codepage).map_err(|error| {
            RassaError::new(format!(
                "failed to decode subtitle data from codepage {codepage:?}: {error}"
            ))
        })?;
        return parse_script_text(&text);
    }

    match std::str::from_utf8(bytes) {
        Ok(text) => parse_script_text(text),
        Err(_) => parse_script_text(&String::from_utf8_lossy(bytes)),
    }
}

pub fn parse_script_text(text: &str) -> RassaResult<ParsedTrack> {
    let mut track = ParsedTrack::default();
    let mut section = String::new();
    let mut style_format: Vec<String> = Vec::new();
    let mut event_format: Vec<String> = Vec::new();
    let mut pending_font_name: Option<String> = None;
    let mut pending_font_data = String::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_matches(|character| character == '\u{feff}' || character == '\r');
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            flush_font_attachment(&mut track, &mut pending_font_name, &mut pending_font_data);
            section.clear();
            section.push_str(&line[1..line.len() - 1].to_ascii_lowercase());
            if section == "v4+ styles" {
                track.track_type = TrackType::Ass;
            } else if section == "v4 styles" && track.track_type == TrackType::Unknown {
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

        let Some((key, value)) = split_once_colon(line) else {
            continue;
        };

        match section.as_str() {
            "script info" => apply_script_info_field(&mut track, key, value),
            "v4+ styles" | "v4 styles" => {
                if key.eq_ignore_ascii_case("Format") {
                    track.style_format = value.trim().to_string();
                    style_format = parse_format_fields(value);
                } else if key.eq_ignore_ascii_case("Style") {
                    if style_format.is_empty() {
                        style_format = default_style_format();
                        if track.style_format.is_empty() {
                            track.style_format = style_format.join(", ");
                        }
                    }
                    if let Some(style) = parse_style_line(value, &style_format) {
                        track.styles.push(style);
                    }
                }
            }
            "events" => {
                if key.eq_ignore_ascii_case("Format") {
                    track.event_format = value.trim().to_string();
                    event_format = parse_format_fields(value);
                } else if key.eq_ignore_ascii_case("Dialogue") {
                    if event_format.is_empty() {
                        event_format = default_event_format();
                        if track.event_format.is_empty() {
                            track.event_format = event_format.join(", ");
                        }
                    }
                    if let Some(event) = parse_event_line(
                        value,
                        &event_format,
                        track.events.len() as i32,
                        &track.styles,
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

    if track.style_format.is_empty() {
        track.style_format = default_style_format().join(", ");
    }
    if track.event_format.is_empty() {
        track.event_format = default_event_format().join(", ");
    }

    Ok(track)
}

fn process_font_line(
    line: &str,
    track: &mut ParsedTrack,
    pending_font_name: &mut Option<String>,
    pending_font_data: &mut String,
) {
    if let Some(name) = line.strip_prefix("fontname:") {
        flush_font_attachment(track, pending_font_name, pending_font_data);
        *pending_font_name = Some(name.trim().to_string());
        return;
    }

    if pending_font_name.is_some() {
        pending_font_data.push_str(line.trim());
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
    let encoded = encoded.trim();
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
        value |= u32::from(byte.saturating_sub(33) & 63) << (6 * (3 - index));
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
    let mut current_wrap_style = inherited_wrap_style.clamp(0, 3);
    let mut current_style = ParsedSpanStyle::from_style(base_style);
    let mut active_line = ParsedTextLine::default();
    let mut buffer = String::new();
    let mut pending_karaoke = None;
    let mut karaoke_cursor_ms = 0;
    let mut drawing_scale = 0;
    let mut current_transforms = Vec::new();
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '{' => {
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
                    &mut parsed,
                    &mut buffer,
                    &mut active_line,
                    &mut pending_karaoke,
                    &mut karaoke_cursor_ms,
                    &mut drawing_scale,
                    &mut current_transforms,
                    &mut current_wrap_style,
                );
            }
            '\\' => match characters.peek().copied() {
                Some('N') => {
                    characters.next();
                    if drawing_scale > 0 {
                        buffer.push(' ');
                    } else {
                        flush_span(
                            &mut buffer,
                            &current_style,
                            pending_karaoke,
                            drawing_scale,
                            &current_transforms,
                            &mut active_line,
                        );
                        push_line(&mut parsed, &mut active_line);
                    }
                }
                Some('n') => {
                    characters.next();
                    if drawing_scale > 0 || current_wrap_style != 2 {
                        buffer.push(' ');
                    } else {
                        flush_span(
                            &mut buffer,
                            &current_style,
                            pending_karaoke,
                            drawing_scale,
                            &current_transforms,
                            &mut active_line,
                        );
                        push_line(&mut parsed, &mut active_line);
                    }
                }
                Some('h') => {
                    characters.next();
                    buffer.push('\u{00A0}');
                }
                Some(next) => {
                    characters.next();
                    buffer.push('\\');
                    buffer.push(next);
                }
                None => buffer.push(character),
            },
            '\n' => {
                flush_span(
                    &mut buffer,
                    &current_style,
                    pending_karaoke,
                    drawing_scale,
                    &current_transforms,
                    &mut active_line,
                );
                push_line(&mut parsed, &mut active_line);
            }
            '\r' => {}
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
    push_line(&mut parsed, &mut active_line);
    if parsed.lines.is_empty() {
        parsed.lines.push(ParsedTextLine::default());
    }
    parsed
}

fn split_once_colon(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim_start()))
}

fn parse_format_fields(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|field| field.trim().to_string())
        .filter(|field| !field.is_empty())
        .collect()
}

fn default_style_format() -> Vec<String> {
    [
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
        "Blur",
        "Justify",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_event_format() -> Vec<String> {
    [
        "Layer", "Start", "End", "Style", "Name", "MarginL", "MarginR", "MarginV", "Effect", "Text",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn parse_style_line(value: &str, format: &[String]) -> Option<ParsedStyle> {
    let fields = split_fields(value, format.len());
    if fields.len() != format.len() {
        return None;
    }

    let mut style = ParsedStyle::default();
    for (key, raw_value) in format.iter().zip(fields) {
        let lowered = key.to_ascii_lowercase();
        match lowered.as_str() {
            "name" => style.name = raw_value.trim().to_string(),
            "fontname" => style.font_name = raw_value.trim().to_string(),
            "fontsize" => style.font_size = parse_f64(raw_value, style.font_size),
            "primarycolour" | "primarycolor" => {
                style.primary_colour = parse_color(raw_value, style.primary_colour)
            }
            "secondarycolour" | "secondarycolor" => {
                style.secondary_colour = parse_color(raw_value, style.secondary_colour)
            }
            "outlinecolour" | "outlinecolor" => {
                style.outline_colour = parse_color(raw_value, style.outline_colour)
            }
            "backcolour" | "backcolor" => {
                style.back_colour = parse_color(raw_value, style.back_colour)
            }
            "bold" => style.bold = parse_bold(raw_value, style.bold),
            "italic" => style.italic = parse_bool(raw_value, style.italic),
            "underline" => style.underline = parse_bool(raw_value, style.underline),
            "strikeout" => style.strike_out = parse_bool(raw_value, style.strike_out),
            "scalex" => style.scale_x = parse_scale(raw_value, style.scale_x),
            "scaley" => style.scale_y = parse_scale(raw_value, style.scale_y),
            "spacing" => style.spacing = parse_f64(raw_value, style.spacing),
            "angle" => style.angle = parse_f64(raw_value, style.angle),
            "borderstyle" => style.border_style = parse_i32(raw_value, style.border_style),
            "outline" => style.outline = parse_f64(raw_value, style.outline),
            "shadow" => style.shadow = parse_f64(raw_value, style.shadow),
            "alignment" => {
                let raw_alignment = parse_i32(raw_value, style.alignment);
                style.alignment = alignment_from_an(raw_alignment).unwrap_or(style.alignment);
            }
            "marginl" => style.margin_l = parse_i32(raw_value, style.margin_l),
            "marginr" => style.margin_r = parse_i32(raw_value, style.margin_r),
            "marginv" => style.margin_v = parse_i32(raw_value, style.margin_v),
            "encoding" => style.encoding = parse_i32(raw_value, style.encoding),
            "treat_fontname_as_pattern" => {
                style.treat_fontname_as_pattern =
                    parse_i32(raw_value, style.treat_fontname_as_pattern)
            }
            "blur" => style.blur = parse_f64(raw_value, style.blur),
            "justify" => style.justify = parse_i32(raw_value, style.justify),
            _ => {}
        }
    }

    Some(style)
}

fn parse_event_line(
    value: &str,
    format: &[String],
    read_order: i32,
    styles: &[ParsedStyle],
) -> Option<ParsedEvent> {
    let fields = split_fields(value, format.len());
    if fields.len() != format.len() {
        return None;
    }

    let mut event = ParsedEvent {
        read_order,
        ..ParsedEvent::default()
    };
    let mut end = 0_i64;

    for (key, raw_value) in format.iter().zip(fields) {
        let lowered = key.to_ascii_lowercase();
        match lowered.as_str() {
            "layer" => event.layer = parse_i32(raw_value, event.layer),
            "start" => event.start = parse_timestamp(raw_value).unwrap_or(event.start),
            "end" => end = parse_timestamp(raw_value).unwrap_or(end),
            "style" => event.style = parse_style_reference(raw_value, styles),
            "name" => event.name = raw_value.trim().to_string(),
            "marginl" => event.margin_l = parse_i32(raw_value, event.margin_l),
            "marginr" => event.margin_r = parse_i32(raw_value, event.margin_r),
            "marginv" => event.margin_v = parse_i32(raw_value, event.margin_v),
            "effect" => event.effect = raw_value.to_string(),
            "text" => event.text = raw_value.to_string(),
            _ => {}
        }
    }

    event.duration = (end - event.start).max(0);
    Some(event)
}

fn split_fields(input: &str, field_count: usize) -> Vec<&str> {
    if field_count == 0 {
        return Vec::new();
    }

    let mut fields = Vec::with_capacity(field_count);
    let mut remainder = input;
    for _ in 0..field_count.saturating_sub(1) {
        if let Some((head, tail)) = remainder.split_once(',') {
            fields.push(head.trim());
            remainder = tail;
        } else {
            fields.push(remainder.trim());
            remainder = "";
        }
    }
    fields.push(remainder.trim());
    fields
}

fn apply_script_info_field(track: &mut ParsedTrack, key: &str, value: &str) {
    match key.to_ascii_lowercase().as_str() {
        "playresx" => track.play_res_x = parse_i32(value, track.play_res_x),
        "playresy" => track.play_res_y = parse_i32(value, track.play_res_y),
        "timer" => track.timer = parse_f64(value, track.timer),
        "wrapstyle" => track.wrap_style = parse_i32(value, track.wrap_style),
        "scaledborderandshadow" => {
            track.scaled_border_and_shadow = parse_bool(value, track.scaled_border_and_shadow)
        }
        "kerning" => track.kerning = parse_bool(value, track.kerning),
        "language" => track.language = value.trim().to_string(),
        "layoutresx" => track.layout_res_x = parse_i32(value, track.layout_res_x),
        "layoutresy" => track.layout_res_y = parse_i32(value, track.layout_res_y),
        "ycbcr matrix" => track.ycbcr_matrix = parse_matrix(value),
        _ => {}
    }
}

fn parse_bool(value: &str, fallback: bool) -> bool {
    match value.trim().parse::<i32>() {
        Ok(parsed) => parsed != 0,
        Err(_) => match value.trim().to_ascii_lowercase().as_str() {
            "yes" | "true" => true,
            "no" | "false" => false,
            _ => fallback,
        },
    }
}

fn parse_bold(value: &str, fallback: bool) -> bool {
    match value.trim().parse::<i32>() {
        Ok(parsed) => parsed == 1 || !(0..700).contains(&parsed),
        Err(_) => parse_bool(value, fallback),
    }
}

fn parse_i32(value: &str, fallback: i32) -> i32 {
    value.trim().parse().unwrap_or(fallback)
}

fn parse_f64(value: &str, fallback: f64) -> f64 {
    value.trim().parse().unwrap_or(fallback)
}

fn parse_scale(value: &str, fallback: f64) -> f64 {
    let parsed = parse_f64(value, fallback * 100.0);
    if parsed > 10.0 {
        parsed / 100.0
    } else {
        parsed
    }
}

fn parse_color(value: &str, fallback: u32) -> u32 {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("&H")
        .or_else(|| trimmed.strip_prefix("&h"))
    {
        let hex = hex.trim_end_matches('&');
        u32::from_str_radix(hex, 16).unwrap_or(fallback)
    } else {
        trimmed.parse().unwrap_or(fallback)
    }
}

fn parse_timestamp(value: &str) -> Option<i64> {
    let mut parts = value.trim().split(':');
    let hours = parts.next()?.trim().parse::<i64>().ok()?;
    let minutes = parts.next()?.trim().parse::<i64>().ok()?;
    let seconds = parts.next()?.trim();
    let (seconds, centiseconds) = if let Some((seconds, fraction)) = seconds.split_once('.') {
        let fraction = format!("{fraction:0<2}");
        (
            seconds.trim().parse::<i64>().ok()?,
            fraction[..2].parse::<i64>().ok()?,
        )
    } else {
        (seconds.parse::<i64>().ok()?, 0)
    };
    Some((((hours * 60 + minutes) * 60) + seconds) * 1000 + centiseconds * 10)
}

fn parse_style_reference(value: &str, styles: &[ParsedStyle]) -> i32 {
    let style_name = value.trim();
    if style_name.is_empty() {
        return 0;
    }

    styles
        .iter()
        .position(|style| style.name.eq_ignore_ascii_case(style_name))
        .map(|index| index as i32)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
fn apply_override_block(
    block: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
    current_style: &mut ParsedSpanStyle,
    parsed: &mut ParsedDialogueText,
    buffer: &mut String,
    active_line: &mut ParsedTextLine,
    pending_karaoke: &mut Option<ParsedKaraokeSpan>,
    karaoke_cursor_ms: &mut i32,
    drawing_scale: &mut i32,
    current_transforms: &mut Vec<ParsedSpanTransform>,
    current_wrap_style: &mut i32,
) {
    for raw_tag in split_override_tags(block) {
        let tag = raw_tag.trim();
        if tag.is_empty() {
            continue;
        }

        let previous = current_style.clone();
        let previous_transforms = current_transforms.clone();
        if let Some(rest) = tag.strip_prefix("fn") {
            let family = rest.trim();
            if !family.is_empty() {
                current_style.font_name = family.to_string();
            }
        } else if let Some(rest) = tag.strip_prefix("kt") {
            flush_span(
                buffer,
                &previous,
                *pending_karaoke,
                *drawing_scale,
                &previous_transforms,
                active_line,
            );
            *karaoke_cursor_ms = parse_karaoke_duration(rest).unwrap_or(0);
            *pending_karaoke = None;
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
            flush_span(
                buffer,
                &previous,
                *pending_karaoke,
                *drawing_scale,
                &previous_transforms,
                active_line,
            );
            if let Some(duration_ms) = parse_karaoke_duration(rest) {
                *pending_karaoke = Some(ParsedKaraokeSpan {
                    start_ms: *karaoke_cursor_ms,
                    duration_ms,
                    mode,
                });
                *karaoke_cursor_ms += duration_ms;
            }
        } else if let Some(rest) = tag.strip_prefix("fscx") {
            current_style.scale_x = parse_scale(rest, base_style.scale_x);
        } else if let Some(rest) = tag.strip_prefix("fscy") {
            current_style.scale_y = parse_scale(rest, base_style.scale_y);
        } else if tag == "fsc" {
            current_style.scale_x = base_style.scale_x;
            current_style.scale_y = base_style.scale_y;
        } else if let Some(rest) = tag.strip_prefix("fsp") {
            current_style.spacing = parse_f64(rest, current_style.spacing);
        } else if let Some(rest) = tag.strip_prefix("frx") {
            current_style.rotation_x = parse_f64(rest, current_style.rotation_x);
        } else if let Some(rest) = tag.strip_prefix("fry") {
            current_style.rotation_y = parse_f64(rest, current_style.rotation_y);
        } else if let Some(rest) = tag.strip_prefix("frz").or_else(|| tag.strip_prefix("fr")) {
            current_style.rotation_z = parse_f64(rest, current_style.rotation_z);
        } else if let Some(rest) = tag.strip_prefix("fax") {
            current_style.shear_x = parse_f64(rest, current_style.shear_x);
        } else if let Some(rest) = tag.strip_prefix("fay") {
            current_style.shear_y = parse_f64(rest, current_style.shear_y);
        } else if let Some(rest) = tag.strip_prefix("fs") {
            current_style.font_size =
                parse_font_size_override(rest, current_style.font_size, base_style.font_size);
        } else if let Some(rest) = tag.strip_prefix("iclip") {
            if let Some(rect) = parse_rect_clip(rest) {
                parsed.clip_rect = Some(rect);
                parsed.vector_clip = None;
                parsed.inverse_clip = true;
            } else if let Some(vector) = parse_vector_clip(rest) {
                parsed.clip_rect = None;
                parsed.vector_clip = Some(vector);
                parsed.inverse_clip = true;
            }
        } else if let Some(rest) = tag.strip_prefix("move") {
            if parsed.position.is_none() && parsed.movement.is_none() {
                parsed.movement = parse_move(rest);
            }
        } else if let Some(rest) = tag.strip_prefix("fade") {
            parsed.fade = parse_fade(rest);
        } else if let Some(rest) = tag.strip_prefix("fad") {
            parsed.fade = parse_fad(rest);
        } else if let Some(rest) = tag.strip_prefix("clip") {
            if let Some(rect) = parse_rect_clip(rest) {
                parsed.clip_rect = Some(rect);
                parsed.vector_clip = None;
                parsed.inverse_clip = false;
            } else if let Some(vector) = parse_vector_clip(rest) {
                parsed.clip_rect = None;
                parsed.vector_clip = Some(vector);
                parsed.inverse_clip = false;
            }
        } else if let Some(rest) = tag.strip_prefix("1c").or_else(|| tag.strip_prefix('c')) {
            current_style.primary_colour = parse_override_color(rest, current_style.primary_colour);
        } else if let Some(rest) = tag.strip_prefix("2c") {
            current_style.secondary_colour =
                parse_override_color(rest, current_style.secondary_colour);
        } else if let Some(rest) = tag.strip_prefix("3c") {
            current_style.outline_colour = parse_override_color(rest, current_style.outline_colour);
        } else if let Some(rest) = tag.strip_prefix("4c") {
            current_style.back_colour = parse_override_color(rest, current_style.back_colour);
        } else if let Some(rest) = tag.strip_prefix("alpha") {
            let alpha = parse_alpha_tag(rest, alpha_of(current_style.primary_colour));
            current_style.primary_colour = with_alpha(current_style.primary_colour, alpha);
            current_style.secondary_colour = with_alpha(current_style.secondary_colour, alpha);
            current_style.outline_colour = with_alpha(current_style.outline_colour, alpha);
            current_style.back_colour = with_alpha(current_style.back_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("1a") {
            let alpha = parse_alpha_tag(rest, alpha_of(current_style.primary_colour));
            current_style.primary_colour = with_alpha(current_style.primary_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("2a") {
            let alpha = parse_alpha_tag(rest, alpha_of(current_style.secondary_colour));
            current_style.secondary_colour = with_alpha(current_style.secondary_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("3a") {
            let alpha = parse_alpha_tag(rest, alpha_of(current_style.outline_colour));
            current_style.outline_colour = with_alpha(current_style.outline_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("4a") {
            let alpha = parse_alpha_tag(rest, alpha_of(current_style.back_colour));
            current_style.back_colour = with_alpha(current_style.back_colour, alpha);
        } else if let Some(rest) = tag.strip_prefix("xbord") {
            current_style.border_x = parse_f64(rest, current_style.border_x);
        } else if let Some(rest) = tag.strip_prefix("ybord") {
            current_style.border_y = parse_f64(rest, current_style.border_y);
        } else if let Some(rest) = tag.strip_prefix("bord") {
            current_style.border = parse_f64(rest, current_style.border);
            current_style.border_x = current_style.border;
            current_style.border_y = current_style.border;
        } else if let Some(rest) = tag.strip_prefix("xshad") {
            current_style.shadow_x = parse_f64(rest, current_style.shadow_x);
        } else if let Some(rest) = tag.strip_prefix("yshad") {
            current_style.shadow_y = parse_f64(rest, current_style.shadow_y);
        } else if let Some(rest) = tag.strip_prefix("shad") {
            current_style.shadow = parse_f64(rest, current_style.shadow);
            current_style.shadow_x = current_style.shadow;
            current_style.shadow_y = current_style.shadow;
        } else if let Some(rest) = tag.strip_prefix("blur") {
            current_style.blur = parse_f64(rest, current_style.blur);
        } else if let Some(rest) = tag.strip_prefix("be") {
            current_style.be = parse_f64(rest, current_style.be);
        } else if let Some(rest) = tag.strip_prefix('t') {
            if let Some(transform) = parse_transform(rest, current_style) {
                current_transforms.push(transform);
            }
        } else if let Some(rest) = tag.strip_prefix('u') {
            current_style.underline = parse_override_bool(rest, current_style.underline);
        } else if let Some(rest) = tag.strip_prefix('s') {
            current_style.strike_out = parse_override_bool(rest, current_style.strike_out);
        } else if let Some(rest) = tag.strip_prefix('b') {
            current_style.bold = parse_override_bold(rest, current_style.bold);
        } else if let Some(rest) = tag.strip_prefix('i') {
            current_style.italic = parse_override_bool(rest, current_style.italic);
        } else if let Some(rest) = tag.strip_prefix("an") {
            if let Ok(value) = rest.trim().parse::<i32>() {
                parsed.alignment = alignment_from_an(value);
            }
        } else if let Some(rest) = tag.strip_prefix('a') {
            if let Ok(value) = rest.trim().parse::<i32>() {
                parsed.alignment = alignment_from_legacy_a(value);
            }
        } else if let Some(rest) = tag.strip_prefix('q') {
            if let Ok(value) = rest.trim().parse::<i32>() {
                let value = value.clamp(0, 3);
                parsed.wrap_style = Some(value);
                *current_wrap_style = value;
            }
        } else if let Some(rest) = tag.strip_prefix("org") {
            parsed.origin = parse_pos(rest);
        } else if let Some(rest) = tag.strip_prefix("pos") {
            if parsed.position.is_none() && parsed.movement.is_none() {
                if let Some(position) = parse_pos(rest) {
                    parsed.position = Some(position);
                }
            }
        } else if let Some(rest) = tag.strip_prefix("pbo") {
            current_style.pbo = parse_f64(rest, current_style.pbo);
        } else if let Some(rest) = tag.strip_prefix('p') {
            flush_span(
                buffer,
                &previous,
                *pending_karaoke,
                *drawing_scale,
                &previous_transforms,
                active_line,
            );
            *drawing_scale = parse_i32(rest, *drawing_scale).max(0);
        } else if let Some(rest) = tag.strip_prefix('r') {
            *current_style = resolve_reset_style(rest, base_style, styles);
            current_transforms.clear();
        }

        suppress_transform_fields_for_override(tag, current_transforms);

        if *current_style != previous || *current_transforms != previous_transforms {
            flush_span(
                buffer,
                &previous,
                *pending_karaoke,
                *drawing_scale,
                &previous_transforms,
                active_line,
            );
        }
    }
}

fn suppress_transform_fields_for_override(
    tag: &str,
    current_transforms: &mut Vec<ParsedSpanTransform>,
) {
    if current_transforms.is_empty() || tag.strip_prefix('t').is_some() {
        return;
    }

    for transform in current_transforms.iter_mut() {
        let style = &mut transform.style;
        if tag
            .strip_prefix("1c")
            .or_else(|| tag.strip_prefix('c'))
            .is_some()
        {
            style.primary_colour = None;
        } else if tag.strip_prefix("2c").is_some() {
            style.secondary_colour = None;
        } else if tag.strip_prefix("3c").is_some() {
            style.outline_colour = None;
        } else if tag.strip_prefix("4c").is_some() {
            style.back_colour = None;
        } else if tag.strip_prefix("alpha").is_some() {
            style.clear_colours();
        } else if tag.strip_prefix("1a").is_some() {
            style.primary_colour = None;
        } else if tag.strip_prefix("2a").is_some() {
            style.secondary_colour = None;
        } else if tag.strip_prefix("3a").is_some() {
            style.outline_colour = None;
        } else if tag.strip_prefix("4a").is_some() {
            style.back_colour = None;
        } else if tag.strip_prefix("fscx").is_some() {
            style.scale_x = None;
        } else if tag.strip_prefix("fscy").is_some() {
            style.scale_y = None;
        } else if tag == "fsc" {
            style.scale_x = None;
            style.scale_y = None;
        } else if tag.strip_prefix("fsp").is_some() {
            style.spacing = None;
        } else if tag.strip_prefix("frx").is_some() {
            style.rotation_x = None;
        } else if tag.strip_prefix("fry").is_some() {
            style.rotation_y = None;
        } else if tag
            .strip_prefix("frz")
            .or_else(|| tag.strip_prefix("fr"))
            .is_some()
        {
            style.rotation_z = None;
        } else if tag.strip_prefix("fax").is_some() {
            style.shear_x = None;
        } else if tag.strip_prefix("fay").is_some() {
            style.shear_y = None;
        } else if tag.strip_prefix("fs").is_some() {
            style.font_size = None;
        } else if tag.strip_prefix("xbord").is_some() {
            style.border_x = None;
        } else if tag.strip_prefix("ybord").is_some() {
            style.border_y = None;
        } else if tag.strip_prefix("bord").is_some() {
            style.border = None;
            style.border_x = None;
            style.border_y = None;
        } else if tag.strip_prefix("xshad").is_some() {
            style.shadow_x = None;
        } else if tag.strip_prefix("yshad").is_some() {
            style.shadow_y = None;
        } else if tag.strip_prefix("shad").is_some() {
            style.shadow = None;
            style.shadow_x = None;
            style.shadow_y = None;
        } else if tag.strip_prefix("blur").is_some() {
            style.blur = None;
        } else if tag.strip_prefix("be").is_some() {
            style.be = None;
        }
    }

    current_transforms.retain(|transform| !transform.style.is_empty());
}

fn parse_transform(value: &str, current_style: &ParsedSpanStyle) -> Option<ParsedSpanTransform> {
    let inside = value.trim().strip_prefix('(')?.strip_suffix(')')?.trim();
    let tag_start = inside.find('\\')?;
    let (timing_part, tags_part) = inside.split_at(tag_start);
    let params = timing_part
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let (start_ms, end_ms, accel) = match params.as_slice() {
        [] => (0, None, 1.0),
        [accel] => (0, None, parse_f64(accel, 1.0)),
        [start, end] => (
            parse_i32(start, 0).max(0),
            Some(parse_i32(end, 0).max(parse_i32(start, 0))),
            1.0,
        ),
        [start, end, accel, ..] => (
            parse_i32(start, 0).max(0),
            Some(parse_i32(end, 0).max(parse_i32(start, 0))),
            parse_f64(accel, 1.0),
        ),
    };

    let mut target_style = current_style.clone();
    for raw_tag in split_override_tags(tags_part) {
        apply_transform_tag(raw_tag.trim(), &mut target_style);
    }

    let animated = diff_animated_style(current_style, &target_style);
    (!animated.is_empty()).then_some(ParsedSpanTransform {
        start_ms,
        end_ms,
        accel: if accel > 0.0 { accel } else { 1.0 },
        style: animated,
    })
}

fn split_override_tags(block: &str) -> Vec<&str> {
    let mut tags = Vec::new();
    let mut start = None;
    let mut depth = 0_i32;

    for (index, character) in block.char_indices() {
        match character {
            '\\' if depth == 0 => {
                if let Some(tag_start) = start.take() {
                    let tag = block[tag_start..index].trim();
                    if !tag.is_empty() {
                        tags.push(tag);
                    }
                }
                start = Some(index + character.len_utf8());
            }
            '(' => depth += 1,
            ')' => depth = (depth - 1).max(0),
            _ => {}
        }
    }

    if let Some(tag_start) = start {
        let tag = block[tag_start..].trim();
        if !tag.is_empty() {
            tags.push(tag);
        }
    }

    tags
}

fn apply_transform_tag(tag: &str, style: &mut ParsedSpanStyle) {
    if let Some(rest) = tag.strip_prefix("1c").or_else(|| tag.strip_prefix('c')) {
        style.primary_colour = parse_override_color(rest, style.primary_colour);
    } else if let Some(rest) = tag.strip_prefix("2c") {
        style.secondary_colour = parse_override_color(rest, style.secondary_colour);
    } else if let Some(rest) = tag.strip_prefix("3c") {
        style.outline_colour = parse_override_color(rest, style.outline_colour);
    } else if let Some(rest) = tag.strip_prefix("4c") {
        style.back_colour = parse_override_color(rest, style.back_colour);
    } else if let Some(rest) = tag.strip_prefix("alpha") {
        let alpha = parse_alpha_tag(rest, alpha_of(style.primary_colour));
        style.primary_colour = with_alpha(style.primary_colour, alpha);
        style.secondary_colour = with_alpha(style.secondary_colour, alpha);
        style.outline_colour = with_alpha(style.outline_colour, alpha);
        style.back_colour = with_alpha(style.back_colour, alpha);
    } else if let Some(rest) = tag.strip_prefix("1a") {
        style.primary_colour = with_alpha(
            style.primary_colour,
            parse_alpha_tag(rest, alpha_of(style.primary_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("2a") {
        style.secondary_colour = with_alpha(
            style.secondary_colour,
            parse_alpha_tag(rest, alpha_of(style.secondary_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("3a") {
        style.outline_colour = with_alpha(
            style.outline_colour,
            parse_alpha_tag(rest, alpha_of(style.outline_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("4a") {
        style.back_colour = with_alpha(
            style.back_colour,
            parse_alpha_tag(rest, alpha_of(style.back_colour)),
        );
    } else if let Some(rest) = tag.strip_prefix("fscx") {
        style.scale_x = parse_scale(rest, style.scale_x);
    } else if let Some(rest) = tag.strip_prefix("fscy") {
        style.scale_y = parse_scale(rest, style.scale_y);
    } else if let Some(rest) = tag.strip_prefix("fsp") {
        style.spacing = parse_f64(rest, style.spacing);
    } else if let Some(rest) = tag.strip_prefix("frx") {
        style.rotation_x = parse_f64(rest, style.rotation_x);
    } else if let Some(rest) = tag.strip_prefix("fry") {
        style.rotation_y = parse_f64(rest, style.rotation_y);
    } else if let Some(rest) = tag.strip_prefix("frz").or_else(|| tag.strip_prefix("fr")) {
        style.rotation_z = parse_f64(rest, style.rotation_z);
    } else if let Some(rest) = tag.strip_prefix("fax") {
        style.shear_x = parse_f64(rest, style.shear_x);
    } else if let Some(rest) = tag.strip_prefix("fay") {
        style.shear_y = parse_f64(rest, style.shear_y);
    } else if let Some(rest) = tag.strip_prefix("fs") {
        style.font_size = parse_f64(rest, style.font_size);
    } else if let Some(rest) = tag.strip_prefix("xbord") {
        style.border_x = parse_f64(rest, style.border_x);
    } else if let Some(rest) = tag.strip_prefix("ybord") {
        style.border_y = parse_f64(rest, style.border_y);
    } else if let Some(rest) = tag.strip_prefix("bord") {
        style.border = parse_f64(rest, style.border);
        style.border_x = style.border;
        style.border_y = style.border;
    } else if let Some(rest) = tag.strip_prefix("xshad") {
        style.shadow_x = parse_f64(rest, style.shadow_x);
    } else if let Some(rest) = tag.strip_prefix("yshad") {
        style.shadow_y = parse_f64(rest, style.shadow_y);
    } else if let Some(rest) = tag.strip_prefix("shad") {
        style.shadow = parse_f64(rest, style.shadow);
        style.shadow_x = style.shadow;
        style.shadow_y = style.shadow;
    } else if let Some(rest) = tag.strip_prefix("blur") {
        style.blur = parse_f64(rest, style.blur);
    } else if let Some(rest) = tag.strip_prefix("be") {
        style.be = parse_f64(rest, style.be);
    }
}

fn diff_animated_style(base: &ParsedSpanStyle, target: &ParsedSpanStyle) -> ParsedAnimatedStyle {
    ParsedAnimatedStyle {
        font_size: ((target.font_size - base.font_size).abs() > f64::EPSILON)
            .then_some(target.font_size),
        scale_x: ((target.scale_x - base.scale_x).abs() > f64::EPSILON).then_some(target.scale_x),
        scale_y: ((target.scale_y - base.scale_y).abs() > f64::EPSILON).then_some(target.scale_y),
        spacing: ((target.spacing - base.spacing).abs() > f64::EPSILON).then_some(target.spacing),
        rotation_x: ((target.rotation_x - base.rotation_x).abs() > f64::EPSILON)
            .then_some(target.rotation_x),
        rotation_y: ((target.rotation_y - base.rotation_y).abs() > f64::EPSILON)
            .then_some(target.rotation_y),
        rotation_z: ((target.rotation_z - base.rotation_z).abs() > f64::EPSILON)
            .then_some(target.rotation_z),
        shear_x: ((target.shear_x - base.shear_x).abs() > f64::EPSILON).then_some(target.shear_x),
        shear_y: ((target.shear_y - base.shear_y).abs() > f64::EPSILON).then_some(target.shear_y),
        primary_colour: (target.primary_colour != base.primary_colour)
            .then_some(target.primary_colour),
        secondary_colour: (target.secondary_colour != base.secondary_colour)
            .then_some(target.secondary_colour),
        outline_colour: (target.outline_colour != base.outline_colour)
            .then_some(target.outline_colour),
        back_colour: (target.back_colour != base.back_colour).then_some(target.back_colour),
        border: ((target.border - base.border).abs() > f64::EPSILON).then_some(target.border),
        border_x: ((target.border_x - base.border_x).abs() > f64::EPSILON)
            .then_some(target.border_x),
        border_y: ((target.border_y - base.border_y).abs() > f64::EPSILON)
            .then_some(target.border_y),
        shadow: ((target.shadow - base.shadow).abs() > f64::EPSILON).then_some(target.shadow),
        shadow_x: ((target.shadow_x - base.shadow_x).abs() > f64::EPSILON)
            .then_some(target.shadow_x),
        shadow_y: ((target.shadow_y - base.shadow_y).abs() > f64::EPSILON)
            .then_some(target.shadow_y),
        blur: ((target.blur - base.blur).abs() > f64::EPSILON).then_some(target.blur),
        be: ((target.be - base.be).abs() > f64::EPSILON).then_some(target.be),
    }
}

fn parse_font_size_override(value: &str, current: f64, base: f64) -> f64 {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return base;
    }

    let parsed = trimmed.parse::<f64>().unwrap_or(0.0);
    let resolved = if trimmed.starts_with(['+', '-']) {
        current * (1.0 + parsed / 10.0)
    } else {
        parsed
    };

    if resolved > 0.0 { resolved } else { base }
}

fn parse_karaoke_duration(value: &str) -> Option<i32> {
    value
        .trim()
        .parse::<i32>()
        .ok()
        .map(|centiseconds| centiseconds.max(0) * 10)
}

fn parse_override_color(value: &str, fallback: u32) -> u32 {
    let trimmed = value.trim();
    let trimmed = trimmed.trim_matches('&').trim_start_matches(['H', 'h']);
    if trimmed.is_empty() {
        return fallback;
    }

    u32::from_str_radix(trimmed, 16).unwrap_or(fallback)
}

fn parse_alpha_tag(value: &str, fallback: u8) -> u8 {
    let trimmed = value.trim();
    let trimmed = trimmed.trim_matches('&').trim_start_matches(['H', 'h']);
    if trimmed.is_empty() {
        return fallback;
    }
    u8::from_str_radix(trimmed, 16).unwrap_or(fallback)
}

fn alpha_of(color: u32) -> u8 {
    ((color >> 24) & 0xFF) as u8
}

fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color & 0x00FF_FFFF) | (u32::from(alpha) << 24)
}

fn parse_override_bool(value: &str, fallback: bool) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        true
    } else {
        parse_bool(trimmed, fallback)
    }
}

fn parse_override_bold(value: &str, fallback: bool) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        true
    } else {
        parse_bold(trimmed, fallback)
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

fn parse_pos(value: &str) -> Option<(i32, i32)> {
    let trimmed = value.trim();
    let inside = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = inside.split(',').map(str::trim);
    let x = parts.next()?.parse::<i32>().ok()?;
    let y = parts.next()?.parse::<i32>().ok()?;
    Some((x, y))
}

fn parse_rect_clip(value: &str) -> Option<Rect> {
    let trimmed = value.trim();
    let inside = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let parts = inside.split(',').map(str::trim).collect::<Vec<_>>();
    if parts.len() != 4 {
        return None;
    }
    let x_min = parts[0].parse::<i32>().ok()?;
    let y_min = parts[1].parse::<i32>().ok()?;
    let x_max = parts[2].parse::<i32>().ok()?;
    let y_max = parts[3].parse::<i32>().ok()?;
    Some(Rect {
        x_min,
        y_min,
        x_max,
        y_max,
    })
}

fn parse_vector_clip(value: &str) -> Option<ParsedVectorClip> {
    let trimmed = value.trim();
    let inside = trimmed.strip_prefix('(')?.strip_suffix(')')?.trim();
    if inside.is_empty() {
        return None;
    }

    let (scale, drawing) = if let Some((scale, drawing)) = inside.split_once(',') {
        if let Ok(scale) = scale.trim().parse::<i32>() {
            (scale.max(1), drawing.trim())
        } else {
            (1, inside)
        }
    } else {
        (1, inside)
    };

    let polygons = parse_drawing_polygons(drawing, scale)?;
    if polygons.is_empty() {
        return None;
    }

    Some(ParsedVectorClip { scale, polygons })
}

fn parse_drawing_polygons(drawing: &str, scale: i32) -> Option<Vec<Vec<Point>>> {
    let tokens = drawing.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return None;
    }

    let mut polygons = Vec::new();
    let mut current = Vec::new();
    let mut spline_state: Option<SplineState> = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].to_ascii_lowercase().as_str() {
            "m" | "n" => {
                spline_state = None;
                if current.len() >= 3 {
                    polygons.push(std::mem::take(&mut current));
                }
                index += 1;
                let (point, next_index) = parse_drawing_point(&tokens, index, scale)?;
                current.push(point);
                index = next_index;
                while let Some((point, next_index)) =
                    parse_drawing_point_optional(&tokens, index, scale)
                {
                    current.push(point);
                    index = next_index;
                }
            }
            "l" => {
                spline_state = None;
                if current.is_empty() {
                    return None;
                }
                index += 1;
                let mut consumed = false;
                while let Some((point, next_index)) =
                    parse_drawing_point_optional(&tokens, index, scale)
                {
                    current.push(point);
                    index = next_index;
                    consumed = true;
                }
                if !consumed {
                    return None;
                }
            }
            "b" => {
                spline_state = None;
                if current.is_empty() {
                    return None;
                }
                index += 1;
                let mut consumed = false;
                while let Some(((control1, control2, end), next_index)) =
                    parse_bezier_segment(&tokens, index, scale)
                {
                    let start = *current.last()?;
                    current.extend(approximate_cubic_bezier(start, control1, control2, end, 16));
                    index = next_index;
                    consumed = true;
                }
                if !consumed {
                    return None;
                }
            }
            "s" => {
                if current.is_empty() {
                    return None;
                }
                index += 1;
                let (point1, next_index) = parse_drawing_point(&tokens, index, scale)?;
                let (point2, next_index) = parse_drawing_point(&tokens, next_index, scale)?;
                let (point3, next_index) = parse_drawing_point(&tokens, next_index, scale)?;
                let start = *current.last()?;
                current.extend(approximate_spline_segment(
                    start, point1, point2, point3, 16,
                ));
                spline_state = Some(SplineState {
                    first_three: [point1, point2, point3],
                    history: vec![start, point1, point2, point3],
                });
                index = next_index;
            }
            "p" => {
                let state = spline_state.as_mut()?;
                index += 1;
                let mut consumed = false;
                while let Some((point, next_index)) =
                    parse_drawing_point_optional(&tokens, index, scale)
                {
                    let len = state.history.len();
                    current.extend(approximate_spline_segment(
                        state.history[len - 3],
                        state.history[len - 2],
                        state.history[len - 1],
                        point,
                        16,
                    ));
                    state.history.push(point);
                    index = next_index;
                    consumed = true;
                }
                if !consumed {
                    return None;
                }
            }
            "c" => {
                let state = spline_state.take()?;
                for point in state.first_three {
                    let len = state.history.len();
                    current.extend(approximate_spline_segment(
                        state.history[len - 3],
                        state.history[len - 2],
                        state.history[len - 1],
                        point,
                        16,
                    ));
                }
                index += 1;
            }
            _ => return None,
        }
    }

    if current.len() >= 3 {
        polygons.push(current);
    }

    Some(polygons)
}

#[derive(Clone, Debug)]
struct SplineState {
    first_three: [Point; 3],
    history: Vec<Point>,
}

fn parse_drawing_point(tokens: &[&str], index: usize, scale: i32) -> Option<(Point, usize)> {
    let x = tokens.get(index)?.parse::<i32>().ok()?;
    let y = tokens.get(index + 1)?.parse::<i32>().ok()?;
    Some((scale_drawing_point(x, y, scale), index + 2))
}

fn parse_drawing_point_optional(
    tokens: &[&str],
    index: usize,
    scale: i32,
) -> Option<(Point, usize)> {
    let x = tokens.get(index)?;
    let y = tokens.get(index + 1)?;
    if x.chars().any(|character| character.is_ascii_alphabetic())
        || y.chars().any(|character| character.is_ascii_alphabetic())
    {
        return None;
    }
    parse_drawing_point(tokens, index, scale)
}

fn parse_bezier_segment(
    tokens: &[&str],
    index: usize,
    scale: i32,
) -> Option<((Point, Point, Point), usize)> {
    let (control1, next_index) = parse_drawing_point(tokens, index, scale)?;
    let (control2, next_index) = parse_drawing_point(tokens, next_index, scale)?;
    let (end, next_index) = parse_drawing_point(tokens, next_index, scale)?;
    Some(((control1, control2, end), next_index))
}

fn approximate_cubic_bezier(
    start: Point,
    control1: Point,
    control2: Point,
    end: Point,
    segments: usize,
) -> Vec<Point> {
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
            x: x.round() as i32,
            y: y.round() as i32,
        };
        if points.last().copied() != Some(point) {
            points.push(point);
        }
    }
    points
}

fn approximate_spline_segment(
    previous: Point,
    point1: Point,
    point2: Point,
    point3: Point,
    segments: usize,
) -> Vec<Point> {
    let x01 = (point1.x - previous.x) / 3;
    let y01 = (point1.y - previous.y) / 3;
    let x12 = (point2.x - point1.x) / 3;
    let y12 = (point2.y - point1.y) / 3;
    let x23 = (point3.x - point2.x) / 3;
    let y23 = (point3.y - point2.y) / 3;

    let start = Point {
        x: point1.x + ((x12 - x01) >> 1),
        y: point1.y + ((y12 - y01) >> 1),
    };
    let control1 = Point {
        x: point1.x + x12,
        y: point1.y + y12,
    };
    let control2 = Point {
        x: point2.x - x12,
        y: point2.y - y12,
    };
    let end = Point {
        x: point2.x + ((x23 - x12) >> 1),
        y: point2.y + ((y23 - y12) >> 1),
    };

    approximate_cubic_bezier(start, control1, control2, end, segments)
}

fn scale_drawing_point(x: i32, y: i32, scale: i32) -> Point {
    let factor = 1_i32
        .checked_shl(scale.saturating_sub(1) as u32)
        .unwrap_or(1)
        .max(1);
    Point {
        x: x / factor,
        y: y / factor,
    }
}

fn bounds_from_polygons(polygons: &[Vec<Point>]) -> Option<Rect> {
    let mut points = polygons.iter().flat_map(|polygon| polygon.iter().copied());
    let first = points.next()?;
    let mut x_min = first.x;
    let mut y_min = first.y;
    let mut x_max = first.x;
    let mut y_max = first.y;
    for point in points {
        x_min = x_min.min(point.x);
        y_min = y_min.min(point.y);
        x_max = x_max.max(point.x);
        y_max = y_max.max(point.y);
    }
    Some(Rect {
        x_min,
        y_min,
        x_max: x_max + 1,
        y_max: y_max + 1,
    })
}

fn parse_move(value: &str) -> Option<ParsedMovement> {
    let trimmed = value.trim();
    let inside = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let parts = inside.split(',').map(str::trim).collect::<Vec<_>>();
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
            let mut t1_ms = t1.parse::<i32>().ok()?;
            let mut t2_ms = t2.parse::<i32>().ok()?;
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

fn parse_fad(value: &str) -> Option<ParsedFade> {
    let trimmed = value.trim();
    let inside = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let parts = inside.split(',').map(str::trim).collect::<Vec<_>>();
    let [fade_in, fade_out] = parts.as_slice() else {
        return None;
    };

    Some(ParsedFade::Simple {
        fade_in_ms: fade_in.parse::<i32>().ok()?,
        fade_out_ms: fade_out.parse::<i32>().ok()?,
    })
}

fn parse_fade(value: &str) -> Option<ParsedFade> {
    let trimmed = value.trim();
    let inside = trimmed.strip_prefix('(')?.strip_suffix(')')?;
    let parts = inside.split(',').map(str::trim).collect::<Vec<_>>();
    let [a1, a2, a3, t1, t2, t3, t4] = parts.as_slice() else {
        return None;
    };

    Some(ParsedFade::Complex {
        alpha1: a1.parse::<i32>().ok()?.clamp(0, 255),
        alpha2: a2.parse::<i32>().ok()?.clamp(0, 255),
        alpha3: a3.parse::<i32>().ok()?.clamp(0, 255),
        t1_ms: t1.parse::<i32>().ok()?,
        t2_ms: t2.parse::<i32>().ok()?,
        t3_ms: t3.parse::<i32>().ok()?,
        t4_ms: t4.parse::<i32>().ok()?,
    })
}

fn resolve_reset_style(
    value: &str,
    base_style: &ParsedStyle,
    styles: &[ParsedStyle],
) -> ParsedSpanStyle {
    let name = value.trim();
    if name.is_empty() {
        return ParsedSpanStyle::from_style(base_style);
    }

    styles
        .iter()
        .find(|style| style.name.eq_ignore_ascii_case(name))
        .map(ParsedSpanStyle::from_style)
        .unwrap_or_else(|| ParsedSpanStyle::from_style(base_style))
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

fn push_line(parsed: &mut ParsedDialogueText, line: &mut ParsedTextLine) {
    if line.text.is_empty() && line.spans.is_empty() && !parsed.lines.is_empty() {
        return;
    }
    parsed.lines.push(std::mem::take(line));
}

fn parse_matrix(value: &str) -> YCbCrMatrix {
    match value.trim().to_ascii_lowercase().as_str() {
        "none" => YCbCrMatrix::None,
        "tv.601" | "bt601(tv)" | "bt.601(tv)" => YCbCrMatrix::Bt601Tv,
        "pc.601" | "bt601(pc)" | "bt.601(pc)" => YCbCrMatrix::Bt601Pc,
        "tv.709" | "bt709(tv)" | "bt.709(tv)" => YCbCrMatrix::Bt709Tv,
        "pc.709" | "bt709(pc)" | "bt.709(pc)" => YCbCrMatrix::Bt709Pc,
        "tv.240m" | "smpte240m(tv)" => YCbCrMatrix::Smpte240mTv,
        "pc.240m" | "smpte240m(pc)" => YCbCrMatrix::Smpte240mPc,
        "tv.fcc" | "fcc(tv)" => YCbCrMatrix::FccTv,
        "pc.fcc" | "fcc(pc)" => YCbCrMatrix::FccPc,
        "" => YCbCrMatrix::Default,
        _ => YCbCrMatrix::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_ass_script() {
        let input = "[Script Info]\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:03.50,Default,,0000,0000,0000,,Hello, world!";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.play_res_x, 1280);
        assert_eq!(track.play_res_y, 720);
        assert_eq!(track.styles.len(), 1);
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].start, 1000);
        assert_eq!(track.events[0].duration, 2500);
        assert_eq!(track.events[0].style, 0);
        assert_eq!(track.events[0].text, "Hello, world!");
        assert_eq!(
            track.styles[0].alignment,
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
            track.styles[0].alignment,
            ass::VALIGN_CENTER | ass::HALIGN_CENTER
        );
    }

    #[test]
    fn resolves_event_style_by_name() {
        let input = "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\nStyle: Sign,DejaVu Sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,8,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Sign,,0000,0000,0000,,Visible text";
        let track = parse_script_text(input).expect("script should parse");

        assert_eq!(track.styles.len(), 2);
        assert_eq!(track.events.len(), 1);
        assert_eq!(track.events[0].style, 1);
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
        assert!(inverse.inverse_clip);
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
    fn parses_numeric_bold_weights_like_libass_boolean_thresholds() {
        let style_track = parse_script_text(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Light,sans,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,400,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\nStyle: Bold,sans,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,700,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n",
        )
        .expect("style script should parse");
        assert!(!style_track.styles[0].bold);
        assert!(style_track.styles[1].bold);

        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\b100}Thin{\\b400}Regular{\\b700}Heavy{\\b1}Legacy{\\b0}Off",
            &base_style,
            &[],
        );

        let spans = &parsed.lines[0].spans;
        assert!(!spans[0].style.bold);
        assert!(spans[0].text.contains("Thin"));
        assert!(spans[0].text.contains("Regular"));
        assert!(spans[1].style.bold);
        assert!(spans[1].text.contains("Heavy"));
        assert!(spans[1].text.contains("Legacy"));
        assert!(!spans[2].style.bold);
        assert_eq!(spans[2].text, "Off");
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
    fn parses_backslash_n_as_space_unless_wrap_style_two() {
        let base_style = ParsedStyle::default();
        let normal = parse_dialogue_text("one\\ntwo", &base_style, &[]);
        assert_eq!(normal.lines.len(), 1);
        assert_eq!(normal.lines[0].spans[0].text, "one two");

        let q2 = parse_dialogue_text("{\\q2}one\\ntwo", &base_style, &[]);
        assert_eq!(q2.lines.len(), 2);
        assert_eq!(q2.lines[0].spans[0].text, "one");
        assert_eq!(q2.lines[1].spans[0].text, "two");
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
        assert_eq!(drawing.polygons.len(), 2);
        assert_eq!(
            drawing.polygons[0].first().copied(),
            Some(Point { x: 0, y: 0 })
        );
        assert_eq!(
            drawing.polygons[1].first().copied(),
            Some(Point { x: 20, y: 20 })
        );
    }

    #[test]
    fn parses_timed_transform_overrides() {
        let base_style = ParsedStyle::default();
        let parsed = parse_dialogue_text(
            "{\\t(100,300,2,\\1c&H112233&\\fs48\\fscx150\\fscy50\\fsp4\\bord6\\blur2)}Text",
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
        assert_eq!(parsed.lines[0].spans[0].style.back_colour, 0x0044_5566);
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
