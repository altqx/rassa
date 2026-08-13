use rassa_core::{RassaError, RassaResult, Rect, ass};
use rassa_fonts::{
    FontMatch, FontProvider, FontQuery, font_match_supports_text, resolve_system_font_for_char,
};
use rassa_parse::{
    LIBASS_OUTLINE_MAX_D6, ParsedDrawing, ParsedEvent, ParsedFade, ParsedKaraokeSpan,
    ParsedMovement, ParsedMovementExact, ParsedRectF64, ParsedSpanStyle, ParsedSpanTransform,
    ParsedStyle, ParsedTrack, ParsedVectorClip, libass_drawing_coordinate_to_d6,
    libass_drawing_scale_base, parse_dialogue_text_with_wrap_style,
};
use rassa_raster::{RasterOptions, Rasterizer};
use rassa_shape::{
    GlyphInfo, GlyphPositioning, ShapeEngine, ShapeRequest, ShapingMode, reorder_bidi_runs,
};
use rassa_unibreak::{LineBreakOpportunity, classify_line_breaks};
#[cfg(test)]
use rassa_unicode::analyze_bidi_with_base;
use rassa_unicode::{BidiDirection, analyze_bidi_with_base_and_brackets};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutFeatures {
    pub wrap_unicode: bool,
    pub bidi_brackets: bool,
    pub whole_text_layout: bool,
}

/// Device-space wrap scales (identity for script-space callers; renderer supplies screen scales).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutWrapScales {
    pub text: f64,
    pub spacing: f64,
    pub drawing: f64,
    pub available_width: f64,
    pub available_width_extra: f64,
}

impl Default for LayoutWrapScales {
    fn default() -> Self {
        Self {
            text: 1.0,
            spacing: 1.0,
            drawing: 1.0,
            available_width: 1.0,
            available_width_extra: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutGlyphRun {
    pub text: String,
    pub direction: BidiDirection,
    pub bidi_level: u8,
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
    pub hard_override: bool,
    pub transform_disables_collision: bool,
    pub font_family: String,
    pub font: FontMatch,
    pub alignment: i32,
    pub justify: i32,
    pub margin_l: i32,
    pub margin_r: i32,
    pub margin_v: i32,
    pub position: Option<(i32, i32)>,
    pub position_exact: Option<(f64, f64)>,
    pub movement: Option<ParsedMovement>,
    pub movement_exact: Option<ParsedMovementExact>,
    pub fade: Option<ParsedFade>,
    pub clip_rect: Option<Rect>,
    pub vector_clip: Option<ParsedVectorClip>,
    pub inverse_clip: bool,
    pub vector_clip_inverse: bool,
    pub wrap_style: Option<i32>,
    pub origin: Option<(i32, i32)>,
    pub origin_exact: Option<(f64, f64)>,
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
        self.layout_track_event_with_options(track, event_index, provider, shaping_mode, false)
    }

    pub fn layout_track_event_with_options<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        event_index: usize,
        provider: &P,
        shaping_mode: ShapingMode,
        wrap_unicode: bool,
    ) -> RassaResult<LayoutEvent> {
        self.layout_track_event_with_features(
            track,
            event_index,
            provider,
            shaping_mode,
            LayoutFeatures {
                wrap_unicode,
                ..LayoutFeatures::default()
            },
        )
    }

    pub fn layout_track_event_with_features<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        event_index: usize,
        provider: &P,
        shaping_mode: ShapingMode,
        features: LayoutFeatures,
    ) -> RassaResult<LayoutEvent> {
        self.layout_track_event_with_features_and_wrap_scales(
            track,
            event_index,
            provider,
            shaping_mode,
            features,
            LayoutWrapScales::default(),
        )
    }

    pub fn layout_track_event_with_features_and_wrap_scales<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        event_index: usize,
        provider: &P,
        shaping_mode: ShapingMode,
        features: LayoutFeatures,
        wrap_scales: LayoutWrapScales,
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
        let banner_no_wrap = banner_effect_forces_no_wrap(event);
        let event_wrap_style = if banner_no_wrap { 2 } else { track.wrap_style };
        let parsed_text = parse_dialogue_text_with_wrap_style(
            &event.text,
            style,
            &track.styles,
            event_wrap_style,
        );
        let font = provider.resolve(&FontQuery {
            family: style.font_name.clone(),
            style: None,
            weight: font_query_weight(style.font_weight),
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
                    track.kerning,
                    features.whole_text_layout,
                    features.bidi_brackets,
                )
            })
            .collect::<RassaResult<Vec<_>>>()?;
        let parsed_wrap_style = parsed_text
            .wrap_style
            .unwrap_or(event_wrap_style)
            .clamp(0, 3);
        let wrap_style = parsed_wrap_style;
        let alignment = parsed_text.alignment.unwrap_or(style.alignment);
        let max_width = auto_wrap_width(track, event, style, parsed_text.position, alignment)
            * finite_nonnegative_or_one(wrap_scales.available_width) as f32
            + finite_nonnegative_or_zero(wrap_scales.available_width_extra) as f32;
        // wrap_lines_smart: \N is a forced break; each explicit segment still auto-wraps.
        let lines = wrap_layout_lines(
            explicit_lines,
            max_width,
            wrap_style,
            &track.language,
            features.wrap_unicode,
            wrap_scales,
        )?
        .into_iter()
        .map(trim_line_edge_whitespace)
        .collect();

        Ok(LayoutEvent {
            event_index,
            style_index,
            text: parsed_text
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            hard_override: parsed_text.hard_override,
            transform_disables_collision: parsed_text.transform_disables_collision,
            font_family: font.family.clone(),
            font: font.clone(),
            alignment,
            justify: normalize_justify(style.justify, alignment),
            margin_l: resolve_margin(event.margin_l, style.margin_l),
            margin_r: resolve_margin(event.margin_r, style.margin_r),
            margin_v: resolve_margin(event.margin_v, style.margin_v),
            position: parsed_text.position,
            position_exact: parsed_text.position_exact,
            movement: parsed_text.movement,
            movement_exact: parsed_text.movement_exact,
            fade: parsed_text.fade,
            clip_rect: parsed_text
                .clip_rect
                .or_else(|| parsed_text.clip_rect_exact.map(rect_from_exact_clip)),
            vector_clip: parsed_text.vector_clip,
            inverse_clip: parsed_text.inverse_clip,
            vector_clip_inverse: parsed_text.vector_clip_inverse,
            wrap_style: parsed_text.wrap_style.or(banner_no_wrap.then_some(2)),
            origin: parsed_text.origin,
            origin_exact: parsed_text.origin_exact,
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

fn rect_from_exact_clip(rect: ParsedRectF64) -> Rect {
    Rect {
        x_min: rect.x_min.floor() as i32,
        y_min: rect.y_min.floor() as i32,
        x_max: rect.x_max.ceil() as i32,
        y_max: rect.y_max.ceil() as i32,
    }
}

fn banner_effect_forces_no_wrap(event: &ParsedEvent) -> bool {
    event.effect.starts_with("Banner;")
}

#[allow(clippy::too_many_arguments)]
fn layout_line_from_text<P: FontProvider>(
    event_index: usize,
    style_index: usize,
    line: &rassa_parse::ParsedTextLine,
    provider: &P,
    shaper: &ShapeEngine,
    language: &str,
    shaping_mode: ShapingMode,
    kerning: bool,
    explicit_whole_text_layout: bool,
    bidi_brackets: bool,
) -> RassaResult<LayoutLine> {
    let mut runs = Vec::new();
    let mut line_direction = BidiDirection::LeftToRight;
    // `\fe-1` enables whole-text layout; an explicit feature request does too but keeps forced-LTR for other encodings.
    let base_direction = if line
        .spans
        .first()
        .is_some_and(|span| span.style.encoding == -1)
    {
        BidiDirection::Neutral
    } else {
        BidiDirection::LeftToRight
    };
    let whole_text_layout = explicit_whole_text_layout || base_direction == BidiDirection::Neutral;
    let whole_bidi = whole_text_layout
        .then(|| analyze_bidi_with_base_and_brackets(&line.text, base_direction, bidi_brackets))
        .transpose()?;
    if let Some(analysis) = &whole_bidi {
        line_direction = analysis.direction;
    }
    let mut line_char_cursor = 0;
    for span in &line.spans {
        let span_char_count = span.text.chars().count();
        let span_char_start = line_char_cursor;
        line_char_cursor += span_char_count;
        let font = provider.resolve(&FontQuery {
            family: font_selection_family(&span.style.font_name).to_owned(),
            style: font_style_name(&span.style),
            weight: font_query_weight(span.style.font_weight),
        });
        if span.text.is_empty() {
            runs.push(LayoutGlyphRun {
                text: String::new(),
                direction: line_direction,
                bidi_level: 0,
                font_family: font.family.clone(),
                font,
                glyphs: Vec::new(),
                width: 0.0,
                style: span.style.clone(),
                transforms: span.transforms.clone(),
                karaoke: None,
                drawing: None,
            });
            continue;
        }
        if let Some(drawing) = &span.drawing {
            let width = drawing_layout_width(&span.text, drawing, span.style.scale_x);
            runs.push(LayoutGlyphRun {
                text: span.text.clone(),
                direction: line_direction,
                bidi_level: whole_bidi
                    .as_ref()
                    .and_then(|analysis| analysis.embedding_levels.get(span_char_start))
                    .copied()
                    .unwrap_or(0),
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
            span.style.font_weight,
        );
        let mut span_chunk_cursor = 0;
        for (chunk_text, chunk_font) in shaped_chunks {
            // ass_resolve_base_direction: \fe-1 auto-detects bidi base; any other encoding forces LTR.
            let base_direction = if span.style.encoding == -1 {
                BidiDirection::Neutral
            } else {
                BidiDirection::LeftToRight
            };
            let chunk_char_count = chunk_text.chars().count();
            let chunk_global_start = span_char_start + span_chunk_cursor;
            span_chunk_cursor += chunk_char_count;
            let mut request = ShapeRequest::new(&chunk_text, &chunk_font.family)
                .with_style(
                    font_style_name(&span.style)
                        .or_else(|| chunk_font.style.clone())
                        .unwrap_or_default(),
                )
                .with_optional_weight(font_query_weight(span.style.font_weight))
                .with_language(language)
                .with_font_size(span.style.font_size as f32)
                .with_kerning(kerning)
                .with_vertical(span.style.font_name.starts_with('@'))
                .with_horizontal_spacing(span.style.spacing.abs() > f64::EPSILON)
                .with_base_direction(base_direction)
                .with_bidi_brackets(bidi_brackets)
                .with_mode(shaping_mode);
            if let Some(analysis) = &whole_bidi {
                let end = chunk_global_start + chunk_char_count;
                if let Some(levels) = analysis.embedding_levels.get(chunk_global_start..end) {
                    request = request
                        .with_resolved_bidi_levels(levels.to_vec())
                        .with_deferred_visual_reorder(true);
                }
            }
            let shaped = shaper.shape_text_with_font(&request, &chunk_font)?;
            for shaped_run in shaped.runs {
                if whole_bidi.is_none() {
                    line_direction = shaped_run.direction;
                }
                let run_font = shaped_run.font.clone();
                let glyphs = apply_vertical_font_advances(shaped_run.glyphs, &span.style);
                runs.push(LayoutGlyphRun {
                    text: shaped_run.text,
                    direction: shaped_run.direction,
                    bidi_level: shaped_run.bidi_level,
                    font_family: run_font.family.clone(),
                    font: run_font,
                    width: text_run_width(&glyphs, &span.style),
                    glyphs,
                    style: span.style.clone(),
                    transforms: span.transforms.clone(),
                    karaoke: span.karaoke,
                    drawing: None,
                });
            }
        }
    }

    if whole_bidi.is_some() {
        let mut tagged = runs
            .into_iter()
            .map(|run| {
                let level = run.bidi_level;
                (run, level)
            })
            .collect::<Vec<_>>();
        reorder_bidi_runs(&mut tagged);
        runs = tagged.into_iter().map(|(run, _)| run).collect();
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

fn drawing_layout_width(text: &str, drawing: &ParsedDrawing, scale_x: f64) -> f32 {
    let scale_x = scale_x.max(0.0);
    if let Some((x_min_d6, x_max_d6)) = drawing_text_x_bounds_d6(text) {
        let scale_base = libass_drawing_scale_base(drawing.scale);
        if scale_base <= 0 {
            return 0.0;
        }
        let scale_base = f64::from(scale_base);
        let width =
            (f64::from(x_max_d6) - f64::from(x_min_d6)).max(0.0) / 64.0 / scale_base * scale_x;
        return bounded_drawing_layout_width(width);
    }

    drawing
        .bounds()
        .map(|bounds| {
            bounded_drawing_layout_width(f64::from((bounds.width() - 1).max(0)) * scale_x)
        })
        .unwrap_or_default()
}

fn bounded_drawing_layout_width(width: f64) -> f32 {
    let max_pixels = f64::from(LIBASS_OUTLINE_MAX_D6) / 64.0;
    if width.is_finite() && (0.0..=max_pixels).contains(&width) {
        width as f32
    } else {
        // Out-of-domain outline transforms are rejected; an invalid drawing contributes no advance.
        0.0
    }
}

fn drawing_text_x_bounds_d6(text: &str) -> Option<(i32, i32)> {
    let tokens = split_ass_drawing_tokens(text);
    let mut index = 0;
    let mut x_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut has_current = false;
    let mut spline_active = false;

    while index < tokens.len() {
        match tokens[index].to_ascii_lowercase().as_str() {
            "m" | "n" => {
                spline_active = false;
                index += 1;
                while let Some((x, next_index)) = parse_drawing_x_coordinate_d6(&tokens, index) {
                    x_min = x_min.min(x);
                    x_max = x_max.max(x);
                    has_current = true;
                    index = next_index;
                }
            }
            "l" if has_current => {
                spline_active = false;
                index += 1;
                while let Some((x, next_index)) = parse_drawing_x_coordinate_d6(&tokens, index) {
                    x_min = x_min.min(x);
                    x_max = x_max.max(x);
                    index = next_index;
                }
            }
            "b" if has_current => {
                spline_active = false;
                index += 1;
                while let Some((xs, next_index)) = parse_drawing_bezier_xs_d6(&tokens, index) {
                    for x in xs {
                        x_min = x_min.min(x);
                        x_max = x_max.max(x);
                    }
                    index = next_index;
                }
            }
            "s" if has_current => {
                index += 1;
                if let Some((xs, next_index)) = parse_drawing_bezier_xs_d6(&tokens, index) {
                    for x in xs {
                        x_min = x_min.min(x);
                        x_max = x_max.max(x);
                    }
                    index = next_index;
                    spline_active = true;
                }
            }
            "p" if spline_active => {
                index += 1;
                while let Some((x, next_index)) = parse_drawing_x_coordinate_d6(&tokens, index) {
                    x_min = x_min.min(x);
                    x_max = x_max.max(x);
                    index = next_index;
                }
            }
            "c" => {
                spline_active = false;
                index += 1;
            }
            _ => index += 1,
        }
    }

    (x_min <= x_max).then_some((x_min, x_max))
}

fn split_ass_drawing_tokens(text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if is_ass_c_space(character) {
            if let Some(token_start) = start.take() {
                tokens.push(&text[token_start..index]);
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(token_start) = start {
        tokens.push(&text[token_start..]);
    }
    tokens
}

fn is_ass_c_space(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}'
    )
}

fn parse_drawing_bezier_xs_d6(tokens: &[&str], index: usize) -> Option<([i32; 3], usize)> {
    let (x1, next_index) = parse_drawing_x_coordinate_d6(tokens, index)?;
    let (x2, next_index) = parse_drawing_x_coordinate_d6(tokens, next_index)?;
    let (x3, next_index) = parse_drawing_x_coordinate_d6(tokens, next_index)?;
    Some(([x1, x2, x3], next_index))
}

fn parse_drawing_x_coordinate_d6(tokens: &[&str], index: usize) -> Option<(i32, usize)> {
    let x = parse_drawing_number_prefix(tokens.get(index)?)?;
    let y = parse_drawing_number_prefix(tokens.get(index + 1)?)?;
    let x = libass_drawing_coordinate_to_d6(x)?;
    libass_drawing_coordinate_to_d6(y)?;
    Some((x, index + 2))
}

fn parse_drawing_number_prefix(token: &str) -> Option<f64> {
    if token.parse::<f64>().is_ok() {
        return token.parse().ok();
    }

    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut previous_was_exponent = false;
    for (index, character) in token.char_indices() {
        let allowed = if character.is_ascii_digit() {
            seen_digit = true;
            previous_was_exponent = false;
            true
        } else if (character == '+' || character == '-') && (index == 0 || previous_was_exponent) {
            previous_was_exponent = false;
            true
        } else if character == '.' && !seen_dot && !previous_was_exponent {
            seen_dot = true;
            true
        } else if (character == 'e' || character == 'E') && seen_digit && !previous_was_exponent {
            previous_was_exponent = true;
            true
        } else {
            false
        };
        if !allowed {
            break;
        }
        end = index + character.len_utf8();
    }

    if !seen_digit || end == 0 || previous_was_exponent {
        return None;
    }
    token[..end].parse().ok()
}

fn auto_wrap_width(
    track: &ParsedTrack,
    event: &ParsedEvent,
    style: &ParsedStyle,
    _position: Option<(i32, i32)>,
    _alignment: i32,
) -> f32 {
    let margin_l = resolve_margin(event.margin_l, style.margin_l).max(0);
    let margin_r = resolve_margin(event.margin_r, style.margin_r).max(0);
    (track.play_res_x - margin_l - margin_r).max(0) as f32
}

fn wrap_layout_lines(
    lines: Vec<LayoutLine>,
    max_width: f32,
    wrap_style: i32,
    language: &str,
    wrap_unicode: bool,
    wrap_scales: LayoutWrapScales,
) -> RassaResult<Vec<LayoutLine>> {
    if wrap_style == 2 || max_width <= 0.0 || !max_width.is_finite() {
        return Ok(lines);
    }

    let mut wrapped = Vec::new();
    for line in lines {
        wrapped.extend(wrap_layout_line(
            line,
            max_width,
            wrap_style,
            language,
            wrap_unicode,
            wrap_scales,
        )?);
    }
    Ok(wrapped)
}

#[derive(Clone, Debug)]
struct LayoutPiece {
    text: String,
    run: LayoutGlyphRun,
    width: f32,
    ink_min: f32,
    ink_max: f32,
    has_ink: bool,
    char_index: usize,
}

fn wrap_layout_line(
    line: LayoutLine,
    max_width: f32,
    wrap_style: i32,
    language: &str,
    wrap_unicode: bool,
    wrap_scales: LayoutWrapScales,
) -> RassaResult<Vec<LayoutLine>> {
    if line.text.chars().count() <= 1 {
        return Ok(vec![line]);
    }

    // ALLOWBREAK default (WRAP_UNICODE off): only ASCII spaces are break opportunities.
    let breaks = if wrap_unicode {
        classify_line_breaks(&line.text, Some(language))?
    } else {
        line.text
            .chars()
            .map(|character| {
                if character == ' ' {
                    LineBreakOpportunity::Allowed
                } else {
                    LineBreakOpportunity::Prohibited
                }
            })
            .collect()
    };
    let pieces = line_to_pieces(&line, wrap_scales);
    if pieces.len() <= 1 {
        return Ok(vec![line]);
    }
    // Wrap tests the positioned glyph bbox, not advance sums; italic overhang can overflow a fitting advance.
    if pieces_max_positioned_width(&pieces) < max_width {
        return Ok(vec![line]);
    }
    let piece_breakable = |piece: &LayoutPiece| {
        matches!(
            breaks.get(piece.char_index),
            Some(LineBreakOpportunity::Allowed | LineBreakOpportunity::Mandatory)
        )
    };

    // wrap_lines_naive: last breakable on overflow; unbreakable words overflow then break at the next space.
    let mut wrapped: Vec<Vec<LayoutPiece>> = Vec::new();
    let mut current: Vec<LayoutPiece> = Vec::new();
    let mut last_break_pos: Option<usize> = None;

    for piece in pieces.iter().cloned() {
        let is_whitespace = is_libass_trimmed_whitespace(&piece.text);
        current.push(piece);
        if pieces_positioned_width(&current) >= max_width && !is_whitespace {
            if let Some(split_at) = last_break_pos.filter(|pos| *pos > 0 && *pos < current.len()) {
                let mut remainder = current.split_off(split_at);
                trim_wrapped_line_edges(&mut current, false);
                if !current.is_empty() {
                    wrapped.push(current);
                }
                trim_wrapped_line_edges(&mut remainder, true);
                current = remainder;
                last_break_pos = None;
            }
        }
        if current.last().map(&piece_breakable).unwrap_or(false) {
            last_break_pos = Some(current.len());
        }
    }

    trim_wrapped_line_edges(&mut current, false);
    if !current.is_empty() {
        wrapped.push(current);
    }

    // wrap_lines_rebalance (styles != 1; 0 and 3 are identical): move the last word if it evens the pair.
    if wrap_style != 1 {
        let mut changed = true;
        while changed {
            changed = false;
            let mut index = 0;
            while index + 1 < wrapped.len() {
                if let Some((left, right)) =
                    rebalance_pair(&wrapped[index], &wrapped[index + 1], &piece_breakable)
                {
                    wrapped[index] = left;
                    wrapped[index + 1] = right;
                    changed = true;
                }
                index += 1;
            }
        }
    }

    if wrapped.is_empty() {
        Ok(vec![line])
    } else {
        Ok(wrapped
            .iter()
            .map(|pieces| line_from_pieces(&line, pieces))
            .collect())
    }
}

/// Accept moving left's last word onto right when that evens the pair (wrap_lines_rebalance).
fn rebalance_pair(
    left: &[LayoutPiece],
    right: &[LayoutPiece],
    piece_breakable: &dyn Fn(&LayoutPiece) -> bool,
) -> Option<(Vec<LayoutPiece>, Vec<LayoutPiece>)> {
    let mut word_start = left.len();
    while word_start > 0 && is_libass_trimmed_whitespace(&left[word_start - 1].text) {
        word_start -= 1;
    }
    let word_end = word_start;
    while word_start > 0 && !piece_breakable(&left[word_start - 1]) {
        word_start -= 1;
    }
    if word_start == 0 || word_end == 0 {
        // Merging line breaks is never beneficial.
        return None;
    }

    let trimmed_width = |pieces: &[LayoutPiece]| {
        let mut end = pieces.len();
        while end > 0 && is_libass_trimmed_whitespace(&pieces[end - 1].text) {
            end -= 1;
        }
        let mut start = 0;
        while start < end && is_libass_trimmed_whitespace(&pieces[start].text) {
            start += 1;
        }
        pieces_positioned_width(&pieces[start..end])
    };

    let l1 = trimmed_width(left);
    let l2 = trimmed_width(right);
    let new_left = &left[..word_start];
    let moved = &left[word_start..];
    let l1_new = trimmed_width(new_left);
    let mut candidate_right = moved.to_vec();
    candidate_right.extend(right.iter().cloned());
    let l2_new = trimmed_width(&candidate_right);

    if (l1_new - l2_new).abs() < (l1 - l2).abs() {
        let mut new_left = new_left.to_vec();
        trim_wrapped_line_edges(&mut new_left, false);
        let mut new_right = candidate_right;
        trim_wrapped_line_edges(&mut new_right, true);
        if new_left.is_empty() || new_right.is_empty() {
            return None;
        }
        Some((new_left, new_right))
    } else {
        None
    }
}

fn line_to_pieces(line: &LayoutLine, wrap_scales: LayoutWrapScales) -> Vec<LayoutPiece> {
    let mut pieces = Vec::new();
    let mut char_index = 0_usize;
    for run in &line.runs {
        let char_count = run.text.chars().count();
        if run.drawing.is_some() || char_count == 0 {
            let device_scale = if run.drawing.is_some() {
                finite_nonnegative_or_one(wrap_scales.drawing) as f32
            } else {
                finite_nonnegative_or_one(wrap_scales.text) as f32
            };
            pieces.push(LayoutPiece {
                text: run.text.clone(),
                run: run.clone(),
                width: run.width * device_scale,
                ink_min: 0.0,
                ink_max: run.width * device_scale,
                has_ink: run.width > 0.0,
                char_index: char_index + char_count.saturating_sub(1),
            });
            char_index += char_count;
            continue;
        }

        let scale_x = run.style.scale_x.max(0.0) as f32;
        let spacing = if run.style.spacing.is_finite() {
            run.style.spacing as f32 * scale_x
        } else {
            0.0
        };
        let text_scale = finite_nonnegative_or_one(wrap_scales.text) as f32;
        let spacing_scale = finite_nonnegative_or_one(wrap_scales.spacing) as f32;
        let glyph_clusters = glyph_cluster_ranges(&run.text, &run.glyphs);
        for (byte_start, byte_end, cluster_start, cluster_end) in
            atomic_text_cluster_ranges(&run.text, &glyph_clusters)
        {
            let cluster_text = run.text[byte_start..byte_end].to_owned();
            let cluster_glyphs = glyph_clusters
                .iter()
                .filter(|(start, end, _, _)| *start < cluster_end && *end > cluster_start)
                .flat_map(|(_, _, glyph_start, glyph_end)| {
                    run.glyphs[*glyph_start..*glyph_end].iter().cloned()
                })
                .collect::<Vec<_>>();
            let mut piece_run = run.clone();
            piece_run.text = cluster_text.clone();
            piece_run.glyphs = cluster_glyphs;
            piece_run.width = text_run_width(&piece_run.glyphs, &piece_run.style);
            let (ink_min, ink_max, has_ink) =
                text_piece_ink_bounds(&piece_run, scale_x, spacing, text_scale, spacing_scale);
            let piece_width = piece_run
                .glyphs
                .iter()
                .map(|glyph| glyph.x_advance * scale_x * text_scale + spacing * spacing_scale)
                .sum();
            pieces.push(LayoutPiece {
                text: cluster_text,
                width: piece_width,
                ink_min,
                ink_max,
                has_ink,
                run: piece_run,
                char_index: char_index + cluster_end.saturating_sub(1),
            });
        }
        char_index += char_count;
    }
    pieces
}

/// Map cluster values to char ranges and glyph slices; grouping by value survives RTL-reversed storage.
fn glyph_cluster_ranges(text: &str, glyphs: &[GlyphInfo]) -> Vec<(usize, usize, usize, usize)> {
    if glyphs.is_empty() {
        return Vec::new();
    }
    let text_char_len = text.chars().count();
    let shaped_clusters = glyphs
        .iter()
        .any(|glyph| glyph.positioning == GlyphPositioning::Shaped);
    let cluster_limit = if shaped_clusters {
        text.len()
    } else {
        text_char_len
    };
    let cluster_to_char = |cluster: usize| {
        if shaped_clusters {
            text.char_indices()
                .take_while(|(byte, _)| *byte <= cluster.min(text.len()))
                .count()
                .saturating_sub(1)
                .min(text_char_len)
        } else {
            cluster.min(text_char_len)
        }
    };
    let mut groups = Vec::new();
    let mut glyph_start = 0;
    while glyph_start < glyphs.len() {
        let cluster = glyphs[glyph_start].cluster;
        let mut glyph_end = glyph_start + 1;
        while glyph_end < glyphs.len() && glyphs[glyph_end].cluster == cluster {
            glyph_end += 1;
        }
        groups.push((cluster, glyph_start, glyph_end));
        glyph_start = glyph_end;
    }
    let mut starts = groups.iter().map(|group| group.0).collect::<Vec<_>>();
    starts.sort_unstable();
    starts.dedup();
    groups
        .into_iter()
        .map(|(cluster, glyph_start, glyph_end)| {
            let cluster_end = starts
                .iter()
                .copied()
                .find(|start| *start > cluster)
                .unwrap_or(cluster_limit);
            let char_start = cluster_to_char(cluster);
            let mut char_end = if cluster_end == cluster_limit {
                text_char_len
            } else {
                cluster_to_char(cluster_end)
            };
            if char_end <= char_start {
                char_end = (char_start + 1).min(text_char_len);
            }
            (char_start, char_end, glyph_start, glyph_end)
        })
        .collect()
}

fn trim_line_edge_whitespace(mut line: LayoutLine) -> LayoutLine {
    // trim_whitespace after wrap: ASCII space and unused newline only; do the whole line, not per-run.
    let mut found_content = false;
    for run in &mut line.runs {
        if run.drawing.is_some() {
            found_content = true;
            continue;
        }
        if found_content {
            continue;
        }
        let char_count = run.text.chars().count();
        let leading = run
            .text
            .chars()
            .take_while(|character| is_libass_trimmed_character(*character))
            .count();
        retain_run_character_range(run, leading, char_count);
        found_content = leading < char_count;
    }

    found_content = false;
    for run in line.runs.iter_mut().rev() {
        if run.drawing.is_some() {
            found_content = true;
            continue;
        }
        if found_content {
            continue;
        }
        let char_count = run.text.chars().count();
        let trailing = run
            .text
            .chars()
            .rev()
            .take_while(|character| is_libass_trimmed_character(*character))
            .count();
        retain_run_character_range(run, 0, char_count.saturating_sub(trailing));
        found_content = trailing < char_count;
    }

    line.glyph_count = line.runs.iter().map(|run| run.glyphs.len()).sum();
    line.width = line.runs.iter().map(|run| run.width).sum();
    line
}

fn retain_run_character_range(run: &mut LayoutGlyphRun, start: usize, end: usize) {
    let char_count = run.text.chars().count();
    let start = start.min(char_count);
    let end = end.clamp(start, char_count);
    if start == 0 && end == char_count {
        return;
    }
    if start == end || run.glyphs.is_empty() {
        run.glyphs.clear();
        run.width = 0.0;
        return;
    }

    let mut retained = vec![false; run.glyphs.len()];
    for (cluster_start, cluster_end, glyph_start, glyph_end) in
        glyph_cluster_ranges(&run.text, &run.glyphs)
    {
        if cluster_start < end && cluster_end > start {
            retained[glyph_start..glyph_end].fill(true);
        }
    }
    let mut index = 0;
    run.glyphs.retain(|_| {
        let keep = retained.get(index).copied().unwrap_or(false);
        index += 1;
        keep
    });
    run.width = text_run_width(&run.glyphs, &run.style);
}

fn is_libass_trimmed_character(character: char) -> bool {
    matches!(character, ' ' | '\n')
}

/// Grapheme clusters minus HarfBuzz-internal boundaries, so ligatures stay atomic without freezing the run.
fn atomic_text_cluster_ranges(
    text: &str,
    glyph_clusters: &[(usize, usize, usize, usize)],
) -> Vec<(usize, usize, usize, usize)> {
    let mut boundaries = text
        .grapheme_indices(true)
        .skip(1)
        .map(|(byte, _)| (byte, text[..byte].chars().count()))
        .collect::<Vec<_>>();
    boundaries.retain(|(_, char_boundary)| {
        !glyph_clusters
            .iter()
            .any(|(start, end, _, _)| *start < *char_boundary && *char_boundary < *end)
    });
    boundaries.push((text.len(), text.chars().count()));

    let mut byte_start = 0;
    let mut char_start = 0;
    boundaries
        .into_iter()
        .map(|(byte_end, char_end)| {
            let range = (byte_start, byte_end, char_start, char_end);
            byte_start = byte_end;
            char_start = char_end;
            range
        })
        .collect()
}

fn text_piece_ink_bounds(
    run: &LayoutGlyphRun,
    scale_x: f32,
    spacing: f32,
    text_scale: f32,
    spacing_scale: f32,
) -> (f32, f32, bool) {
    if run.glyphs.is_empty() {
        return (0.0, run.width, false);
    }
    let rasterizer = Rasterizer::with_options(RasterOptions {
        size_26_6: (run.style.font_size.max(1.0) * 64.0).round() as i32,
        hinting: ass::Hinting::None,
    });
    let Ok(glyphs) = rasterizer.rasterize_glyphs(&run.font, &run.glyphs) else {
        return (0.0, run.width, true);
    };
    let mut pen = 0.0_f32;
    let mut ink_min = f32::INFINITY;
    let mut ink_max = f32::NEG_INFINITY;
    for glyph in glyphs {
        if glyph.width > 0 && glyph.height > 0 && !glyph.bitmap.is_empty() {
            let offset = glyph.offset_x_26_6 as f32 / 64.0;
            let left = pen + (glyph.left as f32 + offset) * scale_x * text_scale;
            ink_min = ink_min.min(left);
            ink_max = ink_max.max(left + glyph.width as f32 * scale_x * text_scale);
        }
        pen += glyph.advance_x_26_6 as f32 / 64.0 * scale_x * text_scale + spacing * spacing_scale;
    }
    if ink_min.is_finite() && ink_max.is_finite() {
        (ink_min, ink_max, true)
    } else {
        (0.0, run.width, false)
    }
}

fn finite_nonnegative_or_one(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        1.0
    }
}

fn finite_nonnegative_or_zero(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}

fn trim_wrapped_line_edges(pieces: &mut Vec<LayoutPiece>, trim_leading: bool) {
    while pieces
        .last()
        .is_some_and(|piece| is_libass_trimmed_whitespace(&piece.text))
    {
        pieces.pop();
    }
    if trim_leading {
        let leading = pieces
            .iter()
            .take_while(|piece| is_libass_trimmed_whitespace(&piece.text))
            .count();
        if leading > 0 {
            pieces.drain(0..leading);
        }
    }
}

fn is_libass_trimmed_whitespace(text: &str) -> bool {
    text.chars().all(is_libass_trimmed_character)
}

fn pieces_positioned_width(pieces: &[LayoutPiece]) -> f32 {
    let Some(first) = pieces.first() else {
        return 0.0;
    };
    // Width is first-cluster left bbox to last-cluster right bbox, not advance sum or intermediate union.
    let start = if first.has_ink { first.ink_min } else { 0.0 };
    let (last, prefix) = pieces.split_last().expect("non-empty pieces");
    let pen = prefix.iter().map(|piece| piece.width).sum::<f32>();
    let end = pen + if last.has_ink { last.ink_max } else { 0.0 };
    (end - start).max(0.0)
}

fn pieces_max_positioned_width(pieces: &[LayoutPiece]) -> f32 {
    let Some(first) = pieces.first() else {
        return 0.0;
    };
    let start = if first.has_ink { first.ink_min } else { 0.0 };
    let mut pen = 0.0_f32;
    let mut max_width = 0.0_f32;
    for piece in pieces {
        if !is_libass_trimmed_whitespace(&piece.text) {
            let end = pen + if piece.has_ink { piece.ink_max } else { 0.0 };
            max_width = max_width.max((end - start).max(0.0));
        }
        pen += piece.width;
    }
    max_width
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

fn apply_vertical_font_advances(
    mut glyphs: Vec<GlyphInfo>,
    style: &ParsedSpanStyle,
) -> Vec<GlyphInfo> {
    if !style.font_name.starts_with('@') {
        return glyphs;
    }
    let advance = style.font_size.max(0.0) as f32;
    if advance <= 0.0 {
        return glyphs;
    }
    for glyph in &mut glyphs {
        if glyph.vertical_rotation_eligible
            && (glyph.x_advance.abs() > f32::EPSILON || glyph.y_advance.abs() > f32::EPSILON)
        {
            glyph.x_advance = advance;
            glyph.y_advance = 0.0;
        }
    }
    glyphs
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
    weight: i32,
) -> Vec<(String, FontMatch)> {
    let selection_family = font_selection_family(family);
    let query = FontQuery {
        family: selection_family.to_string(),
        style: style.clone(),
        weight: font_query_weight(weight),
    };
    let base_font = provider.resolve(&query);
    let mut chunks: Vec<(String, FontMatch)> = Vec::new();
    let mut leading_ignorables = String::new();

    for grapheme in fallback_text_clusters(text) {
        if grapheme.chars().all(is_font_selection_ignorable) {
            if let Some((chunk, _)) = chunks.last_mut() {
                chunk.push_str(&grapheme);
            } else {
                leading_ignorables.push_str(&grapheme);
            }
            continue;
        }
        let font = resolve_font_for_cluster(
            provider,
            &query,
            &base_font,
            selection_family,
            style.as_deref(),
            &grapheme,
        );
        let mut cluster = std::mem::take(&mut leading_ignorables);
        cluster.push_str(&grapheme);

        if let Some((chunk, chunk_font)) = chunks.last_mut() {
            if same_font_match(chunk_font, &font) {
                chunk.push_str(&cluster);
                continue;
            }
        }
        chunks.push((cluster, font));
    }
    if !leading_ignorables.is_empty() {
        if let Some((chunk, _)) = chunks.last_mut() {
            chunk.push_str(&leading_ignorables);
        } else {
            chunks.push((leading_ignorables, base_font));
        }
    }

    chunks
}

fn font_selection_family(family: &str) -> &str {
    family.strip_prefix('@').unwrap_or(family)
}

fn fallback_text_clusters(text: &str) -> Vec<String> {
    let mut clusters: Vec<String> = Vec::new();
    for grapheme in text.graphemes(true) {
        let joins_previous = grapheme.chars().next().is_some_and(is_shaping_control);
        let joins_next = clusters
            .last()
            .and_then(|cluster| cluster.chars().next_back())
            .is_some_and(is_shaping_control);
        if joins_previous || joins_next {
            if let Some(cluster) = clusters.last_mut() {
                cluster.push_str(grapheme);
            } else {
                clusters.push(grapheme.to_owned());
            }
        } else {
            clusters.push(grapheme.to_owned());
        }
    }
    clusters
}

fn is_shaping_control(character: char) -> bool {
    matches!(character, '\u{200c}' | '\u{200d}')
}

fn resolve_font_for_cluster<P: FontProvider>(
    provider: &P,
    query: &FontQuery,
    base_font: &FontMatch,
    family: &str,
    style: Option<&str>,
    cluster: &str,
) -> FontMatch {
    let required = cluster
        .chars()
        .filter(|character| {
            !character.is_whitespace()
                && !character.is_control()
                && !is_font_selection_ignorable(*character)
        })
        .collect::<String>();
    if base_font.path.is_none()
        || required.is_empty()
        || font_match_supports_text(base_font, &required)
    {
        return base_font.clone();
    }

    let local = provider.resolve_for_text(query, &required);
    if local.path.is_some() {
        return local;
    }

    let mut first_fallback = None;
    for character in required.chars() {
        let Some((resolved_family, resolved_path, face_index)) =
            resolve_system_font_for_char(family, style, character)
        else {
            continue;
        };
        let candidate = FontMatch {
            family: resolved_family,
            path: resolved_path,
            face_index,
            style: style.map(str::to_string),
            synthetic_bold: base_font.synthetic_bold,
            synthetic_italic: base_font.synthetic_italic,
            provider: base_font.provider,
        };
        if first_fallback.is_none() {
            first_fallback = Some(candidate.clone());
        }
        if font_match_supports_text(&candidate, &required) {
            return candidate;
        }
    }
    first_fallback.unwrap_or_else(|| base_font.clone())
}

fn is_font_selection_ignorable(character: char) -> bool {
    let codepoint = character as u32;
    matches!(
        codepoint,
        0x00AD
            | 0x034F
            | 0x061C
            | 0x17B4..=0x17B5
            | 0x180B..=0x180F
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x206F
            | 0xFE00..=0xFE0F
            | 0xFEFF
            | 0xFFF0..=0xFFF8
            | 0x1D173..=0x1D17A
            | 0xE0000..=0xE0FFF
    )
}

fn same_font_match(left: &FontMatch, right: &FontMatch) -> bool {
    left.family == right.family
        && left.path == right.path
        && left.face_index == right.face_index
        && left.style == right.style
        && left.synthetic_bold == right.synthetic_bold
        && left.synthetic_italic == right.synthetic_italic
}

fn font_query_weight(weight: i32) -> Option<i32> {
    (weight != 400).then_some(weight)
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
    #[cfg(not(target_arch = "wasm32"))]
    use rassa_fonts::font_match_supports_text;
    use rassa_fonts::{FontProviderKind, FontconfigProvider, NullFontProvider};
    use rassa_parse::{ParsedKaraokeMode, ParsedTrack, parse_script_text};

    fn parse_track(input: &str) -> ParsedTrack {
        parse_script_text(input).expect("script should parse")
    }

    #[derive(Clone)]
    struct FixedFontProvider(FontMatch);

    impl FontProvider for FixedFontProvider {
        fn resolve(&self, _query: &FontQuery) -> FontMatch {
            self.0.clone()
        }
    }

    #[derive(Clone)]
    struct CoverageFontProvider {
        base: FontMatch,
        covered: FontMatch,
    }

    impl FontProvider for CoverageFontProvider {
        fn resolve(&self, _query: &FontQuery) -> FontMatch {
            self.base.clone()
        }

        fn resolve_for_text(&self, _query: &FontQuery, text: &str) -> FontMatch {
            if font_match_supports_text(&self.covered, text) {
                self.covered.clone()
            } else {
                FontMatch::unresolved(
                    self.base.family.clone(),
                    self.base.style.clone(),
                    self.base.provider,
                )
            }
        }
    }

    fn read_be_u16(data: &[u8], offset: usize) -> u16 {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }

    fn read_be_u32(data: &[u8], offset: usize) -> usize {
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize
    }

    fn two_face_font_collection() -> Vec<u8> {
        let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test");
        let first =
            std::fs::read(fixture_root.join("font1.ttf")).expect("first font fixture should read");
        let second =
            std::fs::read(fixture_root.join("font2.otf")).expect("second font fixture should read");
        let first_offset = 20_usize.next_multiple_of(4);
        let second_offset = (first_offset + first.len()).next_multiple_of(4);
        let mut collection = vec![0_u8; second_offset + second.len()];
        collection[0..4].copy_from_slice(b"ttcf");
        collection[4..8].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        collection[8..12].copy_from_slice(&2_u32.to_be_bytes());
        collection[12..16].copy_from_slice(&(first_offset as u32).to_be_bytes());
        collection[16..20].copy_from_slice(&(second_offset as u32).to_be_bytes());
        collection[first_offset..first_offset + first.len()].copy_from_slice(&first);
        collection[second_offset..second_offset + second.len()].copy_from_slice(&second);
        for (font, base_offset) in [(&first, first_offset), (&second, second_offset)] {
            let table_count = read_be_u16(font, 4) as usize;
            for index in 0..table_count {
                let table = base_offset + 12 + index * 16;
                let original_offset = read_be_u32(&collection, table + 8);
                collection[table + 8..table + 12]
                    .copy_from_slice(&((original_offset + base_offset) as u32).to_be_bytes());
            }
        }
        collection
    }

    fn aileron_fixture_font(synthetic_italic: bool) -> FontMatch {
        FontMatch {
            family: "Aileron".to_owned(),
            path: Some(
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
            ),
            face_index: Some(0),
            style: Some("Regular".to_owned()),
            synthetic_bold: false,
            synthetic_italic,
            provider: FontProviderKind::Attached,
        }
    }

    #[test]
    fn coverage_selected_collection_face_survives_layout_and_shaping() {
        let path = std::env::temp_dir().join(format!(
            "rassa-layout-covered-face-{}-{}.ttc",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        ));
        std::fs::write(&path, two_face_font_collection())
            .expect("font collection fixture should write");
        let base = FontMatch {
            family: "Shared Family".to_owned(),
            path: Some(path.clone()),
            face_index: None,
            style: Some("Bold".to_owned()),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Attached,
        };
        let covered = FontMatch {
            face_index: Some(1),
            style: Some("Regular".to_owned()),
            ..base.clone()
        };
        assert!(!font_match_supports_text(&base, "∂"));
        assert!(font_match_supports_text(&covered, "∂"));
        let provider = CoverageFontProvider {
            base,
            covered: covered.clone(),
        };
        let track = parse_track(
            "[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Shared Family,64,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,1,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,∂",
        );
        let layout = LayoutEngine::new()
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Complex)
            .expect("coverage-selected collection face should shape");
        let run = layout
            .lines
            .iter()
            .flat_map(|line| &line.runs)
            .find(|run| !run.glyphs.is_empty())
            .expect("dialogue should produce a glyph run");

        assert_eq!(run.font.face_index, Some(1));
        assert_eq!(run.font.path, covered.path);
        assert!(run.glyphs.iter().all(|glyph| glyph.glyph_id != 0));
        std::fs::remove_file(path).expect("font collection fixture should clean up");
    }

    fn shaped_glyph(cluster: usize, glyph_id: u32, advance: f32) -> GlyphInfo {
        GlyphInfo {
            glyph_id,
            cluster,
            x_advance: advance,
            positioning: GlyphPositioning::Shaped,
            ..GlyphInfo::default()
        }
    }

    #[test]
    fn wrap_keeps_each_ligature_cluster_atomic_without_freezing_the_run() {
        let run = LayoutGlyphRun {
            text: "ffi ffi".to_owned(),
            font: FontMatch::unresolved("Fixture", None, FontProviderKind::Null),
            glyphs: vec![
                shaped_glyph(0, 10, 3.0),
                shaped_glyph(3, 11, 1.0),
                shaped_glyph(4, 10, 3.0),
            ],
            width: 7.0,
            style: ParsedSpanStyle::default(),
            ..LayoutGlyphRun::default()
        };
        let line = LayoutLine {
            text: run.text.clone(),
            glyph_count: run.glyphs.len(),
            width: run.width,
            runs: vec![run],
            ..LayoutLine::default()
        };

        let pieces = line_to_pieces(&line, LayoutWrapScales::default());
        assert_eq!(
            pieces
                .iter()
                .map(|piece| piece.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ffi", " ", "ffi"],
        );
        assert_eq!(
            pieces
                .iter()
                .map(|piece| piece.run.glyphs.len())
                .collect::<Vec<_>>(),
            vec![1, 1, 1],
        );

        let wrapped = wrap_layout_line(line, 4.0, 1, "en", false, LayoutWrapScales::default())
            .expect("ligature run should wrap at its intervening space");
        assert_eq!(
            wrapped
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["ffi", "ffi"],
        );
    }

    #[test]
    fn fallback_partition_never_splits_unicode_text_clusters() {
        let provider = FixedFontProvider(aileron_fixture_font(false));
        let clusters = [
            "e\u{0301}",                        // general combining mark
            "ש\u{05c1}",                        // Hebrew point
            "ب\u{0650}",                        // Arabic mark
            "क्\u{200d}ष",                       // Indic conjunct with ZWJ
            "क्\u{200c}",                        // ZWNJ/default-ignorable tail
            "👩\u{200d}👩\u{200d}👧\u{200d}👦", // emoji ZWJ sequence
            "日\u{fe0f}",                       // variation selector
        ];

        for cluster in clusters {
            assert_eq!(
                cluster.graphemes(true).count(),
                1,
                "test input must be one extended grapheme: {cluster:?}"
            );
            let chunks = split_text_by_font(cluster, &provider, "Aileron", None, 400);
            assert_eq!(
                chunks
                    .iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<String>(),
                cluster,
            );
            assert_eq!(
                chunks.len(),
                1,
                "font fallback split extended cluster {cluster:?}: {chunks:?}"
            );
        }

        for text in ["ب\u{200d}ت", "ب\u{200c}ت"] {
            assert_eq!(
                fallback_text_clusters(text),
                vec![text.to_owned()],
                "ZWJ/ZWNJ must carry shaping context across grapheme boundaries"
            );
            let chunks = split_text_by_font(text, &provider, "Aileron", None, 400);
            assert_eq!(
                chunks
                    .iter()
                    .map(|(chunk, _)| chunk.as_str())
                    .collect::<String>(),
                text,
            );
            assert_eq!(
                chunks.len(),
                1,
                "font fallback must not sever ZWJ/ZWNJ context: {text:?}"
            );
        }

        for text in ["\u{2060}e", "e\u{2060}", "\u{00ad}日\u{e0100}"] {
            let chunks = split_text_by_font(text, &provider, "Aileron", None, 400);
            assert_eq!(
                chunks
                    .iter()
                    .map(|(chunk, _)| chunk.as_str())
                    .collect::<String>(),
                text,
            );
            assert_eq!(
                chunks.len(),
                1,
                "leading/trailing default ignorables must follow adjacent text: {text:?}"
            );
        }
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn positioned_width_accounts_for_italic_overhang_and_negative_bearing() {
        let provider = FixedFontProvider(aileron_fixture_font(true));
        let track = parse_track(
            "[Script Info]\nPlayResX: 1000\nWrapStyle: 2\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,100,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,j j",
        );
        let line = LayoutEngine::new()
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Complex)
            .expect("italic fixture should shape")
            .lines
            .into_iter()
            .next()
            .expect("one unwrapped line");
        let pieces = line_to_pieces(&line, LayoutWrapScales::default());
        let positioned = pieces_positioned_width(&pieces);
        let device_pieces = line_to_pieces(
            &line,
            LayoutWrapScales {
                text: 3.0,
                spacing: 3.0,
                drawing: 3.0,
                available_width: 3.0,
                available_width_extra: 0.0,
            },
        );
        let device_positioned = pieces_positioned_width(&device_pieces);

        assert!(
            pieces.iter().any(|piece| piece.ink_min < 0.0),
            "italic fixture should expose a negative glyph bearing: {pieces:?}"
        );
        assert!(
            positioned > line.width,
            "positioned ink should exceed logical advance: ink={positioned}, advance={}",
            line.width,
        );
        assert!(
            (device_positioned - positioned * 3.0).abs() < 0.05,
            "multi-glyph ink positions must receive the device scale exactly once: source={positioned}, device={device_positioned}"
        );

        let max_width = (positioned + line.width) * 0.5;
        let wrapped =
            wrap_layout_line(line, max_width, 1, "en", false, LayoutWrapScales::default())
                .expect("positioned ink overflow should wrap");
        assert_eq!(
            wrapped
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["j", "j"],
        );
    }

    #[test]
    fn renderer_wrap_scales_measure_glyphs_spacing_and_margin_width_independently() {
        let provider = FixedFontProvider(aileron_fixture_font(false));
        let track = parse_track(
            "[Script Info]\nPlayResX: 640\nPlayResY: 120\nWrapStyle: 1\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,40,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,10,0,1,0,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,one two three four",
        );
        let engine = LayoutEngine::new();
        let source = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Complex)
            .expect("source-space layout succeeds");
        let anisotropic = engine
            .layout_track_event_with_features_and_wrap_scales(
                &track,
                0,
                &provider,
                ShapingMode::Complex,
                LayoutFeatures::default(),
                LayoutWrapScales {
                    text: 9.0,
                    spacing: 3.0,
                    drawing: 3.0,
                    available_width: 3.0,
                    available_width_extra: 0.0,
                },
            )
            .expect("device-space layout succeeds");

        assert_eq!(source.lines.len(), 1);
        assert!(
            anisotropic.lines.len() > source.lines.len(),
            "glyph advances must use vertical scale while \\fsp and margins use horizontal scale"
        );
    }

    #[test]
    fn script_info_kerning_controls_complex_layout_advances() {
        let mut track = parse_track(
            "[Script Info]\nKerning: yes\nPlayResX: 1000\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Noto Serif,64,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\q2}AVAV",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let kerned = engine
            .layout_track_event(&track, 0, &provider)
            .expect("kerned layout succeeds");
        if kerned.lines[0].runs[0].font.path.is_none() {
            eprintln!("skipping: Noto Serif is unavailable");
            return;
        }

        track.kerning = false;
        let unkerned = engine
            .layout_track_event(&track, 0, &provider)
            .expect("unkerned layout succeeds");

        assert!(
            kerned.lines[0].width < unkerned.lines[0].width,
            "Kerning: no must retain unkerned advances: kerned={} unkerned={}",
            kerned.lines[0].width,
            unkerned.lines[0].width,
        );
    }

    #[test]
    fn whole_text_layout_reorders_mixed_bidi_across_override_runs() {
        let track = parse_track(
            "[Script Info]\nPlayResX: 1000\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Sans,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\q2}abc אב{\\b1}ג 123",
        );
        let layout = LayoutEngine::new()
            .layout_track_event_with_features(
                &track,
                0,
                &NullFontProvider,
                ShapingMode::Simple,
                LayoutFeatures {
                    whole_text_layout: true,
                    ..LayoutFeatures::default()
                },
            )
            .expect("whole-text layout succeeds");
        let visual = layout.lines[0]
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter_map(|glyph| char::from_u32(glyph.glyph_id))
            .collect::<String>();
        let expected = analyze_bidi_with_base("abc אבג 123", BidiDirection::LeftToRight)
            .expect("bidi analysis succeeds")
            .visual_text;

        assert_eq!(visual, expected);
        assert!(layout.lines[0].runs.len() >= 3);
    }

    #[test]
    fn encoding_minus_one_implicitly_enables_whole_text_layout() {
        let track = parse_track(
            "[Script Info]\nPlayResX: 1000\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Sans,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,-1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\q2}אב{\\i1}ג abc",
        );
        let layout = LayoutEngine::new()
            .layout_track_event_with_mode(&track, 0, &NullFontProvider, ShapingMode::Simple)
            .expect("implicit whole-text layout succeeds");
        let visual = layout.lines[0]
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter_map(|glyph| char::from_u32(glyph.glyph_id))
            .collect::<String>();
        let expected = analyze_bidi_with_base("אבג abc", BidiDirection::Neutral)
            .expect("bidi analysis succeeds")
            .visual_text;

        assert_eq!(visual, expected);
        assert_eq!(layout.lines[0].direction, BidiDirection::RightToLeft);
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

        assert_eq!(layout.style_index, 2);
        assert_eq!(layout.font_family, "DejaVu Sans");
        assert_eq!(layout.margin_l, 30);
        assert_eq!(layout.margin_r, 22);
        assert_eq!(layout.margin_v, 40);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].glyph_count, "Visible text".chars().count());
        assert_eq!(layout.lines[0].runs.len(), 1);
    }

    #[test]
    fn override_italic_resolves_italic_font_style() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,DejaVu Sans,40,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\i1}italic",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");
        let run = layout.lines[0].runs.first().expect("italic run");

        assert!(run.style.italic);
        assert!(
            run.font
                .style
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("italic"),
            "italic override must request an italic font face/style, got {:?}",
            run.font.style
        );
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

        assert_eq!(
            layout
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta", "gamma", "delta"],
        );
        assert!(layout.lines.iter().all(|line| !line.text.starts_with(' ')));
        assert!(layout.lines.iter().all(|line| !line.text.ends_with(' ')));
    }

    #[test]
    fn layout_wrap_trims_only_libass_ascii_spaces() {
        let track = parse_track(
            "[Script Info]
PlayResX: 8
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,alpha \u{00a0} beta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert!(
            layout
                .lines
                .iter()
                .any(|line| line.text.starts_with('\u{00a0}')),
            "libass trims ASCII spaces at wrap edges but preserves NBSP"
        );
    }

    #[test]
    fn layout_trims_line_edge_ascii_whitespace_across_style_runs() {
        let track = parse_track(
            "[Script Info]\nPlayResX: 1000\nWrapStyle: 2\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Sans,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\fs60}  {\\fs20}A B{\\fs80}   {\\fs20}",
        );
        let line = &LayoutEngine::new()
            .layout_track_event_with_mode(&track, 0, &NullFontProvider, ShapingMode::Simple)
            .expect("edge-whitespace fixture lays out")
            .lines[0];

        let rendered_text = line
            .runs
            .iter()
            .flat_map(|run| &run.glyphs)
            .filter_map(|glyph| char::from_u32(glyph.glyph_id))
            .collect::<String>();
        assert_eq!(rendered_text, "A B");
        assert_eq!(line.glyph_count, 3);
        assert_eq!(line.width, 3.0);
        assert!(
            line.runs
                .iter()
                .filter(|run| run.text.chars().all(is_libass_trimmed_character))
                .all(|run| run.glyphs.is_empty() && run.width == 0.0),
            "whitespace-only edge runs must survive for metrics but carry no placement advance: {line:?}"
        );
    }

    #[test]
    fn layout_trims_nonbreaking_newline_spaces_but_preserves_nbsp() {
        let script = |text: &str| {
            parse_track(&format!(
                "[Script Info]\nPlayResX: 1000\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Sans,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{text}"
            ))
        };
        let layout = |track: &ParsedTrack| {
            LayoutEngine::new()
                .layout_track_event_with_mode(track, 0, &NullFontProvider, ShapingMode::Simple)
                .expect("whitespace fixture lays out")
        };

        let newline_spaces = layout(&script("\\n A \\n{\\fs20}"));
        assert_eq!(newline_spaces.lines[0].glyph_count, 1);
        assert_eq!(newline_spaces.lines[0].width, 1.0);

        let nbsp = layout(&script("\\hA{\\fs20}"));
        assert_eq!(nbsp.lines[0].glyph_count, 2);
        assert!(nbsp.lines[0].width > newline_spaces.lines[0].width);
    }

    #[test]
    fn all_space_line_keeps_metric_runs_but_has_zero_placement_width() {
        let track = parse_track(
            "[Script Info]\nPlayResX: 1000\nWrapStyle: 2\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Sans,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,A\\N{\\fs80}   \\N{\\fs20}A",
        );
        let layout = LayoutEngine::new()
            .layout_track_event_with_mode(&track, 0, &NullFontProvider, ShapingMode::Simple)
            .expect("space-only line lays out");
        let middle = &layout.lines[1];

        assert_eq!(middle.glyph_count, 0);
        assert_eq!(middle.width, 0.0);
        assert!(
            middle.runs.iter().any(|run| run.style.font_size == 80.0),
            "trimmed runs remain available to half-height empty-line metric selection"
        );
    }

    #[test]
    fn layout_wraps_against_default_script_resolution() {
        let track = parse_track(
            "[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,190,190,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,alpha beta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert_eq!(
            layout.lines.len(),
            2,
            "missing PlayRes should still use ASS default resolution for wrapping"
        );
        assert_eq!(layout.lines[0].text, "alpha");
        assert_eq!(layout.lines[1].text, "beta");
    }

    #[test]
    fn banner_effect_forces_no_wrap_like_libass() {
        let track = parse_track(
            "[Script Info]
PlayResX: 40
PlayResY: 80
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,Banner;25;0;0,alpha beta gamma delta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert_eq!(layout.wrap_style, Some(2));
        assert_eq!(layout.lines.len(), 1);
    }

    #[test]
    fn banner_effect_makes_lowercase_n_break_before_q_override_like_libass() {
        let track = parse_track(
            "[Script Info]
PlayResX: 400
PlayResY: 80
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,Banner;25;0;0,alpha\\nbeta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert_eq!(layout.wrap_style, Some(2));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].text, "alpha");
        assert_eq!(layout.lines[1].text, "beta");
    }

    #[test]
    fn banner_effect_q_override_controls_lowercase_n_like_libass() {
        let track = parse_track(
            "[Script Info]
PlayResX: 400
PlayResY: 80
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,Banner;25;0;0,{\\q0}alpha\\nbeta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert_eq!(layout.wrap_style, Some(0));
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].text, "alpha beta");
    }

    #[test]
    fn explicit_hard_break_lines_are_not_auto_wrapped_again() {
        let track = parse_track(
            "[Script Info]\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Main,Fontin Sans Rg,70,&H00FFFFFF,&H000000FF,&H00000000,&HA0000000,-1,0,0,0,100,100,0,0,1,3.5,1.5,2,140,140,45,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:06:06.20,0:06:11.56,Main,,0,0,0,,Eu sei que a Karin é fofa, mas quem seria tão\\N descaradamente indecente em plena luz do dia?!",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("baseline dialogue layout should succeed");

        assert_eq!(
            layout.lines.len(),
            2,
            "explicit \\N from baseline line 122 is already a hard break; auto-wrap must not split it into extra visual lines"
        );
        assert_eq!(
            layout.lines[0].text,
            "Eu sei que a Karin é fofa, mas quem seria tão"
        );
        assert_eq!(
            layout.lines[1].text,
            " descaradamente indecente em plena luz do dia?!"
        );
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
    fn layout_wraps_positioned_center_text_against_margins_not_anchor_space() {
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

        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].text, "alpha beta gamma delta");
    }

    #[test]
    fn layout_wraps_exact_positioned_text_against_margins_not_anchor_space() {
        let track = parse_track(
            "[Script Info]
PlayResX: 40
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,8,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,2,2,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\pos(10.5,20.25)\\an5\\q0}alpha beta gamma delta",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");

        assert_eq!(layout.position_exact, Some((10.5, 20.25)));
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].text, "alpha beta gamma delta");
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
        let unicode = engine
            .layout_track_event_with_options(&track, 0, &provider, ShapingMode::Simple, true)
            .expect("layout should succeed");
        assert!(unicode.lines.len() > 1);
        assert!(unicode.lines.iter().all(|line| line.width <= 2.0));

        let default = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Simple)
            .expect("layout should succeed");
        assert_eq!(
            default.lines.len(),
            1,
            "without ASS_FEATURE_WRAP_UNICODE the spaceless CJK line overflows unbroken"
        );
    }

    #[test]
    fn vertical_font_names_use_font_size_advances_like_libass() {
        let track = parse_track(
            "[Script Info]\nPlayResX: 1920\nPlayResY: 1080\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:09:48.86,0:09:51.61,Placas,,0,0,0,,{\\fs86\\fn@FOT-DNP Shuei4goStd M\\b1}โรงเรียน",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("vertical-font baseline dialogue should layout");
        let line = &layout.lines[0];

        assert!(
            line.width >= 86.0 * 7.0,
            "@font text should advance by roughly one em per character like libass, got width {}",
            line.width
        );
    }

    #[test]
    fn vertical_font_marker_is_not_part_of_the_selected_family_name() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Vertical,@IPAexGothic,40,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,270,1,0,0,7,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Vertical,,0,0,0,,text",
        );
        let layout = LayoutEngine::new()
            .layout_track_event(&track, 0, &NullFontProvider)
            .expect("vertical layout succeeds");

        assert_eq!(layout.lines[0].runs[0].font.family, "IPAexGothic");
        assert!(layout.lines[0].runs[0].style.font_name.starts_with('@'));
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
    fn layout_wrap_keeps_thai_lower_vowel_with_base_glyph() {
        if resolve_system_font_for_char("K2D ExtraBold", Some("Bold"), 'อ').is_none() {
            eprintln!("skipping: system fontconfig has no Thai-capable fallback font");
            return;
        }
        let track = parse_track(
            "[Script Info]\nPlayResX: 400\nPlayResY: 240\nWrapStyle: 0\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED TH2,K2D ExtraBold,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0,0,2,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,ED TH2,,0,0,0,,อุ อู ญ ฐ ฏ ฎ",
        );
        let engine = LayoutEngine::new();
        let provider = FontconfigProvider::new();
        let layout = engine
            .layout_track_event_with_mode(&track, 0, &provider, ShapingMode::Complex)
            .expect("Thai fallback layout should succeed");

        assert!(
            layout
                .lines
                .iter()
                .flat_map(|line| line.runs.iter())
                .any(|run| run.text == "อุ"),
            "auto-wrap must not split Thai lower vowel marks away from their base glyph"
        );
        assert!(
            layout
                .lines
                .iter()
                .flat_map(|line| line.runs.iter())
                .any(|run| run.text == "อู"),
            "auto-wrap must keep Thai U+0E39 with its base glyph"
        );
    }

    #[test]
    fn layout_carries_decimal_rectangular_clip_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\clip(659.3,35,1260.8,48.433333333333)}Clip",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(
            layout.clip_rect,
            Some(Rect {
                x_min: 659,
                y_min: 35,
                x_max: 1260,
                y_max: 48
            }),
            "decimal rectangular clips must survive layout with libass-like truncation"
        );
        assert!(layout.vector_clip.is_none());
        assert!(!layout.inverse_clip);
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
        assert!(!layout.vector_clip_inverse);
    }

    #[test]
    fn layout_carries_combined_rect_and_vector_clip_metadata() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\iclip(m 0 0 l 8 0 8 8 0 8)\\clip(1,2,3,4)}Clip",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(
            layout.clip_rect,
            Some(Rect {
                x_min: 1,
                y_min: 2,
                x_max: 3,
                y_max: 4,
            })
        );
        assert!(!layout.inverse_clip);
        assert!(layout.vector_clip.is_some());
        assert!(layout.vector_clip_inverse);
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
        assert_eq!(layout.lines[0].runs[0].width, 8.0);
    }

    #[test]
    fn layout_splits_drawing_runs_at_override_blocks_like_libass() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 l 8 0 8 8 0 8{\\pos(20,20)}m 20 20 l 28 20 28 28 20 28",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines[0].runs.len(), 2);
        assert!(layout.lines[0].runs[0].drawing.is_some());
        assert!(layout.lines[0].runs[1].drawing.is_some());
    }

    #[test]
    fn layout_drawing_width_does_not_split_unicode_whitespace_like_libass() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m\u{00a0}0 0 l 8 0 8 8 0 8",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert!(layout.lines[0].runs[0].drawing.is_some());
        assert_eq!(layout.lines[0].runs[0].width, 0.0);
    }

    #[test]
    fn layout_wraps_large_drawing_scale_base_like_libass() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p34}m 0 0 l 64 0 64 64 0 64",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        let run = &layout.lines[0].runs[0];
        assert!(run.drawing.is_some());
        assert_eq!(run.width, 32.0);
    }

    #[test]
    fn layout_uses_decimal_drawing_control_bounds_for_width() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 b 41.909 83.818 83.818 65.378 83.818 41.909",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        let width = layout.lines[0].runs[0].width;
        assert!((width - 83.8125).abs() < 0.001, "drawing width was {width}");
    }

    #[test]
    fn layout_drawing_width_ignores_invalid_spline_extension_points() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 l 8 0 8 8 0 8 p 200 0",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.lines[0].runs[0].width, 8.0);
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
    fn auto_justify_uses_effective_alignment_override() {
        let track = parse_track(
            "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding, Justify\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,3,10,10,10,1,0\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an1}Left",
        );
        let engine = LayoutEngine::new();
        let provider = NullFontProvider;
        let layout = engine
            .layout_track_event(&track, 0, &provider)
            .expect("layout should succeed");

        assert_eq!(layout.alignment, ass::VALIGN_SUB | ass::HALIGN_LEFT);
        assert_eq!(layout.justify, ass::ASS_JUSTIFY_LEFT);
    }

    #[test]
    fn vertical_font_advances_only_override_eligible_source_clusters() {
        let style = ParsedSpanStyle {
            font_name: "@Vertical".to_owned(),
            font_size: 40.0,
            ..ParsedSpanStyle::default()
        };
        let glyphs = vec![
            GlyphInfo {
                x_advance: 12.0,
                vertical_rotation_eligible: false,
                ..GlyphInfo::default()
            },
            GlyphInfo {
                x_advance: 13.0,
                vertical_rotation_eligible: true,
                ..GlyphInfo::default()
            },
        ];

        let glyphs = apply_vertical_font_advances(glyphs, &style);

        assert_eq!(glyphs[0].x_advance, 12.0);
        assert_eq!(glyphs[1].x_advance, 40.0);
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
