use rassa_core::{RassaError, RassaResult, Rect, ass};
use rassa_fonts::{
    FontMatch, FontProvider, FontQuery, font_match_supports_text, resolve_system_font_for_char,
};
use rassa_parse::{
    ParsedDrawing, ParsedEvent, ParsedFade, ParsedKaraokeSpan, ParsedMovement, ParsedSpanStyle,
    ParsedSpanTransform, ParsedStyle, ParsedTrack, ParsedVectorClip, parse_dialogue_text,
};
use rassa_shape::{GlyphInfo, ShapeEngine, ShapeRequest, ShapingMode};
use rassa_unibreak::{LineBreakOpportunity, classify_line_breaks};
use rassa_unicode::BidiDirection;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutGlyphRun {
    pub text: String,
    pub direction: BidiDirection,
    pub font_family: String,
    pub font: FontMatch,
    pub glyphs: Vec<GlyphInfo>,
    pub width: f32,
    pub style: ParsedSpanStyle,
    pub transforms: Vec<ParsedSpanTransform>,
    pub karaoke: Option<ParsedKaraokeSpan>,
    pub drawing: Option<ParsedDrawing>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutLine {
    pub event_index: usize,
    pub style_index: usize,
    pub text: String,
    pub direction: BidiDirection,
    pub glyph_count: usize,
    pub width: f32,
    pub runs: Vec<LayoutGlyphRun>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutEvent {
    pub event_index: usize,
    pub style_index: usize,
    pub text: String,
    pub font_family: String,
    pub font: FontMatch,
    pub alignment: i32,
    pub justify: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub position: Option<(i32, i32)>,
    pub movement: Option<ParsedMovement>,
    pub fade: Option<ParsedFade>,
    pub clip_rect: Option<Rect>,
    pub vector_clip: Option<ParsedVectorClip>,
    pub inverse_clip: bool,
    pub wrap_style: Option<i32>,
    pub origin: Option<(i32, i32)>,
    pub lines: Vec<LayoutLine>,
}

#[derive(Default)]
pub struct LayoutEngine {
    shaper: ShapeEngine,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn layout_track_event_with_mode<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        event_index: usize,
        provider: &P,
        shaping_mode: ShapingMode,
    ) -> RassaResult<LayoutEvent> {
        let event = track
            .events
            .get(event_index)
            .ok_or_else(|| RassaError::new(format!("event index {event_index} out of range")))?;
        let style_index = normalize_style_index(track, event);
        let style = track
            .styles
            .get(style_index)
            .unwrap_or(&track.styles[track.default_style as usize]);
        let parsed_text = parse_dialogue_text(&event.text, style, &track.styles);
        let font = provider.resolve(&FontQuery {
            family: style.font_name.clone(),
            style: None,
        });
        let explicit_lines = parsed_text
            .lines
            .iter()
            .map(|line| {
                layout_line_from_text(
                    event_index,
                    style_index,
                    line,
                    provider,
                    &self.shaper,
                    &track.language,
                    shaping_mode,
                )
            })
            .collect::<RassaResult<Vec<_>>>()?;
        let wrap_style = parsed_text
            .wrap_style
            .unwrap_or(track.wrap_style)
            .clamp(0, 3);
        let alignment = parsed_text.alignment.unwrap_or(style.alignment);
        let max_width = auto_wrap_width(track, event, style, parsed_text.position, alignment);
        let lines = wrap_layout_lines(explicit_lines, max_width, wrap_style, &track.language)?;

        Ok(LayoutEvent {
            event_index,
            style_index,
            text: parsed_text
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            font_family: font.family.clone(),
            font: font.clone(),
            alignment: parsed_text.alignment.unwrap_or(style.alignment),
            justify: normalize_justify(style.justify, style.alignment),
            margin_l: resolve_margin(event.margin_l, style.margin_l),
            margin_r: resolve_margin(event.margin_r, style.margin_r),
            margin_v: resolve_margin(event.margin_v, style.margin_v),
            position: parsed_text.position,
            movement: parsed_text.movement,
            fade: parsed_text.fade,
            clip_rect: parsed_text.clip_rect,
            vector_clip: parsed_text.vector_clip,
            inverse_clip: parsed_text.inverse_clip,
            wrap_style: parsed_text.wrap_style,
            origin: parsed_text.origin,
            lines,
        })
    }

    pub fn layout_track_event<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        event_index: usize,
        provider: &P,
    ) -> RassaResult<LayoutEvent> {
        self.layout_track_event_with_mode(track, event_index, provider, ShapingMode::Complex)
    }
}

fn layout_line_from_text<P: FontProvider>(
    event_index: usize,
    style_index: usize,
    line: &rassa_parse::ParsedTextLine,
    provider: &P,
    shaper: &ShapeEngine,
    language: &str,
    shaping_mode: ShapingMode,
) -> RassaResult<LayoutLine> {
    let mut runs = Vec::new();
    let mut line_direction = BidiDirection::LeftToRight;
    for span in &line.spans {
        if span.text.is_empty() {
            continue;
        }
        let font = provider.resolve(&FontQuery {
            family: span.style.font_name.clone(),
            style: font_style_name(&span.style),
        });
        if let Some(drawing) = &span.drawing {
            let width = drawing
                .bounds()
                .map(|bounds| bounds.width() as f32 * span.style.scale_x.max(0.0) as f32)
                .unwrap_or_default();
            runs.push(LayoutGlyphRun {
                text: span.text.clone(),
                direction: line_direction,
                font_family: font.family.clone(),
                font: font.clone(),
                glyphs: Vec::new(),
                width,
                style: span.style.clone(),
                transforms: span.transforms.clone(),
                karaoke: span.karaoke,
                drawing: Some(drawing.clone()),
            });
            continue;
        }
        let shaped_chunks = split_text_by_font(
            &span.text,
            provider,
            &span.style.font_name,
            font_style_name(&span.style),
        );
        for (chunk_text, chunk_font) in shaped_chunks {
            let shaped = shaper.shape_text(
                provider,
                &ShapeRequest::new(&chunk_text, &chunk_font.family)
                    .with_style(chunk_font.style.clone().unwrap_or_default())
                    .with_language(language)
                    .with_font_size(span.style.font_size as f32)
                    .with_mode(shaping_mode),
            )?;
            for shaped_run in shaped.runs {
                line_direction = shaped_run.direction;
                let run_font = shaped_run.font.clone();
                runs.push(LayoutGlyphRun {
                    text: shaped_run.text,
                    direction: shaped_run.direction,
                    font_family: run_font.family.clone(),
                    font: run_font,
                    width: text_run_width(&shaped_run.glyphs, &span.style),
                    glyphs: shaped_run.glyphs,
                    style: span.style.clone(),
                    transforms: span.transforms.clone(),
                    karaoke: span.karaoke,
                    drawing: None,
                });
            }
        }
    }

    let glyph_count = runs.iter().map(|run| run.glyphs.len()).sum();
    let width = runs.iter().map(|run| run.width).sum();
    Ok(LayoutLine {
        event_index,
        style_index,
        text: line.text.clone(),
        direction: line_direction,
        glyph_count,
        width,
        runs,
    })
}

fn auto_wrap_width(
    track: &ParsedTrack,
    event: &ParsedEvent,
    style: &ParsedStyle,
    position: Option<(i32, i32)>,
    alignment: i32,
) -> f32 {
    if track.play_res_x == ParsedTrack::default().play_res_x
        && track.play_res_y == ParsedTrack::default().play_res_y
        && track.layout_res_x == 0
        && track.layout_res_y == 0
    {
        return f32::INFINITY;
    }
    let margin_l = resolve_margin(event.margin_l, style.margin_l).max(0);
    let margin_r = resolve_margin(event.margin_r, style.margin_r).max(0);
    let full_width = (track.play_res_x - margin_l - margin_r).max(0);
    let Some((x, _)) = position else {
        return full_width as f32;
    };

    let left = (x - margin_l).max(0);
    let right = (track.play_res_x - margin_r - x).max(0);
    match alignment & 0x3 {
        ass::HALIGN_LEFT => right as f32,
        ass::HALIGN_RIGHT => left as f32,
        _ => (left.min(right) * 2).min(full_width) as f32,
    }
}

fn wrap_layout_lines(
    lines: Vec<LayoutLine>,
    max_width: f32,
    wrap_style: i32,
    language: &str,
) -> RassaResult<Vec<LayoutLine>> {
    if wrap_style == 2 || max_width <= 0.0 || !max_width.is_finite() {
        return Ok(lines);
    }

    let mut wrapped = Vec::new();
    for line in lines {
        wrapped.extend(wrap_layout_line(line, max_width, language)?);
    }
    Ok(wrapped)
}

#[derive(Clone, Debug)]
struct LayoutPiece {
    text: String,
    run: LayoutGlyphRun,
    width: f32,
    char_index: usize,
}

fn wrap_layout_line(
    line: LayoutLine,
    max_width: f32,
    language: &str,
) -> RassaResult<Vec<LayoutLine>> {
    if line.width <= max_width || line.text.chars().count() <= 1 {
        return Ok(vec![line]);
    }

    let breaks = classify_line_breaks(&line.text, Some(language))?;
    let pieces = line_to_pieces(&line);
    if pieces.len() <= 1 {
        return Ok(vec![line]);
    }

    let mut output = Vec::new();
    let mut current: Vec<LayoutPiece> = Vec::new();
    let mut current_width = 0.0_f32;
    let mut last_break_pos: Option<usize> = None;

    for piece in pieces {
        current_width += piece.width;
        current.push(piece);
        let char_index = current.last().map(|piece| piece.char_index).unwrap_or(0);
        if matches!(
            breaks.get(char_index),
            Some(LineBreakOpportunity::Allowed | LineBreakOpportunity::Mandatory)
        ) {
            last_break_pos = Some(current.len());
        }

        if current_width > max_width && current.len() > 1 {
            let split_at = last_break_pos
                .filter(|pos| *pos > 0 && *pos < current.len())
                .unwrap_or(current.len() - 1);
            let mut remainder = current.split_off(split_at);
            trim_wrapped_line_edges(&mut current, false);
            if !current.is_empty() {
                output.push(line_from_pieces(&line, &current));
            }
            trim_wrapped_line_edges(&mut remainder, true);
            current_width = pieces_width(&remainder);
            current = remainder;
            last_break_pos = last_allowed_break_pos(&current, &breaks);
        }
    }

    trim_wrapped_line_edges(&mut current, false);
    if !current.is_empty() {
        output.push(line_from_pieces(&line, &current));
    }

    if output.is_empty() {
        Ok(vec![line])
    } else {
        Ok(output)
    }
}

fn line_to_pieces(line: &LayoutLine) -> Vec<LayoutPiece> {
    let mut pieces = Vec::new();
    let mut char_index = 0_usize;
    for run in &line.runs {
        let chars = run.text.chars().collect::<Vec<_>>();
        if run.drawing.is_some() || chars.is_empty() || chars.len() != run.glyphs.len() {
            pieces.push(LayoutPiece {
                text: run.text.clone(),
                run: run.clone(),
                width: run.width,
                char_index: char_index + chars.len().saturating_sub(1),
            });
            char_index += chars.len();
            continue;
        }

        let scale_x = run.style.scale_x.max(0.0) as f32;
        let spacing = if run.style.spacing.is_finite() {
            run.style.spacing as f32 * scale_x
        } else {
            0.0
        };
        for (offset, (character, glyph)) in chars.into_iter().zip(run.glyphs.iter()).enumerate() {
            let mut piece_run = run.clone();
            piece_run.text = character.to_string();
            piece_run.glyphs = vec![glyph.clone()];
            piece_run.width = glyph.x_advance * scale_x + spacing;
            pieces.push(LayoutPiece {
                text: character.to_string(),
                width: piece_run.width,
                run: piece_run,
                char_index: char_index + offset,
            });
        }
        char_index += run.text.chars().count();
    }
    pieces
}

fn trim_wrapped_line_edges(pieces: &mut Vec<LayoutPiece>, trim_leading: bool) {
    while pieces
        .last()
        .is_some_and(|piece| piece.text.chars().all(char::is_whitespace))
    {
        pieces.pop();
    }
    if trim_leading {
        let leading = pieces
            .iter()
            .take_while(|piece| piece.text.chars().all(char::is_whitespace))
            .count();
        if leading > 0 {
            pieces.drain(0..leading);
        }
    }
}

fn pieces_width(pieces: &[LayoutPiece]) -> f32 {
    pieces.iter().map(|piece| piece.width).sum()
}

fn last_allowed_break_pos(
    pieces: &[LayoutPiece],
    breaks: &[LineBreakOpportunity],
) -> Option<usize> {
    pieces.iter().enumerate().rev().find_map(|(index, piece)| {
        matches!(
            breaks.get(piece.char_index),
            Some(LineBreakOpportunity::Allowed | LineBreakOpportunity::Mandatory)
        )
        .then_some(index + 1)
    })
}

fn line_from_pieces(source: &LayoutLine, pieces: &[LayoutPiece]) -> LayoutLine {
    let runs = pieces
        .iter()
        .map(|piece| piece.run.clone())
        .collect::<Vec<_>>();
    let text = pieces
        .iter()
        .map(|piece| piece.text.as_str())
        .collect::<String>();
    let glyph_count = runs.iter().map(|run| run.glyphs.len()).sum();
    let width = runs.iter().map(|run| run.width).sum();
    LayoutLine {
        event_index: source.event_index,
        style_index: source.style_index,
        text,
        direction: source.direction,
        glyph_count,
        width,
        runs,
    }
}

fn text_run_width(glyphs: &[GlyphInfo], style: &ParsedSpanStyle) -> f32 {
    let scale_x = style.scale_x.max(0.0) as f32;
    let spacing = if style.spacing.is_finite() {
        style.spacing as f32 * scale_x
    } else {
        0.0
    };
    glyphs
        .iter()
        .map(|glyph| glyph.x_advance * scale_x + spacing)
        .sum()
}

fn split_text_by_font<P: FontProvider>(
    text: &str,
    provider: &P,
    family: &str,
    style: Option<String>,
) -> Vec<(String, FontMatch)> {
    let base_font = provider.resolve(&FontQuery {
        family: family.to_string(),
        style: style.clone(),
    });
    let mut chunks: Vec<(String, FontMatch)> = Vec::new();

    for character in text.chars() {
        let font = if base_font.path.is_none()
            || character.is_whitespace()
            || character.is_control()
            || base_font
                .path
                .as_ref()
                .is_some_and(|_| font_match_supports_text(&base_font, &character.to_string()))
        {
            base_font.clone()
        } else {
            resolve_system_font_for_char(family, style.as_deref(), character)
                .map(|(resolved_family, resolved_path, face_index)| FontMatch {
                    family: resolved_family,
                    path: resolved_path,
                    face_index,
                    style: style.clone(),
                    provider: base_font.provider,
                })
                .unwrap_or_else(|| base_font.clone())
        };

        if let Some((chunk, chunk_font)) = chunks.last_mut() {
            if same_font_match(chunk_font, &font) {
                chunk.push(character);
                continue;
            }
        }
        chunks.push((character.to_string(), font));
    }

    chunks
}

fn same_font_match(left: &FontMatch, right: &FontMatch) -> bool {
    left.family == right.family
        && left.path == right.path
        && left.face_index == right.face_index
        && left.style == right.style
}

fn font_style_name(style: &ParsedSpanStyle) -> Option<String> {
    match (style.bold, style.italic) {
        (true, true) => Some("Bold Italic".to_string()),
        (true, false) => Some("Bold".to_string()),
        (false, true) => Some("Italic".to_string()),
        (false, false) => None,
    }
}

fn normalize_style_index(track: &ParsedTrack, event: &ParsedEvent) -> usize {
    if track.styles.is_empty() {
        return 0;
    }

    let candidate = usize::try_from(event.style).unwrap_or(0);
    if candidate < track.styles.len() {
        candidate
    } else {
        usize::try_from(track.default_style)
            .ok()
            .filter(|index| *index < track.styles.len())
            .unwrap_or(0)
    }
}

fn resolve_margin(event_margin: i32, style_margin: i32) -> i32 {
    if event_margin == 0 {
        style_margin
    } else {
        event_margin
    }
}

fn normalize_justify(justify: i32, alignment: i32) -> i32 {
    if justify != ass::ASS_JUSTIFY_AUTO {
        return justify;
    }

    match alignment & 0x3 {
        ass::HALIGN_LEFT => ass::ASS_JUSTIFY_LEFT,
        ass::HALIGN_RIGHT => ass::ASS_JUSTIFY_RIGHT,
        _ => ass::ASS_JUSTIFY_CENTER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_fonts::{FontconfigProvider, NullFontProvider, font_match_supports_text};
    use rassa_parse::{ParsedKaraokeMode, ParsedTrack, parse_script_text};

    fn parse_track(input: &str) -> ParsedTrack {
        parse_script_text(input).expect("script should parse")
    }

    #[test]
    fn layout_uses_style_font_and_event_margins() {
        let track = parse_track(
            "[Script Info]\nLanguage: en\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding, Justify\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,11,12,13,1,0\nStyle: Sign,DejaVu Sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,9,21,22,23,1,0\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Sign,,0030,0000,0040,,Visible text",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.style_index, 1);
        assert_eq!(layout.font_family, "DejaVu Sans");
        assert_eq!(layout.margin_l, 30);
        assert_eq!(layout.margin_r, 22);
        assert_eq!(layout.margin_v, 40);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].glyph_count, "Visible text".chars().count());
        assert_eq!(layout.lines[0].runs.len(), 1);
    }

    #[test]
    fn layout_splits_lines_on_mandatory_breaks() {
        let mut track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,seed",
        );
        track.events[0].text = "a\nb".to_string();
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text, "a");
        assert_eq!(layout.lines[1].text, "b");
    }

    #[test]
    fn layout_wraps_long_text_at_unicode_line_breaks() {
        let track = parse_track(
            "[Script Info]
PlayResX: 8
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,alpha beta gamma delta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert!(layout.lines.len() > 1);
        assert!(layout.lines.iter().all(|line| line.width <= 4.0));
        assert!(layout.lines.iter().all(|line| !line.text.starts_with(' ')));
        assert!(layout.lines.iter().all(|line| !line.text.ends_with(' ')));
    }

    #[test]
    fn layout_q2_disables_automatic_wrapping() {
        let track = parse_track(
            "[Script Info]
PlayResX: 8
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\q2}alpha beta gamma delta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert_eq!(layout.lines.len(), 1);
        assert!(layout.lines[0].width > 4.0);
    }

    #[test]
    fn layout_wraps_positioned_center_text_within_anchor_space() {
        let track = parse_track(
            "[Script Info]
PlayResX: 40
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\pos(10,20)\\an5\\q0}alpha beta gamma delta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert!(layout.lines.len() > 1);
        assert!(layout.lines.iter().all(|line| line.width <= 16.0));
    }

    #[test]
    fn layout_wraps_cjk_using_unicode_line_break_opportunities() {
        let track = parse_track(
            "[Script Info]
Language: ja
PlayResX: 6
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,日本語日本語",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert!(layout.lines.len() > 1);
        assert!(layout.lines.iter().all(|line| line.width <= 2.0));
    }

    #[test]
    fn layout_applies_font_override_runs() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fnDejaVu Sans}Hello{\\fnArial} world",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].runs.len(), 2);
        assert_eq!(layout.lines[0].runs[0].style.font_name, "DejaVu Sans");
        assert_eq!(layout.lines[0].runs[1].style.font_name, "Arial");
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn layout_splits_cjk_text_to_covered_fallback_font_run() {
        if resolve_system_font_for_char("DejaVu Sans", None, '日').is_none() {
            eprintln!("skipping: system fontconfig has no CJK-capable fallback font");
            return;
        }
        let track = parse_track(
            "[Script Info]\nLanguage: ja\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,DejaVu Sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,abc 日本語",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        let cjk_run = layout.lines[0]
            .runs
            .iter()
            .find(|run| run.text.contains('日'))
            .expect("CJK text should be retained in a glyph run");
        assert!(font_match_supports_text(&cjk_run.font, "日本語"));
        assert_ne!(cjk_run.font_family, "DejaVu Sans");
    }

    #[test]
    fn layout_carries_clip_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\iclip(10,20,30,40)}Clip",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(
            layout.clip_rect,
            Some(Rect {
                x_min: 10,
                y_min: 20,
                x_max: 30,
                y_max: 40
            })
        );
        assert!(layout.vector_clip.is_none());
        assert!(layout.inverse_clip);
    }

    #[test]
    fn layout_carries_vector_clip_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\clip(m 0 0 l 8 0 8 8 0 8)}Clip",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert!(layout.clip_rect.is_none());
        assert!(layout.vector_clip.is_some());
        assert!(!layout.inverse_clip);
    }

    #[test]
    fn layout_carries_move_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\move(1,2,3,4,50,150)}Move",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(
            layout.movement,
            Some(ParsedMovement {
                start: (1, 2),
                end: (3, 4),
                t1_ms: 50,
                t2_ms: 150,
            })
        );
    }

    #[test]
    fn layout_carries_fade_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fad(100,200)}Fade",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(
            layout.fade,
            Some(ParsedFade::Simple {
                fade_in_ms: 100,
                fade_out_ms: 200,
            })
        );
    }

    #[test]
    fn layout_carries_full_fade_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fade(10,20,30,40,50,60,70)}Fade",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(
            layout.fade,
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
    fn layout_carries_karaoke_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\k10}Ka{\\k20}ra",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines[0].runs.len(), 2);
        assert_eq!(
            layout.lines[0].runs[0].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 0,
                duration_ms: 100,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
        assert_eq!(
            layout.lines[0].runs[1].karaoke,
            Some(ParsedKaraokeSpan {
                start_ms: 100,
                duration_ms: 200,
                mode: ParsedKaraokeMode::FillSwap,
            })
        );
    }

    #[test]
    fn layout_carries_transform_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H000000FF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\t(0,1000,\\bord4\\1c&H00112233&)}Hi",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines[0].runs[0].transforms.len(), 1);
        assert_eq!(
            layout.lines[0].runs[0].transforms[0].style.border,
            Some(4.0)
        );
        assert_eq!(
            layout.lines[0].runs[0].transforms[0].style.primary_colour,
            Some(0x0011_2233)
        );
    }

    #[test]
    fn layout_carries_drawing_runs() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 l 8 0 8 8 0 8",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines[0].runs.len(), 1);
        assert!(layout.lines[0].runs[0].drawing.is_some());
        assert_eq!(layout.lines[0].runs[0].width, 9.0);
    }

    #[test]
    fn layout_carries_missing_override_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\u1\\s1\\a10\\q2\\org(320,240)\\frx12\\fry-8\\fax0.25\\fay-0.5\\xbord3\\ybord4\\xshad5\\yshad-6\\be2\\pbo7}Meta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.alignment, ass::VALIGN_CENTER | ass::HALIGN_CENTER);
        assert_eq!(layout.wrap_style, Some(2));
        assert_eq!(layout.origin, Some((320, 240)));
        let style = &layout.lines[0].runs[0].style;
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
    fn layout_accepts_explicit_shaping_mode() {
        let track = parse_track(
            "[Script Info]\nLanguage: en\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,36,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,office",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let simple = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("simple layout should succeed");
        let complex = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Complex)
            .expect("complex layout should succeed");

        assert_eq!(simple.lines.len(), 1);
        assert_eq!(complex.lines.len(), 1);
        assert_eq!(simple.lines[0].text, "office");
        assert_eq!(complex.lines[0].text, "office");
    }
}
