use super::*;

pub(crate) fn translate_planes(mut planes: Vec<ImagePlane>, offset: Point) -> Vec<ImagePlane> {
    if offset == Point::default() {
        return planes;
    }
    for plane in &mut planes {
        plane.destination.x += offset.x;
        plane.destination.y += offset.y;
    }
    planes
}

/// \clip coordinates map like positioned coordinates (libass
/// x2scr_pos_scaled / y2scr_pos: content scale plus margin offsets). Animated
/// clips keep fractional script-space edges until this final mapping step.
pub(crate) fn scale_clip_rect_exact(rect: ParsedRectF64, mapping: &EventMapping) -> Rect {
    Rect {
        x_min: mapping.map_x_pos(rect.x_min).round() as i32,
        y_min: mapping.map_y_pos(rect.y_min).round() as i32,
        x_max: mapping.map_x_pos(rect.x_max).round() as i32,
        y_max: mapping.map_y_pos(rect.y_max).round() as i32,
    }
}

/// libass clip policy (ass_render_event): normal events under use_margins
/// clip only to the full frame; everything else (use_margins off, or
/// explicit events) clips to the content area between the margins.
pub(crate) fn frame_clip_rect(
    track: &ParsedTrack,
    config: &RendererConfig,
    explicit: bool,
) -> Rect {
    let frame = frame_size(track, config);
    if config.use_margins && !explicit {
        Rect {
            x_min: 0,
            y_min: 0,
            x_max: frame.width.max(0),
            y_max: frame.height.max(0),
        }
    } else {
        Rect {
            x_min: config.margins.left.max(0),
            y_min: config.margins.top.max(0),
            x_max: (frame.width - config.margins.right).max(0),
            y_max: (frame.height - config.margins.bottom).max(0),
        }
    }
}

pub(crate) fn compute_horizontal_origin(
    track: &ParsedTrack,
    event: &LayoutEvent,
    line_width: i32,
    block_width: i32,
    horizontal_scroll: bool,
    effective_position: Option<(i32, i32)>,
    mapping: &EventMapping,
) -> i32 {
    let block_width = block_width.max(line_width);
    // libass makes horizontal scrolling use the event's alignment as its
    // justification, regardless of the style's Justify value.
    let requested_justify = if horizontal_scroll {
        match event.alignment & 0x3 {
            ass::HALIGN_CENTER => ass::ASS_JUSTIFY_CENTER,
            ass::HALIGN_RIGHT => ass::ASS_JUSTIFY_RIGHT,
            _ => ass::ASS_JUSTIFY_LEFT,
        }
    } else {
        event.justify
    };
    let justify = match requested_justify {
        ass::ASS_JUSTIFY_LEFT | ass::ASS_JUSTIFY_CENTER | ass::ASS_JUSTIFY_RIGHT => {
            requested_justify
        }
        _ => match event.alignment & 0x3 {
            ass::HALIGN_LEFT => ass::ASS_JUSTIFY_LEFT,
            ass::HALIGN_RIGHT => ass::ASS_JUSTIFY_RIGHT,
            _ => ass::ASS_JUSTIFY_CENTER,
        },
    };
    let line_offset = match justify {
        ass::ASS_JUSTIFY_LEFT => 0,
        ass::ASS_JUSTIFY_RIGHT => block_width - line_width,
        _ => (block_width - line_width) / 2,
    };

    if let Some((x, _)) = effective_position {
        let block_origin = match event.alignment & 0x3 {
            ass::HALIGN_LEFT => x,
            ass::HALIGN_RIGHT => x - block_width,
            _ => x - (block_width + 1) / 2,
        };
        return block_origin + line_offset;
    }
    // libass: left edge = x2scr_left(MarginL), right edge =
    // x2scr_right(PlayResX - MarginR); halign anchors the widest line
    // within them, then Justify aligns every shorter line inside that block.
    // This mirrors ass_render.c align_lines(), including selective Justify
    // overrides supplied through the public API.
    let left_edge = mapping.x_left(f64::from(event.margin_l));
    let right_edge = mapping.x_right(f64::from(track.play_res_x) - f64::from(event.margin_r));
    let block_origin = match event.alignment & 0x3 {
        ass::HALIGN_LEFT => left_edge.round() as i32,
        ass::HALIGN_RIGHT => right_edge.round() as i32 - block_width,
        _ => ((left_edge + right_edge).round() as i32 - block_width) / 2,
    };
    block_origin + line_offset
}

pub(crate) fn scale_position(
    position: Option<(i32, i32)>,
    mapping: &EventMapping,
) -> Option<(i32, i32)> {
    position.map(|(x, y)| {
        (
            mapping.map_x_pos(f64::from(x)).round() as i32,
            mapping.map_y_pos(f64::from(y)).round() as i32,
        )
    })
}

pub(crate) fn resolve_event_position(
    track: &ParsedTrack,
    event: &LayoutEvent,
    now_ms: i64,
) -> Option<(i32, i32)> {
    event
        .position_exact
        .map(round_exact_point)
        .or(event.position)
        .or_else(|| {
            event
                .movement_exact
                .map(|movement| {
                    interpolate_move_exact(movement, track.events.get(event.event_index), now_ms)
                })
                .or_else(|| {
                    event.movement.map(|movement| {
                        interpolate_move(movement, track.events.get(event.event_index), now_ms)
                    })
                })
        })
}

pub(crate) fn interpolate_move(
    movement: ParsedMovement,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> (i32, i32) {
    let event_duration = source_event
        .map(|event| event.duration)
        .unwrap_or_default()
        .max(0) as i32;
    let event_elapsed = source_event
        .map(|event| (now_ms - event.start).clamp(0, event.duration.max(0)) as i32)
        .unwrap_or_default();

    let (t1_ms, t2_ms) = move_timing_window(movement.t1_ms, movement.t2_ms, event_duration);
    let k = if event_elapsed <= t1_ms {
        0.0
    } else if event_elapsed >= t2_ms {
        1.0
    } else {
        let delta = f64::from(move_wrapping_delta(t2_ms, t1_ms));
        f64::from(move_wrapping_delta(event_elapsed, t1_ms)) / delta
    };

    let x = f64::from(movement.end.0 - movement.start.0) * k + f64::from(movement.start.0);
    let y = f64::from(movement.end.1 - movement.start.1) * k + f64::from(movement.start.1);
    (x.round() as i32, y.round() as i32)
}

pub(crate) fn round_exact_point((x, y): (f64, f64)) -> (i32, i32) {
    (x.round() as i32, y.round() as i32)
}

pub(crate) fn interpolate_move_exact(
    movement: ParsedMovementExact,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> (i32, i32) {
    let event_duration = source_event
        .map(|event| event.duration)
        .unwrap_or_default()
        .max(0) as i32;
    let event_elapsed = source_event
        .map(|event| (now_ms - event.start).clamp(0, event.duration.max(0)) as i32)
        .unwrap_or_default();

    let (t1_ms, t2_ms) = move_timing_window(movement.t1_ms, movement.t2_ms, event_duration);
    let k = if event_elapsed <= t1_ms {
        0.0
    } else if event_elapsed >= t2_ms {
        1.0
    } else {
        let delta = f64::from(move_wrapping_delta(t2_ms, t1_ms));
        f64::from(move_wrapping_delta(event_elapsed, t1_ms)) / delta
    };

    let x = (movement.end.0 - movement.start.0) * k + movement.start.0;
    let y = (movement.end.1 - movement.start.1) * k + movement.start.1;
    round_exact_point((x, y))
}

fn move_timing_window(t1_ms: i32, t2_ms: i32, event_duration: i32) -> (i32, i32) {
    if t1_ms <= 0 && t2_ms <= 0 {
        (0, event_duration)
    } else if t1_ms <= t2_ms {
        (t1_ms, t2_ms)
    } else {
        (t2_ms, t1_ms)
    }
}

fn move_wrapping_delta(to: i32, from: i32) -> i32 {
    (to as u32).wrapping_sub(from as u32) as i32
}

#[allow(clippy::too_many_arguments)]
/// Compute per-line tops (top = baseline - asc) following libass
/// ass_render_event (ass_render.c): the event's total text height is the sum
/// of per-line asc+desc (measure_text), the first baseline is anchored per
/// valign / \pos base point (get_base_point), and successive baselines step
/// by desc[i] + asc[i+1] + line_spacing.
pub(crate) fn compute_vertical_layout(
    track: &ParsedTrack,
    metrics: &[LineMetrics],
    alignment: i32,
    margin_v: i32,
    position: Option<(i32, i32)>,
    config: &RendererConfig,
    mapping: &EventMapping,
) -> Vec<i32> {
    let spacing = line_spacing(config);
    let total = total_text_height(metrics, config);

    let first_top = if let Some((_, y)) = position {
        let y = f64::from(y);
        match alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
            ass::VALIGN_TOP => y,
            ass::VALIGN_CENTER => y - total / 2.0,
            _ => y - total,
        }
    } else {
        match alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
            ass::VALIGN_TOP => mapping.y_top(f64::from(margin_v)),
            ass::VALIGN_CENTER => mapping.y_center(f64::from(track.play_res_y) / 2.0) - total / 2.0,
            _ => {
                let scr_bottom = mapping.y_sub(f64::from(track.play_res_y) - f64::from(margin_v));
                let scr_top = mapping.y_top(0.0);
                let line_position = config.line_position.clamp(0.0, 100.0);
                let mut top = scr_bottom + (scr_top - scr_bottom) * line_position / 100.0 - total;
                // libass clips to the top edge when line_position pushes the
                // subtitle off-screen, but never otherwise.
                if top < scr_top && line_position > 0.0 {
                    top = scr_top;
                }
                top
            }
        }
    };

    let mut positions = Vec::with_capacity(metrics.len());
    let mut current = first_top;
    for line in metrics {
        positions.push(current.round() as i32);
        current += line.height() + spacing;
    }
    positions
}

/// One event's finished images plus the collision metadata libass keeps in
/// EventImages.
pub(crate) struct RenderedEvent {
    pub(crate) event_index: usize,
    pub(crate) planes: Vec<ImagePlane>,
    /// libass EventImages rect: metric line box plus borders, not ink.
    pub(crate) collision_rect: Option<Rect>,
    pub(crate) detect_collisions: bool,
    pub(crate) shift_direction: i32,
    pub(crate) frame_clip: Rect,
}

/// Port of libass fit_rect (ass_render.c): accumulate the shift needed to
/// clear every already-used rect in the given direction; `used` is sorted by
/// y_min.
fn fit_rect(rect: Rect, used: &[Rect], direction: i32) -> i32 {
    let mut shift = 0;
    if direction >= 0 {
        for fixed in used {
            if rect.y_max + shift <= fixed.y_min
                || rect.y_min + shift >= fixed.y_max
                || rect.x_max <= fixed.x_min
                || rect.x_min >= fixed.x_max
            {
                continue;
            }
            shift = fixed.y_max - rect.y_min;
        }
    } else {
        for fixed in used.iter().rev() {
            if rect.y_max + shift <= fixed.y_min
                || rect.y_min + shift >= fixed.y_max
                || rect.x_max <= fixed.x_min
                || rect.x_min >= fixed.x_max
            {
                continue;
            }
            shift = fixed.y_min - rect.y_max;
        }
    }
    shift
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.y_min < b.y_max && b.y_min < a.y_max && a.x_min < b.x_max && b.x_min < a.x_max
}

/// Port of libass fix_collisions (ass_render.c): events already positioned
/// in a previous frame stay put while their height is unchanged and they
/// don't overlap another fixed event; everything else is shifted into free
/// space with fit_rect and becomes fixed.
pub(crate) fn fix_collisions(
    cache: &mut std::collections::HashMap<usize, Rect>,
    records: &mut [RenderedEvent],
) {
    let mut used: Vec<Rect> = Vec::new();

    // Pass 1: keep events at their cached positions where still valid.
    let mut fixed_shift = vec![None; records.len()];
    for (index, record) in records.iter().enumerate() {
        if !record.detect_collisions {
            continue;
        }
        let Some(bounds) = record.collision_rect else {
            continue;
        };
        if bounds.width() <= 0 || bounds.height() <= 0 {
            // VSFilter treats zero-area events as effectively fixed.
            continue;
        }
        let Some(cached) = cache.get(&record.event_index).copied() else {
            continue;
        };
        if cached.height() != bounds.height()
            || used.iter().any(|rect| rects_overlap(cached, *rect))
        {
            cache.remove(&record.event_index);
            continue;
        }
        used.push(cached);
        fixed_shift[index] = Some(cached.y_min - bounds.y_min);
    }
    used.sort_by_key(|rect| rect.y_min);

    // Pass 2: fit the remaining events into free space and fix them.
    for (index, record) in records.iter_mut().enumerate() {
        if !record.detect_collisions {
            continue;
        }
        if let Some(shift) = fixed_shift[index] {
            if shift != 0 {
                translate_planes_y(&mut record.planes, shift);
            }
            continue;
        }
        let Some(bounds) = record.collision_rect else {
            continue;
        };
        if bounds.width() <= 0 || bounds.height() <= 0 {
            continue;
        }
        let shift = fit_rect(bounds, &used, record.shift_direction);
        if shift != 0 {
            translate_planes_y(&mut record.planes, shift);
        }
        let assigned = Rect {
            x_min: bounds.x_min,
            y_min: bounds.y_min + shift,
            x_max: bounds.x_max,
            y_max: bounds.y_max + shift,
        };
        cache.insert(record.event_index, assigned);
        used.push(assigned);
        used.sort_by_key(|rect| rect.y_min);
    }
}

/// libass sorts events by layer and fixes collisions independently for each
/// contiguous same-layer group before concatenating the image lists.
pub(crate) fn fix_collisions_by_layer(
    cache: &mut std::collections::HashMap<usize, Rect>,
    records: &mut [RenderedEvent],
    track: &ParsedTrack,
) {
    let mut start = 0;
    while start < records.len() {
        let layer = track
            .events
            .get(records[start].event_index)
            .map(|event| event.layer)
            .unwrap_or(0);
        let mut end = start + 1;
        while end < records.len()
            && track
                .events
                .get(records[end].event_index)
                .map(|event| event.layer)
                .unwrap_or(0)
                == layer
        {
            end += 1;
        }
        fix_collisions(cache, &mut records[start..end]);
        start = end;
    }
}

pub(crate) fn translate_planes_y(planes: &mut [ImagePlane], delta_y: i32) {
    if delta_y == 0 {
        return;
    }
    for plane in planes {
        plane.destination.y += delta_y;
    }
}

/// Decoration bars per libass ass_get_glyph_outline: underline from the post
/// table, strikeout from OS/2, each a rect spanning the run advance at
/// font-metric position/thickness (scaled by \fscy), part of the glyph
/// geometry so it inherits border and shadow treatment.
pub(crate) fn text_decoration_bars(
    style: &ParsedSpanStyle,
    font: &FontMatch,
    baseline_y: i32,
    origin_x: i32,
    width: i32,
) -> Vec<Rect> {
    if width <= 0 || !(style.underline || style.strike_out) {
        return Vec::new();
    }
    let size_26_6 = (style.font_size.max(1.0) * 64.0).round() as i32;
    let Some(metrics) = font_vertical_metrics(font, size_26_6) else {
        return Vec::new();
    };
    let scale_y = style_scale(style.scale_y);
    let mut bars = Vec::new();
    let mut push_bar = |line: (i32, i32)| {
        let top = (f64::from(line.0) / 64.0 * scale_y).round() as i32;
        let thickness = ((f64::from(line.1) / 64.0 * scale_y).round() as i32).max(1);
        bars.push(Rect {
            x_min: origin_x,
            y_min: baseline_y + top,
            x_max: origin_x + width,
            y_max: baseline_y + top + thickness,
        });
    };
    if style.underline {
        if let Some(line) = metrics.underline_26_6 {
            push_bar(line);
        }
    }
    if style.strike_out {
        if let Some(line) = metrics.strikeout_26_6 {
            push_bar(line);
        }
    }
    bars
}

pub(crate) fn solid_plane_from_rect(rect: Rect, color: u32, kind: ass::ImageType) -> ImagePlane {
    let width = rect.width().max(0);
    let height = rect.height().max(0);
    ImagePlane {
        size: Size { width, height },
        stride: width,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: rect.x_min,
            y: rect.y_min,
        },
        kind,
        bitmap: vec![255; (width * height).max(0) as usize],
    }
}

/// Composite a run's glyph bitmaps into one plane.  Glyphs sit on the
/// line baseline: y = ascender - glyph.top, where ascender is the line's
/// metric ascent (libass: pos.y = baseline; bitmaps offset by glyph top).
#[cfg(test)]
pub(crate) fn combined_image_plane_from_glyphs(
    glyphs: &[RasterGlyph],
    origin_x_26_6: i32,
    line_top: i32,
    ascender: Option<i32>,
    color: u32,
    kind: ass::ImageType,
    blur_radius: u32,
) -> Option<ImagePlane> {
    combined_image_plane_from_glyphs_xy(
        glyphs,
        origin_x_26_6,
        line_top,
        ascender,
        color,
        kind,
        BitmapBlur {
            radius_x: blur_radius,
            radius_y: blur_radius,
            be: 0,
        },
    )
}

pub(crate) fn combined_image_plane_from_glyphs_xy(
    glyphs: &[RasterGlyph],
    origin_x_26_6: i32,
    line_top: i32,
    ascender: Option<i32>,
    color: u32,
    kind: ass::ImageType,
    blur: BitmapBlur,
) -> Option<ImagePlane> {
    let ascender =
        ascender.unwrap_or_else(|| glyphs.iter().map(|glyph| glyph.top).max().unwrap_or(0));
    // libass accumulates the pen in 26.6 units and floors per glyph
    // (render_and_combine_glyphs: x = pos.x >> 6).
    let mut pen_x = origin_x_26_6;
    let mut pen_y = 0_i32;
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x_26_6;
            pen_y += glyph.advance_y_26_6;
            continue;
        }
        let offset_x_26_6 = if glyph.offset_x_26_6 == 0 && glyph.offset_x != 0 {
            glyph.offset_x * 64
        } else {
            glyph.offset_x_26_6
        };
        let offset_y_26_6 = if glyph.offset_y_26_6 == 0 && glyph.offset_y != 0 {
            glyph.offset_y * 64
        } else {
            glyph.offset_y_26_6
        };
        let x = ((pen_x + offset_x_26_6) >> 6) + glyph.left;
        let y = ascender - glyph.top + ((-pen_y + offset_y_26_6) >> 6);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + glyph.width);
        max_y = max_y.max(y + glyph.height);
        pen_x += glyph.advance_x_26_6;
        pen_y += glyph.advance_y_26_6;
    }

    if min_x == i32::MAX || min_y == i32::MAX || max_x <= min_x || max_y <= min_y {
        return None;
    }

    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let mut bitmap = vec![0_u8; width * height];
    pen_x = origin_x_26_6;
    pen_y = 0;
    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x_26_6;
            pen_y += glyph.advance_y_26_6;
            continue;
        }
        let offset_x_26_6 = if glyph.offset_x_26_6 == 0 && glyph.offset_x != 0 {
            glyph.offset_x * 64
        } else {
            glyph.offset_x_26_6
        };
        let offset_y_26_6 = if glyph.offset_y_26_6 == 0 && glyph.offset_y != 0 {
            glyph.offset_y * 64
        } else {
            glyph.offset_y_26_6
        };
        let x0 = (((pen_x + offset_x_26_6) >> 6) + glyph.left - min_x) as usize;
        let y0 = (ascender - glyph.top + ((-pen_y + offset_y_26_6) >> 6) - min_y) as usize;
        let glyph_width = glyph.width as usize;
        let glyph_height = glyph.height as usize;
        let glyph_stride = glyph.stride as usize;
        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let src = glyph.bitmap[y * glyph_stride + x];
                let dst = &mut bitmap[(y0 + y) * width + x0 + x];
                // Glyph outlines within a composite are coverage masks, not
                // set membership: overlapping coverage adds and clips at 255.
                *dst = dst.saturating_add(src);
            }
        }
        pen_x += glyph.advance_x_26_6;
        pen_y += glyph.advance_y_26_6;
    }

    let (bitmap, width, height, pad_x, pad_y) = blur_bitmap_xy(bitmap, width, height, blur);
    Some(ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: min_x - pad_x as i32,
            y: line_top + min_y - pad_y as i32,
        },
        kind,
        bitmap,
    })
}
