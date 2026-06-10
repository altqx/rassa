use super::*;

pub(crate) fn merge_compatible_event_planes(planes: Vec<ImagePlane>) -> Vec<ImagePlane> {
    let mut merged: Vec<ImagePlane> = Vec::new();
    for plane in planes {
        if let Some(target) = merged
            .iter_mut()
            .find(|candidate| compatible_plane_merge(candidate, &plane))
        {
            merge_plane_into(target, plane);
        } else {
            merged.push(plane);
        }
    }
    merged
}

/// libass render_and_combine_glyphs combines bitmaps of the same type and
/// color only when their rectangles intersect; everything else stays a
/// separate image.
pub(crate) fn compatible_plane_merge(a: &ImagePlane, b: &ImagePlane) -> bool {
    if a.kind != b.kind || a.color != b.color || a.stride <= 0 || b.stride <= 0 {
        return false;
    }
    let a_rect = Rect {
        x_min: a.destination.x,
        y_min: a.destination.y,
        x_max: a.destination.x + a.size.width,
        y_max: a.destination.y + a.size.height,
    };
    let b_rect = Rect {
        x_min: b.destination.x,
        y_min: b.destination.y,
        x_max: b.destination.x + b.size.width,
        y_max: b.destination.y + b.size.height,
    };
    a_rect.intersect(b_rect).is_some()
}

pub(crate) fn merge_plane_into(target: &mut ImagePlane, plane: ImagePlane) {
    let x_min = target.destination.x.min(plane.destination.x);
    let y_min = target.destination.y.min(plane.destination.y);
    let x_max =
        (target.destination.x + target.size.width).max(plane.destination.x + plane.size.width);
    let y_max =
        (target.destination.y + target.size.height).max(plane.destination.y + plane.size.height);
    let width = (x_max - x_min).max(0);
    let height = (y_max - y_min).max(0);
    let stride = width;
    let mut bitmap = vec![0_u8; (stride * height).max(0) as usize];
    blit_plane(&mut bitmap, stride, x_min, y_min, target);
    blit_plane(&mut bitmap, stride, x_min, y_min, &plane);
    target.destination = Point { x: x_min, y: y_min };
    target.size = Size { width, height };
    target.stride = stride;
    target.bitmap = bitmap;
}

pub(crate) fn blit_plane(
    bitmap: &mut [u8],
    stride: i32,
    origin_x: i32,
    origin_y: i32,
    plane: &ImagePlane,
) {
    if stride <= 0 || plane.stride <= 0 || plane.size.width <= 0 || plane.size.height <= 0 {
        return;
    }
    let dst_stride = stride as usize;
    let src_stride = plane.stride as usize;
    for y in 0..plane.size.height as usize {
        for x in 0..plane.size.width as usize {
            let src = plane.bitmap.get(y * src_stride + x).copied().unwrap_or(0);
            if src == 0 {
                continue;
            }
            let dst_x = (plane.destination.x - origin_x) as usize + x;
            let dst_y = (plane.destination.y - origin_y) as usize + y;
            let dst = dst_y * dst_stride + dst_x;
            if let Some(value) = bitmap.get_mut(dst) {
                *value = (*value).max(src);
            }
        }
    }
}

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

pub(crate) fn extend_planes_for_effect_motion(
    planes: Vec<ImagePlane>,
    left_pad: i32,
    right_pad: i32,
    top_pad: i32,
    bottom_pad: i32,
) -> Vec<ImagePlane> {
    planes
        .into_iter()
        .map(|plane| extend_plane_edges(plane, left_pad, right_pad, top_pad, bottom_pad))
        .collect()
}

pub(crate) fn extend_plane_edges(
    plane: ImagePlane,
    left_pad: i32,
    right_pad: i32,
    top_pad: i32,
    bottom_pad: i32,
) -> ImagePlane {
    if plane.size.width <= 0
        || plane.size.height <= 0
        || plane.stride <= 0
        || plane.bitmap.is_empty()
    {
        return plane;
    }
    let left_pad = left_pad.max(0);
    let right_pad = right_pad.max(0);
    let top_pad = top_pad.max(0);
    let bottom_pad = bottom_pad.max(0);
    if left_pad + right_pad + top_pad + bottom_pad == 0 {
        return plane;
    }
    let old_width = plane.size.width as usize;
    let old_stride = plane.stride as usize;
    let Some(ink) = plane_ink_bounds(&plane) else {
        return plane;
    };
    let ink_x_min = (ink.x_min - plane.destination.x).max(0) as usize;
    let ink_y_min = (ink.y_min - plane.destination.y).max(0) as usize;
    let ink_x_max = (ink.x_max - plane.destination.x).min(plane.size.width) as usize;
    let ink_y_max = (ink.y_max - plane.destination.y).min(plane.size.height) as usize;
    let ink_height = ink_y_max.saturating_sub(ink_y_min);
    if ink_x_max <= ink_x_min || ink_height == 0 {
        return plane;
    }

    let pixel = left_pad.max(right_pad).max(top_pad).max(bottom_pad).max(1);
    let floor_to_pixel = |value: i32| value.div_euclid(pixel) * pixel;
    let ceil_to_pixel = |value: i32| {
        value.div_euclid(pixel) * pixel + i32::from(value.rem_euclid(pixel) != 0) * pixel
    };

    let new_height = ink_height + top_pad as usize + bottom_pad as usize;
    let dest_y = plane.destination.y + ink_y_min as i32 - top_pad;
    let mut row_spans = Vec::with_capacity(new_height);
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;

    for dst_y in 0..new_height {
        let ink_row = if dst_y < top_pad as usize {
            0
        } else if dst_y >= top_pad as usize + ink_height {
            ink_height - 1
        } else {
            dst_y - top_pad as usize
        };
        let src_y = ink_y_min + ink_row;
        let src_row = &plane.bitmap[src_y * old_stride..src_y * old_stride + old_width];
        let first_lit = src_row[ink_x_min..ink_x_max]
            .iter()
            .position(|value| *value > 0)
            .map(|x| x + ink_x_min);
        let last_lit = src_row[ink_x_min..ink_x_max]
            .iter()
            .rposition(|value| *value > 0)
            .map(|x| x + ink_x_min);
        let Some(first_lit) = first_lit else {
            row_spans.push(None);
            continue;
        };
        let last_lit = last_lit.expect("row with first lit pixel should also have last lit pixel");
        let vertical_pad_row = dst_y < top_pad as usize || dst_y >= top_pad as usize + ink_height;
        let corner_row =
            (top_pad > 0 || bottom_pad > 0) && (ink_row == 0 || ink_row + 1 == ink_height);
        let suppress_horizontal_pad = vertical_pad_row || corner_row;
        let first_global = plane.destination.x + first_lit as i32;
        let last_exclusive_global = plane.destination.x + last_lit as i32 + 1;
        let (span_start, span_end) = if suppress_horizontal_pad {
            (
                ceil_to_pixel(first_global),
                ceil_to_pixel(last_exclusive_global),
            )
        } else {
            (
                floor_to_pixel(first_global - left_pad),
                ceil_to_pixel(last_exclusive_global + right_pad),
            )
        };
        if span_end <= span_start {
            row_spans.push(None);
            continue;
        }
        min_x = min_x.min(span_start);
        max_x = max_x.max(span_end);
        row_spans.push(Some((span_start, span_end)));
    }

    if min_x == i32::MAX || max_x <= min_x {
        return plane;
    }
    let new_width = (max_x - min_x) as usize;
    let mut bitmap = vec![0_u8; new_width * new_height];
    for (dst_y, span) in row_spans.into_iter().enumerate() {
        let Some((span_start, span_end)) = span else {
            continue;
        };
        let start = (span_start - min_x) as usize;
        let end = (span_end - min_x) as usize;
        bitmap[dst_y * new_width + start..dst_y * new_width + end].fill(255);
    }

    ImagePlane {
        destination: Point {
            x: min_x,
            y: dest_y,
        },
        size: Size {
            width: new_width as i32,
            height: new_height as i32,
        },
        stride: new_width as i32,
        bitmap,
        ..plane
    }
}

pub(crate) fn scale_clip_rect(rect: Rect, scale_x: f64, scale_y: f64) -> Rect {
    let scale_x = style_scale(scale_x);
    let scale_y = style_scale(scale_y);
    Rect {
        x_min: (f64::from(rect.x_min) * scale_x).round() as i32,
        y_min: (f64::from(rect.y_min) * scale_y).round() as i32,
        x_max: (f64::from(rect.x_max) * scale_x).round() as i32,
        y_max: (f64::from(rect.y_max) * scale_y).round() as i32,
    }
}

pub(crate) fn frame_clip_rect(
    track: &ParsedTrack,
    config: &RendererConfig,
    event: &LayoutEvent,
    effective_position: Option<(i32, i32)>,
) -> Rect {
    let frame_width = if config.frame.width > 0 {
        config.frame.width
    } else {
        track.play_res_x.max(0)
    };
    let frame_height = if config.frame.height > 0 {
        config.frame.height
    } else {
        track.play_res_y.max(0)
    };
    if config.use_margins
        && effective_position.is_none()
        && event.clip_rect.is_none()
        && event.vector_clip.is_none()
    {
        Rect {
            x_min: config.margins.left.max(0),
            y_min: config.margins.top.max(0),
            x_max: (frame_width - config.margins.right).max(0),
            y_max: (frame_height - config.margins.bottom).max(0),
        }
    } else {
        Rect {
            x_min: 0,
            y_min: 0,
            x_max: frame_width,
            y_max: frame_height,
        }
    }
}

pub(crate) fn compute_horizontal_origin(
    track: &ParsedTrack,
    event: &LayoutEvent,
    line_width: i32,
    effective_position: Option<(i32, i32)>,
    scale_x: f64,
) -> i32 {
    let scale_x = style_scale(scale_x);
    if let Some((x, _)) = effective_position {
        return match event.alignment & 0x3 {
            ass::HALIGN_LEFT => x,
            ass::HALIGN_RIGHT => x - line_width,
            _ => x - (line_width + 1) / 2,
        };
    }
    let frame_width = (f64::from(track.play_res_x) * scale_x).round() as i32;
    let margin_l = (f64::from(event.margin_l) * scale_x).round() as i32;
    let margin_r = (f64::from(event.margin_r) * scale_x).round() as i32;
    match event.alignment & 0x3 {
        ass::HALIGN_LEFT => margin_l,
        ass::HALIGN_RIGHT => (frame_width - margin_r - line_width).max(0),
        _ => ((margin_l + frame_width - margin_r - line_width) / 2).max(0),
    }
}

pub(crate) fn scale_position(
    position: Option<(i32, i32)>,
    scale_x: f64,
    scale_y: f64,
) -> Option<(i32, i32)> {
    let scale_x = style_scale(scale_x);
    let scale_y = style_scale(scale_y);
    position.map(|(x, y)| {
        (
            (f64::from(x) * scale_x).round() as i32,
            (f64::from(y) * scale_y).round() as i32,
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

pub(crate) fn event_layer(track: &ParsedTrack, event: &LayoutEvent) -> i32 {
    track
        .events
        .get(event.event_index)
        .map(|source| source.layer)
        .unwrap_or_default()
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

    let (t1_ms, t2_ms) = if movement.t1_ms <= 0 && movement.t2_ms <= 0 {
        (0, event_duration)
    } else if movement.t1_ms <= movement.t2_ms {
        (movement.t1_ms.max(0), movement.t2_ms.max(0))
    } else {
        (movement.t2_ms.max(0), movement.t1_ms.max(0))
    };
    let k = if event_elapsed <= t1_ms {
        0.0
    } else if event_elapsed >= t2_ms {
        1.0
    } else {
        let delta = (t2_ms - t1_ms).max(1) as f64;
        f64::from(event_elapsed - t1_ms) / delta
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

    let (t1_ms, t2_ms) = if movement.t1_ms <= 0 && movement.t2_ms <= 0 {
        (0, event_duration)
    } else if movement.t1_ms <= movement.t2_ms {
        (movement.t1_ms.max(0), movement.t2_ms.max(0))
    } else {
        (movement.t2_ms.max(0), movement.t1_ms.max(0))
    };
    let k = if event_elapsed <= t1_ms {
        0.0
    } else if event_elapsed >= t2_ms {
        1.0
    } else {
        let delta = (t2_ms - t1_ms).max(1) as f64;
        f64::from(event_elapsed - t1_ms) / delta
    };

    let x = (movement.end.0 - movement.start.0) * k + movement.start.0;
    let y = (movement.end.1 - movement.start.1) * k + movement.start.1;
    round_exact_point((x, y))
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
    render_scale: RenderScale,
) -> Vec<i32> {
    let scale_y = style_scale(render_scale.y);
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
            ass::VALIGN_TOP => f64::from(margin_v) * scale_y,
            ass::VALIGN_CENTER => (f64::from(track.play_res_y) * scale_y - total) / 2.0,
            _ => {
                let scr_bottom = (f64::from(track.play_res_y) - f64::from(margin_v)) * scale_y;
                let scr_top = 0.0;
                let line_position = config.line_position.clamp(0.0, 100.0);
                let mut top =
                    scr_bottom + (scr_top - scr_bottom) * line_position / 100.0 - total;
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
        if cached.height() != bounds.height() || used.iter().any(|rect| rects_overlap(cached, *rect))
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
pub(crate) fn combined_image_plane_from_glyphs(
    glyphs: &[RasterGlyph],
    origin_x_26_6: i32,
    line_top: i32,
    ascender: Option<i32>,
    color: u32,
    kind: ass::ImageType,
    blur_radius: u32,
) -> Option<ImagePlane> {
    let ascender =
        ascender.unwrap_or_else(|| glyphs.iter().map(|glyph| glyph.top).max().unwrap_or(0));
    // libass accumulates the pen in 26.6 units and floors per glyph
    // (render_and_combine_glyphs: x = pos.x >> 6).
    let mut pen_x = origin_x_26_6;
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x_26_6;
            continue;
        }
        let x = (pen_x >> 6) + glyph.left + glyph.offset_x;
        let y = ascender - glyph.top + glyph.offset_y;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + glyph.width);
        max_y = max_y.max(y + glyph.height);
        pen_x += glyph.advance_x_26_6;
    }

    if min_x == i32::MAX || min_y == i32::MAX || max_x <= min_x || max_y <= min_y {
        return None;
    }

    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let mut bitmap = vec![0_u8; width * height];
    pen_x = origin_x_26_6;
    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x_26_6;
            continue;
        }
        let x0 = ((pen_x >> 6) + glyph.left + glyph.offset_x - min_x) as usize;
        let y0 = (ascender - glyph.top + glyph.offset_y - min_y) as usize;
        let glyph_width = glyph.width as usize;
        let glyph_height = glyph.height as usize;
        let glyph_stride = glyph.stride as usize;
        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let src = glyph.bitmap[y * glyph_stride + x];
                let dst = &mut bitmap[(y0 + y) * width + x0 + x];
                *dst = (*dst).max(src);
            }
        }
        pen_x += glyph.advance_x_26_6;
    }

    let (bitmap, width, height, pad) = blur_bitmap(bitmap, width, height, blur_radius);
    Some(ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: min_x - pad as i32,
            y: line_top + min_y - pad as i32,
        },
        kind,
        bitmap,
    })
}

