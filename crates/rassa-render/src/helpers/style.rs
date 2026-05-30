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
        let start_ms = transform.start_ms.max(0);
        let end_ms = transform
            .end_ms
            .unwrap_or(event.duration.max(0) as i32)
            .max(start_ms);
        let progress = if elapsed <= start_ms {
            0.0
        } else if elapsed >= end_ms {
            1.0
        } else {
            let linear = f64::from(elapsed - start_ms) / f64::from((end_ms - start_ms).max(1));
            linear.powf(transform.accel)
        };

        if let Some(font_size) = transform.style.font_size {
            style.font_size = interpolate_f64(style.font_size, font_size, progress);
        }
        if let Some(scale_x) = transform.style.scale_x {
            style.scale_x = interpolate_f64(style.scale_x, scale_x, progress);
        }
        if let Some(scale_y) = transform.style.scale_y {
            style.scale_y = interpolate_f64(style.scale_y, scale_y, progress);
        }
        if let Some(spacing) = transform.style.spacing {
            style.spacing = interpolate_f64(style.spacing, spacing, progress);
        }
        if let Some(rotation_x) = transform.style.rotation_x {
            style.rotation_x = interpolate_f64(style.rotation_x, rotation_x, progress);
        }
        if let Some(rotation_y) = transform.style.rotation_y {
            style.rotation_y = interpolate_f64(style.rotation_y, rotation_y, progress);
        }
        if let Some(rotation_z) = transform.style.rotation_z {
            style.rotation_z = interpolate_f64(style.rotation_z, rotation_z, progress);
        }
        if let Some(shear_x) = transform.style.shear_x {
            style.shear_x = interpolate_f64(style.shear_x, shear_x, progress);
        }
        if let Some(shear_y) = transform.style.shear_y {
            style.shear_y = interpolate_f64(style.shear_y, shear_y, progress);
        }
        if let Some(color) = transform.style.primary_colour {
            style.primary_colour = interpolate_color(style.primary_colour, color, progress);
        }
        if let Some(color) = transform.style.secondary_colour {
            style.secondary_colour = interpolate_color(style.secondary_colour, color, progress);
        }
        if let Some(color) = transform.style.outline_colour {
            style.outline_colour = interpolate_color(style.outline_colour, color, progress);
        }
        if let Some(color) = transform.style.back_colour {
            style.back_colour = interpolate_color(style.back_colour, color, progress);
        }
        if let Some(border) = transform.style.border {
            style.border = interpolate_f64(style.border, border, progress);
            style.border_x = style.border;
            style.border_y = style.border;
        }
        if let Some(border_x) = transform.style.border_x {
            style.border_x = interpolate_f64(style.border_x, border_x, progress);
        }
        if let Some(border_y) = transform.style.border_y {
            style.border_y = interpolate_f64(style.border_y, border_y, progress);
        }
        if let Some(blur) = transform.style.blur {
            style.blur = interpolate_f64(style.blur, blur, progress);
        }
        if let Some(be) = transform.style.be {
            style.be = interpolate_f64(style.be, be, progress);
        }
        if let Some(shadow) = transform.style.shadow {
            style.shadow = interpolate_f64(style.shadow, shadow, progress);
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

    style
}

pub(crate) fn apply_renderer_style_scale(
    mut style: ParsedSpanStyle,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: f64,
) -> ParsedSpanStyle {
    let scale = renderer_font_scale(config) * style_scale(render_scale);
    if (scale - 1.0).abs() >= f64::EPSILON {
        style.font_size *= scale;
        style.spacing *= scale;
        style.border *= scale;
        style.border_x *= scale;
        style.border_y *= scale;
        style.shadow *= scale;
        style.shadow_x *= scale;
        style.shadow_y *= scale;
        style.blur *= scale;
        style.be *= scale;
    }

    if !track.scaled_border_and_shadow {
        let geometry_scale = border_shadow_compensation_scale(track, config);
        if geometry_scale > 0.0 && (geometry_scale - 1.0).abs() >= f64::EPSILON {
            style.border /= geometry_scale;
            style.border_x /= geometry_scale;
            style.border_y /= geometry_scale;
            style.shadow /= geometry_scale;
            style.shadow_x /= geometry_scale;
            style.shadow_y /= geometry_scale;
            style.blur /= geometry_scale;
            style.be /= geometry_scale;
        }
    }
    style
}

pub(crate) fn apply_text_spacing(
    glyphs: Vec<RasterGlyph>,
    style: &ParsedSpanStyle,
) -> Vec<RasterGlyph> {
    let spacing = text_spacing_advance(style);
    if spacing == 0 {
        return glyphs;
    }

    glyphs
        .into_iter()
        .map(|glyph| RasterGlyph {
            advance_x: glyph.advance_x + spacing,
            ..glyph
        })
        .collect()
}

pub(crate) fn text_spacing_advance(style: &ParsedSpanStyle) -> i32 {
    if !style.spacing.is_finite() {
        return 0;
    }
    (style.spacing * style_scale(style.scale_x)).round() as i32
}

pub(crate) fn renderer_font_scale(config: &RendererConfig) -> f64 {
    if config.font_scale.is_finite() && config.font_scale > 0.0 {
        config.font_scale
    } else {
        1.0
    }
}

pub(crate) fn border_shadow_compensation_scale(
    track: &ParsedTrack,
    config: &RendererConfig,
) -> f64 {
    let scale_x = output_scale_x(track, config).abs();
    let scale_y = output_scale_y(track, config).abs();
    let scale = (scale_x + scale_y) / 2.0;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
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
            x_advance: glyph.x_advance * scale_x,
            y_advance: glyph.y_advance * scale_y,
            x_offset: glyph.x_offset * scale_x,
            y_offset: glyph.y_offset * scale_y,
        })
        .collect()
}

pub(crate) fn apply_vertical_font_raster_advances(
    mut glyphs: Vec<RasterGlyph>,
    style: &ParsedSpanStyle,
) -> Vec<RasterGlyph> {
    if !style.font_name.starts_with('@') {
        return glyphs;
    }
    let advance = style.font_size.round().max(1.0) as i32;
    let vertical_origin_shift = (style.font_size * 0.35).round() as i32;
    for glyph in &mut glyphs {
        rotate_raster_glyph_clockwise(glyph);
        glyph.offset_x += (style.font_size * 0.24).round() as i32;
        glyph.offset_y += vertical_origin_shift;
        if glyph.advance_x != 0 || glyph.advance_y != 0 {
            glyph.advance_x = advance;
            glyph.advance_y = 0;
        }
    }
    glyphs
}

pub(crate) fn rotate_raster_glyph_clockwise(glyph: &mut RasterGlyph) {
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
            let dst_x = old_height - 1 - y;
            let dst_y = x;
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
    pub(crate) uniform: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TextLineMetrics {
    pub(crate) ascender: i32,
    pub(crate) height: Option<i32>,
    pub(crate) positioned_center_metric_anchor: bool,
    pub(crate) positioned_center_metric_plane_adjust: bool,
}

pub(crate) fn line_raster_ascender(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: RenderScale,
    use_metric_ascender: bool,
) -> i32 {
    let mut metric_ascender = 0_i32;
    let mut raster_ascender = 0_i32;
    for run in &line.runs {
        if run.drawing.is_some() || run.glyphs.is_empty() {
            continue;
        }
        let effective_style = apply_renderer_style_scale(
            resolve_run_style(run, source_event, now_ms),
            track,
            config,
            render_scale.uniform,
        );
        if use_metric_ascender {
            if let Some(ascender) = font_metric_ascender_for_run(run, &effective_style) {
                metric_ascender = metric_ascender.max(ascender);
            }
        }
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: (effective_style.font_size.max(1.0) * 64.0).round() as i32,
            hinting: config.hinting,
        });
        let glyph_infos = scale_glyph_infos(&run.glyphs, render_scale.x, render_scale.y);
        let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos) else {
            continue;
        };
        let raster_glyphs = scale_raster_glyphs(
            raster_glyphs,
            effective_style.scale_x,
            effective_style.scale_y,
        );
        let raster_glyphs = apply_text_spacing(raster_glyphs, &effective_style);
        raster_ascender = raster_ascender.max(
            raster_glyphs
                .iter()
                .map(|glyph| glyph.top)
                .max()
                .unwrap_or(0),
        );
    }
    if use_metric_ascender {
        metric_ascender.max(raster_ascender)
    } else {
        raster_ascender
    }
}

pub(crate) fn scale_raster_glyph(glyph: RasterGlyph, scale_x: f64, scale_y: f64) -> RasterGlyph {
    if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
        return RasterGlyph {
            advance_x: (f64::from(glyph.advance_x) * scale_x).round() as i32,
            advance_y: (f64::from(glyph.advance_y) * scale_y).round() as i32,
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
        advance_x: (f64::from(glyph.advance_x) * scale_x).round() as i32,
        advance_y: (f64::from(glyph.advance_y) * scale_y).round() as i32,
        bitmap,
        ..glyph
    }
}

pub(crate) fn interpolate_f64(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

pub(crate) fn interpolate_color(from: u32, to: u32, progress: f64) -> u32 {
    let progress = progress.clamp(0.0, 1.0);
    let mut result = 0_u32;
    for shift in [0_u32, 8, 16, 24] {
        let from_channel = ((from >> shift) & 0xFF) as u8;
        let to_channel = ((to >> shift) & 0xFF) as u8;
        let value =
            f64::from(from_channel) + (f64::from(to_channel) - f64::from(from_channel)) * progress;
        result |= u32::from(value.round() as u8) << shift;
    }
    result
}

pub(crate) fn compute_fad_alpha(
    fade: ParsedFade,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> u8 {
    let Some(event) = source_event else {
        return 0;
    };
    let elapsed = now_ms - event.start;
    let duration = event.duration.max(0) as i32;

    let alpha = match fade {
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
    };

    alpha.clamp(0, 255) as u8
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

pub(crate) fn with_fade_alpha(color: u32, fade_alpha: u8) -> u32 {
    if fade_alpha == 0 {
        return color;
    }
    let existing_alpha = color & 0xFF;
    let combined_alpha = existing_alpha - ((existing_alpha * u32::from(fade_alpha) + 0x7F) / 0xFF)
        + u32::from(fade_alpha);
    (color & 0xFFFF_FF00) | combined_alpha.min(0xFF)
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
