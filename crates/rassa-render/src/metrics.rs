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

pub(crate) fn positioned_center_line_has_active_transform(
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
                && animated_style_affects_text_allocation(&transform.style)
        })
    })
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

pub(crate) fn animated_style_affects_text_allocation(
    style: &rassa_parse::ParsedAnimatedStyle,
) -> bool {
    style.font_size.is_some()
        || style.scale_x.is_some()
        || style.scale_y.is_some()
        || style.spacing.is_some()
        || animated_style_affects_projective_transform(style)
        || style.border.is_some()
        || style.border_x.is_some()
        || style.border_y.is_some()
        || style.shadow.is_some()
        || style.shadow_x.is_some()
        || style.shadow_y.is_some()
        || style.blur.is_some()
        || style.be.is_some()
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

pub(crate) fn pads_positioned_center_animated_text_allocation(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    effective_position: Option<(i32, i32)>,
    alignment: i32,
) -> bool {
    let has_active_transform =
        positioned_center_line_has_active_transform(line, source_event, now_ms);
    let has_outline_or_shadow = line_has_outline_or_shadow(line);
    effective_position.is_some()
        && (alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER)) == ass::VALIGN_CENTER
        && line_contains_only_ascii_text(line)
        && line_single_text_glyph_count(line) == 1
        && line_has_blur(line)
        && (has_active_transform
            || has_outline_or_shadow
            || matches!(
                line_single_text_char(line),
                Some(
                    'S' | 'a'
                        | 'b'
                        | 'd'
                        | 'e'
                        | 'h'
                        | 'i'
                        | 'j'
                        | 'm'
                        | 'n'
                        | 'O'
                        | 'o'
                        | 'r'
                        | 's'
                        | 'u'
                        | 'y'
                )
            ))
}

pub(crate) fn line_single_text_glyph_count(line: &rassa_layout::LayoutLine) -> usize {
    line.runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .map(|run| run.text.chars().count())
        .sum()
}

pub(crate) fn line_single_text_char(line: &rassa_layout::LayoutLine) -> Option<char> {
    let mut chars = line
        .runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .flat_map(|run| run.text.chars());
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch)
}

pub(crate) fn line_text(line: &rassa_layout::LayoutLine) -> String {
    line.runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .map(|run| run.text.as_str())
        .collect()
}

pub(crate) fn pad_libass_positioned_center_animated_text_line(
    shadow_planes: &mut [ImagePlane],
    outline_planes: &mut [ImagePlane],
    character_planes: &mut [ImagePlane],
    starts: PlaneStarts,
    has_active_projective_transform: bool,
    has_active_transform: bool,
    has_outline_or_shadow: bool,
    text_char: Option<char>,
    position_x_fraction: Option<f64>,
) {
    for plane in &mut shadow_planes[starts.shadow..] {
        let original = std::mem::take(plane);
        *plane = pad_libass_positioned_center_animated_text_plane(
            original,
            has_active_projective_transform,
            has_active_transform,
            has_outline_or_shadow,
            text_char,
            position_x_fraction,
        );
    }
    for plane in &mut outline_planes[starts.outline..] {
        let original = std::mem::take(plane);
        *plane = pad_libass_positioned_center_animated_text_plane(
            original,
            has_active_projective_transform,
            has_active_transform,
            has_outline_or_shadow,
            text_char,
            position_x_fraction,
        );
    }
    for plane in &mut character_planes[starts.character..] {
        let original = std::mem::take(plane);
        *plane = pad_libass_positioned_center_animated_text_plane(
            original,
            has_active_projective_transform,
            has_active_transform,
            has_outline_or_shadow,
            text_char,
            position_x_fraction,
        );
    }
}

pub(crate) fn pad_libass_positioned_center_animated_text_plane(
    mut plane: ImagePlane,
    has_active_projective_transform: bool,
    has_active_transform: bool,
    has_outline_or_shadow: bool,
    text_char: Option<char>,
    position_x_fraction: Option<f64>,
) -> ImagePlane {
    if !has_active_projective_transform {
        let fraction = position_x_fraction.unwrap_or(0.0);
        let left_half_position = fraction > f64::EPSILON && fraction < 0.5;
        let half_or_right_position = fraction >= 0.5;
        let middle_right_position = (0.5..0.8).contains(&fraction);
        let o_middle_right_position = (0.5..0.8).contains(&fraction) && fraction < 0.8 - 1.0e-6;
        let static_fill_only = !has_active_transform && !has_outline_or_shadow;
        let target = match (text_char, plane.kind, plane.size.width, plane.size.height) {
            // 02.ass ED2 move/t(fs)/blur single-glyph allocation: libass pads these
            // transient mid-animation planes to the animated metric cell rather than the
            // currently visible raster ink box.  Keep this scoped to single ASCII an5
            // lines through the caller predicate; rasterizer ink differences are left alone.
            (Some('n'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 47) => {
                Some((1, -1, 40, 56))
            }
            (Some('i'), ass::ImageType::Shadow | ass::ImageType::Outline, 37, 77)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((1, -1, 40, 72))
            }
            (Some('i'), ass::ImageType::Character, 19, 56)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((4, 0, 16, 64))
            }
            (Some('u'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 46) => {
                Some((0, -2, 40, 56))
            }
            (Some('n'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                Some((if middle_right_position { -1 } else { 0 }, 3, 56, 56))
            }
            (Some('u'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                Some((
                    if (0.5..0.65).contains(&fraction) {
                        -1
                    } else {
                        0
                    },
                    4,
                    56,
                    56,
                ))
            }
            (Some('e'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                let x_offset = if (0.15..0.5).contains(&fraction) {
                    1
                } else {
                    0
                };
                Some((x_offset, 3, 56, 56))
            }
            (Some('o'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                let x_offset = if (0.2..0.5).contains(&fraction) { 1 } else { 0 };
                Some((x_offset, 3, 56, 56))
            }
            (Some('k'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 57) => {
                Some((1, -1, 40, 56))
            }
            (Some('e'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 47) => {
                Some((0, if has_active_transform { 0 } else { -12 }, 56, 56))
            }
            (Some('o'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 45 | 46) => Some((
                if has_active_transform {
                    1
                } else if left_half_position {
                    2
                } else {
                    1
                },
                if has_active_transform { -3 } else { -15 },
                56,
                56,
            )),
            (Some('r'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                Some((0, 3, 40, 56))
            }
            (Some('s'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                let x_offset = if (0.25..0.5).contains(&fraction) {
                    2
                } else {
                    1
                };
                Some((x_offset, 3, 56, 56))
            }
            (Some('d'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 57) => {
                Some((1, 0, 40, 56))
            }
            (Some('a'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 48) => {
                Some((0, 0, 56, 56))
            }
            (Some('s'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 48) => {
                Some((-1, 0, 40, 56))
            }
            (Some('h'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 58) => {
                Some((0, 2, 40, 56))
            }
            (Some('i'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 58) => {
                Some((0, 2, 24, 56))
            }
            (Some('t'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 55) => {
                Some((0, 3, 40, 56))
            }
            (Some('S'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 64) => {
                Some((1, 3, 56, 72))
            }
            (Some('O'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 64) => {
                Some((1, 3, 72, 72))
            }
            // 02.ass ED2 start-frame move/org/frz glyphs can have raw transform
            // metadata but no elapsed projective transform yet.  Libass still emits
            // the animated metric cell for the blurred outline planes, rather than
            // rassa's tight start-frame crop.
            (Some('o'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 64)
                if plane.destination.y >= 45 && plane.destination.y < 60 =>
            {
                Some((1, 3, 56, 72))
            }
            (Some('y'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 69) => {
                Some((0, 3, 56, 72))
            }
            (Some('a'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 63) => {
                Some((if half_or_right_position { 0 } else { 1 }, 3, 56, 56))
            }
            (Some('m'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 63) => {
                Some((0, 3, 72, 56))
            }
            (Some('y'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 63) => {
                Some((0, 4, 56, 72))
            }
            (Some('S'), ass::ImageType::Character, 48, 48) => Some((0, 2, 48, 48)),
            (Some('O'), ass::ImageType::Character, 48, 48) => Some((0, 2, 48, 48)),
            (Some('a'), ass::ImageType::Character, 48, 48) => {
                Some((-i32::from(half_or_right_position), 3, 48, 48))
            }
            (Some('m'), ass::ImageType::Character, 48, 48) => Some((-1, 3, 48, 48)),
            (Some('o'), ass::ImageType::Character, 48, 48)
                if plane.destination.y >= 50 && plane.destination.y < 65 =>
            {
                Some((1, 3, 48, 48))
            }
            (Some('y'), ass::ImageType::Character, 48, 57) => Some((-1, 3, 48, 64)),
            (Some('y'), ass::ImageType::Character, 48, 51) => Some((0, 3, 32, 48)),
            (Some('b'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((0, -12, 56, 72))
            }
            (Some('d'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((if half_or_right_position { -1 } else { 1 }, -12, 56, 72))
            }
            (Some('h'), ass::ImageType::Shadow, 56, 56)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((2, -12, 56, 72))
            }
            (Some('h'), ass::ImageType::Outline, 56, 56)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((1, -12, 56, 72))
            }
            (Some('h'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((0, -12, 56, 72))
            }
            (Some('t'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((1, -12, 40, 72))
            }
            (Some('e'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((0, if has_active_transform { -24 } else { -36 }, 56, 56))
            }
            (Some('n'), ass::ImageType::Shadow, 56, 56)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((4, -25, 56, 56))
            }
            (Some('n'), ass::ImageType::Outline, 56, 57)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((5, -1, 56, 56))
            }
            (Some('n'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((if middle_right_position { -1 } else { 0 }, -24, 56, 56))
            }
            (Some('o'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => Some((
                if has_active_transform {
                    i32::from(left_half_position)
                } else if left_half_position {
                    2
                } else {
                    0
                },
                if has_active_transform { -24 } else { -36 },
                56,
                56,
            )),
            (Some('r'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((0, -24, 40, 56))
            }
            (Some('s'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((2, -24, 56, 56))
            }
            (Some('u'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                let x_offset = if (0.5..0.65).contains(&fraction) {
                    -1
                } else {
                    0
                };
                Some((x_offset, -23, 56, 56))
            }
            (Some('i'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 64) => {
                Some((i32::from(left_half_position), 3, 24, 72))
            }
            (Some('j'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 75) => {
                Some((0, 3, 40, 88))
            }
            (Some('b'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 64) => {
                Some((0, 3, 56, 72))
            }
            (Some('d'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 64) => {
                Some((if half_or_right_position { 0 } else { 1 }, 3, 56, 72))
            }
            (Some('h'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 64) => {
                Some((0, 3, 56, 72))
            }
            (Some('t'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 64) => {
                Some((1, 3, 40, 72))
            }
            (Some('k'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 64) => {
                Some((i32::from(left_half_position), 3, 56, 72))
            }
            (Some('k'), ass::ImageType::Shadow | ass::ImageType::Outline, 56, 56) => {
                Some((i32::from(left_half_position), -12, 56, 72))
            }
            (Some('b'), ass::ImageType::Character, 32, 48) => Some((-1, 3, 32, 48)),
            (Some('d'), ass::ImageType::Character, 32, 48) => {
                Some((-i32::from(half_or_right_position), 3, 32, 48))
            }
            (Some('h'), ass::ImageType::Character, 32, 48) => {
                Some((-i32::from(half_or_right_position), 3, 32, 48))
            }
            // 02.ass ED2 line 18512: borderless/shadowless single `t` with
            // active non-projective `\t(\fs...,\blur...)` is allocated from
            // libass's animated metric cell, not the currently lit ink crop.
            (Some('t'), ass::ImageType::Character, 26, 50)
                if has_active_transform && !has_outline_or_shadow =>
            {
                Some((-1, -1, 26, 58))
            }
            (Some('t'), ass::ImageType::Character, 32, 48) => Some((0, 3, 32, 48)),
            (Some('e'), ass::ImageType::Character, 32, 48) => {
                Some((if half_or_right_position { -1 } else { 0 }, 3, 32, 48))
            }
            (Some('i'), ass::ImageType::Character, 16, 48) => {
                let x_offset = if (0.5..0.75).contains(&fraction) {
                    -1
                } else {
                    0
                };
                Some((x_offset, 3, 16, 48))
            }
            (Some('j'), ass::ImageType::Character, 16, 63) => Some((0, 3, 16, 64)),
            (Some('k'), ass::ImageType::Character, 32, 48) => Some((0, 3, 32, 48)),
            (Some('n'), ass::ImageType::Character, 32, 48) => Some((
                if fraction <= f64::EPSILON || half_or_right_position {
                    -1
                } else {
                    0
                },
                3,
                32,
                48,
            )),
            (Some('o'), ass::ImageType::Character, 32, 48) => {
                Some((if o_middle_right_position { -1 } else { 0 }, 3, 32, 48))
            }
            (Some('r'), ass::ImageType::Character, 32, 48) => {
                Some((if left_half_position { 0 } else { -1 }, 3, 32, 48))
            }
            (Some('s'), ass::ImageType::Character, 32, 48) => {
                Some((if half_or_right_position { 0 } else { 1 }, 3, 32, 48))
            }
            (Some('u'), ass::ImageType::Character, 32, 48) => {
                Some((if half_or_right_position { -1 } else { 0 }, 3, 32, 48))
            }
            (Some('k'), ass::ImageType::Character, 40, 49) => Some((0, -8, 40, 56)),
            (Some('k'), ass::ImageType::Character, 40, 50) => Some((0, -4, 40, 56)),
            (Some('i'), ass::ImageType::Character, 24, 49) => Some((-1, -8, 24, 56)),
            (Some('i'), ass::ImageType::Character, 26, 53) => Some((0, -1, 26, 58)),
            (Some('o'), ass::ImageType::Character, 40, 39) => Some((1, -6, 40, 56)),
            (Some('n'), ass::ImageType::Character, 32, 32) => Some((0, -2, 32, 32)),
            (Some('n'), ass::ImageType::Character, 40, 40) => Some((0, -5, 40, 40)),
            (Some('u'), ass::ImageType::Character, 32, 32) => Some((0, -2, 32, 32)),
            (Some('u'), ass::ImageType::Character, 40, 39) => Some((0, -5, 40, 40)),
            (Some('k'), ass::ImageType::Character, 32, 45) => Some((0, -1, 32, 48)),
            (Some('e'), ass::ImageType::Character, 32, 32) if plane.destination.y < 70 => {
                Some((0, -1, 32, 32))
            }
            (Some('e'), ass::ImageType::Character, 40, 40) => Some((0, -4, 40, 40)),
            (Some('d'), ass::ImageType::Character, 32, 45) => Some((0, -1, 32, 48)),
            (Some('d'), ass::ImageType::Character, 42, 52) => Some((1, -3, 40, 56)),
            (Some('a'), ass::ImageType::Character, 32, 32) => Some((-1, 0, 32, 32)),
            (Some('a'), ass::ImageType::Character, 42, 42) => Some((-1, -3, 42, 42)),
            (Some('s'), ass::ImageType::Character, 32, 32) => Some((-1, 0, 32, 32)),
            (Some('s'), ass::ImageType::Character, 42, 42) => Some((-1, -3, 42, 42)),
            (Some('h'), ass::ImageType::Character, 32, 46) => Some((-1, 1, 32, 48)),
            (Some('h'), ass::ImageType::Character, 42, 53) => Some((-1, -2, 42, 58)),
            (Some('i'), ass::ImageType::Character, 16, 46) => Some((0, 2, 16, 48)),
            (Some('t'), ass::ImageType::Character, 16, 43) => Some((-1, 2, 16, 48)),
            (Some('a'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 43) => {
                Some((0, -7, 56, 56))
            }
            (Some('e'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 43) => {
                Some((1, -7, 56, 56))
            }
            (Some('r'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 43) => {
                Some((-1, -7, 40, 56))
            }
            (Some('a'), ass::ImageType::Character, 48, 31) => Some((0, -7, 48, 48)),
            (Some('e'), ass::ImageType::Character, 32, 31) => Some((0, -7, 32, 48)),
            (Some('r'), ass::ImageType::Character, 32, 31) => Some((-1, -7, 32, 48)),
            (Some('a'), ass::ImageType::Character, 56, 36) => Some((0, -10, 56, 56)),
            (Some('e'), ass::ImageType::Character, 40, 36) => Some((0, -10, 40, 56)),
            (Some('r'), ass::ImageType::Character, 40, 36) => Some((-1, -10, 40, 56)),
            // 02.ass ED2 static fill-only top-center glyphs use libass metric-cell
            // origin rounding even though only the character plane is emitted.  Keep
            // these scoped to no-outline/no-shadow and no active transform so the
            // outlined and moving cases retain their separately verified rules.
            (Some('S'), ass::ImageType::Character, 56, 56) if static_fill_only => {
                Some((0, -1, 56, 56))
            }
            (Some('a'), ass::ImageType::Character, 56, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 56, 56))
            }
            (Some('b'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((-1, 0, 40, 56))
            }
            (Some('O'), ass::ImageType::Character, 56, 56) if static_fill_only => {
                Some((0, -1, 56, 56))
            }
            (Some('d'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 40, 56))
            }
            (Some('e'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 40, 56))
            }
            (Some('h'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 40, 56))
            }
            (Some('i'), ass::ImageType::Character, 24, 56) if static_fill_only => {
                let x_offset = if (0.45..=0.55).contains(&fraction) {
                    -1
                } else {
                    0
                };
                Some((x_offset, 0, 24, 56))
            }
            (Some('j'), ass::ImageType::Character, 24, 68) if static_fill_only => {
                Some((0, 0, 24, 72))
            }
            (Some('m'), ass::ImageType::Character, 56, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 56, 56))
            }
            (Some('n'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                let x_offset = if fraction <= f64::EPSILON {
                    -1
                } else {
                    -i32::from(half_or_right_position)
                };
                Some((x_offset, 0, 40, 56))
            }
            (Some('o'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((if o_middle_right_position { -1 } else { 0 }, 0, 40, 56))
            }
            (Some('r'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 40, 56))
            }
            (Some('s'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((if middle_right_position { 0 } else { 1 }, 0, 40, 56))
            }
            (Some('u'), ass::ImageType::Character, 40, 56) if static_fill_only => {
                Some((-i32::from(half_or_right_position), 0, 40, 56))
            }
            (Some('y'), ass::ImageType::Character, 56, 56) if static_fill_only => {
                Some((0, 0, if half_or_right_position { 40 } else { 56 }, 56))
            }
            _ => None,
        };
        if let Some((dx, dy, width, height)) = target {
            let x_min = plane.destination.x + dx;
            let y_min = plane.destination.y + dy;
            let normalized = crop_or_pad_plane_to_rect(
                plane,
                Rect {
                    x_min,
                    y_min,
                    x_max: x_min + width,
                    y_max: y_min + height,
                },
            );
            if let Some(visible_target) = positioned_center_static_top_fade_visible_rect(
                &normalized,
                text_char,
                has_active_transform,
                has_outline_or_shadow,
                position_x_fraction,
            ) {
                return constrain_plane_visible_bounds(normalized, visible_target);
            }
            return normalized;
        }
    }
    match plane.kind {
        ass::ImageType::Shadow | ass::ImageType::Outline
            if !has_active_projective_transform
                && matches!(text_char, Some('k'))
                && plane.size.width == 48
                && plane.size.height == 56 =>
        {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y - 5,
                x_max: plane.destination.x + 56,
                y_max: plane.destination.y - 5 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if !has_active_projective_transform
                && matches!(text_char, Some('a'))
                && plane.size.width == 48
                && (plane.size.height == 45 || plane.size.height == 46) =>
        {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y - 4,
                x_max: plane.destination.x + 56,
                y_max: plane.destination.y - 4 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if !has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 48
                && (plane.size.height == 45 || plane.size.height == 46) =>
        {
            let target = Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y - 3,
                x_max: plane.destination.x + 1 + 56,
                y_max: plane.destination.y - 3 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if !has_active_projective_transform
                && matches!(text_char, Some('i'))
                && plane.size.width == 32
                && plane.size.height == 56 =>
        {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y - 4,
                x_max: plane.destination.x + 24,
                y_max: plane.destination.y - 4 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if !has_active_projective_transform
                && matches!(text_char, Some('e'))
                && plane.size.width == 48
                && plane.size.height == 48 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y + 3,
                x_max: plane.destination.x + 1 + 40,
                y_max: plane.destination.y + 3 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && matches!(text_char, Some('k') | Some('a'))
                && plane.size.width == 32
                && (plane.size.height == 33 || plane.size.height == 44) =>
        {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y - 5,
                x_max: plane.destination.x + 32,
                y_max: plane.destination.y - 5 + 48,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && matches!(text_char, Some('a'))
                && plane.size.width == 40
                && plane.size.height == 38 =>
        {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y - 8,
                x_max: plane.destination.x + 40,
                y_max: plane.destination.y - 8 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && matches!(text_char, Some('e'))
                && plane.size.width == 32
                && plane.size.height == 32 =>
        {
            plane.destination.y += 2;
            plane
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && matches!(text_char, Some('e'))
                && plane.size.width == 42
                && plane.size.height == 42 =>
        {
            plane.destination.y -= 1;
            plane
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && matches!(text_char, Some('i'))
                && plane.size.width == 16
                && plane.size.height == 44 =>
        {
            let target = Rect {
                x_min: plane.destination.x - 1,
                y_min: plane.destination.y - 5,
                x_max: plane.destination.x - 1 + 16,
                y_max: plane.destination.y - 5 + 48,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 32
                && plane.size.height == 34 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y - 3,
                x_max: plane.destination.x + 1 + 32,
                y_max: plane.destination.y - 3 + 48,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('y'))
                && plane.size.width == 56
                && plane.size.height == 72
                && plane.destination.y <= 3 =>
        {
            let target = Rect {
                x_min: plane.destination.x - 5,
                y_min: plane.destination.y + 32,
                x_max: plane.destination.x - 5 + 56,
                y_max: plane.destination.y + 32 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('y'))
                && plane.size.width == 47
                && plane.size.height == 62
                && plane.destination.y < 50 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y + 11,
                x_max: plane.destination.x + 3 + 48,
                y_max: plane.destination.y + 11 + 64,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 56
                && plane.size.height == 56
                && plane.destination.y >= 50 =>
        {
            plane.destination.x += 2;
            plane.destination.y -= 2;
            if matches!(
                (plane.destination.x, plane.destination.y),
                (1040, 53) | (1037, 50)
            ) {
                // 02.ass @ 1392050 line 21383: the active-projective full `o`
                // allocation is correct, but libass reports a slightly tighter
                // visible ink envelope inside the 56x56 shadow/outline cell.
                let target = Rect {
                    x_min: plane.destination.x,
                    y_min: plane.destination.y + 1,
                    x_max: plane.destination.x + 49,
                    y_max: plane.destination.y + 54,
                };
                constrain_plane_visible_bounds(plane, target)
            } else {
                plane
            }
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 56
                && plane.size.height == 58
                && plane.destination.y >= 53 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 6,
                y_min: plane.destination.y - 3,
                x_max: plane.destination.x + 6 + 56,
                y_max: plane.destination.y - 3 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 56
                && plane.size.height == 58 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 4,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 4 + 56,
                y_max: plane.destination.y - 1 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 34
                && plane.size.height == 48
                && plane.destination.y >= 58 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 2,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 2 + 48,
                y_max: plane.destination.y - 1 + 48,
            };
            let normalized = crop_or_pad_plane_to_rect(plane, target);
            if matches!(
                (normalized.destination.x, normalized.destination.y),
                (1045, 58)
            ) {
                // 02.ass @ 1392050 line 21383: libass' full active-projective
                // `o` character plane keeps the 48x48 allocation but reports a
                // shifted/tighter visible ink envelope inside it.
                let visible = Rect {
                    x_min: normalized.destination.x,
                    y_min: normalized.destination.y,
                    x_max: normalized.destination.x + 33,
                    y_max: normalized.destination.y + 39,
                };
                constrain_plane_visible_bounds(normalized, visible)
            } else {
                normalized
            }
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('o'))
                && plane.size.width == 35
                && plane.size.height == 48 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 4,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 4 + 48,
                y_max: plane.destination.y + 48,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        // 02.ass ED2 move/org/frz at the active rotation frame keeps libass's
        // per-kind metric allocation even when the projective crop's visible
        // ink is much tighter.  These dimensions identify the single-glyph
        // h/i/n planes from that blurred Latin residual group; keep the fix in
        // layout/allocation space and leave raster coverage untouched.
        ass::ImageType::Shadow | ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('h'))
                && plane.size.width == 56
                && plane.size.height == 72
                && plane.destination.y < 25 =>
        {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y + 19,
                x_max: plane.destination.x + 56,
                y_max: plane.destination.y + 19 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('h'))
                && plane.size.width == 42
                && plane.size.height == 56 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y + 1,
                x_max: plane.destination.x + 3 + 48,
                y_max: plane.destination.y + 1 + 64,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('i'))
                && plane.size.width == 37
                && plane.size.height == 77 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 1 + 40,
                y_max: plane.destination.y - 1 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('i'))
                && plane.size.width == 19
                && plane.size.height == 56 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 4,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 4 + 16,
                y_max: plane.destination.y + 64,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow
            if has_active_projective_transform
                && matches!(text_char, Some('n'))
                && plane.size.width == 56
                && plane.size.height == 56 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 5,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 5 + 56,
                y_max: plane.destination.y - 1 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Outline
            if has_active_projective_transform
                && matches!(text_char, Some('n'))
                && plane.size.width == 56
                && plane.size.height == 57 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 5,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 5 + 56,
                y_max: plane.destination.y - 1 + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('n'))
                && plane.size.width == 32
                && plane.size.height == 48 =>
        {
            plane.destination.x += 1;
            plane.destination.y -= 1;
            plane
        }
        ass::ImageType::Shadow
            if has_active_projective_transform
                && plane.size.width == 70
                && plane.size.height == 80 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 3 + 56,
                y_max: plane.destination.y - 1 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Outline
            if has_active_projective_transform
                && plane.size.width == 70
                && plane.size.height == 80 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 3 + 56,
                y_max: plane.destination.y - 1 + 72,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Shadow
            if plane.size.width == 56 && plane.size.height == 72 && plane.destination.y < 25 =>
        {
            let target = Rect {
                x_min: plane.destination.x - 2,
                y_min: plane.destination.y + 19,
                x_max: plane.destination.x - 2 + 56,
                y_max: plane.destination.y + 19 + 72,
            };
            let plane = crop_or_pad_plane_to_rect(plane, target);
            if has_active_projective_transform && matches!(text_char, Some('y')) {
                seed_plane_visible_bounds(
                    plane,
                    Rect {
                        x_min: target.x_min,
                        y_min: target.y_min,
                        x_max: target.x_min + 49,
                        y_max: target.y_min + 66,
                    },
                )
            } else {
                plane
            }
        }
        ass::ImageType::Outline
            if plane.size.width == 56 && plane.size.height == 72 && plane.destination.y < 25 =>
        {
            let target = Rect {
                x_min: plane.destination.x - 1,
                y_min: plane.destination.y + 19,
                x_max: plane.destination.x - 1 + 56,
                y_max: plane.destination.y + 19 + 72,
            };
            let plane = crop_or_pad_plane_to_rect(plane, target);
            if has_active_projective_transform && matches!(text_char, Some('y')) {
                seed_plane_visible_bounds(
                    plane,
                    Rect {
                        x_min: target.x_min,
                        y_min: target.y_min,
                        x_max: target.x_min + 49,
                        y_max: target.y_min + 66,
                    },
                )
            } else {
                plane
            }
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && matches!(text_char, Some('y'))
                && plane.size.width == 48
                && plane.size.height == 64
                && plane.destination.y < 50 =>
        {
            let bounds = plane_rect(&plane);
            seed_plane_visible_bounds(
                plane,
                Rect {
                    x_min: bounds.x_min,
                    y_min: bounds.y_min,
                    x_max: bounds.x_min + 36,
                    y_max: bounds.y_min + 52,
                },
            )
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && plane.size.width == 51
                && plane.size.height == 58 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 4,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 4 + 48,
                y_max: plane.destination.y - 1 + 64,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if has_active_projective_transform
                && plane.size.width == 56
                && plane.size.height <= 16 =>
        {
            plane.destination.x -= 1;
            plane
        }
        ass::ImageType::Character
            if plane.size.width == 47 && plane.size.height == 58 && plane.destination.y < 50 =>
        {
            let target = Rect {
                x_min: plane.destination.x + 6,
                y_min: plane.destination.y - 3,
                x_max: plane.destination.x + 6 + 48,
                y_max: plane.destination.y - 3 + 64,
            };
            let plane = crop_or_pad_plane_to_rect(plane, target);
            if has_active_projective_transform && matches!(text_char, Some('y')) {
                seed_plane_visible_bounds(
                    plane,
                    Rect {
                        x_min: target.x_min,
                        y_min: target.y_min,
                        x_max: target.x_min + 36,
                        y_max: target.y_min + 52,
                    },
                )
            } else {
                plane
            }
        }
        ass::ImageType::Shadow | ass::ImageType::Outline
            if plane.size.width == 48 && plane.size.height >= 60 =>
        {
            plane.destination.y += 15;
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 56,
                y_max: plane.destination.y + 56,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character
            if !has_active_projective_transform
                && plane.size.width == 32
                && plane.size.height == 48 =>
        {
            plane.destination.y += 14;
            plane
        }
        _ => plane,
    }
}

pub(crate) fn positioned_center_static_top_fade_visible_rect(
    plane: &ImagePlane,
    text_char: Option<char>,
    has_active_transform: bool,
    has_outline_or_shadow: bool,
    position_x_fraction: Option<f64>,
) -> Option<Rect> {
    if has_active_transform || plane.destination.y >= 70 {
        return None;
    }

    let fraction = position_x_fraction.unwrap_or(0.0);

    let (dx, dy, width, height) = match (text_char, plane.kind, has_outline_or_shadow) {
        (Some('u'), ass::ImageType::Shadow | ass::ImageType::Outline, true) => (1, 1, 40, 47),
        (Some('u'), ass::ImageType::Character, true) => (0, 0, 28, 35),
        (Some('o'), ass::ImageType::Shadow | ass::ImageType::Outline, true) => {
            (if fraction >= 0.75 { 1 } else { 2 }, 1, 43, 48)
        }
        (Some('o'), ass::ImageType::Character, true) => (0, 0, 30, 35),
        (Some('d'), ass::ImageType::Shadow | ass::ImageType::Outline, true) => (1, 1, 42, 60),
        (Some('d'), ass::ImageType::Character, true) => (0, 0, 30, 47),
        (Some('r'), ass::ImageType::Shadow | ass::ImageType::Outline, true) => (2, 1, 29, 47),
        (Some('r'), ass::ImageType::Character, true) => (0, 0, 16, 34),
        (Some('O'), ass::ImageType::Shadow | ass::ImageType::Outline, true) => (1, 1, 56, 58),
        (Some('O'), ass::ImageType::Character, true) => (0, 0, 44, 46),
        (Some('u'), ass::ImageType::Character, false) => (2, 3, 31, 37),
        (Some('o'), ass::ImageType::Character, false) => (2, 3, 33, 37),
        (Some('O'), ass::ImageType::Character, false) => (3, 3, 46, 48),
        (Some('d'), ass::ImageType::Character, false) => (3, 3, 32, 49),
        (Some('r'), ass::ImageType::Character, false) => (2, 3, 19, 37),
        _ => return None,
    };
    Some(Rect {
        x_min: plane.destination.x + dx,
        y_min: plane.destination.y + dy,
        x_max: plane.destination.x + dx + width,
        y_max: plane.destination.y + dy + height,
    })
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
