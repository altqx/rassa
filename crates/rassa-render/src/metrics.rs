use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FontVerticalMetrics {
    pub(crate) ascender_26_6: i32,
    pub(crate) descender_26_6: i32,
}

pub(crate) fn layout_line_height(config: &RendererConfig, scale_y: f64) -> i32 {
    let scale_y = style_scale(scale_y);
    let extra_spacing = if config.line_spacing.is_finite() {
        (config.line_spacing * scale_y).round() as i32
    } else {
        0
    };
    ((f64::from(LINE_HEIGHT) * scale_y).round() as i32 + extra_spacing).max(1)
}

pub(crate) fn layout_line_height_for_line(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return drawing_only_line_height(line, scale_y);
    }

    text_layout_line_height_for_line(line, config, scale_y)
}

pub(crate) fn positioned_layout_line_height_for_line(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
    _alignment: i32,
) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return drawing_only_line_height(line, scale_y);
    }

    let layout_height = layout_line_height(config, scale_y);
    if style_scale(scale_y) < 1.0 {
        return layout_height;
    }
    layout_height.max(font_metric_height_for_line(line, scale_y))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn positioned_layout_line_height_for_line_at(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: RenderScale,
    alignment: i32,
) -> i32 {
    let _ = (source_event, now_ms, track, config, render_scale, alignment);
    positioned_layout_line_height_for_line(line, config, render_scale.y, alignment)
}

pub(crate) fn text_layout_line_height_for_line(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    let scale_y = style_scale(scale_y);
    let max_font_size = line
        .runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .map(|run| run.style.font_size)
        .filter(|size| size.is_finite() && *size > 0.0)
        .fold(0.0_f64, f64::max);
    let extra_spacing = if config.line_spacing.is_finite() {
        (config.line_spacing * scale_y).round() as i32
    } else {
        0
    };
    ((max_font_size * scale_y).round() as i32 + extra_spacing).max(1)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rendered_text_alignment_width(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: RenderScale,
    use_visible_ink_bounds: bool,
    alignment: i32,
) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        let mut width = (f64::from(line.width) * style_scale(render_scale.x)).round() as i32;
        let suppress_center_padding = source_event
            .map(|event| {
                event.text.contains("\\clip")
                    || event.text.contains("\\iclip")
                    || event.text.contains("\\t(")
            })
            .unwrap_or(false);
        let has_blur = line
            .runs
            .iter()
            .any(|run| run.style.blur.max(run.style.be) > 0.0);
        let centered_identity_drawing = !suppress_center_padding
            && has_blur
            && (alignment & ass::HALIGN_CENTER) == ass::HALIGN_CENTER
            && line.runs.iter().all(|run| {
                let effective_style = resolve_run_style(run, source_event, now_ms);
                style_transform(&effective_style).is_identity()
            });
        if centered_identity_drawing {
            width += (10.0 * render_scale.x.max(0.0)).round() as i32;
        }
        return width.max(1);
    }

    let mut width = 0_i32;
    let mut leading_ink_offset = i32::MAX;
    let mut all_text_runs_identity_transform = true;
    for run in &line.runs {
        if run.drawing.is_some() {
            width += (f64::from(run.width) * style_scale(render_scale.x)).round() as i32;
            continue;
        }
        if run.glyphs.is_empty() {
            continue;
        }
        let effective_style = apply_renderer_style_scale(
            resolve_run_style(run, source_event, now_ms),
            track,
            config,
            render_scale.uniform,
        );
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: (effective_style.font_size.max(1.0) * 64.0).round() as i32,
            hinting: config.hinting,
        });
        if !style_transform(&effective_style).is_identity() {
            all_text_runs_identity_transform = false;
        }
        let glyph_infos = scale_glyph_infos(&run.glyphs, render_scale.x, render_scale.y);
        let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos) else {
            width += (f64::from(run.width) * style_scale(render_scale.x)).round() as i32;
            continue;
        };
        let raster_glyphs = apply_vertical_font_raster_advances(raster_glyphs, &effective_style);
        let raster_glyphs = scale_raster_glyphs(
            raster_glyphs,
            effective_style.scale_x,
            effective_style.scale_y,
        );
        let raster_glyphs = apply_text_spacing(raster_glyphs, &effective_style);
        for glyph in &raster_glyphs {
            if glyph.width > 0 && glyph.height > 0 && glyph.bitmap.iter().any(|value| *value > 0) {
                leading_ink_offset = leading_ink_offset.min(width + glyph.left);
            }
            width += glyph.advance_x;
        }
    }

    if leading_ink_offset != i32::MAX && leading_ink_offset > 0 {
        if use_visible_ink_bounds {
            if !all_text_runs_identity_transform {
                // libass positions transformed text from a padded event bitmap before applying the
                // transform.  Keep padding there, but do not shrink untransformed positioned text:
                // compute_string_bbox() anchors the original glyph advance width, and manually
                // subtracting pixels moves centered \pos text one high-resolution pixel to the right.
                width += leading_ink_offset * 2;
            }
        } else {
            width += leading_ink_offset;
        }
    }
    width.max(1)
}

pub(crate) fn font_metric_height_for_line(line: &rassa_layout::LayoutLine, scale_y: f64) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return drawing_only_line_height(line, scale_y);
    }

    let scale_y = style_scale(scale_y);
    line.runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .filter_map(|run| font_metric_height_for_run(run, scale_y))
        .max()
        .unwrap_or_else(|| (max_text_font_size(line) * scale_y).round() as i32)
        .max(1)
}

pub(crate) fn font_metric_height_for_run(run: &LayoutGlyphRun, scale_y: f64) -> Option<i32> {
    if run.style.font_name.starts_with('@')
        || !(run.style.font_size.is_finite() && run.style.font_size > 0.0)
    {
        return None;
    }
    let size_26_6 = (run.style.font_size * scale_y).max(1.0).round() as i32 * 64;
    let metrics = font_vertical_metrics(&run.font, size_26_6)?;
    let height = f64::from(metrics.ascender_26_6 + metrics.descender_26_6) / 64.0;
    Some((height * style_scale(run.style.scale_y)).round() as i32)
}

pub(crate) fn font_metric_ascender_for_run(
    run: &LayoutGlyphRun,
    effective_style: &ParsedSpanStyle,
) -> Option<i32> {
    if effective_style.font_name.starts_with('@')
        || !(effective_style.font_size.is_finite() && effective_style.font_size > 0.0)
    {
        return None;
    }
    let size_26_6 = (effective_style.font_size.max(1.0) * 64.0).round() as i32;
    let metrics = font_vertical_metrics(&run.font, size_26_6)?;
    Some(
        (f64::from(metrics.ascender_26_6) / 64.0 * style_scale(effective_style.scale_y)).round()
            as i32,
    )
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
pub(crate) fn font_vertical_metrics(
    font: &FontMatch,
    size_26_6: i32,
) -> Option<FontVerticalMetrics> {
    let font_path = font.path.as_ref()?;
    let library = Library::init().ok()?;
    let mut face = library
        .new_face(font_path, font.face_index.unwrap_or(0) as isize)
        .ok()?;
    request_real_dim_size(&mut face, size_26_6.max(64))?;
    let metrics = face.size_metrics()?;
    let ascender = unsafe { ffi::FT_MulFix(face.ascender().into(), metrics.y_scale) } as i32;
    let descender = unsafe { ffi::FT_MulFix((-face.descender()).into(), metrics.y_scale) } as i32;
    Some(FontVerticalMetrics {
        ascender_26_6: ascender,
        descender_26_6: descender,
    })
}

#[cfg(any(target_os = "macos", target_arch = "wasm32", not(unix)))]
pub(crate) fn font_vertical_metrics(
    _font: &FontMatch,
    _size_26_6: i32,
) -> Option<FontVerticalMetrics> {
    None
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
pub(crate) fn request_real_dim_size(face: &mut freetype::Face, size_26_6: i32) -> Option<()> {
    let mut request = ffi::FT_Size_RequestRec {
        size_request_type: ffi::FT_SIZE_REQUEST_TYPE_REAL_DIM,
        width: 0,
        height: size_26_6.into(),
        horiResolution: 0,
        vertResolution: 0,
    };
    let err = unsafe {
        ffi::FT_Request_Size(
            face.raw_mut() as *mut ffi::FT_FaceRec,
            &mut request as ffi::FT_Size_Request,
        )
    };
    (err == 0).then_some(())
}

pub(crate) fn max_text_font_size(line: &rassa_layout::LayoutLine) -> f64 {
    line.runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .map(|run| run.style.font_size)
        .filter(|size| size.is_finite() && *size > 0.0)
        .fold(0.0_f64, f64::max)
}

pub(crate) fn drawing_only_line_height(
    line: &rassa_layout::LayoutLine,
    render_scale_y: f64,
) -> i32 {
    let render_scale_y = style_scale(render_scale_y);
    line.runs
        .iter()
        .filter_map(|run| {
            let drawing = run.drawing.as_ref()?;
            let bounds = drawing.bounds()?;
            let drawing_height = (bounds.height() - 1).max(0) as f64;
            Some((drawing_height * style_scale(run.style.scale_y) * render_scale_y).round() as i32)
        })
        .max()
        .unwrap_or(0)
        .max(1)
}

pub(crate) fn unpositioned_text_y_correction(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return 0;
    }
    let layout_height = text_layout_line_height_for_line(line, config, scale_y);
    // Keep non-\pos layout on the historical bitmap-box baseline.  The newer
    // font-metric height is only for ASS positioned text anchors; using it here
    // raises margin-aligned text several pixels above libass.
    let visual_height = legacy_unpositioned_text_visual_height(line, scale_y).max(1);
    (layout_height - visual_height).max(0) / 3
}

pub(crate) fn legacy_unpositioned_text_visual_height(
    line: &rassa_layout::LayoutLine,
    scale_y: f64,
) -> i32 {
    let scale_y = style_scale(scale_y);
    (max_text_font_size(line) * scale_y * 0.52).round() as i32
}

pub(crate) fn positioned_text_y_correction(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
    alignment: i32,
    _center_transformed_position: bool,
) -> i32 {
    let layout_height = positioned_layout_line_height_for_line(line, config, scale_y, alignment);
    let metric_height = font_metric_height_for_line(line, scale_y).max(1);
    ((layout_height - metric_height).max(0) * 4) / 9
}

pub(crate) fn positioned_center_line_has_active_projective_transform(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> bool {
    line.runs.iter().any(|run| {
        let effective_style = resolve_run_style(run, source_event, now_ms);
        if !style_transform(&effective_style).is_identity() {
            return true;
        }

        let Some(event) = source_event else {
            return false;
        };
        let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0)) as i32;
        run.transforms.iter().any(|transform| {
            elapsed > transform.start_ms.max(0)
                && animated_style_affects_projective_transform(&transform.style)
        })
    })
}

pub(crate) fn animated_style_affects_projective_transform(
    style: &rassa_parse::ParsedAnimatedStyle,
) -> bool {
    style.rotation_x.is_some()
        || style.rotation_y.is_some()
        || style.rotation_z.is_some()
        || style.shear_x.is_some()
        || style.shear_y.is_some()
}

pub(crate) fn line_text(line: &rassa_layout::LayoutLine) -> String {
    line.runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .map(|run| run.text.as_str())
        .collect()
}

pub(crate) fn renderer_blur_radius(blur: f64) -> u32 {
    if !(blur.is_finite() && blur > 0.0) {
        return 0;
    }
    (blur * 4.0).ceil().max(1.0) as u32
}

pub(crate) fn style_clip_bleed(style: &ParsedSpanStyle) -> i32 {
    let border_bleed = style.border_x.max(style.border_y).max(style.border) * 4.0;
    let shadow_bleed = style
        .shadow_x
        .abs()
        .max(style.shadow_y.abs())
        .max(style.shadow);
    let blur_bleed = renderer_blur_radius(style.blur.max(style.be)) as f64;
    (border_bleed + shadow_bleed + blur_bleed).ceil().max(0.0) as i32
}

pub(crate) fn expand_rect(rect: Rect, amount: i32) -> Rect {
    if amount <= 0 {
        return rect;
    }
    Rect {
        x_min: rect.x_min - amount,
        y_min: rect.y_min - amount,
        x_max: rect.x_max + amount,
        y_max: rect.y_max + amount,
    }
}

pub(crate) fn visible_bounds_for_planes(planes: &[ImagePlane]) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for plane in planes {
        let stride = plane.stride.max(0) as usize;
        if stride == 0 {
            continue;
        }
        for y in 0..plane.size.height.max(0) as usize {
            for x in 0..plane.size.width.max(0) as usize {
                if plane.bitmap[y * stride + x] == 0 {
                    continue;
                }
                let px = plane.destination.x + x as i32;
                let py = plane.destination.y + y as i32;
                match &mut bounds {
                    Some(rect) => {
                        rect.x_min = rect.x_min.min(px);
                        rect.y_min = rect.y_min.min(py);
                        rect.x_max = rect.x_max.max(px + 1);
                        rect.y_max = rect.y_max.max(py + 1);
                    }
                    None => {
                        bounds = Some(Rect {
                            x_min: px,
                            y_min: py,
                            x_max: px + 1,
                            y_max: py + 1,
                        });
                    }
                }
            }
        }
    }
    bounds
}

pub(crate) fn translate_planes_y(planes: &mut [ImagePlane], delta_y: i32) {
    if delta_y == 0 {
        return;
    }
    for plane in planes {
        plane.destination.y += delta_y;
    }
}
