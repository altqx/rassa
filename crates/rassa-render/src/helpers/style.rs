use super::*;

pub(crate) fn resolve_run_style(
    run: &LayoutGlyphRun,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> ParsedSpanStyle {
    let Some(event) = source_event else {
        return run.style.clone();
    };

    let mut style = run.style.clone();
    let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0)) as i32;
    for transform in &run.transforms {
        let progress = transform_progress(transform, elapsed, event.duration);

        if !transform.style.font_size_steps.is_empty() {
            for step in &transform.style.font_size_steps {
                style.font_size = apply_font_size_transform_step(style.font_size, *step, progress);
            }
        } else if let Some(font_size) = transform.style.font_size {
            style.font_size = interpolate_f64(style.font_size, font_size, progress);
        }
        if !transform.style.scale_x_steps.is_empty() {
            for step in &transform.style.scale_x_steps {
                style.scale_x = apply_scale_transform_step(style.scale_x, *step, progress);
            }
        } else if let Some(scale_x) = transform.style.scale_x {
            style.scale_x = interpolate_nonnegative(style.scale_x, scale_x, progress);
        }
        if !transform.style.scale_y_steps.is_empty() {
            for step in &transform.style.scale_y_steps {
                style.scale_y = apply_scale_transform_step(style.scale_y, *step, progress);
            }
        } else if let Some(scale_y) = transform.style.scale_y {
            style.scale_y = interpolate_nonnegative(style.scale_y, scale_y, progress);
        }
        if !transform.style.spacing_steps.is_empty() {
            for step in &transform.style.spacing_steps {
                style.spacing = apply_linear_transform_step(style.spacing, *step, progress);
            }
        } else if let Some(spacing) = transform.style.spacing {
            style.spacing = interpolate_f64(style.spacing, spacing, progress);
        }
        if !transform.style.rotation_x_steps.is_empty() {
            for step in &transform.style.rotation_x_steps {
                style.rotation_x = apply_linear_transform_step(style.rotation_x, *step, progress);
            }
        } else if let Some(rotation_x) = transform.style.rotation_x {
            style.rotation_x = interpolate_f64(style.rotation_x, rotation_x, progress);
        }
        if !transform.style.rotation_y_steps.is_empty() {
            for step in &transform.style.rotation_y_steps {
                style.rotation_y = apply_linear_transform_step(style.rotation_y, *step, progress);
            }
        } else if let Some(rotation_y) = transform.style.rotation_y {
            style.rotation_y = interpolate_f64(style.rotation_y, rotation_y, progress);
        }
        if !transform.style.rotation_z_steps.is_empty() {
            for step in &transform.style.rotation_z_steps {
                style.rotation_z = apply_linear_transform_step(style.rotation_z, *step, progress);
            }
        } else if let Some(rotation_z) = transform.style.rotation_z {
            style.rotation_z = interpolate_f64(style.rotation_z, rotation_z, progress);
        }
        if !transform.style.shear_x_steps.is_empty() {
            for step in &transform.style.shear_x_steps {
                style.shear_x = apply_linear_transform_step(style.shear_x, *step, progress);
            }
        } else if let Some(shear_x) = transform.style.shear_x {
            style.shear_x = interpolate_f64(style.shear_x, shear_x, progress);
        }
        if !transform.style.shear_y_steps.is_empty() {
            for step in &transform.style.shear_y_steps {
                style.shear_y = apply_linear_transform_step(style.shear_y, *step, progress);
            }
        } else if let Some(shear_y) = transform.style.shear_y {
            style.shear_y = interpolate_f64(style.shear_y, shear_y, progress);
        }
        if !transform.style.primary_colour_steps.is_empty() {
            for step in &transform.style.primary_colour_steps {
                style.primary_colour =
                    apply_colour_transform_step(style.primary_colour, *step, progress);
            }
        } else if let Some(color) = transform.style.primary_colour {
            style.primary_colour = interpolate_color(style.primary_colour, color, progress);
        }
        if !transform.style.secondary_colour_steps.is_empty() {
            for step in &transform.style.secondary_colour_steps {
                style.secondary_colour =
                    apply_colour_transform_step(style.secondary_colour, *step, progress);
            }
        } else if let Some(color) = transform.style.secondary_colour {
            style.secondary_colour = interpolate_color(style.secondary_colour, color, progress);
        }
        if !transform.style.outline_colour_steps.is_empty() {
            for step in &transform.style.outline_colour_steps {
                style.outline_colour =
                    apply_colour_transform_step(style.outline_colour, *step, progress);
            }
        } else if let Some(color) = transform.style.outline_colour {
            style.outline_colour = interpolate_color(style.outline_colour, color, progress);
        }
        if !transform.style.back_colour_steps.is_empty() {
            for step in &transform.style.back_colour_steps {
                style.back_colour = apply_colour_transform_step(style.back_colour, *step, progress);
            }
        } else if let Some(color) = transform.style.back_colour {
            style.back_colour = interpolate_color(style.back_colour, color, progress);
        }
        if !transform.style.border_x_steps.is_empty() || !transform.style.border_y_steps.is_empty()
        {
            for step in &transform.style.border_x_steps {
                style.border_x = apply_axis_transform_step(style.border_x, *step, progress);
            }
            for step in &transform.style.border_y_steps {
                style.border_y = apply_axis_transform_step(style.border_y, *step, progress);
            }
            style.border = style.border_x.max(style.border_y);
        } else {
            // Interpolate \xbord/\ybord from the pre-\bord base; \t(\bord) would otherwise compound.
            let base_border_x = style.border_x;
            let base_border_y = style.border_y;
            if let Some(border) = transform.style.border {
                style.border = interpolate_nonnegative(style.border, border, progress);
                style.border_x = style.border;
                style.border_y = style.border;
            }
            if let Some(border_x) = transform.style.border_x {
                style.border_x = interpolate_nonnegative(base_border_x, border_x, progress);
            }
            if let Some(border_y) = transform.style.border_y {
                style.border_y = interpolate_nonnegative(base_border_y, border_y, progress);
            }
        }
        if !transform.style.blur_steps.is_empty() {
            for step in &transform.style.blur_steps {
                style.blur = apply_blur_transform_step(style.blur, *step, progress);
            }
        } else if let Some(blur) = transform.style.blur {
            style.blur = interpolate_blur(style.blur, blur, progress);
        }
        if !transform.style.be_steps.is_empty() {
            for step in &transform.style.be_steps {
                style.be = apply_be_transform_step(style.be, *step, progress);
            }
        } else if let Some(be) = transform.style.be {
            style.be = interpolate_be(style.be, be, progress);
        }
        if !transform.style.shadow_x_steps.is_empty() || !transform.style.shadow_y_steps.is_empty()
        {
            for step in &transform.style.shadow_x_steps {
                style.shadow_x = apply_axis_transform_step(style.shadow_x, *step, progress);
            }
            for step in &transform.style.shadow_y_steps {
                style.shadow_y = apply_axis_transform_step(style.shadow_y, *step, progress);
            }
            style.shadow = style.shadow_x.max(style.shadow_y);
        } else {
            if let Some(shadow) = transform.style.shadow {
                style.shadow = interpolate_nonnegative(style.shadow, shadow, progress);
                style.shadow_x = style.shadow;
                style.shadow_y = style.shadow;
            }
            if let Some(shadow_x) = transform.style.shadow_x {
                style.shadow_x = interpolate_f64(style.shadow_x, shadow_x, progress);
            }
            if let Some(shadow_y) = transform.style.shadow_y {
                style.shadow_y = interpolate_f64(style.shadow_y, shadow_y, progress);
            }
        }
    }

    style
}

fn apply_font_size_transform_step(
    current: f64,
    step: ParsedFontSizeTransform,
    progress: f64,
) -> f64 {
    let (resolved, reset) = match step {
        ParsedFontSizeTransform::Reset { reset } => return reset,
        ParsedFontSizeTransform::Absolute { value, reset } => {
            (current * (1.0 - progress) + value * progress, reset)
        }
        ParsedFontSizeTransform::Relative { value, reset } => {
            (current * (1.0 + progress * value / 10.0), reset)
        }
    };

    if resolved > 0.0 { resolved } else { reset }
}

fn apply_scale_transform_step(current: f64, step: ParsedScaleTransform, progress: f64) -> f64 {
    match step {
        ParsedScaleTransform::Reset { reset } => reset,
        ParsedScaleTransform::Absolute { value, .. } => {
            (current * (1.0 - progress) + value * progress).max(0.0)
        }
    }
}

fn apply_linear_transform_step(current: f64, step: ParsedLinearTransform, progress: f64) -> f64 {
    match step {
        ParsedLinearTransform::Reset { reset } => reset,
        ParsedLinearTransform::Absolute { value, .. } => interpolate_f64(current, value, progress),
    }
}

fn apply_axis_transform_step(current: f64, step: ParsedAxisTransform, progress: f64) -> f64 {
    match step {
        ParsedAxisTransform::Reset { reset } => reset,
        ParsedAxisTransform::Absolute { value, clamp, .. } => {
            let resolved = interpolate_f64(current, value, progress);
            if clamp { resolved.max(0.0) } else { resolved }
        }
    }
}

fn apply_colour_transform_step(current: u32, step: ParsedColourTransform, progress: f64) -> u32 {
    match step {
        ParsedColourTransform::ResetRgb { reset } => {
            (current & 0xFF00_0000) | (reset & 0x00FF_FFFF)
        }
        ParsedColourTransform::Rgb { value } => {
            (current & 0xFF00_0000)
                | (interpolate_color(current & 0x00FF_FFFF, value & 0x00FF_FFFF, progress)
                    & 0x00FF_FFFF)
        }
        ParsedColourTransform::ResetAlpha { reset } => with_alpha(current, reset),
        ParsedColourTransform::Alpha { value } => {
            let current_alpha = ((current >> 24) & 0xFF) as u8;
            let alpha = interpolate_alpha_channel(current_alpha, value, progress);
            with_alpha(current, alpha)
        }
    }
}

fn interpolate_alpha_channel(from: u8, to: i32, progress: f64) -> u8 {
    let progress = progress.clamp(0.0, 1.0);
    let old = f64::from(from);
    let new = f64::from(to as u32);
    libass_dtoi32(new * progress + old * (1.0 - progress)) as u8
}

fn libass_dtoi32(value: f64) -> i32 {
    if value.is_nan() || value <= f64::from(i32::MIN) || value >= f64::from(i32::MAX) + 1.0 {
        return i32::MIN;
    }
    value.trunc() as i32
}

fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color & 0x00FF_FFFF) | (u32::from(alpha) << 24)
}

fn apply_blur_transform_step(current: f64, step: ParsedLinearTransform, progress: f64) -> f64 {
    match step {
        ParsedLinearTransform::Reset { reset } => reset,
        ParsedLinearTransform::Absolute { value, .. } => interpolate_blur(current, value, progress),
    }
}

fn apply_be_transform_step(current: f64, step: ParsedLinearTransform, progress: f64) -> f64 {
    match step {
        ParsedLinearTransform::Reset { reset } => reset,
        ParsedLinearTransform::Absolute { value, .. } => interpolate_be(current, value, progress),
    }
}

fn transform_progress(
    transform: &rassa_parse::ParsedSpanTransform,
    elapsed: i32,
    duration: i64,
) -> f64 {
    let start_ms = transform.start_ms;
    let mut end_ms = transform.end_ms.unwrap_or(0);
    if end_ms == 0 {
        end_ms = duration.max(0) as i32;
    }

    if elapsed < start_ms {
        0.0
    } else if elapsed >= end_ms {
        1.0
    } else {
        let delta = (end_ms as u32).wrapping_sub(start_ms as u32) as i32;
        let elapsed_delta = (elapsed as u32).wrapping_sub(start_ms as u32) as i32;
        (f64::from(elapsed_delta) / f64::from(delta)).powf(transform.accel)
    }
}

/// Lerp animated \t(\clip/\iclip) from the static clip (or full frame); mode follows the latest clip tag.
pub(crate) fn resolve_rect_clip(
    event: &LayoutEvent,
    track: &ParsedTrack,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Option<(ParsedRectF64, bool)> {
    let animated = event
        .lines
        .iter()
        .flat_map(|line| line.runs.iter())
        .flat_map(|run| run.transforms.iter())
        .filter(|transform| transform.style.clip_rect.is_some())
        .collect::<Vec<_>>();
    if animated.is_empty() {
        return event.clip_rect.map(|rect| {
            (
                ParsedRectF64 {
                    x_min: f64::from(rect.x_min),
                    y_min: f64::from(rect.y_min),
                    x_max: f64::from(rect.x_max),
                    y_max: f64::from(rect.y_max),
                },
                event.inverse_clip,
            )
        });
    }
    let source = source_event?;
    let elapsed = (now_ms - source.start).clamp(0, source.duration.max(0)) as i32;
    let base = event.clip_rect.unwrap_or(Rect {
        x_min: 0,
        y_min: 0,
        x_max: track.play_res_x,
        y_max: track.play_res_y,
    });
    let mut current = [
        f64::from(base.x_min),
        f64::from(base.y_min),
        f64::from(base.x_max),
        f64::from(base.y_max),
    ];
    let mut inverse = event.inverse_clip;
    for transform in animated {
        let target = transform
            .style
            .clip_rect
            .expect("filtered to clip transforms");
        if let Some(animated_inverse) = transform.style.clip_inverse {
            inverse = animated_inverse;
        }
        let progress = transform_progress(transform, elapsed, source.duration);
        let lerp = |from: f64, to: f64| from + (to - from) * progress;
        current = [
            lerp(current[0], target.x_min),
            lerp(current[1], target.y_min),
            lerp(current[2], target.x_max),
            lerp(current[3], target.y_max),
        ];
    }
    Some((
        ParsedRectF64 {
            x_min: current[0],
            y_min: current[1],
            x_max: current[2],
            y_max: current[3],
        },
        inverse,
    ))
}

pub(crate) fn apply_renderer_style_scale(
    mut style: ParsedSpanStyle,
    track: &ParsedTrack,
    config: &RendererConfig,
    font_scale: f64,
    render_scale: RenderScale,
) -> ParsedSpanStyle {
    let font_scale = if font_scale.is_finite() {
        font_scale.max(0.0)
    } else {
        1.0
    };
    let screen_x = style_scale(render_scale.x);
    let screen_y = style_scale(render_scale.y);
    style.font_size *= font_scale * screen_y;
    // Net \fsp scale is screen_scale_x (libass uses screen_scale_x/PAR then reapplies PAR).
    style.spacing *= font_scale * screen_x;

    let (geometry_x, geometry_y) = if track.scaled_border_and_shadow {
        (screen_x, screen_y)
    } else {
        unscaled_border_shadow_scales(track, config)
    };
    let geometry_x = font_scale * geometry_x;
    let geometry_y = font_scale * geometry_y;
    style.border_x *= geometry_x;
    style.border_y *= geometry_y;
    style.border = style.border_x.max(style.border_y);
    style.shadow_x *= geometry_x;
    style.shadow_y *= geometry_y;
    style.shadow = style.shadow_x.abs().max(style.shadow_y.abs());

    // Blur stays in script space here; blur_scale_x/y apply later and \be is an unscaled pass count.
    style
}

pub(crate) fn apply_text_spacing(
    glyphs: Vec<RasterGlyph>,
    style: &ParsedSpanStyle,
) -> Vec<RasterGlyph> {
    let spacing_26_6 = text_spacing_advance_26_6(style);
    if spacing_26_6 == 0 {
        return glyphs;
    }

    glyphs
        .into_iter()
        .map(|glyph| RasterGlyph {
            advance_x: glyph.advance_x + ((spacing_26_6 + 32) >> 6),
            advance_x_26_6: glyph.advance_x_26_6 + spacing_26_6,
            ..glyph
        })
        .collect()
}

/// \fsp advance in 26.6: double_to_d6(spacing)*scale_x per cluster, not rounded to whole pixels.
pub(crate) fn text_spacing_advance_26_6(style: &ParsedSpanStyle) -> i32 {
    if !style.spacing.is_finite() {
        return 0;
    }
    (style.spacing * style_scale(style.scale_x) * 64.0).round() as i32
}

pub(crate) fn renderer_font_scale(config: &RendererConfig) -> f64 {
    if config.font_scale.is_finite() {
        config.font_scale
    } else {
        1.0
    }
}

pub(crate) fn renderer_font_scale_for_event(config: &RendererConfig, explicit: bool) -> f64 {
    if config.selective_font_scale && explicit {
        1.0
    } else {
        renderer_font_scale(config)
    }
}

pub(crate) fn unscaled_border_shadow_scales(
    track: &ParsedTrack,
    config: &RendererConfig,
) -> (f64, f64) {
    let frame = frame_content_size(track, config);
    let layout = filter_layout_resolution(track, config);
    let x = f64::from(frame.width.max(1)) / f64::from(layout.width.max(1));
    let y = f64::from(frame.height.max(1)) / f64::from(layout.height.max(1));
    (style_scale(x), style_scale(y))
}

pub(crate) fn scale_glyph_infos(
    glyphs: &[GlyphInfo],
    scale_x: f64,
    scale_y: f64,
) -> Vec<GlyphInfo> {
    let scale_x = style_scale(scale_x) as f32;
    let scale_y = style_scale(scale_y) as f32;
    glyphs
        .iter()
        .map(|glyph| GlyphInfo {
            glyph_id: glyph.glyph_id,
            cluster: glyph.cluster,
            vertical_rotation_eligible: glyph.vertical_rotation_eligible,
            x_advance: glyph.x_advance * scale_x,
            y_advance: glyph.y_advance * scale_y,
            x_offset: glyph.x_offset * scale_x,
            y_offset: glyph.y_offset * scale_y,
            positioning: glyph.positioning,
        })
        .collect()
}

/// Scale layout positions to this frame's font size; PlayResX must not stretch HarfBuzz advances.
pub(crate) fn shaped_position_render_scale(
    run: &LayoutGlyphRun,
    effective_style: &ParsedSpanStyle,
    _render_scale: RenderScale,
) -> (f64, f64) {
    let source_size = if run.style.font_size.is_finite() && run.style.font_size > 0.0 {
        run.style.font_size
    } else {
        1.0
    };
    let target_size = if effective_style.font_size.is_finite() {
        effective_style.font_size.max(1.0)
    } else {
        1.0
    };
    let size_scale = target_size / source_size;
    (size_scale, size_scale)
}

/// Counterclockwise @font rotation for source codepoints ≥ U+02F1 (cluster property, not glyph id).
pub(crate) fn apply_vertical_font_raster_advances(
    mut glyphs: Vec<RasterGlyph>,
    glyph_infos: &[GlyphInfo],
    style: &ParsedSpanStyle,
    font: &FontMatch,
) -> Vec<RasterGlyph> {
    if !style.font_name.starts_with('@') {
        return glyphs;
    }
    let size_26_6 = (style.font_size.max(1.0) * 64.0).round() as i32;
    let typo_descender = font_vertical_metrics(font, size_26_6)
        .map(|metrics| f64::from(metrics.typo_descender_26_6) / 64.0)
        .unwrap_or(0.0);
    for (glyph, glyph_info) in glyphs.iter_mut().zip(glyph_infos) {
        if !glyph_info.vertical_rotation_eligible {
            continue;
        }
        let vert_advance = if glyph.vert_advance_26_6 > 0 {
            f64::from(glyph.vert_advance_26_6) / 64.0
        } else {
            style.font_size.max(1.0)
        };
        let offs_x = vert_advance + typo_descender;
        let offs_y = -typo_descender;
        let old_left = glyph.left;
        let old_top = glyph.top;
        let old_width = glyph.width;
        rotate_raster_glyph_counterclockwise(glyph);
        glyph.left = offs_x.round() as i32 - old_top;
        glyph.top = old_width - old_left - offs_y.round() as i32;
        if glyph.advance_x != 0 || glyph.advance_y != 0 {
            glyph.advance_x = vert_advance.round() as i32;
            glyph.advance_y = 0;
            glyph.advance_x_26_6 = (vert_advance * 64.0).round() as i32;
            glyph.advance_y_26_6 = 0;
        }
    }
    glyphs
}

pub(crate) fn rotate_raster_glyph_counterclockwise(glyph: &mut RasterGlyph) {
    if glyph.width <= 0 || glyph.height <= 0 || glyph.stride <= 0 || glyph.bitmap.is_empty() {
        return;
    }
    let old_width = glyph.width as usize;
    let old_height = glyph.height as usize;
    let old_stride = glyph.stride as usize;
    let new_width = old_height;
    let new_height = old_width;
    let mut rotated = vec![0_u8; new_width * new_height];
    for y in 0..old_height {
        for x in 0..old_width {
            let src = y * old_stride + x;
            if src >= glyph.bitmap.len() {
                continue;
            }
            let dst_x = y;
            let dst_y = old_width - 1 - x;
            rotated[dst_y * new_width + dst_x] = glyph.bitmap[src];
        }
    }
    glyph.width = new_width as i32;
    glyph.height = new_height as i32;
    glyph.stride = new_width as i32;
    glyph.bitmap = rotated;
}

pub(crate) fn scale_raster_glyphs(
    glyphs: Vec<RasterGlyph>,
    scale_x: f64,
    scale_y: f64,
) -> Vec<RasterGlyph> {
    let scale_x = style_scale(scale_x);
    let scale_y = style_scale(scale_y);
    if (scale_x - 1.0).abs() < f64::EPSILON && (scale_y - 1.0).abs() < f64::EPSILON {
        return glyphs;
    }

    glyphs
        .into_iter()
        .map(|glyph| scale_raster_glyph(glyph, scale_x, scale_y))
        .collect()
}

pub(crate) fn style_scale(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RenderScale {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

pub(crate) fn scale_raster_glyph(glyph: RasterGlyph, scale_x: f64, scale_y: f64) -> RasterGlyph {
    if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
        return RasterGlyph {
            offset_x: (f64::from(glyph.offset_x) * scale_x).round() as i32,
            offset_y: (f64::from(glyph.offset_y) * scale_y).round() as i32,
            offset_x_26_6: (f64::from(glyph.offset_x_26_6) * scale_x).round() as i32,
            offset_y_26_6: (f64::from(glyph.offset_y_26_6) * scale_y).round() as i32,
            advance_x: (f64::from(glyph.advance_x) * scale_x).round() as i32,
            advance_y: (f64::from(glyph.advance_y) * scale_y).round() as i32,
            advance_x_26_6: (f64::from(glyph.advance_x_26_6) * scale_x).round() as i32,
            advance_y_26_6: (f64::from(glyph.advance_y_26_6) * scale_y).round() as i32,
            ..glyph
        };
    }

    let src_width = glyph.width as usize;
    let src_height = glyph.height as usize;
    let src_stride = glyph.stride.max(0) as usize;
    let dst_width = (f64::from(glyph.width) * scale_x).round().max(1.0) as usize;
    let dst_height = (f64::from(glyph.height) * scale_y).round().max(1.0) as usize;
    let mut bitmap = vec![0_u8; dst_width * dst_height];
    for row in 0..dst_height {
        let src_row = ((row * src_height) / dst_height).min(src_height - 1);
        for column in 0..dst_width {
            let src_column = ((column * src_width) / dst_width).min(src_width - 1);
            bitmap[row * dst_width + column] = glyph.bitmap[src_row * src_stride + src_column];
        }
    }

    RasterGlyph {
        width: dst_width as i32,
        height: dst_height as i32,
        stride: dst_width as i32,
        left: (f64::from(glyph.left) * scale_x).round() as i32,
        top: (f64::from(glyph.top) * scale_y).round() as i32,
        offset_x: (f64::from(glyph.offset_x) * scale_x).round() as i32,
        offset_y: (f64::from(glyph.offset_y) * scale_y).round() as i32,
        offset_x_26_6: (f64::from(glyph.offset_x_26_6) * scale_x).round() as i32,
        offset_y_26_6: (f64::from(glyph.offset_y_26_6) * scale_y).round() as i32,
        advance_x: (f64::from(glyph.advance_x) * scale_x).round() as i32,
        advance_y: (f64::from(glyph.advance_y) * scale_y).round() as i32,
        advance_x_26_6: (f64::from(glyph.advance_x_26_6) * scale_x).round() as i32,
        advance_y_26_6: (f64::from(glyph.advance_y_26_6) * scale_y).round() as i32,
        bitmap,
        ..glyph
    }
}

pub(crate) fn interpolate_f64(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

pub(crate) fn interpolate_blur(from: f64, to: f64, progress: f64) -> f64 {
    interpolate_f64(from, to, progress).clamp(0.0, 100.0)
}

pub(crate) fn interpolate_nonnegative(from: f64, to: f64, progress: f64) -> f64 {
    interpolate_f64(from, to, progress).max(0.0)
}

pub(crate) fn interpolate_be(from: f64, to: f64, progress: f64) -> f64 {
    libass_be_value(interpolate_f64(from, to, progress))
}

fn libass_be_value(raw: f64) -> f64 {
    let shifted = raw + 0.5;
    if shifted.is_nan() || shifted <= f64::from(i32::MIN) || shifted >= f64::from(i32::MAX) + 1.0 {
        return 0.0;
    }

    shifted.trunc().clamp(0.0, 127.0)
}

pub(crate) fn interpolate_color(from: u32, to: u32, progress: f64) -> u32 {
    let progress = progress.clamp(0.0, 1.0);
    let mut result = 0_u32;
    for shift in [0_u32, 8, 16, 24] {
        let from_channel = ((from >> shift) & 0xFF) as u8;
        let to_channel = ((to >> shift) & 0xFF) as u8;
        let value =
            f64::from(from_channel) + (f64::from(to_channel) - f64::from(from_channel)) * progress;
        result |= u32::from(value as u8) << shift;
    }
    result
}

pub(crate) fn compute_fad_alpha(
    fade: ParsedFade,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> i32 {
    let Some(event) = source_event else {
        return 0;
    };
    let elapsed = now_ms - event.start;
    let duration = event.duration.max(0) as i32;

    match fade {
        ParsedFade::Simple {
            fade_in_ms,
            fade_out_ms,
        } => interpolate_alpha(
            elapsed,
            0,
            fade_in_ms,
            (duration as u32).wrapping_sub(fade_out_ms as u32) as i32,
            duration,
            0xFF,
            0,
            0xFF,
        ),
        ParsedFade::Complex {
            alpha1,
            alpha2,
            alpha3,
            mut t1_ms,
            t2_ms,
            mut t3_ms,
            mut t4_ms,
        } => {
            if t1_ms == -1 && t4_ms == -1 {
                t1_ms = 0;
                t4_ms = duration;
                t3_ms = (t4_ms as u32).wrapping_sub(t3_ms as u32) as i32;
            }
            interpolate_alpha(elapsed, t1_ms, t2_ms, t3_ms, t4_ms, alpha1, alpha2, alpha3)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn interpolate_alpha(
    now: i64,
    t1: i32,
    t2: i32,
    t3: i32,
    t4: i32,
    a1: i32,
    a2: i32,
    a3: i32,
) -> i32 {
    if now < i64::from(t1) {
        a1
    } else if now < i64::from(t2) {
        let denom = (t2 as u32).wrapping_sub(t1 as u32) as i32;
        if denom == 0 {
            a2
        } else {
            let cf = ((now as u32).wrapping_sub(t1 as u32) as i32) as f64 / f64::from(denom);
            (f64::from(a1) * (1.0 - cf) + f64::from(a2) * cf) as i32
        }
    } else if now < i64::from(t3) {
        a2
    } else if now < i64::from(t4) {
        let denom = (t4 as u32).wrapping_sub(t3 as u32) as i32;
        if denom == 0 {
            a3
        } else {
            let cf = ((now as u32).wrapping_sub(t3 as u32) as i32) as f64 / f64::from(denom);
            (f64::from(a2) * (1.0 - cf) + f64::from(a3) * cf) as i32
        }
    } else {
        a3
    }
}

pub(crate) fn with_fade_alpha(color: u32, fade_alpha: i32) -> u32 {
    if fade_alpha <= 0 {
        return color;
    }
    let existing_alpha = color & 0xFF;
    let fade_alpha = fade_alpha as u32;
    let combined_alpha = existing_alpha
        - (((u64::from(existing_alpha) * u64::from(fade_alpha)) + 0x7F) / 0xFF) as u32
        + fade_alpha;
    (color & 0xFFFF_FF00) | (combined_alpha & 0xFF)
}

pub(crate) fn ass_color_to_rgba(color: u32) -> u32 {
    let alpha = (color >> 24) & 0xff;
    let blue = (color >> 16) & 0xff;
    let green = (color >> 8) & 0xff;
    let red = color & 0xff;
    (red << 24) | (green << 16) | (blue << 8) | alpha
}

pub(crate) fn rgba_color_from_ass(color: u32) -> RgbaColor {
    RgbaColor(ass_color_to_rgba(color))
}
