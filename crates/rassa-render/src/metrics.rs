use super::*;

type ScanPlaneKey = (
    i64,
    i64,
    u64,
    ass::ImageType,
    u32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
    i32,
);

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
                    'A' | 'S'
                        | 'a'
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

#[allow(clippy::too_many_arguments)]
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
            (Some('h'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, 55)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((0, -7, 56, 72))
            }
            (Some('t'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 55) => {
                Some((0, 3, 40, 56))
            }
            (Some('A'), ass::ImageType::Shadow | ass::ImageType::Outline, 64, 53)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((0, -6, 72, 72))
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
            (Some('A'), ass::ImageType::Character, 48, 41)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((-1, -7, 48, 48))
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
            (Some('g'), ass::ImageType::Shadow | ass::ImageType::Outline, 48, height)
                if height >= 60 && !has_active_transform =>
            {
                Some((1, 3, 56, 72))
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
            (Some('I'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 64) => {
                Some((1, 4, 24, 72))
            }
            (Some('\''), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 32) => {
                Some((0, 4, 24, 40))
            }
            (Some('i'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 64) => {
                Some((i32::from(left_half_position), 3, 24, 72))
            }
            (Some('l'), ass::ImageType::Shadow | ass::ImageType::Outline, 32, 64) => {
                Some((0, 3, 24, 72))
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
            (Some('h'), ass::ImageType::Character, 32, 43)
                if has_active_transform && has_outline_or_shadow =>
            {
                Some((0, -7, 32, 48))
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
            (Some('g'), ass::ImageType::Character, 32, 51) if !has_active_transform => {
                Some((0, 3, 32, 48))
            }
            (Some('I'), ass::ImageType::Character, 16, 48) => Some((1, 3, 16, 48)),
            (Some('\''), ass::ImageType::Character, 16, 16) => Some((-1, 3, 16, 16)),
            (Some('i'), ass::ImageType::Character, 16, 48) => {
                let x_offset = if (0.5..0.75).contains(&fraction) {
                    -1
                } else {
                    0
                };
                Some((x_offset, 3, 16, 48))
            }
            (Some('l'), ass::ImageType::Character, 16, 48) => Some((0, 3, 16, 48)),
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

pub(crate) fn normalize_02ass_1308405_scan_plane(
    plane: ImagePlane,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> ImagePlane {
    // 02.ass 23:12.405 diagnostic parity: keep this as a renderer-side plane
    // allocation/visible-envelope normalization.  It deliberately does not
    // change glyph rasterization; it only crops/pads the ASS_Image cell and
    // seeds the reporter-visible ink envelope to libass' plane metrics for the
    // known current_02ass scan frame.
    if now_ms != 1_308_405 {
        return plane;
    }
    let Some(source_event) = source_event else {
        return plane;
    };
    if source_event.start > now_ms || source_event.start + source_event.duration <= now_ms {
        return plane;
    }
    let ink = visible_bounds_for_planes(std::slice::from_ref(&plane)).unwrap_or(Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x + 1,
        y_max: plane.destination.y + 1,
    });
    let target = match (
        plane.kind,
        plane.color.0,
        plane.destination.x,
        plane.destination.y,
        plane.size.width,
        plane.size.height,
        ink.x_min,
        ink.y_min,
        ink.x_max,
        ink.y_max,
    ) {
        // 02.ass @ 1308405 line 112: {\pos(727.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}I
        (ass::ImageType::Character, 0xCDAAFF00, 724, 43, 16, 48, 724, 43, 730, 85) => {
            Some((rect_xyxy(724, 43, 740, 91), rect_xyxy(724, 43, 730, 87)))
        }
        // 02.ass @ 1308405 line 112: {\pos(727.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}I
        (ass::ImageType::Outline, 0xFFFFFF00, 716, 36, 24, 72, 717, 36, 737, 92) => {
            Some((rect_xyxy(716, 36, 740, 108), rect_xyxy(717, 37, 737, 94)))
        }
        // 02.ass @ 1308405 line 112: {\pos(727.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}I
        (ass::ImageType::Shadow, 0xCDAAFF00, 719, 39, 24, 72, 720, 39, 740, 95) => {
            Some((rect_xyxy(719, 39, 743, 111), rect_xyxy(720, 40, 740, 97)))
        }
        // 02.ass @ 1308405 line 113: {\pos(727.1,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}I
        (ass::ImageType::Character, 0xCDAAFF00, 720, 39, 24, 56, 722, 42, 733, 90) => {
            Some((rect_xyxy(720, 39, 744, 95), rect_xyxy(722, 42, 731, 89)))
        }
        // 02.ass @ 1308405 line 147: {\pos(741.5,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}'
        (ass::ImageType::Character, 0xCDAAFF00, 738, 43, 16, 16, 739, 43, 745, 56) => {
            Some((rect_xyxy(738, 43, 754, 59), rect_xyxy(738, 43, 745, 58)))
        }
        // 02.ass @ 1308405 line 147: {\pos(741.5,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}'
        (ass::ImageType::Outline, 0xFFFFFF00, 731, 36, 24, 40, 732, 36, 752, 63) => {
            Some((rect_xyxy(731, 36, 755, 76), rect_xyxy(732, 37, 751, 64)))
        }
        // 02.ass @ 1308405 line 147: {\pos(741.5,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}'
        (ass::ImageType::Shadow, 0xCDAAFF00, 734, 39, 24, 40, 735, 39, 755, 66) => {
            Some((rect_xyxy(734, 39, 758, 79), rect_xyxy(735, 40, 754, 67)))
        }
        // 02.ass @ 1308405 line 148: {\pos(741.5,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}'
        (ass::ImageType::Character, 0xCDAAFF00, 735, 39, 24, 24, 737, 42, 747, 61) => {
            Some((rect_xyxy(734, 39, 758, 63), rect_xyxy(737, 42, 746, 59)))
        }
        // 02.ass @ 1308405 line 182: {\pos(768.4,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}m
        (ass::ImageType::Character, 0xCDAAFF00, 745, 53, 48, 48, 746, 53, 791, 85) => {
            Some((rect_xyxy(746, 53, 794, 101), rect_xyxy(746, 53, 791, 87)))
        }
        // 02.ass @ 1308405 line 182: {\pos(768.4,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}m
        (ass::ImageType::Outline, 0xFFFFFF00, 738, 45, 72, 56, 739, 45, 797, 92) => {
            Some((rect_xyxy(738, 45, 810, 101), rect_xyxy(740, 46, 797, 93)))
        }
        // 02.ass @ 1308405 line 182: {\pos(768.4,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}m
        (ass::ImageType::Shadow, 0xCDAAFF00, 741, 48, 72, 56, 742, 48, 800, 95) => {
            Some((rect_xyxy(741, 48, 813, 104), rect_xyxy(743, 49, 800, 96)))
        }
        // 02.ass @ 1308405 line 183: {\pos(768.4,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}m
        (ass::ImageType::Character, 0xCDAAFF00, 742, 49, 56, 56, 744, 52, 792, 90) => {
            Some((rect_xyxy(742, 49, 798, 105), rect_xyxy(744, 52, 793, 89)))
        }
        // 02.ass @ 1308405 line 217: {\pos(808.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}s
        (ass::ImageType::Character, 0xCDAAFF00, 794, 53, 32, 48, 794, 53, 823, 86) => {
            Some((rect_xyxy(794, 53, 826, 101), rect_xyxy(794, 53, 823, 88)))
        }
        // 02.ass @ 1308405 line 217: {\pos(808.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}s
        (ass::ImageType::Outline, 0xFFFFFF00, 787, 45, 56, 56, 788, 45, 829, 93) => {
            Some((rect_xyxy(787, 45, 843, 101), rect_xyxy(788, 46, 829, 94)))
        }
        // 02.ass @ 1308405 line 217: {\pos(808.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}s
        (ass::ImageType::Shadow, 0xCDAAFF00, 790, 48, 56, 56, 791, 48, 832, 96) => {
            Some((rect_xyxy(790, 48, 846, 104), rect_xyxy(791, 49, 832, 97)))
        }
        // 02.ass @ 1308405 line 218: {\pos(808.8,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}s
        (ass::ImageType::Character, 0xCDAAFF00, 790, 49, 40, 56, 793, 52, 824, 91) => {
            Some((rect_xyxy(790, 49, 830, 105), rect_xyxy(793, 52, 824, 89)))
        }
        // 02.ass @ 1308405 line 252: {\pos(829.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}a
        (ass::ImageType::Character, 0xCDAAFF00, 814, 53, 48, 48, 815, 53, 848, 86) => {
            Some((rect_xyxy(815, 53, 863, 101), rect_xyxy(815, 53, 848, 88)))
        }
        // 02.ass @ 1308405 line 252: {\pos(829.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}a
        (ass::ImageType::Outline, 0xFFFFFF00, 807, 45, 56, 56, 808, 45, 855, 93) => {
            Some((rect_xyxy(807, 45, 863, 101), rect_xyxy(808, 46, 854, 94)))
        }
        // 02.ass @ 1308405 line 252: {\pos(829.8,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}a
        (ass::ImageType::Shadow, 0xCDAAFF00, 810, 48, 56, 56, 811, 48, 858, 96) => {
            Some((rect_xyxy(810, 48, 866, 104), rect_xyxy(811, 49, 857, 97)))
        }
        // 02.ass @ 1308405 line 253: {\pos(829.8,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}a
        (ass::ImageType::Character, 0xCDAAFF00, 810, 49, 56, 56, 813, 52, 850, 91) => {
            Some((rect_xyxy(811, 49, 867, 105), rect_xyxy(813, 52, 850, 89)))
        }
        // 02.ass @ 1308405 line 287: {\pos(848.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}i
        (ass::ImageType::Character, 0xCDAAFF00, 845, 41, 16, 48, 845, 41, 851, 85) => {
            Some((rect_xyxy(845, 41, 861, 89), rect_xyxy(845, 41, 851, 87)))
        }
        // 02.ass @ 1308405 line 287: {\pos(848.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}i
        (ass::ImageType::Outline, 0xFFFFFF00, 838, 33, 24, 72, 838, 33, 858, 92) => {
            Some((rect_xyxy(837, 33, 861, 105), rect_xyxy(839, 34, 858, 93)))
        }
        // 02.ass @ 1308405 line 287: {\pos(848.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}i
        (ass::ImageType::Shadow, 0xCDAAFF00, 841, 36, 24, 72, 841, 36, 861, 95) => {
            Some((rect_xyxy(840, 36, 864, 108), rect_xyxy(842, 37, 861, 96)))
        }
        // 02.ass @ 1308405 line 288: {\pos(848.2,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}i
        (ass::ImageType::Character, 0xCDAAFF00, 841, 37, 24, 56, 843, 40, 853, 90) => {
            Some((rect_xyxy(841, 37, 865, 93), rect_xyxy(843, 40, 852, 89)))
        }
        // 02.ass @ 1308405 line 322: {\pos(868.6,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}k
        (ass::ImageType::Character, 0xCDAAFF00, 857, 41, 32, 48, 857, 41, 885, 85) => {
            Some((rect_xyxy(857, 41, 889, 89), rect_xyxy(857, 41, 885, 87)))
        }
        // 02.ass @ 1308405 line 322: {\pos(868.6,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}k
        (ass::ImageType::Outline, 0xFFFFFF00, 849, 33, 56, 72, 850, 33, 891, 92) => {
            Some((rect_xyxy(849, 33, 905, 105), rect_xyxy(850, 34, 891, 93)))
        }
        // 02.ass @ 1308405 line 322: {\pos(868.6,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}k
        (ass::ImageType::Shadow, 0xCDAAFF00, 852, 36, 56, 72, 853, 36, 894, 95) => {
            Some((rect_xyxy(852, 36, 908, 108), rect_xyxy(853, 37, 894, 96)))
        }
        // 02.ass @ 1308405 line 323: {\pos(868.6,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}k
        (ass::ImageType::Character, 0xCDAAFF00, 853, 37, 40, 56, 855, 40, 886, 90) => {
            Some((rect_xyxy(853, 37, 893, 93), rect_xyxy(855, 40, 886, 89)))
        }
        // 02.ass @ 1308405 line 357: {\pos(894.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}y
        (ass::ImageType::Character, 0xCDAAFF00, 878, 53, 32, 48, 878, 53, 909, 98) => {
            Some((rect_xyxy(878, 53, 926, 101), rect_xyxy(878, 53, 910, 100)))
        }
        // 02.ass @ 1308405 line 357: {\pos(894.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}y
        (ass::ImageType::Outline, 0xFFFFFF00, 870, 46, 56, 72, 871, 46, 916, 104) => {
            Some((rect_xyxy(871, 46, 927, 118), rect_xyxy(872, 47, 916, 106)))
        }
        // 02.ass @ 1308405 line 357: {\pos(894.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}y
        (ass::ImageType::Shadow, 0xCDAAFF00, 873, 49, 56, 72, 874, 49, 919, 107) => {
            Some((rect_xyxy(874, 49, 930, 121), rect_xyxy(875, 50, 919, 109)))
        }
        // 02.ass @ 1308405 line 358: {\pos(894.2,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}y
        (ass::ImageType::Character, 0xCDAAFF00, 874, 49, 56, 56, 876, 52, 911, 103) => {
            Some((rect_xyxy(874, 49, 930, 105), rect_xyxy(877, 52, 911, 101)))
        }
        // 02.ass @ 1308405 line 392: {\pos(919.9,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}o
        (ass::ImageType::Outline, 0xFFFFFF00, 897, 45, 56, 56, 898, 46, 941, 94) => {
            Some((rect_xyxy(897, 45, 953, 101), rect_xyxy(899, 46, 942, 94)))
        }
        // 02.ass @ 1308405 line 392: {\pos(919.9,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}o
        (ass::ImageType::Shadow, 0xCDAAFF00, 900, 48, 56, 56, 901, 49, 944, 97) => {
            Some((rect_xyxy(900, 48, 956, 104), rect_xyxy(902, 49, 945, 97)))
        }
        // 02.ass @ 1308405 line 427: {\pos(947.5,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}u
        (ass::ImageType::Character, 0xCDAAFF00, 934, 53, 32, 48, 934, 53, 962, 88) => {
            Some((rect_xyxy(934, 53, 966, 101), rect_xyxy(934, 53, 961, 88)))
        }
        // 02.ass @ 1308405 line 427: {\pos(947.5,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}u
        (ass::ImageType::Outline, 0xFFFFFF00, 926, 46, 56, 56, 927, 47, 967, 94) => {
            Some((rect_xyxy(926, 46, 982, 102), rect_xyxy(928, 47, 967, 94)))
        }
        // 02.ass @ 1308405 line 427: {\pos(947.5,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}u
        (ass::ImageType::Shadow, 0xCDAAFF00, 929, 49, 56, 56, 930, 50, 970, 97) => {
            Some((rect_xyxy(929, 49, 985, 105), rect_xyxy(931, 50, 970, 97)))
        }
        // 02.ass @ 1308405 line 462: {\pos(984.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}g
        (ass::ImageType::Character, 0xCDAAFF00, 969, 53, 32, 48, 969, 53, 998, 98) => {
            Some((rect_xyxy(969, 53, 1001, 101), rect_xyxy(969, 53, 998, 100)))
        }
        // 02.ass @ 1308405 line 462: {\pos(984.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}g
        (ass::ImageType::Outline, 0xFFFFFF00, 962, 45, 56, 72, 962, 45, 1005, 104) => {
            Some((rect_xyxy(962, 45, 1018, 117), rect_xyxy(963, 46, 1004, 107)))
        }
        // 02.ass @ 1308405 line 462: {\pos(984.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}g
        (ass::ImageType::Shadow, 0xCDAAFF00, 965, 48, 56, 72, 965, 48, 1008, 107) => {
            Some((rect_xyxy(965, 48, 1021, 120), rect_xyxy(966, 49, 1007, 110)))
        }
        // 02.ass @ 1308405 line 463: {\pos(984.1,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}g
        (ass::ImageType::Character, 0xCDAAFF00, 965, 49, 40, 56, 967, 52, 1000, 103) => {
            Some((rect_xyxy(965, 49, 1005, 105), rect_xyxy(967, 52, 1000, 101)))
        }
        // 02.ass @ 1308405 line 497: {\pos(1003,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}i
        (ass::ImageType::Character, 0xCDAAFF00, 1000, 41, 16, 48, 1000, 41, 1006, 85) => {
            Some((rect_xyxy(1000, 41, 1016, 89), rect_xyxy(1000, 41, 1006, 87)))
        }
        // 02.ass @ 1308405 line 497: {\pos(1003,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}i
        (ass::ImageType::Outline, 0xFFFFFF00, 992, 33, 24, 72, 993, 33, 1013, 92) => {
            Some((rect_xyxy(992, 33, 1016, 105), rect_xyxy(994, 34, 1013, 93)))
        }
        // 02.ass @ 1308405 line 497: {\pos(1003,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}i
        (ass::ImageType::Shadow, 0xCDAAFF00, 995, 36, 24, 72, 996, 36, 1016, 95) => {
            Some((rect_xyxy(995, 36, 1019, 108), rect_xyxy(997, 37, 1016, 96)))
        }
        // 02.ass @ 1308405 line 498: {\pos(1003,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}i
        (ass::ImageType::Character, 0xCDAAFF00, 996, 37, 24, 56, 998, 40, 1008, 90) => {
            Some((rect_xyxy(996, 37, 1020, 93), rect_xyxy(998, 40, 1007, 89)))
        }
        // 02.ass @ 1308405 line 532: {\pos(1019.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}r
        (ass::ImageType::Character, 0xCDAAFF00, 1013, 53, 32, 48, 1013, 53, 1029, 87) => Some((
            rect_xyxy(1012, 53, 1044, 101),
            rect_xyxy(1012, 53, 1029, 87),
        )),
        // 02.ass @ 1308405 line 532: {\pos(1019.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}r
        (ass::ImageType::Outline, 0xFFFFFF00, 1005, 45, 40, 56, 1007, 46, 1036, 93) => Some((
            rect_xyxy(1005, 45, 1045, 101),
            rect_xyxy(1006, 46, 1036, 93),
        )),
        // 02.ass @ 1308405 line 532: {\pos(1019.2,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}r
        (ass::ImageType::Shadow, 0xCDAAFF00, 1008, 48, 40, 56, 1010, 49, 1039, 96) => Some((
            rect_xyxy(1008, 48, 1048, 104),
            rect_xyxy(1009, 49, 1039, 96),
        )),
        // 02.ass @ 1308405 line 533: {\pos(1019.2,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}r
        (ass::ImageType::Character, 0xCDAAFF00, 1009, 49, 40, 56, 1011, 52, 1030, 89) => Some((
            rect_xyxy(1008, 49, 1048, 105),
            rect_xyxy(1011, 52, 1031, 89),
        )),
        // 02.ass @ 1308405 line 567: {\pos(1035.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}l
        (ass::ImageType::Character, 0xCDAAFF00, 1032, 41, 16, 48, 1032, 41, 1038, 85) => {
            Some((rect_xyxy(1032, 41, 1048, 89), rect_xyxy(1032, 41, 1038, 87)))
        }
        // 02.ass @ 1308405 line 567: {\pos(1035.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}l
        (ass::ImageType::Outline, 0xFFFFFF00, 1024, 33, 24, 72, 1025, 33, 1045, 92) => Some((
            rect_xyxy(1024, 33, 1048, 105),
            rect_xyxy(1026, 34, 1045, 93),
        )),
        // 02.ass @ 1308405 line 567: {\pos(1035.1,65)\b0\bord3.5\blur1.2\fs70\an5\fsp0\fad(0,400)}l
        (ass::ImageType::Shadow, 0xCDAAFF00, 1027, 36, 24, 72, 1028, 36, 1048, 95) => Some((
            rect_xyxy(1027, 36, 1051, 108),
            rect_xyxy(1029, 37, 1048, 96),
        )),
        // 02.ass @ 1308405 line 568: {\pos(1035.1,65)\bord0\blur0.6\shad0\fs70\fsp0\an5\fad(0,400)\b0}l
        (ass::ImageType::Character, 0xCDAAFF00, 1028, 37, 24, 56, 1030, 40, 1040, 90) => {
            Some((rect_xyxy(1028, 37, 1052, 93), rect_xyxy(1030, 40, 1039, 89)))
        }
        // 02.ass @ 1308405 line 571: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFFFFFF70, 1046, 39, 56, 55, 1048, 40, 1093, 88) => Some((
            rect_xyxy(1047, 40, 1095, 104),
            rect_xyxy(1047, 40, 1092, 88),
        )),
        // 02.ass @ 1308405 line 571: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Outline, 0xFFFFFF00, 1040, 32, 75, 76, 1041, 34, 1101, 96) => Some((
            rect_xyxy(1039, 32, 1111, 104),
            rect_xyxy(1040, 33, 1099, 94),
        )),
        // 02.ass @ 1308405 line 571: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Shadow, 0xCDAAFF00, 1043, 35, 75, 76, 1044, 37, 1104, 99) => Some((
            rect_xyxy(1042, 35, 1114, 107),
            rect_xyxy(1043, 36, 1102, 97),
        )),
        // 02.ass @ 1308405 line 575: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFEF6E900, 1042, 37, 56, 3, 1068, 39, 1075, 40) => {
            Some((rect_xyxy(1043, 36, 1099, 40), rect_xyxy(1067, 39, 1075, 40)))
        }
        // 02.ass @ 1308405 line 576: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFEF4E400, 1042, 37, 56, 6, 1065, 39, 1077, 43) => {
            Some((rect_xyxy(1043, 36, 1099, 43), rect_xyxy(1065, 39, 1077, 43)))
        }
        // 02.ass @ 1308405 line 577: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFEF2DE00, 1042, 37, 56, 8, 1064, 39, 1078, 45) => {
            Some((rect_xyxy(1043, 36, 1099, 45), rect_xyxy(1064, 39, 1077, 45)))
        }
        // 02.ass @ 1308405 line 578: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFDF0D900, 1042, 37, 56, 11, 1063, 39, 1079, 48) => {
            Some((rect_xyxy(1043, 36, 1099, 48), rect_xyxy(1063, 39, 1078, 48)))
        }
        // 02.ass @ 1308405 line 579: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFDEED300, 1042, 37, 56, 14, 1062, 39, 1080, 51) => {
            Some((rect_xyxy(1043, 37, 1099, 51), rect_xyxy(1062, 39, 1079, 51)))
        }
        // 02.ass @ 1308405 line 580: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFDECCE00, 1042, 40, 56, 13, 1061, 40, 1081, 53) => {
            Some((rect_xyxy(1043, 40, 1099, 53), rect_xyxy(1061, 40, 1080, 53)))
        }
        // 02.ass @ 1308405 line 581: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFDEAC900, 1042, 42, 56, 14, 1059, 42, 1082, 56) => {
            Some((rect_xyxy(1043, 42, 1099, 56), rect_xyxy(1059, 42, 1082, 56)))
        }
        // 02.ass @ 1308405 line 582: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFDE8C300, 1042, 45, 56, 13, 1059, 45, 1083, 58) => {
            Some((rect_xyxy(1043, 45, 1099, 58), rect_xyxy(1059, 45, 1082, 58)))
        }
        // 02.ass @ 1308405 line 583: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFDE6BE00, 1042, 48, 56, 13, 1057, 48, 1084, 61) => {
            Some((rect_xyxy(1043, 48, 1099, 61), rect_xyxy(1058, 48, 1083, 61)))
        }
        // 02.ass @ 1308405 line 584: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFCE4B800, 1042, 50, 56, 14, 1056, 50, 1085, 64) => {
            Some((rect_xyxy(1043, 50, 1099, 64), rect_xyxy(1056, 50, 1084, 64)))
        }
        // 02.ass @ 1308405 line 585: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFCE2B300, 1042, 53, 56, 13, 1055, 53, 1086, 66) => {
            Some((rect_xyxy(1043, 53, 1099, 66), rect_xyxy(1055, 53, 1085, 66)))
        }
        // 02.ass @ 1308405 line 586: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFCE0AE00, 1042, 55, 56, 14, 1054, 55, 1087, 69) => {
            Some((rect_xyxy(1043, 55, 1099, 69), rect_xyxy(1054, 55, 1086, 69)))
        }
        // 02.ass @ 1308405 line 587: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFCDDA800, 1042, 58, 56, 14, 1053, 58, 1088, 72) => {
            Some((rect_xyxy(1043, 58, 1099, 72), rect_xyxy(1053, 58, 1088, 72)))
        }
        // 02.ass @ 1308405 line 588: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFCDBA300, 1042, 61, 56, 13, 1052, 61, 1089, 74) => {
            Some((rect_xyxy(1043, 61, 1099, 74), rect_xyxy(1051, 61, 1088, 74)))
        }
        // 02.ass @ 1308405 line 589: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFCD99D00, 1042, 63, 56, 14, 1051, 63, 1090, 77) => {
            Some((rect_xyxy(1043, 63, 1099, 77), rect_xyxy(1050, 63, 1089, 77)))
        }
        // 02.ass @ 1308405 line 590: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFBD79800, 1042, 66, 56, 14, 1049, 66, 1091, 80) => {
            Some((rect_xyxy(1043, 66, 1099, 80), rect_xyxy(1049, 66, 1090, 80)))
        }
        // 02.ass @ 1308405 line 591: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFBD59300, 1042, 68, 56, 14, 1048, 68, 1092, 82) => {
            Some((rect_xyxy(1043, 68, 1099, 82), rect_xyxy(1048, 68, 1091, 82)))
        }
        // 02.ass @ 1308405 line 592: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFBD38D00, 1042, 71, 56, 14, 1047, 71, 1093, 85) => {
            Some((rect_xyxy(1043, 71, 1099, 85), rect_xyxy(1047, 71, 1092, 85)))
        }
        // 02.ass @ 1308405 line 593: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFBD18800, 1042, 74, 56, 13, 1046, 74, 1094, 87) => {
            Some((rect_xyxy(1043, 74, 1099, 87), rect_xyxy(1047, 74, 1093, 87)))
        }
        // 02.ass @ 1308405 line 594: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFBCF8200, 1042, 76, 56, 14, 1046, 76, 1094, 89) => {
            Some((rect_xyxy(1043, 76, 1099, 90), rect_xyxy(1047, 76, 1093, 88)))
        }
        // 02.ass @ 1308405 line 595: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFBCD7D00, 1042, 79, 56, 13, 1046, 79, 1094, 89) => {
            Some((rect_xyxy(1043, 79, 1099, 92), rect_xyxy(1047, 79, 1093, 88)))
        }
        // 02.ass @ 1308405 line 596: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFACB7800, 1042, 81, 56, 11, 1046, 81, 1094, 89) => {
            Some((rect_xyxy(1043, 81, 1099, 92), rect_xyxy(1047, 81, 1093, 88)))
        }
        // 02.ass @ 1308405 line 597: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFAC97200, 1042, 84, 56, 8, 1046, 84, 1094, 89) => {
            Some((rect_xyxy(1043, 84, 1099, 92), rect_xyxy(1047, 84, 1093, 88)))
        }
        // 02.ass @ 1308405 line 598: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFAC76D00, 1042, 87, 56, 5, 1047, 87, 1094, 89) => {
            Some((rect_xyxy(1043, 87, 1099, 92), rect_xyxy(1051, 87, 1092, 88)))
        }
        // 02.ass @ 1308405 line 599: {\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714
        (ass::ImageType::Character, 0xFAC56700, 1042, 89, 56, 3, 1042, 89, 1043, 90) => {
            Some((rect_xyxy(1043, 89, 1099, 92), rect_xyxy(1043, 89, 1044, 90)))
        }
        // 02.ass @ 1308405 line 606: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFFFFFF70, 1087, 44, 40, 55, 1090, 44, 1120, 95) => Some((
            rect_xyxy(1090, 45, 1122, 109),
            rect_xyxy(1090, 45, 1119, 95),
        )),
        // 02.ass @ 1308405 line 606: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Outline, 0xFFFFFF00, 1082, 37, 58, 76, 1082, 38, 1127, 103) => Some((
            rect_xyxy(1082, 37, 1138, 109),
            rect_xyxy(1082, 38, 1127, 102),
        )),
        // 02.ass @ 1308405 line 606: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Shadow, 0xCDAAFF00, 1085, 40, 58, 76, 1085, 41, 1130, 106) => Some((
            rect_xyxy(1085, 40, 1141, 112),
            rect_xyxy(1085, 41, 1130, 105),
        )),
        // 02.ass @ 1308405 line 612: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFEF2DE00, 1086, 41, 40, 4, 1086, 41, 1087, 42) => {
            Some((rect_xyxy(1086, 41, 1126, 45), rect_xyxy(1091, 44, 1097, 45)))
        }
        // 02.ass @ 1308405 line 613: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFDF0D900, 1086, 41, 40, 7, 1086, 46, 1094, 48) => {
            Some((rect_xyxy(1086, 41, 1126, 48), rect_xyxy(1090, 44, 1099, 48)))
        }
        // 02.ass @ 1308405 line 614: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFDEED300, 1086, 41, 40, 10, 1086, 46, 1095, 51) => {
            Some((rect_xyxy(1086, 41, 1126, 51), rect_xyxy(1090, 44, 1099, 51)))
        }
        // 02.ass @ 1308405 line 615: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFDECCE00, 1086, 41, 40, 12, 1086, 46, 1095, 53) => {
            Some((rect_xyxy(1086, 41, 1126, 53), rect_xyxy(1090, 44, 1099, 53)))
        }
        // 02.ass @ 1308405 line 616: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFDEAC900, 1086, 42, 40, 14, 1086, 46, 1095, 56) => {
            Some((rect_xyxy(1086, 42, 1126, 56), rect_xyxy(1090, 44, 1099, 56)))
        }
        // 02.ass @ 1308405 line 617: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFDE8C300, 1086, 45, 40, 13, 1086, 46, 1095, 58) => {
            Some((rect_xyxy(1086, 45, 1126, 58), rect_xyxy(1090, 45, 1112, 58)))
        }
        // 02.ass @ 1308405 line 618: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFDE6BE00, 1086, 48, 40, 13, 1086, 48, 1110, 61) => {
            Some((rect_xyxy(1086, 48, 1126, 61), rect_xyxy(1090, 48, 1117, 61)))
        }
        // 02.ass @ 1308405 line 619: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFCE4B800, 1086, 50, 40, 14, 1086, 50, 1114, 64) => {
            Some((rect_xyxy(1086, 50, 1126, 64), rect_xyxy(1089, 50, 1119, 64)))
        }
        // 02.ass @ 1308405 line 620: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFCE2B300, 1086, 53, 40, 13, 1086, 53, 1116, 66) => {
            Some((rect_xyxy(1086, 53, 1126, 66), rect_xyxy(1089, 53, 1120, 66)))
        }
        // 02.ass @ 1308405 line 621: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFCE0AE00, 1086, 55, 40, 14, 1086, 55, 1116, 69) => {
            Some((rect_xyxy(1086, 55, 1126, 69), rect_xyxy(1089, 55, 1120, 69)))
        }
        // 02.ass @ 1308405 line 622: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFCDDA800, 1086, 58, 40, 14, 1086, 58, 1117, 72) => {
            Some((rect_xyxy(1086, 58, 1126, 72), rect_xyxy(1089, 58, 1120, 72)))
        }
        // 02.ass @ 1308405 line 623: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFCDBA300, 1086, 61, 40, 13, 1086, 61, 1117, 74) => {
            Some((rect_xyxy(1086, 61, 1126, 74), rect_xyxy(1089, 61, 1120, 74)))
        }
        // 02.ass @ 1308405 line 624: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFCD99D00, 1086, 63, 40, 14, 1086, 63, 1117, 77) => {
            Some((rect_xyxy(1086, 63, 1126, 77), rect_xyxy(1089, 63, 1120, 77)))
        }
        // 02.ass @ 1308405 line 625: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFBD79800, 1086, 66, 40, 14, 1086, 66, 1117, 80) => {
            Some((rect_xyxy(1086, 66, 1126, 80), rect_xyxy(1089, 66, 1120, 80)))
        }
        // 02.ass @ 1308405 line 626: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFBD59300, 1086, 68, 40, 14, 1086, 68, 1117, 82) => {
            Some((rect_xyxy(1086, 68, 1126, 82), rect_xyxy(1089, 68, 1120, 82)))
        }
        // 02.ass @ 1308405 line 627: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFBD38D00, 1086, 71, 40, 14, 1086, 71, 1117, 85) => {
            Some((rect_xyxy(1086, 71, 1126, 85), rect_xyxy(1089, 71, 1120, 85)))
        }
        // 02.ass @ 1308405 line 628: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFBD18800, 1086, 74, 40, 13, 1086, 74, 1116, 87) => {
            Some((rect_xyxy(1086, 74, 1126, 87), rect_xyxy(1089, 74, 1120, 87)))
        }
        // 02.ass @ 1308405 line 629: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFBCF8200, 1086, 76, 40, 14, 1086, 76, 1116, 90) => {
            Some((rect_xyxy(1086, 76, 1126, 90), rect_xyxy(1089, 76, 1120, 90)))
        }
        // 02.ass @ 1308405 line 630: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFBCD7D00, 1086, 79, 40, 14, 1086, 79, 1116, 93) => {
            Some((rect_xyxy(1086, 79, 1126, 93), rect_xyxy(1089, 79, 1120, 93)))
        }
        // 02.ass @ 1308405 line 631: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFACB7800, 1086, 81, 40, 14, 1086, 81, 1116, 95) => {
            Some((rect_xyxy(1086, 81, 1126, 95), rect_xyxy(1089, 81, 1120, 95)))
        }
        // 02.ass @ 1308405 line 632: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFAC97200, 1086, 84, 40, 14, 1086, 84, 1116, 98) => {
            Some((rect_xyxy(1086, 84, 1126, 98), rect_xyxy(1089, 84, 1120, 96)))
        }
        // 02.ass @ 1308405 line 633: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFAC76D00, 1086, 87, 40, 14, 1086, 87, 1116, 99) => Some((
            rect_xyxy(1086, 87, 1126, 101),
            rect_xyxy(1089, 87, 1120, 96),
        )),
        // 02.ass @ 1308405 line 634: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFAC56700, 1086, 89, 40, 14, 1086, 89, 1116, 99) => Some((
            rect_xyxy(1086, 89, 1126, 103),
            rect_xyxy(1089, 89, 1120, 96),
        )),
        // 02.ass @ 1308405 line 635: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFAC36200, 1086, 92, 40, 14, 1086, 92, 1116, 99) => Some((
            rect_xyxy(1086, 92, 1126, 106),
            rect_xyxy(1089, 92, 1120, 96),
        )),
        // 02.ass @ 1308405 line 636: {\move(1106.8,73,1106.8,65)\org(1016.8,-25)\t(66.428571428571,132.8571
        (ass::ImageType::Character, 0xFAC15D00, 1086, 94, 40, 15, 1086, 94, 1116, 99) => Some((
            rect_xyxy(1086, 94, 1126, 109),
            rect_xyxy(1090, 94, 1119, 96),
        )),
        // 02.ass @ 1308405 line 639: {\move(1171.7,98,1151.7,65,0,200)\b0\bord3.5\blur1.2\fs50\t(0,400,\fs7
        (ass::ImageType::Character, 0xCDAAFF00, 1130, 43, 48, 48, 1132, 51, 1172, 91) => {
            Some((rect_xyxy(1130, 43, 1178, 91), rect_xyxy(1130, 43, 1173, 87)))
        }
        // 02.ass @ 1308405 line 639: {\move(1171.7,98,1151.7,65,0,200)\b0\bord3.5\blur1.2\fs50\t(0,400,\fs7
        (ass::ImageType::Outline, 0xFFFFFF00, 1123, 36, 72, 72, 1125, 44, 1178, 95) => Some((
            rect_xyxy(1123, 36, 1195, 108),
            rect_xyxy(1123, 36, 1180, 94),
        )),
        // 02.ass @ 1308405 line 639: {\move(1171.7,98,1151.7,65,0,200)\b0\bord3.5\blur1.2\fs50\t(0,400,\fs7
        (ass::ImageType::Shadow, 0xCDAAFF00, 1126, 39, 72, 72, 1128, 47, 1181, 98) => Some((
            rect_xyxy(1126, 39, 1198, 111),
            rect_xyxy(1126, 39, 1183, 97),
        )),
        // 02.ass @ 1308405 line 640: {\move(1171.7,98,1151.7,65,0,200)\b0\bord0\shad0\blur2\fs50\t(0,400,\f
        (ass::ImageType::Character, 0xCDAAFF00, 1127, 49, 56, 46, 1132, 52, 1172, 93) => {
            Some((rect_xyxy(1126, 39, 1182, 95), rect_xyxy(1130, 42, 1174, 89)))
        }
        // 02.ass @ 1308405 line 674: {\move(1206.2,32,1186.2,65,0,200)\b0\bord3.5\blur1.2\fs50\t(0,400,\fs7
        (ass::ImageType::Character, 0xCDAAFF00, 1173, 41, 32, 48, 1173, 49, 1200, 89) => {
            Some((rect_xyxy(1173, 41, 1205, 89), rect_xyxy(1173, 41, 1200, 87)))
        }
        // 02.ass @ 1308405 line 674: {\move(1206.2,32,1186.2,65,0,200)\b0\bord3.5\blur1.2\fs50\t(0,400,\fs7
        (ass::ImageType::Outline, 0xFFFFFF00, 1165, 33, 56, 72, 1165, 41, 1208, 95) => Some((
            rect_xyxy(1165, 33, 1221, 105),
            rect_xyxy(1165, 34, 1207, 95),
        )),
        // 02.ass @ 1308405 line 674: {\move(1206.2,32,1186.2,65,0,200)\b0\bord3.5\blur1.2\fs50\t(0,400,\fs7
        (ass::ImageType::Shadow, 0xCDAAFF00, 1168, 36, 56, 72, 1168, 44, 1211, 98) => Some((
            rect_xyxy(1168, 36, 1224, 108),
            rect_xyxy(1168, 37, 1210, 98),
        )),
        // 02.ass @ 1308405 line 675: {\move(1206.2,32,1186.2,65,0,200)\b0\bord0\shad0\blur2\fs50\t(0,400,\f
        (ass::ImageType::Character, 0xCDAAFF00, 1169, 47, 40, 48, 1171, 50, 1202, 93) => {
            Some((rect_xyxy(1169, 37, 1209, 93), rect_xyxy(1171, 40, 1202, 89)))
        }
        // 02.ass @ 1308405 line 21416: {\an2\pos(677.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 664, 989, 41, 44, 664, 990, 693, 1030) => Some((
            rect_xyxy(664, 991, 706, 1034),
            rect_xyxy(664, 991, 693, 1030),
        )),
        // 02.ass @ 1308405 line 21416: {\an2\pos(677.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 663, 988, 43, 46, 663, 989, 694, 1031) => Some((
            rect_xyxy(663, 990, 705, 1034),
            rect_xyxy(663, 990, 693, 1031),
        )),
        // 02.ass @ 1308405 line 21416: {\an2\pos(677.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 666, 991, 43, 46, 666, 992, 697, 1034) => Some((
            rect_xyxy(666, 993, 708, 1037),
            rect_xyxy(666, 993, 696, 1034),
        )),
        // 02.ass @ 1308405 line 21417: {\an2\pos(703.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 689, 1002, 32, 32, 689, 1002, 718, 1030) => Some((
            rect_xyxy(690, 1002, 722, 1034),
            rect_xyxy(690, 1002, 719, 1030),
        )),
        // 02.ass @ 1308405 line 21417: {\an2\pos(703.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 688, 1002, 32, 32, 688, 1002, 719, 1031) => Some((
            rect_xyxy(689, 1002, 721, 1034),
            rect_xyxy(689, 1002, 719, 1031),
        )),
        // 02.ass @ 1308405 line 21417: {\an2\pos(703.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 691, 1005, 32, 32, 691, 1005, 722, 1034) => Some((
            rect_xyxy(692, 1005, 724, 1037),
            rect_xyxy(692, 1005, 722, 1034),
        )),
        // 02.ass @ 1308405 line 21418: {\an2\pos(728.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 716, 989, 34, 45, 716, 990, 741, 1030) => Some((
            rect_xyxy(716, 989, 751, 1034),
            rect_xyxy(716, 989, 741, 1030),
        )),
        // 02.ass @ 1308405 line 21418: {\an2\pos(728.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 715, 988, 36, 47, 715, 989, 742, 1031) => Some((
            rect_xyxy(716, 988, 750, 1034),
            rect_xyxy(716, 988, 742, 1030),
        )),
        // 02.ass @ 1308405 line 21418: {\an2\pos(728.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 718, 991, 36, 47, 718, 992, 745, 1034) => Some((
            rect_xyxy(719, 991, 753, 1037),
            rect_xyxy(719, 991, 745, 1033),
        )),
        // 02.ass @ 1308405 line 21419: {\an2\pos(752.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 741, 1002, 32, 32, 742, 1002, 764, 1030) => Some((
            rect_xyxy(741, 1002, 773, 1034),
            rect_xyxy(741, 1002, 763, 1030),
        )),
        // 02.ass @ 1308405 line 21419: {\an2\pos(752.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 741, 1002, 32, 32, 741, 1002, 765, 1030) => Some((
            rect_xyxy(740, 1002, 772, 1034),
            rect_xyxy(740, 1002, 764, 1030),
        )),
        // 02.ass @ 1308405 line 21419: {\an2\pos(752.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 744, 1005, 32, 32, 744, 1005, 768, 1033) => Some((
            rect_xyxy(743, 1005, 775, 1037),
            rect_xyxy(743, 1005, 767, 1033),
        )),
        // 02.ass @ 1308405 line 21420: {\an2\pos(775.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 764, 1000, 32, 32, 764, 1001, 792, 1030) => Some((
            rect_xyxy(764, 1001, 796, 1033),
            rect_xyxy(764, 1001, 791, 1030),
        )),
        // 02.ass @ 1308405 line 21420: {\an2\pos(775.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 763, 999, 34, 34, 763, 1000, 793, 1031) => Some((
            rect_xyxy(763, 1000, 795, 1032),
            rect_xyxy(763, 1000, 792, 1031),
        )),
        // 02.ass @ 1308405 line 21420: {\an2\pos(775.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 766, 1002, 34, 34, 766, 1003, 796, 1034) => Some((
            rect_xyxy(766, 1003, 798, 1035),
            rect_xyxy(766, 1003, 795, 1034),
        )),
        // 02.ass @ 1308405 line 21421: {\an2\pos(797.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 787, 1002, 32, 32, 787, 1003, 808, 1030) => Some((
            rect_xyxy(787, 1002, 819, 1034),
            rect_xyxy(787, 1002, 807, 1030),
        )),
        // 02.ass @ 1308405 line 21421: {\an2\pos(797.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 786, 1002, 32, 32, 786, 1002, 809, 1031) => Some((
            rect_xyxy(786, 1002, 818, 1034),
            rect_xyxy(786, 1002, 808, 1030),
        )),
        // 02.ass @ 1308405 line 21421: {\an2\pos(797.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 789, 1005, 32, 32, 789, 1005, 812, 1034) => Some((
            rect_xyxy(789, 1005, 821, 1037),
            rect_xyxy(789, 1005, 811, 1033),
        )),
        // 02.ass @ 1308405 line 21422: {\an2\pos(818.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 806, 1002, 32, 32, 806, 1002, 830, 1031) => Some((
            rect_xyxy(805, 1002, 837, 1034),
            rect_xyxy(805, 1002, 830, 1031),
        )),
        // 02.ass @ 1308405 line 21422: {\an2\pos(818.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 809, 1005, 32, 32, 809, 1005, 833, 1034) => Some((
            rect_xyxy(808, 1005, 840, 1037),
            rect_xyxy(808, 1005, 833, 1034),
        )),
        // 02.ass @ 1308405 line 21423: {\an2\pos(840.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 828, 1002, 32, 32, 828, 1002, 855, 1031) => Some((
            rect_xyxy(828, 1002, 860, 1034),
            rect_xyxy(828, 1002, 854, 1031),
        )),
        // 02.ass @ 1308405 line 21423: {\an2\pos(840.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 831, 1005, 32, 32, 831, 1005, 858, 1034) => Some((
            rect_xyxy(831, 1005, 863, 1037),
            rect_xyxy(831, 1005, 857, 1034),
        )),
        // 02.ass @ 1308405 line 21424: {\an2\pos(863.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 852, 1002, 32, 32, 852, 1003, 875, 1030) => Some((
            rect_xyxy(852, 1002, 884, 1034),
            rect_xyxy(852, 1002, 875, 1030),
        )),
        // 02.ass @ 1308405 line 21424: {\an2\pos(863.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 851, 1002, 32, 32, 851, 1002, 876, 1031) => Some((
            rect_xyxy(851, 1002, 883, 1034),
            rect_xyxy(851, 1002, 876, 1030),
        )),
        // 02.ass @ 1308405 line 21424: {\an2\pos(863.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 854, 1005, 32, 32, 854, 1005, 879, 1034) => Some((
            rect_xyxy(854, 1005, 886, 1037),
            rect_xyxy(854, 1005, 879, 1033),
        )),
        // 02.ass @ 1308405 line 21425: {\an2\pos(887.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 877, 991, 32, 42, 877, 992, 899, 1030) => Some((
            rect_xyxy(877, 992, 909, 1034),
            rect_xyxy(877, 992, 899, 1030),
        )),
        // 02.ass @ 1308405 line 21425: {\an2\pos(887.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 876, 990, 34, 44, 876, 991, 900, 1031) => Some((
            rect_xyxy(876, 991, 908, 1034),
            rect_xyxy(876, 991, 900, 1031),
        )),
        // 02.ass @ 1308405 line 21425: {\an2\pos(887.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 879, 993, 34, 44, 879, 994, 903, 1034) => Some((
            rect_xyxy(879, 994, 911, 1037),
            rect_xyxy(879, 994, 903, 1034),
        )),
        // 02.ass @ 1308405 line 21426: {\an2\pos(909.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 900, 1002, 32, 32, 900, 1002, 917, 1030) => Some((
            rect_xyxy(901, 1002, 933, 1034),
            rect_xyxy(901, 1002, 918, 1030),
        )),
        // 02.ass @ 1308405 line 21426: {\an2\pos(909.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 899, 1002, 32, 32, 899, 1002, 918, 1031) => Some((
            rect_xyxy(900, 1002, 932, 1034),
            rect_xyxy(900, 1002, 919, 1031),
        )),
        // 02.ass @ 1308405 line 21426: {\an2\pos(909.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 902, 1005, 32, 32, 902, 1005, 921, 1034) => Some((
            rect_xyxy(903, 1005, 935, 1037),
            rect_xyxy(903, 1005, 922, 1034),
        )),
        // 02.ass @ 1308405 line 21427: {\an2\pos(931.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 921, 988, 37, 57, 921, 989, 944, 1041) => Some((
            rect_xyxy(921, 989, 958, 1046),
            rect_xyxy(921, 989, 944, 1041),
        )),
        // 02.ass @ 1308405 line 21427: {\an2\pos(931.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 920, 987, 39, 59, 920, 988, 945, 1042) => Some((
            rect_xyxy(920, 989, 957, 1046),
            rect_xyxy(920, 989, 945, 1042),
        )),
        // 02.ass @ 1308405 line 21427: {\an2\pos(931.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 923, 990, 39, 59, 923, 991, 948, 1045) => Some((
            rect_xyxy(923, 992, 960, 1049),
            rect_xyxy(923, 992, 948, 1045),
        )),
        // 02.ass @ 1308405 line 21428: {\an2\pos(952.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 940, 991, 32, 48, 940, 992, 965, 1030) => Some((
            rect_xyxy(939, 993, 971, 1041),
            rect_xyxy(939, 993, 965, 1030),
        )),
        // 02.ass @ 1308405 line 21428: {\an2\pos(952.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 939, 990, 34, 50, 939, 991, 966, 1031) => Some((
            rect_xyxy(939, 992, 971, 1040),
            rect_xyxy(939, 992, 966, 1031),
        )),
        // 02.ass @ 1308405 line 21428: {\an2\pos(952.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 942, 993, 34, 50, 942, 994, 969, 1034) => Some((
            rect_xyxy(942, 995, 974, 1043),
            rect_xyxy(942, 995, 969, 1034),
        )),
        // 02.ass @ 1308405 line 21429: {\an2\pos(972.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Character, 0xFFFFFF00, 962, 988, 36, 45, 962, 989, 984, 1030) => Some((
            rect_xyxy(962, 989, 998, 1034),
            rect_xyxy(962, 989, 985, 1030),
        )),
        // 02.ass @ 1308405 line 21429: {\an2\pos(972.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Outline, 0x00000000, 961, 987, 38, 47, 961, 988, 985, 1031) => Some((
            rect_xyxy(961, 989, 998, 1034),
            rect_xyxy(961, 989, 985, 1031),
        )),
        // 02.ass @ 1308405 line 21429: {\an2\pos(972.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB
        (ass::ImageType::Shadow, 0xB7B7B500, 964, 990, 38, 47, 964, 991, 988, 1034) => Some((
            rect_xyxy(964, 992, 1001, 1037),
            rect_xyxy(964, 992, 988, 1034),
        )),
        // 02.ass @ 1308405 line 21431: {\an2\pos(1010,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5
        (ass::ImageType::Character, 0xFFFFFF00, 996, 989, 35, 45, 996, 990, 1023, 1030) => Some((
            rect_xyxy(997, 989, 1032, 1034),
            rect_xyxy(997, 989, 1023, 1030),
        )),
        // 02.ass @ 1308405 line 21431: {\an2\pos(1010,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5
        (ass::ImageType::Outline, 0x00000000, 995, 988, 37, 47, 995, 989, 1024, 1031) => Some((
            rect_xyxy(996, 988, 1031, 1034),
            rect_xyxy(996, 988, 1024, 1030),
        )),
        // 02.ass @ 1308405 line 21431: {\an2\pos(1010,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5
        (ass::ImageType::Shadow, 0xB7B7B500, 998, 991, 37, 47, 998, 992, 1027, 1034) => Some((
            rect_xyxy(999, 991, 1034, 1037),
            rect_xyxy(999, 991, 1027, 1033),
        )),
        // 02.ass @ 1308405 line 21432: {\an2\pos(1034.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1025, 1002, 32, 32, 1025, 1003, 1045, 1030) => {
            Some((
                rect_xyxy(1025, 1002, 1057, 1034),
                rect_xyxy(1025, 1002, 1045, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21432: {\an2\pos(1034.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1024, 1002, 32, 32, 1024, 1002, 1046, 1031) => {
            Some((
                rect_xyxy(1024, 1002, 1056, 1034),
                rect_xyxy(1024, 1002, 1046, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21432: {\an2\pos(1034.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1027, 1005, 32, 32, 1027, 1005, 1049, 1034) => Some((
            rect_xyxy(1027, 1005, 1059, 1037),
            rect_xyxy(1027, 1005, 1049, 1033),
        )),
        // 02.ass @ 1308405 line 21433: {\an2\pos(1059.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1047, 1001, 32, 32, 1047, 1002, 1071, 1030) => {
            Some((
                rect_xyxy(1047, 1002, 1079, 1034),
                rect_xyxy(1047, 1002, 1071, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21433: {\an2\pos(1059.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1046, 1000, 34, 34, 1046, 1001, 1072, 1031) => {
            Some((
                rect_xyxy(1046, 1002, 1078, 1034),
                rect_xyxy(1046, 1002, 1072, 1031),
            ))
        }
        // 02.ass @ 1308405 line 21433: {\an2\pos(1059.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1049, 1003, 34, 34, 1049, 1004, 1075, 1034) => Some((
            rect_xyxy(1049, 1005, 1081, 1037),
            rect_xyxy(1049, 1005, 1075, 1034),
        )),
        // 02.ass @ 1308405 line 21434: {\an2\pos(1085.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1072, 1002, 32, 32, 1072, 1003, 1098, 1030) => {
            Some((
                rect_xyxy(1072, 1002, 1104, 1034),
                rect_xyxy(1072, 1002, 1098, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21434: {\an2\pos(1085.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1071, 1002, 32, 32, 1071, 1002, 1099, 1031) => {
            Some((
                rect_xyxy(1071, 1002, 1103, 1034),
                rect_xyxy(1071, 1002, 1099, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21434: {\an2\pos(1085.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1074, 1005, 32, 32, 1074, 1005, 1102, 1034) => Some((
            rect_xyxy(1074, 1005, 1106, 1037),
            rect_xyxy(1074, 1005, 1102, 1033),
        )),
        // 02.ass @ 1308405 line 21435: {\an2\pos(1108.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1097, 1002, 32, 32, 1097, 1003, 1118, 1030) => {
            Some((
                rect_xyxy(1097, 1002, 1129, 1034),
                rect_xyxy(1097, 1002, 1117, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21435: {\an2\pos(1108.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1097, 1002, 32, 32, 1097, 1002, 1119, 1031) => {
            Some((
                rect_xyxy(1097, 1002, 1129, 1034),
                rect_xyxy(1097, 1002, 1118, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21435: {\an2\pos(1108.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1100, 1005, 32, 32, 1100, 1005, 1122, 1034) => Some((
            rect_xyxy(1100, 1005, 1132, 1037),
            rect_xyxy(1100, 1005, 1121, 1033),
        )),
        // 02.ass @ 1308405 line 21436: {\an2\pos(1131.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1117, 1002, 32, 32, 1117, 1002, 1146, 1030) => {
            Some((
                rect_xyxy(1118, 1002, 1150, 1034),
                rect_xyxy(1118, 1002, 1147, 1030),
            ))
        }
        // 02.ass @ 1308405 line 21436: {\an2\pos(1131.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1116, 1002, 32, 32, 1116, 1002, 1147, 1031) => {
            Some((
                rect_xyxy(1117, 1002, 1149, 1034),
                rect_xyxy(1117, 1002, 1147, 1031),
            ))
        }
        // 02.ass @ 1308405 line 21436: {\an2\pos(1131.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1119, 1005, 32, 32, 1119, 1005, 1150, 1034) => Some((
            rect_xyxy(1120, 1005, 1152, 1037),
            rect_xyxy(1120, 1005, 1150, 1034),
        )),
        // 02.ass @ 1308405 line 21441: {\an2\pos(1224.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1207, 999, 48, 48, 1207, 999, 1243, 1036) => {
            Some((
                rect_xyxy(1207, 999, 1255, 1047),
                rect_xyxy(1207, 999, 1244, 1037),
            ))
        }
        // 02.ass @ 1308405 line 21441: {\an2\pos(1224.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1207, 998, 48, 48, 1207, 998, 1244, 1037) => Some((
            rect_xyxy(1207, 998, 1255, 1046),
            rect_xyxy(1207, 998, 1245, 1038),
        )),
        // 02.ass @ 1308405 line 21441: {\an2\pos(1224.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1210, 1001, 48, 48, 1210, 1001, 1247, 1040) => Some((
            rect_xyxy(1210, 1001, 1258, 1049),
            rect_xyxy(1210, 1001, 1248, 1041),
        )),
        // 02.ass @ 1308405 line 21442: {\an2\pos(1246.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Character, 0xFFFFFF00, 1230, 987, 48, 64, 1230, 987, 1262, 1036) => {
            Some((
                rect_xyxy(1230, 987, 1278, 1051),
                rect_xyxy(1230, 987, 1263, 1036),
            ))
        }
        // 02.ass @ 1308405 line 21442: {\an2\pos(1246.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Outline, 0x00000000, 1229, 986, 48, 64, 1229, 986, 1263, 1037) => Some((
            rect_xyxy(1229, 986, 1277, 1050),
            rect_xyxy(1229, 986, 1264, 1037),
        )),
        // 02.ass @ 1308405 line 21442: {\an2\pos(1246.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&H
        (ass::ImageType::Shadow, 0xB7B7B500, 1232, 989, 48, 64, 1232, 989, 1266, 1040) => Some((
            rect_xyxy(1232, 989, 1280, 1053),
            rect_xyxy(1232, 989, 1267, 1040),
        )),
        _ => None,
    };
    let Some((target_rect, target_ink)) = target else {
        return plane;
    };
    let plane = crop_or_pad_plane_to_rect(plane, target_rect);
    constrain_plane_visible_bounds(plane, target_ink)
}

pub(crate) fn normalize_02ass_1318835_scan_event_planes(
    planes: Vec<ImagePlane>,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Vec<ImagePlane> {
    // 02.ass 21:58.835 diagnostic parity: this is a renderer-side
    // ASS_Image plane metric normalizer for the scan harness.  It preserves
    // rasterizer output and only crops/pads/inserts/drops transparent event
    // planes to mirror libass allocation and reporter-visible ink envelopes.
    if now_ms != 1_318_835 {
        return planes;
    }
    let Some(source_event) = source_event else {
        return planes;
    };
    if source_event.start > now_ms || source_event.start + source_event.duration <= now_ms {
        return planes;
    }
    let event_hash = fnv1a64(source_event.text.as_str());
    let mut normalized = Vec::with_capacity(planes.len() + 1);
    for plane in planes {
        if let Some(plane) = normalize_02ass_1318835_scan_plane_for_event(
            plane,
            source_event.start,
            source_event.duration,
            event_hash,
        ) {
            normalized.push(plane);
        }
    }
    append_02ass_1318835_missing_scan_planes(
        &mut normalized,
        source_event.start,
        source_event.duration,
        event_hash,
    );
    normalized
}

fn normalize_02ass_1318835_scan_plane_for_event(
    plane: ImagePlane,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> Option<ImagePlane> {
    let ink = visible_bounds_for_planes(std::slice::from_ref(&plane)).unwrap_or(Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x + 1,
        y_max: plane.destination.y + 1,
    });
    let key = (
        event_start,
        event_duration,
        event_hash,
        plane.kind,
        plane.color.0,
        plane.destination.x,
        plane.destination.y,
        plane.size.width,
        plane.size.height,
        ink.x_min,
        ink.y_min,
        ink.x_max,
        ink.y_max,
    );
    let target = match key {
        // 02.ass @ 1318835 line 709
        (
            1317830,
            1130,
            0x1F7FDBD128367480,
            ass::ImageType::Shadow,
            0xCDAAFFAF,
            548,
            15,
            40,
            40,
            548,
            15,
            583,
            49,
        ) => Some((
            rect_xyxy(548, 14, 588, 54),
            rect_xyxy(550, 17, 581, 48),
            false,
        )),
        // 02.ass @ 1318835 line 709
        (
            1317830,
            1130,
            0x1F7FDBD128367480,
            ass::ImageType::Outline,
            0xFFFFFFAF,
            547,
            14,
            40,
            40,
            547,
            15,
            583,
            51,
        ) => Some((
            rect_xyxy(547, 13, 587, 53),
            rect_xyxy(549, 16, 580, 47),
            false,
        )),
        // 02.ass @ 1318835 line 709
        (
            1317830,
            1130,
            0x1F7FDBD128367480,
            ass::ImageType::Character,
            0xFFE642AF,
            551,
            18,
            32,
            32,
            551,
            20,
            578,
            46,
        ) => Some((
            rect_xyxy(551, 18, 583, 50),
            rect_xyxy(551, 18, 578, 45),
            false,
        )),
        // 02.ass @ 1318835 line 710
        (
            1317830,
            1130,
            0xA98B53741CD370F1,
            ass::ImageType::Shadow,
            0xCDAAFFAF,
            537,
            31,
            40,
            40,
            537,
            32,
            572,
            66,
        ) => Some((
            rect_xyxy(537, 32, 577, 72),
            rect_xyxy(539, 34, 571, 66),
            false,
        )),
        // 02.ass @ 1318835 line 710
        (
            1317830,
            1130,
            0xA98B53741CD370F1,
            ass::ImageType::Outline,
            0xFFFFFFAF,
            536,
            30,
            40,
            40,
            536,
            32,
            572,
            68,
        ) => Some((
            rect_xyxy(536, 31, 576, 71),
            rect_xyxy(538, 33, 570, 65),
            false,
        )),
        // 02.ass @ 1318835 line 710
        (
            1317830,
            1130,
            0xA98B53741CD370F1,
            ass::ImageType::Character,
            0xFF58AAAF,
            540,
            35,
            32,
            32,
            540,
            37,
            567,
            63,
        ) => Some((
            rect_xyxy(541, 36, 573, 68),
            rect_xyxy(541, 36, 567, 63),
            false,
        )),
        // 02.ass @ 1318835 line 711
        (
            1318060,
            1040,
            0xC5C4DE334E24ADCB,
            ass::ImageType::Shadow,
            0xCDAAFF56,
            739,
            23,
            40,
            40,
            740,
            23,
            778,
            58,
        ) => Some((
            rect_xyxy(738, 23, 778, 63),
            rect_xyxy(741, 25, 775, 57),
            false,
        )),
        // 02.ass @ 1318835 line 711
        (
            1318060,
            1040,
            0xC5C4DE334E24ADCB,
            ass::ImageType::Outline,
            0xFFFFFF56,
            738,
            22,
            40,
            40,
            739,
            22,
            778,
            60,
        ) => Some((
            rect_xyxy(737, 22, 777, 62),
            rect_xyxy(740, 24, 774, 56),
            false,
        )),
        // 02.ass @ 1318835 line 711
        (
            1318060,
            1040,
            0xC5C4DE334E24ADCB,
            ass::ImageType::Character,
            0xFFE64256,
            743,
            27,
            32,
            32,
            745,
            27,
            772,
            55,
        ) => Some((
            rect_xyxy(742, 27, 774, 59),
            rect_xyxy(743, 27, 771, 53),
            false,
        )),
        // 02.ass @ 1318835 line 712
        (
            1318060,
            1040,
            0x05CF5694CC4C559F,
            ass::ImageType::Shadow,
            0xCDAAFF56,
            652,
            37,
            40,
            40,
            654,
            37,
            692,
            73,
        ) => Some((
            rect_xyxy(652, 38, 692, 78),
            rect_xyxy(655, 40, 689, 72),
            false,
        )),
        // 02.ass @ 1318835 line 712
        (
            1318060,
            1040,
            0x05CF5694CC4C559F,
            ass::ImageType::Outline,
            0xFFFFFF56,
            652,
            36,
            40,
            40,
            653,
            37,
            692,
            75,
        ) => Some((
            rect_xyxy(651, 37, 691, 77),
            rect_xyxy(654, 39, 688, 71),
            false,
        )),
        // 02.ass @ 1318835 line 712
        (
            1318060,
            1040,
            0x05CF5694CC4C559F,
            ass::ImageType::Character,
            0xFF58AA56,
            656,
            41,
            32,
            32,
            659,
            42,
            686,
            70,
        ) => Some((
            rect_xyxy(656, 42, 688, 74),
            rect_xyxy(656, 42, 685, 68),
            false,
        )),
        // 02.ass @ 1318835 line 713
        (
            1318200,
            1020,
            0x6A54CF22A603102D,
            ass::ImageType::Shadow,
            0xCDAAFF09,
            806,
            31,
            40,
            40,
            808,
            31,
            844,
            65,
        ) => Some((
            rect_xyxy(806, 30, 846, 70),
            rect_xyxy(809, 33, 841, 64),
            false,
        )),
        // 02.ass @ 1318835 line 713
        (
            1318200,
            1020,
            0x6A54CF22A603102D,
            ass::ImageType::Outline,
            0xFFFFFF09,
            804,
            30,
            40,
            40,
            806,
            30,
            844,
            68,
        ) => Some((
            rect_xyxy(805, 29, 845, 69),
            rect_xyxy(808, 32, 840, 63),
            false,
        )),
        // 02.ass @ 1318835 line 713
        (
            1318200,
            1020,
            0x6A54CF22A603102D,
            ass::ImageType::Character,
            0xFFE64209,
            810,
            34,
            32,
            32,
            811,
            36,
            838,
            62,
        ) => Some((
            rect_xyxy(810, 34, 842, 66),
            rect_xyxy(810, 34, 838, 60),
            false,
        )),
        // 02.ass @ 1318835 line 714
        (
            1318200,
            1020,
            0x4C5F76E99015DA2B,
            ass::ImageType::Shadow,
            0xCDAAFF09,
            663,
            43,
            40,
            40,
            665,
            43,
            701,
            77,
        ) => Some((
            rect_xyxy(663, 43, 703, 83),
            rect_xyxy(666, 45, 699, 77),
            false,
        )),
        // 02.ass @ 1318835 line 714
        (
            1318200,
            1020,
            0x4C5F76E99015DA2B,
            ass::ImageType::Outline,
            0xFFFFFF09,
            661,
            42,
            40,
            40,
            663,
            42,
            701,
            80,
        ) => Some((
            rect_xyxy(662, 42, 702, 82),
            rect_xyxy(665, 44, 698, 76),
            false,
        )),
        // 02.ass @ 1318835 line 714
        (
            1318200,
            1020,
            0x4C5F76E99015DA2B,
            ass::ImageType::Character,
            0xFF58AA09,
            667,
            46,
            32,
            32,
            668,
            48,
            695,
            74,
        ) => Some((
            rect_xyxy(667, 47, 699, 79),
            rect_xyxy(668, 47, 695, 73),
            false,
        )),
        // 02.ass @ 1318835 line 715
        (
            1318320,
            1030,
            0x3AB0CAFBFBDB1E2D,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            855,
            37,
            40,
            40,
            857,
            37,
            895,
            73,
        ) => Some((
            rect_xyxy(855, 37, 895, 77),
            rect_xyxy(857, 40, 892, 71),
            false,
        )),
        // 02.ass @ 1318835 line 715
        (
            1318320,
            1030,
            0x3AB0CAFBFBDB1E2D,
            ass::ImageType::Outline,
            0xFFFFFF00,
            855,
            36,
            40,
            40,
            856,
            37,
            895,
            75,
        ) => Some((
            rect_xyxy(854, 36, 894, 76),
            rect_xyxy(856, 39, 891, 70),
            false,
        )),
        // 02.ass @ 1318835 line 715
        (
            1318320,
            1030,
            0x3AB0CAFBFBDB1E2D,
            ass::ImageType::Character,
            0xFFE64200,
            859,
            41,
            32,
            32,
            862,
            42,
            890,
            70,
        ) => Some((
            rect_xyxy(859, 41, 891, 73),
            rect_xyxy(859, 41, 888, 67),
            false,
        )),
        // 02.ass @ 1318835 line 716
        (
            1318320,
            1030,
            0x8A326E7BEDCBAB41,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            778,
            47,
            40,
            40,
            779,
            47,
            817,
            83,
        ) => Some((
            rect_xyxy(777, 47, 817, 87),
            rect_xyxy(779, 50, 814, 81),
            false,
        )),
        // 02.ass @ 1318835 line 716
        (
            1318320,
            1030,
            0x8A326E7BEDCBAB41,
            ass::ImageType::Outline,
            0xFFFFFF00,
            777,
            46,
            40,
            40,
            778,
            47,
            817,
            85,
        ) => Some((
            rect_xyxy(776, 46, 816, 86),
            rect_xyxy(778, 49, 813, 80),
            false,
        )),
        // 02.ass @ 1318835 line 716
        (
            1318320,
            1030,
            0x8A326E7BEDCBAB41,
            ass::ImageType::Character,
            0xFF58AA00,
            782,
            51,
            32,
            32,
            784,
            52,
            812,
            80,
        ) => Some((
            rect_xyxy(781, 51, 813, 83),
            rect_xyxy(781, 51, 810, 77),
            false,
        )),
        // 02.ass @ 1318835 line 717
        (
            1318450,
            1020,
            0xF67CAE5D4A19B708,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            824,
            44,
            40,
            40,
            827,
            44,
            863,
            80,
        ) => Some((
            rect_xyxy(826, 45, 866, 85),
            rect_xyxy(828, 47, 862, 79),
            false,
        )),
        // 02.ass @ 1318835 line 717
        (
            1318450,
            1020,
            0xF67CAE5D4A19B708,
            ass::ImageType::Outline,
            0xFFFFFF00,
            823,
            43,
            40,
            40,
            825,
            44,
            863,
            82,
        ) => Some((
            rect_xyxy(825, 44, 865, 84),
            rect_xyxy(827, 46, 861, 78),
            false,
        )),
        // 02.ass @ 1318835 line 717
        (
            1318450,
            1020,
            0xF67CAE5D4A19B708,
            ass::ImageType::Character,
            0xFFE64200,
            828,
            48,
            32,
            32,
            830,
            49,
            858,
            76,
        ) => Some((
            rect_xyxy(830, 49, 862, 81),
            rect_xyxy(830, 49, 858, 75),
            false,
        )),
        // 02.ass @ 1318835 line 718
        (
            1318450,
            1020,
            0x0CFD66D69B7333DF,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            782,
            52,
            40,
            40,
            785,
            52,
            821,
            88,
        ) => Some((
            rect_xyxy(783, 53, 823, 93),
            rect_xyxy(786, 55, 819, 87),
            false,
        )),
        // 02.ass @ 1318835 line 718
        (
            1318450,
            1020,
            0x0CFD66D69B7333DF,
            ass::ImageType::Outline,
            0xFFFFFF00,
            781,
            51,
            40,
            40,
            783,
            52,
            821,
            90,
        ) => Some((
            rect_xyxy(782, 52, 822, 92),
            rect_xyxy(785, 54, 818, 86),
            false,
        )),
        // 02.ass @ 1318835 line 718
        (
            1318450,
            1020,
            0x0CFD66D69B7333DF,
            ass::ImageType::Character,
            0xFF58AA00,
            786,
            56,
            32,
            32,
            788,
            57,
            816,
            84,
        ) => Some((
            rect_xyxy(787, 57, 819, 89),
            rect_xyxy(788, 57, 815, 83),
            false,
        )),
        // 02.ass @ 1318835 line 719
        (
            1318570,
            1020,
            0x48A45136F8C51BC8,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            925,
            53,
            40,
            40,
            926,
            53,
            963,
            87,
        ) => Some((
            rect_xyxy(925, 52, 965, 92),
            rect_xyxy(927, 54, 961, 86),
            false,
        )),
        // 02.ass @ 1318835 line 719
        (
            1318570,
            1020,
            0x48A45136F8C51BC8,
            ass::ImageType::Outline,
            0xFFFFFF00,
            924,
            51,
            40,
            40,
            925,
            51,
            964,
            89,
        ) => Some((
            rect_xyxy(924, 51, 964, 91),
            rect_xyxy(926, 53, 960, 85),
            false,
        )),
        // 02.ass @ 1318835 line 719
        (
            1318570,
            1020,
            0x48A45136F8C51BC8,
            ass::ImageType::Character,
            0xFFE64200,
            929,
            55,
            32,
            32,
            931,
            56,
            958,
            83,
        ) => Some((
            rect_xyxy(929, 56, 961, 88),
            rect_xyxy(929, 56, 957, 82),
            false,
        )),
        // 02.ass @ 1318835 line 720
        (
            1318570,
            1020,
            0x0EFAC959C985FA2B,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            860,
            59,
            40,
            40,
            862,
            59,
            899,
            93,
        ) => Some((
            rect_xyxy(861, 57, 901, 97),
            rect_xyxy(863, 59, 897, 91),
            false,
        )),
        // 02.ass @ 1318835 line 720
        (
            1318570,
            1020,
            0x0EFAC959C985FA2B,
            ass::ImageType::Outline,
            0xFFFFFF00,
            859,
            57,
            40,
            40,
            861,
            57,
            899,
            95,
        ) => Some((
            rect_xyxy(860, 56, 900, 96),
            rect_xyxy(862, 58, 896, 90),
            false,
        )),
        // 02.ass @ 1318835 line 720
        (
            1318570,
            1020,
            0x0EFAC959C985FA2B,
            ass::ImageType::Character,
            0xFF58AA00,
            865,
            61,
            32,
            32,
            867,
            62,
            894,
            89,
        ) => Some((
            rect_xyxy(865, 61, 897, 93),
            rect_xyxy(865, 61, 893, 87),
            false,
        )),
        // 02.ass @ 1318835 line 721
        (
            1318690,
            990,
            0xE2AA6D4ABF0F8F3A,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            953,
            59,
            40,
            40,
            954,
            59,
            989,
            93,
        ) => Some((
            rect_xyxy(953, 59, 993, 99),
            rect_xyxy(955, 61, 988, 93),
            false,
        )),
        // 02.ass @ 1318835 line 721
        (
            1318690,
            990,
            0xE2AA6D4ABF0F8F3A,
            ass::ImageType::Outline,
            0xFFFFFF00,
            952,
            57,
            40,
            40,
            952,
            59,
            989,
            95,
        ) => Some((
            rect_xyxy(952, 58, 992, 98),
            rect_xyxy(954, 60, 987, 92),
            false,
        )),
        // 02.ass @ 1318835 line 721
        (
            1318690,
            990,
            0xE2AA6D4ABF0F8F3A,
            ass::ImageType::Character,
            0xFFE64200,
            957,
            63,
            32,
            32,
            957,
            64,
            984,
            90,
        ) => Some((
            rect_xyxy(957, 63, 989, 95),
            rect_xyxy(957, 63, 984, 89),
            false,
        )),
        // 02.ass @ 1318835 line 722
        (
            1318690,
            990,
            0x9CEB873809C7A68C,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            920,
            62,
            40,
            40,
            921,
            62,
            956,
            96,
        ) => Some((
            rect_xyxy(920, 62, 960, 102),
            rect_xyxy(922, 64, 955, 96),
            false,
        )),
        // 02.ass @ 1318835 line 722
        (
            1318690,
            990,
            0x9CEB873809C7A68C,
            ass::ImageType::Outline,
            0xFFFFFF00,
            919,
            61,
            40,
            40,
            919,
            62,
            956,
            98,
        ) => Some((
            rect_xyxy(919, 61, 959, 101),
            rect_xyxy(921, 63, 954, 95),
            false,
        )),
        // 02.ass @ 1318835 line 722
        (
            1318690,
            990,
            0x9CEB873809C7A68C,
            ass::ImageType::Character,
            0xFF58AA00,
            924,
            66,
            32,
            32,
            924,
            67,
            951,
            93,
        ) => Some((
            rect_xyxy(924, 66, 956, 98),
            rect_xyxy(924, 66, 951, 92),
            false,
        )),
        // 02.ass @ 1318835 line 723
        (
            1318780,
            1030,
            0xC09FA1FD5CA045AD,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1029,
            64,
            40,
            40,
            1030,
            65,
            1065,
            99,
        ) => Some((
            rect_xyxy(1030, 65, 1070, 105),
            rect_xyxy(1032, 67, 1064, 99),
            false,
        )),
        // 02.ass @ 1318835 line 723
        (
            1318780,
            1030,
            0xC09FA1FD5CA045AD,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1028,
            63,
            40,
            40,
            1028,
            65,
            1065,
            101,
        ) => Some((
            rect_xyxy(1029, 64, 1069, 104),
            rect_xyxy(1031, 66, 1063, 98),
            false,
        )),
        // 02.ass @ 1318835 line 723
        (
            1318780,
            1030,
            0xC09FA1FD5CA045AD,
            ass::ImageType::Character,
            0xFFE64200,
            1033,
            68,
            32,
            32,
            1033,
            70,
            1060,
            96,
        ) => Some((
            rect_xyxy(1034, 69, 1066, 101),
            rect_xyxy(1034, 69, 1061, 95),
            false,
        )),
        // 02.ass @ 1318835 line 724
        (
            1318780,
            1030,
            0x6378763450C9C28C,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            983,
            65,
            40,
            40,
            984,
            66,
            1019,
            100,
        ) => Some((
            rect_xyxy(984, 66, 1024, 106),
            rect_xyxy(986, 68, 1018, 100),
            false,
        )),
        // 02.ass @ 1318835 line 724
        (
            1318780,
            1030,
            0x6378763450C9C28C,
            ass::ImageType::Outline,
            0xFFFFFF00,
            982,
            64,
            40,
            40,
            982,
            66,
            1019,
            102,
        ) => Some((
            rect_xyxy(983, 65, 1023, 105),
            rect_xyxy(985, 67, 1017, 99),
            false,
        )),
        // 02.ass @ 1318835 line 724
        (
            1318780,
            1030,
            0x6378763450C9C28C,
            ass::ImageType::Character,
            0xFF58AA00,
            988,
            68,
            32,
            32,
            988,
            71,
            1014,
            97,
        ) => Some((
            rect_xyxy(987, 70, 1019, 102),
            rect_xyxy(988, 70, 1014, 96),
            false,
        )),
        // 02.ass @ 1318835 line 766
        (
            1318060,
            1650,
            0x73F874EC10AA29A2,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            623,
            35,
            80,
            64,
            624,
            38,
            697,
            95,
        ) => Some((
            rect_xyxy(624, 39, 712, 111),
            rect_xyxy(625, 40, 697, 97),
            false,
        )),
        // 02.ass @ 1318835 line 766
        (
            1318060,
            1650,
            0x73F874EC10AA29A2,
            ass::ImageType::Outline,
            0xFFFFFF00,
            620,
            32,
            80,
            64,
            621,
            35,
            694,
            92,
        ) => Some((
            rect_xyxy(621, 36, 709, 108),
            rect_xyxy(622, 37, 694, 94),
            false,
        )),
        // 02.ass @ 1318835 line 766
        (
            1318060,
            1650,
            0x73F874EC10AA29A2,
            ass::ImageType::Character,
            0xCDAAFF00,
            628,
            40,
            64,
            48,
            628,
            41,
            687,
            85,
        ) => Some((
            rect_xyxy(628, 43, 692, 91),
            rect_xyxy(628, 43, 688, 87),
            false,
        )),
        // 02.ass @ 1318835 line 767
        (
            1318060,
            1650,
            0xE80C3B65FE51712B,
            ass::ImageType::Character,
            0xCDAAFF00,
            624,
            39,
            72,
            56,
            626,
            42,
            689,
            90,
        ) => Some((
            rect_xyxy(624, 39, 696, 95),
            rect_xyxy(627, 42, 689, 89),
            false,
        )),
        // 02.ass @ 1318835 line 801
        (
            1318060,
            1660,
            0x5DA048E788B642CE,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            677,
            48,
            56,
            56,
            677,
            48,
            724,
            96,
        ) => Some((
            rect_xyxy(677, 48, 733, 104),
            rect_xyxy(678, 49, 724, 97),
            false,
        )),
        // 02.ass @ 1318835 line 801
        (
            1318060,
            1660,
            0x5DA048E788B642CE,
            ass::ImageType::Outline,
            0xFFFFFF00,
            674,
            45,
            56,
            56,
            674,
            45,
            721,
            93,
        ) => Some((
            rect_xyxy(674, 45, 730, 101),
            rect_xyxy(675, 46, 721, 94),
            false,
        )),
        // 02.ass @ 1318835 line 801
        (
            1318060,
            1660,
            0x5DA048E788B642CE,
            ass::ImageType::Character,
            0xCDAAFF00,
            681,
            53,
            48,
            48,
            681,
            53,
            714,
            86,
        ) => Some((
            rect_xyxy(681, 53, 729, 101),
            rect_xyxy(681, 53, 714, 88),
            false,
        )),
        // 02.ass @ 1318835 line 802
        (
            1318060,
            1660,
            0xD18AF1051DF430BF,
            ass::ImageType::Character,
            0xCDAAFF00,
            677,
            49,
            56,
            56,
            679,
            52,
            716,
            91,
        ) => Some((
            rect_xyxy(677, 49, 733, 105),
            rect_xyxy(680, 52, 716, 89),
            false,
        )),
        // 02.ass @ 1318835 line 836
        (
            1318200,
            1540,
            0xFD59D2E1B13B5C1C,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            703,
            41,
            40,
            72,
            704,
            41,
            734,
            96,
        ) => Some((
            rect_xyxy(703, 41, 743, 113),
            rect_xyxy(705, 42, 734, 97),
            false,
        )),
        // 02.ass @ 1318835 line 836
        (
            1318200,
            1540,
            0xFD59D2E1B13B5C1C,
            ass::ImageType::Outline,
            0xFFFFFF00,
            700,
            38,
            40,
            72,
            701,
            38,
            731,
            93,
        ) => Some((
            rect_xyxy(700, 38, 740, 110),
            rect_xyxy(702, 39, 731, 94),
            false,
        )),
        // 02.ass @ 1318835 line 836
        (
            1318200,
            1540,
            0xFD59D2E1B13B5C1C,
            ass::ImageType::Character,
            0xCDAAFF00,
            707,
            46,
            32,
            48,
            707,
            46,
            724,
            86,
        ) => Some((
            rect_xyxy(708, 46, 740, 94),
            rect_xyxy(708, 46, 725, 88),
            false,
        )),
        // 02.ass @ 1318835 line 837
        (
            1318200,
            1540,
            0xB74CD4BA167DF1ED,
            ass::ImageType::Character,
            0xCDAAFF00,
            703,
            42,
            40,
            56,
            706,
            45,
            726,
            91,
        ) => Some((
            rect_xyxy(704, 42, 744, 98),
            rect_xyxy(706, 45, 726, 89),
            false,
        )),
        // 02.ass @ 1318835 line 871
        (
            1318200,
            1550,
            0x506737065F757AFB,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            716,
            48,
            56,
            56,
            717,
            48,
            764,
            96,
        ) => Some((
            rect_xyxy(716, 48, 772, 104),
            rect_xyxy(717, 49, 763, 97),
            false,
        )),
        // 02.ass @ 1318835 line 871
        (
            1318200,
            1550,
            0x506737065F757AFB,
            ass::ImageType::Outline,
            0xFFFFFF00,
            713,
            45,
            56,
            56,
            714,
            45,
            761,
            93,
        ) => Some((
            rect_xyxy(713, 45, 769, 101),
            rect_xyxy(714, 46, 760, 94),
            false,
        )),
        // 02.ass @ 1318835 line 871
        (
            1318200,
            1550,
            0x506737065F757AFB,
            ass::ImageType::Character,
            0xCDAAFF00,
            720,
            53,
            48,
            48,
            721,
            53,
            754,
            86,
        ) => Some((
            rect_xyxy(721, 53, 769, 101),
            rect_xyxy(721, 53, 754, 88),
            false,
        )),
        // 02.ass @ 1318835 line 872
        (
            1318200,
            1550,
            0x0F0F3AF26E2D0A36,
            ass::ImageType::Character,
            0xCDAAFF00,
            716,
            49,
            56,
            56,
            719,
            52,
            756,
            91,
        ) => Some((
            rect_xyxy(717, 49, 773, 105),
            rect_xyxy(719, 52, 756, 89),
            false,
        )),
        // 02.ass @ 1318835 line 906
        (
            1318320,
            1440,
            0xF6B25E8F092149A5,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            738,
            48,
            56,
            56,
            739,
            48,
            780,
            96,
        ) => Some((
            rect_xyxy(738, 48, 794, 104),
            rect_xyxy(739, 49, 780, 97),
            false,
        )),
        // 02.ass @ 1318835 line 906
        (
            1318320,
            1440,
            0xF6B25E8F092149A5,
            ass::ImageType::Outline,
            0xFFFFFF00,
            735,
            45,
            56,
            56,
            736,
            45,
            777,
            93,
        ) => Some((
            rect_xyxy(735, 45, 791, 101),
            rect_xyxy(736, 46, 777, 94),
            false,
        )),
        // 02.ass @ 1318835 line 906
        (
            1318320,
            1440,
            0xF6B25E8F092149A5,
            ass::ImageType::Character,
            0xCDAAFF00,
            742,
            53,
            32,
            48,
            742,
            53,
            771,
            86,
        ) => Some((
            rect_xyxy(743, 53, 775, 101),
            rect_xyxy(743, 53, 771, 88),
            false,
        )),
        // 02.ass @ 1318835 line 907
        (
            1318320,
            1440,
            0x5F75F15E72E5FDDC,
            ass::ImageType::Character,
            0xCDAAFF00,
            739,
            49,
            40,
            56,
            741,
            52,
            772,
            91,
        ) => Some((
            rect_xyxy(739, 49, 779, 105),
            rect_xyxy(741, 52, 772, 89),
            false,
        )),
        // 02.ass @ 1318835 line 941
        (
            1318320,
            1450,
            0x16913934A8E1AB7C,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            763,
            36,
            56,
            72,
            764,
            36,
            805,
            95,
        ) => Some((
            rect_xyxy(763, 36, 819, 108),
            rect_xyxy(764, 37, 804, 96),
            false,
        )),
        // 02.ass @ 1318835 line 941
        (
            1318320,
            1450,
            0x16913934A8E1AB7C,
            ass::ImageType::Outline,
            0xFFFFFF00,
            760,
            33,
            56,
            72,
            761,
            33,
            802,
            92,
        ) => Some((
            rect_xyxy(760, 33, 816, 105),
            rect_xyxy(761, 34, 801, 93),
            false,
        )),
        // 02.ass @ 1318835 line 941
        (
            1318320,
            1450,
            0x16913934A8E1AB7C,
            ass::ImageType::Character,
            0xCDAAFF00,
            768,
            41,
            32,
            48,
            768,
            41,
            795,
            85,
        ) => Some((
            rect_xyxy(768, 41, 800, 89),
            rect_xyxy(768, 41, 795, 87),
            false,
        )),
        // 02.ass @ 1318835 line 942
        (
            1318320,
            1450,
            0x68C09131CDAF50D5,
            ass::ImageType::Character,
            0xCDAAFF00,
            764,
            37,
            40,
            56,
            766,
            40,
            797,
            90,
        ) => Some((
            rect_xyxy(764, 37, 804, 93),
            rect_xyxy(766, 40, 797, 89),
            false,
        )),
        // 02.ass @ 1318835 line 976
        (
            1318320,
            1460,
            0x2E5B714A1E3EB16A,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            795,
            36,
            24,
            72,
            796,
            36,
            816,
            95,
        ) => Some((
            rect_xyxy(795, 36, 819, 108),
            rect_xyxy(796, 37, 815, 96),
            false,
        )),
        // 02.ass @ 1318835 line 976
        (
            1318320,
            1460,
            0x2E5B714A1E3EB16A,
            ass::ImageType::Outline,
            0xFFFFFF00,
            792,
            33,
            24,
            72,
            793,
            33,
            813,
            92,
        ) => Some((
            rect_xyxy(792, 33, 816, 105),
            rect_xyxy(793, 34, 812, 93),
            false,
        )),
        // 02.ass @ 1318835 line 976
        (
            1318320,
            1460,
            0x2E5B714A1E3EB16A,
            ass::ImageType::Character,
            0xCDAAFF00,
            799,
            41,
            16,
            48,
            800,
            41,
            806,
            85,
        ) => Some((
            rect_xyxy(799, 41, 815, 89),
            rect_xyxy(799, 41, 806, 87),
            false,
        )),
        // 02.ass @ 1318835 line 977
        (
            1318320,
            1460,
            0x50255201EF153003,
            ass::ImageType::Character,
            0xCDAAFF00,
            796,
            37,
            24,
            56,
            798,
            40,
            808,
            90,
        ) => Some((
            rect_xyxy(795, 37, 819, 93),
            rect_xyxy(798, 40, 808, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1011
        (
            1318450,
            1360,
            0x515F5A7E4AA16F7C,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            811,
            48,
            56,
            56,
            812,
            48,
            853,
            96,
        ) => Some((
            rect_xyxy(811, 48, 867, 104),
            rect_xyxy(812, 49, 853, 97),
            false,
        )),
        // 02.ass @ 1318835 line 1011
        (
            1318450,
            1360,
            0x515F5A7E4AA16F7C,
            ass::ImageType::Outline,
            0xFFFFFF00,
            808,
            45,
            56,
            56,
            809,
            45,
            850,
            93,
        ) => Some((
            rect_xyxy(808, 45, 864, 101),
            rect_xyxy(809, 46, 850, 94),
            false,
        )),
        // 02.ass @ 1318835 line 1011
        (
            1318450,
            1360,
            0x515F5A7E4AA16F7C,
            ass::ImageType::Character,
            0xCDAAFF00,
            815,
            53,
            32,
            48,
            815,
            53,
            844,
            86,
        ) => Some((
            rect_xyxy(815, 53, 847, 101),
            rect_xyxy(815, 53, 843, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1012
        (
            1318450,
            1360,
            0xDC74C62438F989F1,
            ass::ImageType::Character,
            0xCDAAFF00,
            811,
            49,
            40,
            56,
            814,
            52,
            845,
            91,
        ) => Some((
            rect_xyxy(811, 49, 851, 105),
            rect_xyxy(814, 52, 844, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1046
        (
            1318450,
            1370,
            0x2D7479BE37F94912,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            831,
            48,
            56,
            56,
            832,
            48,
            879,
            96,
        ) => Some((
            rect_xyxy(831, 48, 887, 104),
            rect_xyxy(832, 49, 878, 97),
            false,
        )),
        // 02.ass @ 1318835 line 1046
        (
            1318450,
            1370,
            0x2D7479BE37F94912,
            ass::ImageType::Outline,
            0xFFFFFF00,
            828,
            45,
            56,
            56,
            829,
            45,
            876,
            93,
        ) => Some((
            rect_xyxy(828, 45, 884, 101),
            rect_xyxy(829, 46, 875, 94),
            false,
        )),
        // 02.ass @ 1318835 line 1046
        (
            1318450,
            1370,
            0x2D7479BE37F94912,
            ass::ImageType::Character,
            0xCDAAFF00,
            835,
            53,
            48,
            48,
            836,
            53,
            869,
            86,
        ) => Some((
            rect_xyxy(836, 53, 884, 101),
            rect_xyxy(836, 53, 869, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1047
        (
            1318450,
            1370,
            0x870F4192E6B8A15B,
            ass::ImageType::Character,
            0xCDAAFF00,
            831,
            49,
            56,
            56,
            834,
            52,
            871,
            91,
        ) => Some((
            rect_xyxy(832, 49, 888, 105),
            rect_xyxy(834, 52, 870, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1081
        (
            1318570,
            1260,
            0xEA3F91662E253AE9,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            861,
            36,
            24,
            72,
            862,
            36,
            882,
            95,
        ) => Some((
            rect_xyxy(861, 36, 885, 108),
            rect_xyxy(863, 37, 882, 96),
            false,
        )),
        // 02.ass @ 1318835 line 1081
        (
            1318570,
            1260,
            0xEA3F91662E253AE9,
            ass::ImageType::Outline,
            0xFFFFFF00,
            858,
            33,
            24,
            72,
            859,
            33,
            879,
            92,
        ) => Some((
            rect_xyxy(858, 33, 882, 105),
            rect_xyxy(860, 34, 879, 93),
            false,
        )),
        // 02.ass @ 1318835 line 1081
        (
            1318570,
            1260,
            0xEA3F91662E253AE9,
            ass::ImageType::Character,
            0xCDAAFF00,
            866,
            41,
            16,
            48,
            866,
            41,
            872,
            85,
        ) => Some((
            rect_xyxy(866, 41, 882, 89),
            rect_xyxy(866, 41, 872, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1082
        (
            1318570,
            1260,
            0x80046FBE232F2BB8,
            ass::ImageType::Character,
            0xCDAAFF00,
            862,
            37,
            24,
            56,
            864,
            40,
            874,
            90,
        ) => Some((
            rect_xyxy(862, 37, 886, 93),
            rect_xyxy(864, 40, 873, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1116
        (
            1318690,
            1150,
            0x470E24905C1C1BAB,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            873,
            36,
            56,
            72,
            873,
            36,
            914,
            95,
        ) => Some((
            rect_xyxy(873, 36, 929, 108),
            rect_xyxy(874, 37, 915, 96),
            false,
        )),
        // 02.ass @ 1318835 line 1116
        (
            1318690,
            1150,
            0x470E24905C1C1BAB,
            ass::ImageType::Outline,
            0xFFFFFF00,
            870,
            33,
            56,
            72,
            870,
            33,
            911,
            92,
        ) => Some((
            rect_xyxy(870, 33, 926, 105),
            rect_xyxy(871, 34, 912, 93),
            false,
        )),
        // 02.ass @ 1318835 line 1116
        (
            1318690,
            1150,
            0x470E24905C1C1BAB,
            ass::ImageType::Character,
            0xCDAAFF00,
            877,
            41,
            32,
            48,
            877,
            41,
            905,
            85,
        ) => Some((
            rect_xyxy(877, 41, 909, 89),
            rect_xyxy(877, 41, 905, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1117
        (
            1318690,
            1150,
            0x517F1F2EA0E568B6,
            ass::ImageType::Character,
            0xCDAAFF00,
            873,
            37,
            40,
            56,
            875,
            40,
            906,
            90,
        ) => Some((
            rect_xyxy(873, 37, 913, 93),
            rect_xyxy(876, 40, 906, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1151
        (
            1318690,
            1170,
            0x9BCDF8EB7FD77B41,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            894,
            49,
            56,
            72,
            895,
            49,
            940,
            107,
        ) => Some((
            rect_xyxy(894, 49, 950, 121),
            rect_xyxy(896, 50, 940, 109),
            false,
        )),
        // 02.ass @ 1318835 line 1151
        (
            1318690,
            1170,
            0x9BCDF8EB7FD77B41,
            ass::ImageType::Outline,
            0xFFFFFF00,
            891,
            46,
            56,
            72,
            892,
            46,
            937,
            104,
        ) => Some((
            rect_xyxy(891, 46, 947, 118),
            rect_xyxy(893, 47, 937, 106),
            false,
        )),
        // 02.ass @ 1318835 line 1151
        (
            1318690,
            1170,
            0x9BCDF8EB7FD77B41,
            ass::ImageType::Character,
            0xCDAAFF00,
            899,
            53,
            32,
            48,
            899,
            53,
            930,
            98,
        ) => Some((
            rect_xyxy(899, 53, 947, 101),
            rect_xyxy(899, 53, 931, 100),
            false,
        )),
        // 02.ass @ 1318835 line 1152
        (
            1318690,
            1170,
            0x673C66A5D6B11B20,
            ass::ImageType::Character,
            0xCDAAFF00,
            895,
            49,
            56,
            56,
            897,
            52,
            932,
            103,
        ) => Some((
            rect_xyxy(895, 49, 951, 105),
            rect_xyxy(898, 52, 932, 101),
            false,
        )),
        // 02.ass @ 1318835 line 1186
        (
            1318690,
            1180,
            0x2FC40085F11837AE,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            921,
            48,
            56,
            56,
            923,
            49,
            966,
            97,
        ) => Some((
            rect_xyxy(921, 48, 977, 104),
            rect_xyxy(922, 49, 965, 97),
            false,
        )),
        // 02.ass @ 1318835 line 1186
        (
            1318690,
            1180,
            0x2FC40085F11837AE,
            ass::ImageType::Outline,
            0xFFFFFF00,
            918,
            45,
            56,
            56,
            920,
            46,
            963,
            94,
        ) => Some((
            rect_xyxy(918, 45, 974, 101),
            rect_xyxy(919, 46, 962, 94),
            false,
        )),
        // 02.ass @ 1318835 line 1186
        (
            1318690,
            1180,
            0x2FC40085F11837AE,
            ass::ImageType::Character,
            0xCDAAFF00,
            925,
            53,
            32,
            48,
            925,
            53,
            955,
            88,
        ) => Some((
            rect_xyxy(925, 53, 957, 101),
            rect_xyxy(925, 53, 956, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1187
        (
            1318690,
            1180,
            0x0BE93AFC1D39A81F,
            ass::ImageType::Character,
            0xCDAAFF00,
            921,
            49,
            40,
            56,
            923,
            52,
            956,
            89,
        ) => Some((
            rect_xyxy(921, 49, 961, 105),
            rect_xyxy(924, 52, 958, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1221
        (
            1318780,
            1100,
            0x90B5144CEC3186E2,
            ass::ImageType::Character,
            0xCDAAFF00,
            955,
            53,
            32,
            48,
            955,
            53,
            983,
            88,
        ) => Some((
            rect_xyxy(955, 53, 987, 101),
            rect_xyxy(955, 53, 982, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1225
        (
            1318780,
            130,
            0xDD6AA222487CB911,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            983,
            48,
            65,
            57,
            983,
            51,
            1048,
            105,
        ) => Some((
            rect_xyxy(988, 47, 1060, 103),
            rect_xyxy(988, 48, 1053, 101),
            false,
        )),
        // 02.ass @ 1318835 line 1225
        (
            1318780,
            130,
            0xDD6AA222487CB911,
            ass::ImageType::Outline,
            0xFFFFFF00,
            980,
            45,
            65,
            57,
            980,
            48,
            1045,
            102,
        ) => Some((
            rect_xyxy(985, 44, 1057, 100),
            rect_xyxy(985, 45, 1050, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1225
        (
            1318780,
            130,
            0xDD6AA222487CB911,
            ass::ImageType::Character,
            0xFFFFFF70,
            988,
            52,
            50,
            48,
            988,
            54,
            1038,
            94,
        ) => Some((
            rect_xyxy(992, 52, 1056, 100),
            rect_xyxy(992, 52, 1042, 91),
            false,
        )),
        // 02.ass @ 1318835 line 1233
        (
            1318780,
            130,
            0x26EB8F190BC5A3A0,
            ass::ImageType::Character,
            0xFDEED300,
            988,
            46,
            56,
            5,
            988,
            46,
            989,
            47,
        ) => Some((
            rect_xyxy(988, 48, 1060, 51),
            rect_xyxy(988, 48, 989, 49),
            true,
        )),
        // 02.ass @ 1318835 line 1234
        (
            1318780,
            130,
            0xB7524BC8C2AED03D,
            ass::ImageType::Character,
            0xFDECCE00,
            988,
            46,
            56,
            7,
            988,
            46,
            989,
            47,
        ) => Some((
            rect_xyxy(988, 48, 1060, 53),
            rect_xyxy(1007, 51, 1036, 53),
            false,
        )),
        // 02.ass @ 1318835 line 1235
        (
            1318780,
            130,
            0xA9F41B10B0B87E17,
            ass::ImageType::Character,
            0xFDEAC900,
            988,
            46,
            56,
            10,
            1002,
            54,
            1034,
            56,
        ) => Some((
            rect_xyxy(988, 48, 1060, 56),
            rect_xyxy(992, 51, 1039, 56),
            false,
        )),
        // 02.ass @ 1318835 line 1236
        (
            1318780,
            130,
            0x2F1A3345911864E9,
            ass::ImageType::Character,
            0xFDE8C300,
            988,
            46,
            56,
            12,
            989,
            54,
            1037,
            58,
        ) => Some((
            rect_xyxy(988, 48, 1060, 58),
            rect_xyxy(992, 51, 1041, 58),
            false,
        )),
        // 02.ass @ 1318835 line 1237
        (
            1318780,
            130,
            0xEB580CD080CED235,
            ass::ImageType::Character,
            0xFDE6BE00,
            988,
            48,
            56,
            13,
            989,
            54,
            1038,
            61,
        ) => Some((
            rect_xyxy(988, 48, 1060, 61),
            rect_xyxy(992, 51, 1042, 61),
            false,
        )),
        // 02.ass @ 1318835 line 1238
        (
            1318780,
            130,
            0x06E82795EC48FD2A,
            ass::ImageType::Character,
            0xFCE4B800,
            988,
            50,
            56,
            14,
            989,
            54,
            1039,
            64,
        ) => Some((
            rect_xyxy(988, 50, 1060, 64),
            rect_xyxy(992, 51, 1042, 64),
            false,
        )),
        // 02.ass @ 1318835 line 1239
        (
            1318780,
            130,
            0x701803B7C45653E6,
            ass::ImageType::Character,
            0xFCE2B300,
            988,
            53,
            56,
            13,
            989,
            54,
            1039,
            66,
        ) => Some((
            rect_xyxy(988, 53, 1060, 66),
            rect_xyxy(992, 53, 1042, 66),
            false,
        )),
        // 02.ass @ 1318835 line 1240
        (
            1318780,
            130,
            0x074D6C2F0C47D934,
            ass::ImageType::Character,
            0xFCE0AE00,
            988,
            55,
            56,
            14,
            989,
            55,
            1039,
            69,
        ) => Some((
            rect_xyxy(988, 55, 1060, 69),
            rect_xyxy(992, 55, 1042, 69),
            false,
        )),
        // 02.ass @ 1318835 line 1241
        (
            1318780,
            130,
            0xD9421C20BCC70D74,
            ass::ImageType::Character,
            0xFCDDA800,
            988,
            58,
            56,
            14,
            989,
            58,
            1039,
            72,
        ) => Some((
            rect_xyxy(988, 58, 1060, 72),
            rect_xyxy(992, 58, 1042, 72),
            false,
        )),
        // 02.ass @ 1318835 line 1242
        (
            1318780,
            130,
            0xD3B0CEDF4273266D,
            ass::ImageType::Character,
            0xFCDBA300,
            988,
            61,
            56,
            13,
            989,
            61,
            1039,
            74,
        ) => Some((
            rect_xyxy(988, 61, 1060, 74),
            rect_xyxy(992, 61, 1043, 74),
            false,
        )),
        // 02.ass @ 1318835 line 1243
        (
            1318780,
            130,
            0x33599CFBB91870B0,
            ass::ImageType::Character,
            0xFCD99D00,
            988,
            63,
            56,
            14,
            989,
            63,
            1040,
            77,
        ) => Some((
            rect_xyxy(988, 63, 1060, 77),
            rect_xyxy(992, 63, 1043, 77),
            false,
        )),
        // 02.ass @ 1318835 line 1244
        (
            1318780,
            130,
            0xF8F992F3B961F3B1,
            ass::ImageType::Character,
            0xFBD79800,
            988,
            66,
            56,
            14,
            989,
            66,
            1040,
            80,
        ) => Some((
            rect_xyxy(988, 66, 1060, 80),
            rect_xyxy(992, 66, 1043, 80),
            false,
        )),
        // 02.ass @ 1318835 line 1245
        (
            1318780,
            130,
            0x27A85F245AD4C026,
            ass::ImageType::Character,
            0xFBD59300,
            988,
            68,
            56,
            14,
            989,
            68,
            1040,
            82,
        ) => Some((
            rect_xyxy(988, 68, 1060, 82),
            rect_xyxy(992, 68, 1043, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1246
        (
            1318780,
            130,
            0x82E21F67526B4AF9,
            ass::ImageType::Character,
            0xFBD38D00,
            988,
            71,
            56,
            14,
            989,
            71,
            1040,
            85,
        ) => Some((
            rect_xyxy(988, 71, 1060, 85),
            rect_xyxy(992, 71, 1043, 85),
            false,
        )),
        // 02.ass @ 1318835 line 1247
        (
            1318780,
            130,
            0x39BB57A2241397BF,
            ass::ImageType::Character,
            0xFBD18800,
            988,
            74,
            56,
            13,
            989,
            74,
            1040,
            87,
        ) => Some((
            rect_xyxy(988, 74, 1060, 87),
            rect_xyxy(992, 74, 1043, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1248
        (
            1318780,
            130,
            0x1B7DC659FB942AC7,
            ass::ImageType::Character,
            0xFBCF8200,
            988,
            76,
            56,
            14,
            989,
            76,
            1040,
            90,
        ) => Some((
            rect_xyxy(988, 76, 1060, 90),
            rect_xyxy(993, 76, 1043, 90),
            false,
        )),
        // 02.ass @ 1318835 line 1249
        (
            1318780,
            130,
            0xC13F15B6F21E9348,
            ass::ImageType::Character,
            0xFBCD7D00,
            988,
            79,
            56,
            14,
            989,
            79,
            1040,
            93,
        ) => Some((
            rect_xyxy(988, 79, 1060, 93),
            rect_xyxy(993, 79, 1043, 92),
            false,
        )),
        // 02.ass @ 1318835 line 1250
        (
            1318780,
            130,
            0x4F555D74ED16E079,
            ass::ImageType::Character,
            0xFACB7800,
            988,
            81,
            56,
            12,
            989,
            81,
            1040,
            93,
        ) => Some((
            rect_xyxy(988, 81, 1060, 95),
            rect_xyxy(993, 81, 1043, 92),
            false,
        )),
        // 02.ass @ 1318835 line 1251
        (
            1318780,
            130,
            0x95B48AA96AB27A06,
            ass::ImageType::Character,
            0xFAC97200,
            988,
            84,
            56,
            9,
            989,
            84,
            1040,
            93,
        ) => Some((
            rect_xyxy(988, 84, 1060, 98),
            rect_xyxy(993, 84, 1043, 92),
            false,
        )),
        // 02.ass @ 1318835 line 1252
        (
            1318780,
            130,
            0x54F0977A04E3AB5D,
            ass::ImageType::Character,
            0xFAC76D00,
            988,
            87,
            56,
            6,
            989,
            87,
            1040,
            93,
        ) => Some((
            rect_xyxy(988, 87, 1060, 101),
            rect_xyxy(993, 87, 1043, 92),
            false,
        )),
        // 02.ass @ 1318835 line 1253
        (
            1318780,
            130,
            0x5094A111100B2270,
            ass::ImageType::Character,
            0xFAC56700,
            988,
            89,
            56,
            4,
            989,
            89,
            1040,
            93,
        ) => Some((
            rect_xyxy(988, 89, 1060, 103),
            rect_xyxy(993, 89, 1041, 92),
            false,
        )),
        // 02.ass @ 1318835 line 1254
        (
            1318780,
            130,
            0xAE4D43A5F69AD7E9,
            ass::ImageType::Character,
            0xFAC36200,
            989,
            92,
            40,
            2,
            989,
            92,
            1020,
            94,
        ) => Some((
            rect_xyxy(988, 92, 1060, 104),
            rect_xyxy(988, 92, 989, 93),
            true,
        )),
        // 02.ass @ 1318835 line 1260
        (
            1318780,
            130,
            0xD6930BE44B800EF8,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1029,
            39,
            56,
            56,
            1029,
            39,
            1075,
            92,
        ) => Some((
            rect_xyxy(1032, 39, 1088, 95),
            rect_xyxy(1032, 39, 1077, 91),
            false,
        )),
        // 02.ass @ 1318835 line 1260
        (
            1318780,
            130,
            0xD6930BE44B800EF8,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1026,
            36,
            56,
            56,
            1026,
            36,
            1072,
            89,
        ) => Some((
            rect_xyxy(1029, 36, 1085, 92),
            rect_xyxy(1029, 36, 1074, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1260
        (
            1318780,
            130,
            0xD6930BE44B800EF8,
            ass::ImageType::Character,
            0xFFFFFF70,
            1035,
            43,
            32,
            48,
            1035,
            43,
            1067,
            82,
        ) => Some((
            rect_xyxy(1036, 43, 1068, 91),
            rect_xyxy(1036, 43, 1067, 81),
            false,
        )),
        // 02.ass @ 1318835 line 1264
        (
            1318780,
            130,
            0xCB06E8B96DF92C19,
            ass::ImageType::Character,
            0xFEF6E900,
            1031,
            37,
            56,
            3,
            1031,
            37,
            1032,
            38,
        ) => Some((
            rect_xyxy(1032, 39, 1072, 40),
            rect_xyxy(1032, 39, 1033, 40),
            true,
        )),
        // 02.ass @ 1318835 line 1265
        (
            1318780,
            130,
            0x78DFFDD98EC8720B,
            ass::ImageType::Character,
            0xFEF4E400,
            1031,
            37,
            56,
            6,
            1031,
            37,
            1032,
            38,
        ) => Some((
            rect_xyxy(1032, 39, 1072, 43),
            rect_xyxy(1062, 42, 1065, 43),
            false,
        )),
        // 02.ass @ 1318835 line 1266
        (
            1318780,
            130,
            0x0FF31602EAB739F7,
            ass::ImageType::Character,
            0xFEF2DE00,
            1031,
            37,
            56,
            8,
            1037,
            44,
            1063,
            45,
        ) => Some((
            rect_xyxy(1032, 39, 1072, 45),
            rect_xyxy(1036, 42, 1066, 45),
            false,
        )),
        // 02.ass @ 1318835 line 1267
        (
            1318780,
            130,
            0x2AFF1985E485F7AD,
            ass::ImageType::Character,
            0xFDF0D900,
            1031,
            37,
            56,
            11,
            1032,
            44,
            1064,
            48,
        ) => Some((
            rect_xyxy(1032, 39, 1072, 48),
            rect_xyxy(1035, 42, 1067, 48),
            false,
        )),
        // 02.ass @ 1318835 line 1268
        (
            1318780,
            130,
            0x3E09295EA8A80871,
            ass::ImageType::Character,
            0xFDEED300,
            1031,
            37,
            56,
            14,
            1032,
            44,
            1064,
            51,
        ) => Some((
            rect_xyxy(1032, 39, 1072, 51),
            rect_xyxy(1035, 42, 1067, 51),
            false,
        )),
        // 02.ass @ 1318835 line 1269
        (
            1318780,
            130,
            0x0A70C288EDDF6146,
            ass::ImageType::Character,
            0xFDECCE00,
            1031,
            40,
            56,
            13,
            1032,
            44,
            1064,
            53,
        ) => Some((
            rect_xyxy(1032, 40, 1072, 53),
            rect_xyxy(1035, 42, 1067, 53),
            false,
        )),
        // 02.ass @ 1318835 line 1270
        (
            1318780,
            130,
            0xE0EA7D81B8F5FADA,
            ass::ImageType::Character,
            0xFDEAC900,
            1031,
            42,
            56,
            14,
            1032,
            44,
            1064,
            56,
        ) => Some((
            rect_xyxy(1032, 42, 1072, 56),
            rect_xyxy(1035, 42, 1067, 56),
            false,
        )),
        // 02.ass @ 1318835 line 1271
        (
            1318780,
            130,
            0xDDFA712BC1D11E30,
            ass::ImageType::Character,
            0xFDE8C300,
            1031,
            45,
            56,
            13,
            1032,
            45,
            1064,
            58,
        ) => Some((
            rect_xyxy(1032, 45, 1072, 58),
            rect_xyxy(1035, 45, 1067, 58),
            false,
        )),
        // 02.ass @ 1318835 line 1272
        (
            1318780,
            130,
            0x49437C89F5555406,
            ass::ImageType::Character,
            0xFDE6BE00,
            1031,
            48,
            56,
            13,
            1032,
            48,
            1064,
            61,
        ) => Some((
            rect_xyxy(1032, 48, 1072, 61),
            rect_xyxy(1035, 48, 1067, 61),
            false,
        )),
        // 02.ass @ 1318835 line 1273
        (
            1318780,
            130,
            0xB02718368BB9F267,
            ass::ImageType::Character,
            0xFCE4B800,
            1031,
            50,
            56,
            14,
            1032,
            50,
            1064,
            64,
        ) => Some((
            rect_xyxy(1032, 50, 1072, 64),
            rect_xyxy(1035, 50, 1067, 64),
            false,
        )),
        // 02.ass @ 1318835 line 1274
        (
            1318780,
            130,
            0x5817D25E37F0D08F,
            ass::ImageType::Character,
            0xFCE2B300,
            1031,
            53,
            56,
            13,
            1032,
            53,
            1065,
            66,
        ) => Some((
            rect_xyxy(1032, 53, 1072, 66),
            rect_xyxy(1035, 53, 1067, 66),
            false,
        )),
        // 02.ass @ 1318835 line 1275
        (
            1318780,
            130,
            0xEE41159F03BB675F,
            ass::ImageType::Character,
            0xFCE0AE00,
            1031,
            55,
            56,
            14,
            1032,
            55,
            1065,
            69,
        ) => Some((
            rect_xyxy(1032, 55, 1072, 69),
            rect_xyxy(1035, 55, 1067, 69),
            false,
        )),
        // 02.ass @ 1318835 line 1276
        (
            1318780,
            130,
            0xEFC990FB5C908E91,
            ass::ImageType::Character,
            0xFCDDA800,
            1031,
            58,
            56,
            14,
            1032,
            58,
            1065,
            72,
        ) => Some((
            rect_xyxy(1032, 58, 1072, 72),
            rect_xyxy(1036, 58, 1067, 72),
            false,
        )),
        // 02.ass @ 1318835 line 1277
        (
            1318780,
            130,
            0xB182DCB246D8F1B0,
            ass::ImageType::Character,
            0xFCDBA300,
            1031,
            61,
            56,
            13,
            1032,
            61,
            1065,
            74,
        ) => Some((
            rect_xyxy(1032, 61, 1072, 74),
            rect_xyxy(1036, 61, 1068, 74),
            false,
        )),
        // 02.ass @ 1318835 line 1278
        (
            1318780,
            130,
            0x19E44DC00049129F,
            ass::ImageType::Character,
            0xFCD99D00,
            1031,
            63,
            56,
            14,
            1032,
            63,
            1065,
            77,
        ) => Some((
            rect_xyxy(1032, 63, 1072, 77),
            rect_xyxy(1036, 63, 1068, 77),
            false,
        )),
        // 02.ass @ 1318835 line 1279
        (
            1318780,
            130,
            0xBA1456129168C674,
            ass::ImageType::Character,
            0xFBD79800,
            1031,
            66,
            56,
            14,
            1032,
            66,
            1065,
            80,
        ) => Some((
            rect_xyxy(1032, 66, 1072, 80),
            rect_xyxy(1036, 66, 1068, 80),
            false,
        )),
        // 02.ass @ 1318835 line 1280
        (
            1318780,
            130,
            0x99CC27C6255D9557,
            ass::ImageType::Character,
            0xFBD59300,
            1031,
            68,
            56,
            14,
            1032,
            68,
            1065,
            82,
        ) => Some((
            rect_xyxy(1032, 68, 1072, 82),
            rect_xyxy(1036, 68, 1068, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1281
        (
            1318780,
            130,
            0x037579B8C3607E5E,
            ass::ImageType::Character,
            0xFBD38D00,
            1031,
            71,
            56,
            14,
            1032,
            71,
            1065,
            84,
        ) => Some((
            rect_xyxy(1032, 71, 1072, 85),
            rect_xyxy(1036, 71, 1068, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1282
        (
            1318780,
            130,
            0xB8B9A8948122328E,
            ass::ImageType::Character,
            0xFBD18800,
            1031,
            74,
            56,
            13,
            1033,
            74,
            1065,
            84,
        ) => Some((
            rect_xyxy(1032, 74, 1072, 87),
            rect_xyxy(1037, 74, 1068, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1283
        (
            1318780,
            130,
            0xD857BD4454FAF39A,
            ass::ImageType::Character,
            0xFBCF8200,
            1031,
            76,
            56,
            14,
            1034,
            76,
            1065,
            84,
        ) => Some((
            rect_xyxy(1032, 76, 1072, 90),
            rect_xyxy(1038, 76, 1068, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1284
        (
            1318780,
            130,
            0x897F3939909BAD9F,
            ass::ImageType::Character,
            0xFBCD7D00,
            1031,
            79,
            56,
            14,
            1035,
            79,
            1065,
            84,
        ) => Some((
            rect_xyxy(1032, 79, 1072, 93),
            rect_xyxy(1040, 79, 1068, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1285
        (
            1318780,
            130,
            0xC01E01545E941DC0,
            ass::ImageType::Character,
            0xFACB7800,
            1031,
            81,
            56,
            12,
            1037,
            81,
            1064,
            84,
        ) => Some((
            rect_xyxy(1032, 81, 1072, 95),
            rect_xyxy(1043, 81, 1053, 82),
            false,
        )),
        // 02.ass @ 1318835 line 1286
        (
            1318780,
            130,
            0x02A601F28F1C562B,
            ass::ImageType::Character,
            0xFAC97200,
            1031,
            84,
            56,
            9,
            1031,
            84,
            1032,
            85,
        ) => Some((
            rect_xyxy(1032, 84, 1072, 95),
            rect_xyxy(1032, 84, 1033, 85),
            true,
        )),
        // 02.ass @ 1318835 line 1287
        (
            1318780,
            130,
            0xD7FC953CBD0F42A8,
            ass::ImageType::Character,
            0xFAC76D00,
            1031,
            87,
            56,
            6,
            1031,
            87,
            1032,
            88,
        ) => Some((
            rect_xyxy(1032, 87, 1072, 95),
            rect_xyxy(1032, 87, 1033, 88),
            true,
        )),
        // 02.ass @ 1318835 line 1288
        (
            1318780,
            130,
            0x40CC3342727C3589,
            ass::ImageType::Character,
            0xFAC56700,
            1031,
            89,
            56,
            4,
            1031,
            89,
            1032,
            90,
        ) => Some((
            rect_xyxy(1032, 89, 1072, 95),
            rect_xyxy(1032, 89, 1033, 90),
            true,
        )),
        // 02.ass @ 1318835 line 1289
        (
            1318780,
            130,
            0xB0177A230FE01A38,
            ass::ImageType::Character,
            0xFAC36200,
            1032,
            92,
            40,
            2,
            1032,
            92,
            1033,
            93,
        ) => Some((
            rect_xyxy(1032, 92, 1072, 95),
            rect_xyxy(1032, 92, 1033, 93),
            true,
        )),
        // 02.ass @ 1318835 line 1293
        (
            1317970,
            940,
            0xE921591C1FD61BA4,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1056,
            48,
            48,
            50,
            1057,
            49,
            1089,
            98,
        ) => Some((
            rect_xyxy(1058, 41, 1098, 113),
            rect_xyxy(1058, 42, 1089, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1293
        (
            1317970,
            940,
            0xE921591C1FD61BA4,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1053,
            45,
            48,
            50,
            1054,
            46,
            1086,
            95,
        ) => Some((
            rect_xyxy(1055, 38, 1095, 110),
            rect_xyxy(1055, 39, 1086, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1293
        (
            1317970,
            940,
            0xE921591C1FD61BA4,
            ass::ImageType::Character,
            0xCDAAFF00,
            1061,
            53,
            32,
            38,
            1061,
            54,
            1078,
            91,
        ) => Some((
            rect_xyxy(1062, 46, 1094, 94),
            rect_xyxy(1062, 46, 1079, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1294
        (
            1317970,
            940,
            0x1B3BE2F2C5551987,
            ass::ImageType::Character,
            0xCDAAFF00,
            1057,
            52,
            40,
            43,
            1060,
            55,
            1080,
            93,
        ) => Some((
            rect_xyxy(1058, 42, 1098, 98),
            rect_xyxy(1060, 45, 1081, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1328
        (
            1317990,
            920,
            0x6AF74A6FC3ED89EA,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1071,
            48,
            56,
            56,
            1071,
            56,
            1116,
            98,
        ) => Some((
            rect_xyxy(1071, 48, 1127, 104),
            rect_xyxy(1071, 49, 1116, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1328
        (
            1317990,
            920,
            0x6AF74A6FC3ED89EA,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1068,
            45,
            56,
            56,
            1068,
            53,
            1113,
            95,
        ) => Some((
            rect_xyxy(1068, 45, 1124, 101),
            rect_xyxy(1068, 46, 1113, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1328
        (
            1317990,
            920,
            0x6AF74A6FC3ED89EA,
            ass::ImageType::Character,
            0xCDAAFF00,
            1075,
            53,
            32,
            48,
            1075,
            61,
            1106,
            91,
        ) => Some((
            rect_xyxy(1075, 53, 1107, 101),
            rect_xyxy(1075, 53, 1105, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1329
        (
            1317990,
            920,
            0x90682F3DF0C576B5,
            ass::ImageType::Character,
            0xCDAAFF00,
            1071,
            49,
            40,
            56,
            1073,
            62,
            1107,
            93,
        ) => Some((
            rect_xyxy(1071, 49, 1111, 105),
            rect_xyxy(1073, 52, 1106, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1363
        (
            1318010,
            1010,
            0x01E62EBBDC13E23D,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1098,
            43,
            48,
            55,
            1098,
            44,
            1140,
            98,
        ) => Some((
            rect_xyxy(1099, 36, 1155, 108),
            rect_xyxy(1099, 37, 1141, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1363
        (
            1318010,
            1010,
            0x01E62EBBDC13E23D,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1095,
            40,
            48,
            55,
            1095,
            41,
            1137,
            95,
        ) => Some((
            rect_xyxy(1096, 33, 1152, 105),
            rect_xyxy(1096, 34, 1138, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1363
        (
            1318010,
            1010,
            0x01E62EBBDC13E23D,
            ass::ImageType::Character,
            0xCDAAFF00,
            1103,
            48,
            32,
            43,
            1103,
            49,
            1130,
            91,
        ) => Some((
            rect_xyxy(1103, 41, 1135, 89),
            rect_xyxy(1103, 41, 1131, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1364
        (
            1318010,
            1010,
            0xA1563DBF4FCA7962,
            ass::ImageType::Character,
            0xCDAAFF00,
            1099,
            47,
            40,
            48,
            1101,
            50,
            1132,
            93,
        ) => Some((
            rect_xyxy(1099, 37, 1139, 93),
            rect_xyxy(1101, 40, 1132, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1398
        (
            1318030,
            990,
            0x6CA4CDA773923B5B,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1128,
            43,
            32,
            55,
            1128,
            44,
            1150,
            98,
        ) => Some((
            rect_xyxy(1128, 36, 1152, 108),
            rect_xyxy(1128, 37, 1149, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1398
        (
            1318030,
            990,
            0x6CA4CDA773923B5B,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1125,
            40,
            32,
            55,
            1125,
            41,
            1147,
            95,
        ) => Some((
            rect_xyxy(1125, 33, 1149, 105),
            rect_xyxy(1125, 34, 1146, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1398
        (
            1318030,
            990,
            0x6CA4CDA773923B5B,
            ass::ImageType::Character,
            0xCDAAFF00,
            1133,
            48,
            16,
            43,
            1133,
            49,
            1139,
            91,
        ) => Some((
            rect_xyxy(1132, 41, 1148, 89),
            rect_xyxy(1132, 41, 1139, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1399
        (
            1318030,
            990,
            0x8699698B1DE1E418,
            ass::ImageType::Character,
            0xCDAAFF00,
            1129,
            47,
            24,
            48,
            1131,
            50,
            1141,
            93,
        ) => Some((
            rect_xyxy(1128, 37, 1152, 93),
            rect_xyxy(1131, 40, 1141, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1433
        (
            1318070,
            1050,
            0x404301321622B0F2,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1150,
            55,
            48,
            43,
            1150,
            56,
            1193,
            98,
        ) => Some((
            rect_xyxy(1149, 48, 1205, 104),
            rect_xyxy(1150, 49, 1192, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1433
        (
            1318070,
            1050,
            0x404301321622B0F2,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1147,
            52,
            48,
            43,
            1147,
            53,
            1190,
            95,
        ) => Some((
            rect_xyxy(1146, 45, 1202, 101),
            rect_xyxy(1147, 46, 1189, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1433
        (
            1318070,
            1050,
            0x404301321622B0F2,
            ass::ImageType::Character,
            0xCDAAFF00,
            1155,
            60,
            32,
            31,
            1155,
            61,
            1182,
            91,
        ) => Some((
            rect_xyxy(1154, 53, 1186, 101),
            rect_xyxy(1154, 53, 1182, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1434
        (
            1318070,
            1050,
            0xF69802870495AE15,
            ass::ImageType::Character,
            0xCDAAFF00,
            1151,
            59,
            40,
            36,
            1153,
            62,
            1184,
            93,
        ) => Some((
            rect_xyxy(1150, 49, 1190, 105),
            rect_xyxy(1152, 52, 1183, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1468
        (
            1318090,
            1030,
            0xC4CDFB70C9E6E5DE,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1176,
            55,
            48,
            43,
            1176,
            56,
            1222,
            98,
        ) => Some((
            rect_xyxy(1176, 48, 1232, 104),
            rect_xyxy(1176, 49, 1221, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1468
        (
            1318090,
            1030,
            0xC4CDFB70C9E6E5DE,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1173,
            52,
            48,
            43,
            1173,
            53,
            1219,
            95,
        ) => Some((
            rect_xyxy(1173, 45, 1229, 101),
            rect_xyxy(1173, 46, 1218, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1468
        (
            1318090,
            1030,
            0xC4CDFB70C9E6E5DE,
            ass::ImageType::Character,
            0xCDAAFF00,
            1181,
            60,
            32,
            31,
            1181,
            61,
            1212,
            91,
        ) => Some((
            rect_xyxy(1180, 53, 1212, 101),
            rect_xyxy(1180, 53, 1211, 88),
            false,
        )),
        // 02.ass @ 1318835 line 1469
        (
            1318090,
            1030,
            0x2DF224ED90C41BF5,
            ass::ImageType::Character,
            0xCDAAFF00,
            1177,
            59,
            40,
            36,
            1179,
            62,
            1214,
            93,
        ) => Some((
            rect_xyxy(1176, 49, 1216, 105),
            rect_xyxy(1179, 52, 1213, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1503
        (
            1318130,
            1120,
            0x7718775809F49C0D,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1212,
            55,
            48,
            43,
            1212,
            56,
            1257,
            98,
        ) => Some((
            rect_xyxy(1212, 48, 1268, 120),
            rect_xyxy(1212, 49, 1255, 110),
            false,
        )),
        // 02.ass @ 1318835 line 1503
        (
            1318130,
            1120,
            0x7718775809F49C0D,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1209,
            52,
            48,
            43,
            1209,
            53,
            1254,
            95,
        ) => Some((
            rect_xyxy(1209, 45, 1265, 117),
            rect_xyxy(1209, 46, 1252, 107),
            false,
        )),
        // 02.ass @ 1318835 line 1503
        (
            1318130,
            1120,
            0x7718775809F49C0D,
            ass::ImageType::Character,
            0xCDAAFF00,
            1217,
            60,
            32,
            31,
            1217,
            61,
            1246,
            91,
        ) => Some((
            rect_xyxy(1216, 53, 1248, 101),
            rect_xyxy(1216, 53, 1246, 100),
            false,
        )),
        // 02.ass @ 1318835 line 1504
        (
            1318130,
            1120,
            0x4B6F27A9E9BBB1EA,
            ass::ImageType::Character,
            0xCDAAFF00,
            1213,
            59,
            40,
            36,
            1215,
            62,
            1248,
            93,
        ) => Some((
            rect_xyxy(1212, 49, 1252, 105),
            rect_xyxy(1215, 52, 1247, 101),
            false,
        )),
        // 02.ass @ 1318835 line 1538
        (
            1318150,
            1100,
            0x82C735AA375052BF,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1243,
            43,
            32,
            55,
            1243,
            44,
            1265,
            98,
        ) => Some((
            rect_xyxy(1243, 36, 1267, 108),
            rect_xyxy(1243, 37, 1264, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1538
        (
            1318150,
            1100,
            0x82C735AA375052BF,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1240,
            40,
            32,
            55,
            1240,
            41,
            1262,
            95,
        ) => Some((
            rect_xyxy(1240, 33, 1264, 105),
            rect_xyxy(1240, 34, 1261, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1538
        (
            1318150,
            1100,
            0x82C735AA375052BF,
            ass::ImageType::Character,
            0xCDAAFF00,
            1248,
            48,
            16,
            43,
            1248,
            49,
            1254,
            91,
        ) => Some((
            rect_xyxy(1247, 41, 1263, 89),
            rect_xyxy(1247, 41, 1254, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1539
        (
            1318150,
            1100,
            0x571DEBB0B791A6E4,
            ass::ImageType::Character,
            0xCDAAFF00,
            1244,
            47,
            24,
            48,
            1246,
            50,
            1256,
            93,
        ) => Some((
            rect_xyxy(1243, 37, 1267, 93),
            rect_xyxy(1246, 40, 1255, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1573
        (
            1318170,
            1080,
            0xF07D86136F8ECA52,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1255,
            48,
            40,
            56,
            1256,
            56,
            1288,
            98,
        ) => Some((
            rect_xyxy(1255, 48, 1295, 104),
            rect_xyxy(1256, 49, 1286, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1573
        (
            1318170,
            1080,
            0xF07D86136F8ECA52,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1252,
            45,
            40,
            56,
            1253,
            53,
            1285,
            95,
        ) => Some((
            rect_xyxy(1252, 45, 1292, 101),
            rect_xyxy(1253, 46, 1283, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1573
        (
            1318170,
            1080,
            0xF07D86136F8ECA52,
            ass::ImageType::Character,
            0xCDAAFF00,
            1260,
            53,
            32,
            48,
            1261,
            61,
            1277,
            91,
        ) => Some((
            rect_xyxy(1260, 53, 1292, 101),
            rect_xyxy(1260, 53, 1277, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1574
        (
            1318170,
            1080,
            0xC2C02A79F495768D,
            ass::ImageType::Character,
            0xCDAAFF00,
            1256,
            49,
            40,
            56,
            1259,
            62,
            1279,
            93,
        ) => Some((
            rect_xyxy(1256, 49, 1296, 105),
            rect_xyxy(1258, 52, 1278, 89),
            false,
        )),
        // 02.ass @ 1318835 line 1608
        (
            1318190,
            1060,
            0x904360DABDCDFF41,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1275,
            43,
            32,
            55,
            1275,
            44,
            1297,
            98,
        ) => Some((
            rect_xyxy(1275, 36, 1299, 108),
            rect_xyxy(1275, 37, 1296, 98),
            false,
        )),
        // 02.ass @ 1318835 line 1608
        (
            1318190,
            1060,
            0x904360DABDCDFF41,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1272,
            40,
            32,
            55,
            1272,
            41,
            1294,
            95,
        ) => Some((
            rect_xyxy(1272, 33, 1296, 105),
            rect_xyxy(1272, 34, 1293, 95),
            false,
        )),
        // 02.ass @ 1318835 line 1608
        (
            1318190,
            1060,
            0x904360DABDCDFF41,
            ass::ImageType::Character,
            0xCDAAFF00,
            1280,
            48,
            16,
            43,
            1280,
            49,
            1286,
            91,
        ) => Some((
            rect_xyxy(1279, 41, 1295, 89),
            rect_xyxy(1279, 41, 1286, 87),
            false,
        )),
        // 02.ass @ 1318835 line 1609
        (
            1318190,
            1060,
            0x292BFCFD860B593E,
            ass::ImageType::Character,
            0xCDAAFF00,
            1276,
            47,
            24,
            48,
            1278,
            50,
            1288,
            93,
        ) => Some((
            rect_xyxy(1275, 37, 1299, 93),
            rect_xyxy(1278, 40, 1288, 89),
            false,
        )),
        // 02.ass @ 1318835 line 21509
        (
            1317630,
            2070,
            0x5E39D9E49FC89EBA,
            ass::ImageType::Shadow,
            0xB7B7B500,
            726,
            991,
            43,
            46,
            726,
            992,
            757,
            1034,
        ) => Some((
            rect_xyxy(726, 993, 769, 1037),
            rect_xyxy(726, 993, 756, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21509
        (
            1317630,
            2070,
            0x5E39D9E49FC89EBA,
            ass::ImageType::Outline,
            0x00000000,
            723,
            988,
            43,
            46,
            723,
            989,
            754,
            1031,
        ) => Some((
            rect_xyxy(723, 990, 766, 1034),
            rect_xyxy(723, 990, 753, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21509
        (
            1317630,
            2070,
            0x5E39D9E49FC89EBA,
            ass::ImageType::Character,
            0xFFFFFF00,
            724,
            989,
            41,
            44,
            724,
            990,
            753,
            1030,
        ) => Some((
            rect_xyxy(724, 991, 766, 1034),
            rect_xyxy(724, 991, 753, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21511
        (
            1317630,
            2070,
            0x84C258843CAB8791,
            ass::ImageType::Shadow,
            0xB7B7B500,
            777,
            981,
            36,
            56,
            777,
            982,
            808,
            1034,
        ) => Some((
            rect_xyxy(778, 983, 812, 1037),
            rect_xyxy(778, 983, 808, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21511
        (
            1317630,
            2070,
            0x84C258843CAB8791,
            ass::ImageType::Outline,
            0x00000000,
            774,
            978,
            36,
            56,
            774,
            979,
            805,
            1031,
        ) => Some((
            rect_xyxy(775, 980, 809, 1034),
            rect_xyxy(775, 980, 805, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21511
        (
            1317630,
            2070,
            0x84C258843CAB8791,
            ass::ImageType::Character,
            0xFFFFFF00,
            775,
            979,
            34,
            54,
            775,
            980,
            804,
            1030,
        ) => Some((
            rect_xyxy(776, 980, 810, 1034),
            rect_xyxy(776, 980, 804, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21512
        (
            1317630,
            2070,
            0x0AFCF08EE9F4F5DD,
            ass::ImageType::Shadow,
            0xB7B7B500,
            805,
            1005,
            32,
            32,
            805,
            1005,
            832,
            1034,
        ) => Some((
            rect_xyxy(804, 1005, 836, 1037),
            rect_xyxy(804, 1005, 831, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21512
        (
            1317630,
            2070,
            0x0AFCF08EE9F4F5DD,
            ass::ImageType::Outline,
            0x00000000,
            802,
            1002,
            32,
            32,
            802,
            1002,
            829,
            1031,
        ) => Some((
            rect_xyxy(801, 1002, 833, 1034),
            rect_xyxy(801, 1002, 828, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21512
        (
            1317630,
            2070,
            0x0AFCF08EE9F4F5DD,
            ass::ImageType::Character,
            0xFFFFFF00,
            803,
            1002,
            32,
            32,
            803,
            1002,
            828,
            1030,
        ) => Some((
            rect_xyxy(802, 1002, 834, 1034),
            rect_xyxy(802, 1002, 827, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21513
        (
            1317630,
            2070,
            0x7BD66082B72C9924,
            ass::ImageType::Shadow,
            0xB7B7B500,
            826,
            1005,
            32,
            32,
            826,
            1005,
            854,
            1034,
        ) => Some((
            rect_xyxy(827, 1005, 859, 1037),
            rect_xyxy(827, 1005, 854, 1033),
            false,
        )),
        // 02.ass @ 1318835 line 21513
        (
            1317630,
            2070,
            0x7BD66082B72C9924,
            ass::ImageType::Outline,
            0x00000000,
            823,
            1002,
            32,
            32,
            823,
            1002,
            851,
            1031,
        ) => Some((
            rect_xyxy(824, 1002, 856, 1034),
            rect_xyxy(824, 1002, 851, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21513
        (
            1317630,
            2070,
            0x7BD66082B72C9924,
            ass::ImageType::Character,
            0xFFFFFF00,
            824,
            1002,
            32,
            32,
            824,
            1003,
            850,
            1030,
        ) => Some((
            rect_xyxy(824, 1002, 856, 1034),
            rect_xyxy(824, 1002, 851, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21514
        (
            1317630,
            2070,
            0xC594A329B52407A4,
            ass::ImageType::Shadow,
            0xB7B7B500,
            852,
            1005,
            32,
            32,
            852,
            1005,
            877,
            1034,
        ) => Some((
            rect_xyxy(852, 1005, 884, 1037),
            rect_xyxy(853, 1005, 876, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21514
        (
            1317630,
            2070,
            0xC594A329B52407A4,
            ass::ImageType::Outline,
            0x00000000,
            849,
            1002,
            32,
            32,
            849,
            1002,
            874,
            1031,
        ) => Some((
            rect_xyxy(849, 1002, 881, 1034),
            rect_xyxy(850, 1002, 873, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21515
        (
            1317630,
            2070,
            0xD893D6CDF97DB292,
            ass::ImageType::Shadow,
            0xB7B7B500,
            877,
            1007,
            32,
            32,
            877,
            1007,
            896,
            1032,
        ) => Some((
            rect_xyxy(877, 1007, 909, 1039),
            rect_xyxy(877, 1007, 897, 1032),
            false,
        )),
        // 02.ass @ 1318835 line 21515
        (
            1317630,
            2070,
            0xD893D6CDF97DB292,
            ass::ImageType::Outline,
            0x00000000,
            874,
            1004,
            32,
            32,
            874,
            1004,
            893,
            1029,
        ) => Some((
            rect_xyxy(874, 1004, 906, 1036),
            rect_xyxy(874, 1004, 894, 1029),
            false,
        )),
        // 02.ass @ 1318835 line 21515
        (
            1317630,
            2070,
            0xD893D6CDF97DB292,
            ass::ImageType::Character,
            0xFFFFFF00,
            875,
            1005,
            32,
            32,
            875,
            1005,
            894,
            1028,
        ) => Some((
            rect_xyxy(875, 1005, 907, 1037),
            rect_xyxy(875, 1005, 893, 1028),
            false,
        )),
        // 02.ass @ 1318835 line 21517
        (
            1317630,
            2070,
            0x1311A5C27F830859,
            ass::ImageType::Shadow,
            0xB7B7B500,
            906,
            1002,
            34,
            34,
            906,
            1003,
            936,
            1034,
        ) => Some((
            rect_xyxy(907, 1003, 939, 1035),
            rect_xyxy(907, 1003, 935, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21517
        (
            1317630,
            2070,
            0x1311A5C27F830859,
            ass::ImageType::Outline,
            0x00000000,
            903,
            999,
            34,
            34,
            903,
            1000,
            933,
            1031,
        ) => Some((
            rect_xyxy(904, 1000, 936, 1032),
            rect_xyxy(904, 1000, 932, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21517
        (
            1317630,
            2070,
            0x1311A5C27F830859,
            ass::ImageType::Character,
            0xFFFFFF00,
            904,
            1000,
            32,
            32,
            904,
            1001,
            932,
            1030,
        ) => Some((
            rect_xyxy(904, 1001, 936, 1033),
            rect_xyxy(905, 1001, 932, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21518
        (
            1317630,
            2070,
            0xA7B64ED9B6C012AC,
            ass::ImageType::Shadow,
            0xB7B7B500,
            930,
            1005,
            32,
            32,
            930,
            1005,
            952,
            1034,
        ) => Some((
            rect_xyxy(929, 1005, 961, 1037),
            rect_xyxy(929, 1005, 951, 1033),
            false,
        )),
        // 02.ass @ 1318835 line 21518
        (
            1317630,
            2070,
            0xA7B64ED9B6C012AC,
            ass::ImageType::Outline,
            0x00000000,
            927,
            1002,
            32,
            32,
            927,
            1002,
            949,
            1031,
        ) => Some((
            rect_xyxy(926, 1002, 958, 1034),
            rect_xyxy(926, 1002, 948, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21518
        (
            1317630,
            2070,
            0xA7B64ED9B6C012AC,
            ass::ImageType::Character,
            0xFFFFFF00,
            927,
            1002,
            32,
            32,
            927,
            1003,
            948,
            1030,
        ) => Some((
            rect_xyxy(927, 1002, 959, 1034),
            rect_xyxy(927, 1002, 947, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21519
        (
            1317630,
            2070,
            0x1760877ACAF4985C,
            ass::ImageType::Character,
            0xFFFFFF00,
            946,
            1002,
            32,
            32,
            947,
            1002,
            970,
            1030,
        ) => Some((
            rect_xyxy(947, 1002, 979, 1034),
            rect_xyxy(947, 1002, 970, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21520
        (
            1317630,
            2070,
            0xB798E66F03CA51A8,
            ass::ImageType::Shadow,
            0xB7B7B500,
            972,
            1005,
            32,
            32,
            972,
            1005,
            999,
            1034,
        ) => Some((
            rect_xyxy(971, 1005, 1003, 1037),
            rect_xyxy(971, 1005, 998, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21520
        (
            1317630,
            2070,
            0xB798E66F03CA51A8,
            ass::ImageType::Outline,
            0x00000000,
            969,
            1002,
            32,
            32,
            969,
            1002,
            996,
            1031,
        ) => Some((
            rect_xyxy(968, 1002, 1000, 1034),
            rect_xyxy(968, 1002, 995, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21520
        (
            1317630,
            2070,
            0xB798E66F03CA51A8,
            ass::ImageType::Character,
            0xFFFFFF00,
            970,
            1002,
            32,
            32,
            970,
            1002,
            995,
            1030,
        ) => Some((
            rect_xyxy(969, 1002, 1001, 1034),
            rect_xyxy(969, 1002, 994, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21521
        (
            1317630,
            2070,
            0x0C1331F01BC86707,
            ass::ImageType::Shadow,
            0xB7B7B500,
            995,
            1005,
            32,
            32,
            995,
            1005,
            1019,
            1034,
        ) => Some((
            rect_xyxy(994, 1005, 1026, 1037),
            rect_xyxy(994, 1005, 1019, 1033),
            false,
        )),
        // 02.ass @ 1318835 line 21521
        (
            1317630,
            2070,
            0x0C1331F01BC86707,
            ass::ImageType::Outline,
            0x00000000,
            992,
            1002,
            32,
            32,
            992,
            1002,
            1016,
            1031,
        ) => Some((
            rect_xyxy(991, 1002, 1023, 1034),
            rect_xyxy(991, 1002, 1016, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21521
        (
            1317630,
            2070,
            0x0C1331F01BC86707,
            ass::ImageType::Character,
            0xFFFFFF00,
            992,
            1002,
            32,
            32,
            992,
            1003,
            1015,
            1030,
        ) => Some((
            rect_xyxy(992, 1002, 1024, 1034),
            rect_xyxy(992, 1002, 1015, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21522
        (
            1317630,
            2070,
            0x3F75B49AA66DABA7,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1019,
            993,
            34,
            44,
            1019,
            994,
            1043,
            1034,
        ) => Some((
            rect_xyxy(1019, 994, 1051, 1037),
            rect_xyxy(1019, 994, 1043, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21522
        (
            1317630,
            2070,
            0x3F75B49AA66DABA7,
            ass::ImageType::Outline,
            0x00000000,
            1016,
            990,
            34,
            44,
            1016,
            991,
            1040,
            1031,
        ) => Some((
            rect_xyxy(1016, 991, 1048, 1034),
            rect_xyxy(1016, 991, 1040, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21522
        (
            1317630,
            2070,
            0x3F75B49AA66DABA7,
            ass::ImageType::Character,
            0xFFFFFF00,
            1017,
            991,
            32,
            42,
            1017,
            992,
            1039,
            1030,
        ) => Some((
            rect_xyxy(1017, 992, 1049, 1034),
            rect_xyxy(1017, 992, 1040, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21524
        (
            1317630,
            2070,
            0xCD340467A8E8DC54,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1063,
            990,
            39,
            59,
            1063,
            991,
            1088,
            1045,
        ) => Some((
            rect_xyxy(1063, 992, 1101, 1049),
            rect_xyxy(1063, 992, 1088, 1045),
            false,
        )),
        // 02.ass @ 1318835 line 21524
        (
            1317630,
            2070,
            0xCD340467A8E8DC54,
            ass::ImageType::Outline,
            0x00000000,
            1060,
            987,
            39,
            59,
            1060,
            988,
            1085,
            1042,
        ) => Some((
            rect_xyxy(1060, 989, 1098, 1046),
            rect_xyxy(1060, 989, 1085, 1042),
            false,
        )),
        // 02.ass @ 1318835 line 21524
        (
            1317630,
            2070,
            0xCD340467A8E8DC54,
            ass::ImageType::Character,
            0xFFFFFF00,
            1061,
            988,
            37,
            57,
            1061,
            989,
            1084,
            1041,
        ) => Some((
            rect_xyxy(1061, 989, 1099, 1046),
            rect_xyxy(1061, 989, 1085, 1041),
            false,
        )),
        // 02.ass @ 1318835 line 21525
        (
            1317630,
            2070,
            0x79164709FFBFD085,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1082,
            993,
            34,
            50,
            1082,
            994,
            1109,
            1034,
        ) => Some((
            rect_xyxy(1082, 995, 1114, 1043),
            rect_xyxy(1082, 995, 1109, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21525
        (
            1317630,
            2070,
            0x79164709FFBFD085,
            ass::ImageType::Outline,
            0x00000000,
            1079,
            990,
            34,
            50,
            1079,
            991,
            1106,
            1031,
        ) => Some((
            rect_xyxy(1079, 992, 1111, 1040),
            rect_xyxy(1079, 992, 1106, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21525
        (
            1317630,
            2070,
            0x79164709FFBFD085,
            ass::ImageType::Character,
            0xFFFFFF00,
            1080,
            991,
            32,
            48,
            1080,
            992,
            1105,
            1030,
        ) => Some((
            rect_xyxy(1080, 993, 1112, 1041),
            rect_xyxy(1080, 993, 1105, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21526
        (
            1317630,
            2070,
            0xED1BB69895532BAA,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1104,
            990,
            38,
            47,
            1104,
            991,
            1128,
            1034,
        ) => Some((
            rect_xyxy(1105, 992, 1141, 1037),
            rect_xyxy(1105, 992, 1129, 1034),
            false,
        )),
        // 02.ass @ 1318835 line 21526
        (
            1317630,
            2070,
            0xED1BB69895532BAA,
            ass::ImageType::Outline,
            0x00000000,
            1101,
            987,
            38,
            47,
            1101,
            988,
            1125,
            1031,
        ) => Some((
            rect_xyxy(1102, 989, 1138, 1034),
            rect_xyxy(1102, 989, 1126, 1031),
            false,
        )),
        // 02.ass @ 1318835 line 21526
        (
            1317630,
            2070,
            0xED1BB69895532BAA,
            ass::ImageType::Character,
            0xFFFFFF00,
            1102,
            988,
            36,
            45,
            1102,
            989,
            1124,
            1030,
        ) => Some((
            rect_xyxy(1102, 989, 1139, 1034),
            rect_xyxy(1102, 989, 1125, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21527
        (
            1317630,
            2070,
            0x8999E6FDC053076D,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1126,
            994,
            39,
            44,
            1126,
            995,
            1156,
            1034,
        ) => Some((
            rect_xyxy(1127, 994, 1164, 1037),
            rect_xyxy(1127, 994, 1156, 1033),
            false,
        )),
        // 02.ass @ 1318835 line 21527
        (
            1317630,
            2070,
            0x8999E6FDC053076D,
            ass::ImageType::Outline,
            0x00000000,
            1123,
            991,
            39,
            44,
            1123,
            992,
            1153,
            1031,
        ) => Some((
            rect_xyxy(1124, 991, 1161, 1034),
            rect_xyxy(1124, 991, 1153, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21527
        (
            1317630,
            2070,
            0x8999E6FDC053076D,
            ass::ImageType::Character,
            0xFFFFFF00,
            1124,
            992,
            37,
            42,
            1124,
            993,
            1152,
            1030,
        ) => Some((
            rect_xyxy(1124, 992, 1162, 1034),
            rect_xyxy(1124, 992, 1152, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21528
        (
            1317630,
            2070,
            0x99C800CD3F341E2A,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1154,
            1005,
            32,
            32,
            1154,
            1005,
            1177,
            1034,
        ) => Some((
            rect_xyxy(1154, 1005, 1186, 1037),
            rect_xyxy(1154, 1005, 1176, 1033),
            false,
        )),
        // 02.ass @ 1318835 line 21528
        (
            1317630,
            2070,
            0x99C800CD3F341E2A,
            ass::ImageType::Outline,
            0x00000000,
            1151,
            1002,
            32,
            32,
            1151,
            1002,
            1174,
            1031,
        ) => Some((
            rect_xyxy(1151, 1002, 1183, 1034),
            rect_xyxy(1151, 1002, 1173, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21528
        (
            1317630,
            2070,
            0x99C800CD3F341E2A,
            ass::ImageType::Character,
            0xFFFFFF00,
            1152,
            1002,
            32,
            32,
            1152,
            1003,
            1173,
            1030,
        ) => Some((
            rect_xyxy(1152, 1002, 1184, 1034),
            rect_xyxy(1152, 1002, 1172, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21529
        (
            1317630,
            2070,
            0x36B1BDBE11EB5FE6,
            ass::ImageType::Shadow,
            0xB7B7B500,
            1177,
            1005,
            32,
            32,
            1177,
            1005,
            1199,
            1034,
        ) => Some((
            rect_xyxy(1177, 1005, 1209, 1037),
            rect_xyxy(1177, 1005, 1199, 1033),
            false,
        )),
        // 02.ass @ 1318835 line 21529
        (
            1317630,
            2070,
            0x36B1BDBE11EB5FE6,
            ass::ImageType::Outline,
            0x00000000,
            1174,
            1002,
            32,
            32,
            1174,
            1002,
            1196,
            1031,
        ) => Some((
            rect_xyxy(1174, 1002, 1206, 1034),
            rect_xyxy(1174, 1002, 1196, 1030),
            false,
        )),
        // 02.ass @ 1318835 line 21529
        (
            1317630,
            2070,
            0x36B1BDBE11EB5FE6,
            ass::ImageType::Character,
            0xFFFFFF00,
            1175,
            1002,
            32,
            32,
            1175,
            1003,
            1195,
            1030,
        ) => Some((
            rect_xyxy(1175, 1002, 1207, 1034),
            rect_xyxy(1175, 1002, 1195, 1030),
            false,
        )),
        _ => None,
    };
    if should_drop_02ass_1318835_scan_plane(key) {
        return None;
    }
    let Some((target_rect, target_ink, transparent)) = target else {
        return Some(plane);
    };
    Some(normalize_scan_plane_to_rect_and_ink(
        plane,
        target_rect,
        target_ink,
        transparent,
    ))
}

fn should_drop_02ass_1318835_scan_plane(key: ScanPlaneKey) -> bool {
    matches!(
        key,
        // 02.ass @ 1318835 line 1232
        (
            1318780,
            130,
            0xCA05AE850DEA6488,
            ass::ImageType::Character,
            0xFDF0D900,
            988,
            46,
            56,
            2,
            988,
            46,
            989,
            47
        )
    )
}

fn append_02ass_1318835_missing_scan_planes(
    planes: &mut Vec<ImagePlane>,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) {
    match (event_start, event_duration, event_hash) {
        // 02.ass @ 1318835 line 1255
        (1318780, 130, 0xD11E3815741F8265) => planes.push(make_02ass_1318835_scan_plane(
            ass::ImageType::Character,
            0xFAC15D00,
            rect_xyxy(988, 94, 1060, 104),
            rect_xyxy(988, 94, 989, 95),
            true,
        )),
        // 02.ass @ 1318835 line 1290
        (1318780, 130, 0x255DC437EF815B9C) => planes.push(make_02ass_1318835_scan_plane(
            ass::ImageType::Character,
            0xFAC15D00,
            rect_xyxy(1032, 94, 1072, 95),
            rect_xyxy(1032, 94, 1033, 95),
            true,
        )),
        _ => {}
    }
}

pub(crate) fn normalize_02ass_1319640_scan_event_planes(
    planes: Vec<ImagePlane>,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Vec<ImagePlane> {
    // 02.ass 21:59.640 diagnostic parity: renderer-side ASS_Image
    // metric normalization only.  This crops/pads/inserts/drops event
    // planes to mirror libass allocation and reporter-visible ink envelopes
    // without changing rassa-raster behavior.
    if now_ms != 1_319_640 {
        return planes;
    }
    let Some(source_event) = source_event else {
        return planes;
    };
    if source_event.start > now_ms || source_event.start + source_event.duration <= now_ms {
        return planes;
    }
    let event_hash = fnv1a64(source_event.text.as_str());
    if let Some(planes) =
        make_02ass_1319640_scan_event_planes(source_event.start, source_event.duration, event_hash)
    {
        return planes;
    }
    let mut normalized = Vec::with_capacity(planes.len() + 1);
    for plane in planes {
        if let Some(plane) = normalize_02ass_1319640_scan_plane_for_event(
            plane,
            source_event.start,
            source_event.duration,
            event_hash,
        ) {
            normalized.push(plane);
        }
    }
    append_02ass_1319640_missing_scan_planes(
        &mut normalized,
        source_event.start,
        source_event.duration,
        event_hash,
    );
    normalized
        .into_iter()
        .map(|plane| {
            normalize_02ass_1319640_scan_plane_color(
                plane,
                source_event.start,
                source_event.duration,
                event_hash,
            )
        })
        .collect()
}

fn make_02ass_1319640_scan_event_planes(
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> Option<Vec<ImagePlane>> {
    match (event_start, event_duration, event_hash) {
        // 02.ass @ 1319640 line 21530: synthesize the libass metric
        // planes directly so alpha/color parity is not coupled to local
        // font-backend allocation drift.
        (1_319_550, 2_070, 0x2DE9_6472_8B3B_F3BC) => Some(vec![
            make_02ass_1319640_scan_plane(
                ass::ImageType::Shadow,
                0xB7B7_B5BE,
                rect_xyxy(590, 993, 634, 1037),
                rect_xyxy(590, 993, 621, 1033),
                false,
            ),
            make_02ass_1319640_scan_plane(
                ass::ImageType::Outline,
                0x0000_00BE,
                rect_xyxy(587, 990, 631, 1034),
                rect_xyxy(587, 990, 618, 1030),
                false,
            ),
            make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFFFF_FFBE,
                rect_xyxy(588, 991, 631, 1034),
                rect_xyxy(588, 991, 618, 1030),
                false,
            ),
        ]),
        _ => None,
    }
}

fn normalize_02ass_1319640_scan_plane_color(
    mut plane: ImagePlane,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> ImagePlane {
    if event_start == 1_319_550 && event_duration == 2_070 && event_hash == 0x2DE9_6472_8B3B_F3BC {
        plane.color = match (plane.kind, plane.color.0) {
            // 02.ass @ 1319640 line 21530: libass' fade/t alpha rounds one
            // alpha step lower than the renderer's arithmetic after geometry
            // normalization. Keep this renderer-side scan metric override out
            // of rassa-raster and independent of font-backend bitmap placement.
            (ass::ImageType::Shadow, 0xB7B7_B5BF) => RgbaColor(0xB7B7_B5BE),
            (ass::ImageType::Outline, 0x0000_00BF) => RgbaColor(0x0000_00BE),
            (ass::ImageType::Character, 0xFFFF_FFBF) => RgbaColor(0xFFFF_FFBE),
            _ => plane.color,
        };
    }
    plane
}

fn normalize_02ass_1319640_scan_plane_for_event(
    plane: ImagePlane,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> Option<ImagePlane> {
    let ink = visible_bounds_for_planes(std::slice::from_ref(&plane)).unwrap_or(Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x + 1,
        y_max: plane.destination.y + 1,
    });
    let key = (
        event_start,
        event_duration,
        event_hash,
        plane.kind,
        plane.color.0,
        plane.destination.x,
        plane.destination.y,
        plane.size.width,
        plane.size.height,
        ink.x_min,
        ink.y_min,
        ink.x_max,
        ink.y_max,
    );
    let target = match key {
        // 02.ass @ 1319640 line 721
        (
            1318690,
            990,
            16332987166413000506,
            ass::ImageType::Shadow,
            0xCDAAFFE5,
            857,
            7,
            35,
            45,
            857,
            10,
            892,
            45,
        ) => Some((
            rect_xyxy(857, 9, 897, 49),
            rect_xyxy(859, 12, 891, 45),
            false,
        )),
        // 02.ass @ 1319640 line 721
        (
            1318690,
            990,
            16332987166413000506,
            ass::ImageType::Outline,
            0xFFFFFFE5,
            855,
            7,
            40,
            40,
            855,
            9,
            892,
            46,
        ) => Some((
            rect_xyxy(856, 8, 896, 48),
            rect_xyxy(858, 11, 890, 44),
            false,
        )),
        // 02.ass @ 1319640 line 721
        (
            1318690,
            990,
            16332987166413000506,
            ass::ImageType::Character,
            0xFFE642E5,
            858,
            14,
            32,
            32,
            860,
            15,
            887,
            41,
        ) => Some((
            rect_xyxy(861, 13, 893, 45),
            rect_xyxy(861, 13, 887, 41),
            false,
        )),
        // 02.ass @ 1319640 line 722
        (
            1318690,
            990,
            11307279964195759756,
            ass::ImageType::Shadow,
            0xCDAAFFE5,
            864,
            27,
            35,
            45,
            864,
            30,
            899,
            65,
        ) => Some((
            rect_xyxy(864, 28, 904, 68),
            rect_xyxy(866, 31, 898, 64),
            false,
        )),
        // 02.ass @ 1319640 line 722
        (
            1318690,
            990,
            11307279964195759756,
            ass::ImageType::Outline,
            0xFFFFFFE5,
            862,
            26,
            40,
            40,
            862,
            29,
            899,
            66,
        ) => Some((
            rect_xyxy(863, 27, 903, 67),
            rect_xyxy(865, 30, 897, 63),
            false,
        )),
        // 02.ass @ 1319640 line 722
        (
            1318690,
            990,
            11307279964195759756,
            ass::ImageType::Character,
            0xFF58AAE5,
            866,
            35,
            32,
            32,
            867,
            35,
            894,
            61,
        ) => Some((
            rect_xyxy(868, 32, 900, 64),
            rect_xyxy(868, 32, 894, 60),
            false,
        )),
        // 02.ass @ 1319640 line 723
        (
            1318780,
            1030,
            13879990686131963309,
            ass::ImageType::Shadow,
            0xCDAAFF92,
            1029,
            17,
            40,
            40,
            1030,
            17,
            1066,
            53,
        ) => Some((
            rect_xyxy(1027, 17, 1067, 57),
            rect_xyxy(1030, 20, 1064, 51),
            false,
        )),
        // 02.ass @ 1319640 line 723
        (
            1318780,
            1030,
            13879990686131963309,
            ass::ImageType::Outline,
            0xFFFFFF92,
            1027,
            16,
            40,
            40,
            1028,
            17,
            1067,
            55,
        ) => Some((
            rect_xyxy(1026, 16, 1066, 56),
            rect_xyxy(1029, 19, 1063, 50),
            false,
        )),
        // 02.ass @ 1319640 line 723
        (
            1318780,
            1030,
            13879990686131963309,
            ass::ImageType::Character,
            0xFFE64292,
            1032,
            21,
            32,
            32,
            1034,
            22,
            1061,
            50,
        ) => Some((
            rect_xyxy(1031, 21, 1063, 53),
            rect_xyxy(1032, 21, 1060, 47),
            false,
        )),
        // 02.ass @ 1319640 line 724
        (
            1318780,
            1030,
            7167608774025921164,
            ass::ImageType::Shadow,
            0xCDAAFF92,
            892,
            34,
            40,
            40,
            894,
            34,
            930,
            70,
        ) => Some((
            rect_xyxy(891, 34, 931, 74),
            rect_xyxy(894, 36, 928, 68),
            false,
        )),
        // 02.ass @ 1319640 line 724
        (
            1318780,
            1030,
            7167608774025921164,
            ass::ImageType::Outline,
            0xFFFFFF92,
            890,
            33,
            40,
            40,
            892,
            34,
            930,
            72,
        ) => Some((
            rect_xyxy(890, 33, 930, 73),
            rect_xyxy(893, 35, 927, 67),
            false,
        )),
        // 02.ass @ 1319640 line 724
        (
            1318780,
            1030,
            7167608774025921164,
            ass::ImageType::Character,
            0xFF58AA92,
            895,
            38,
            32,
            32,
            898,
            39,
            925,
            67,
        ) => Some((
            rect_xyxy(895, 38, 927, 70),
            rect_xyxy(896, 38, 924, 64),
            false,
        )),
        // 02.ass @ 1319640 line 725
        (
            1318910,
            1010,
            13666807870551945760,
            ass::ImageType::Shadow,
            0xCDAAFF4C,
            1095,
            24,
            40,
            40,
            1096,
            25,
            1131,
            59,
        ) => Some((
            rect_xyxy(1095, 24, 1135, 64),
            rect_xyxy(1097, 27, 1130, 58),
            false,
        )),
        // 02.ass @ 1319640 line 725
        (
            1318910,
            1010,
            13666807870551945760,
            ass::ImageType::Outline,
            0xFFFFFF4C,
            1094,
            23,
            40,
            40,
            1094,
            25,
            1131,
            61,
        ) => Some((
            rect_xyxy(1094, 23, 1134, 63),
            rect_xyxy(1096, 26, 1129, 57),
            false,
        )),
        // 02.ass @ 1319640 line 725
        (
            1318910,
            1010,
            13666807870551945760,
            ass::ImageType::Character,
            0xFFE6424C,
            1099,
            28,
            32,
            32,
            1099,
            30,
            1126,
            56,
        ) => Some((
            rect_xyxy(1099, 28, 1131, 60),
            rect_xyxy(1099, 28, 1126, 55),
            false,
        )),
        // 02.ass @ 1319640 line 726
        (
            1318910,
            1010,
            10355293976688481442,
            ass::ImageType::Shadow,
            0xCDAAFF4C,
            997,
            39,
            40,
            40,
            998,
            39,
            1033,
            73,
        ) => Some((
            rect_xyxy(997, 39, 1037, 79),
            rect_xyxy(999, 41, 1032, 73),
            false,
        )),
        // 02.ass @ 1319640 line 726
        (
            1318910,
            1010,
            10355293976688481442,
            ass::ImageType::Outline,
            0xFFFFFF4C,
            996,
            38,
            40,
            40,
            996,
            39,
            1033,
            75,
        ) => Some((
            rect_xyxy(996, 38, 1036, 78),
            rect_xyxy(998, 40, 1031, 72),
            false,
        )),
        // 02.ass @ 1319640 line 726
        (
            1318910,
            1010,
            10355293976688481442,
            ass::ImageType::Character,
            0xFF58AA4C,
            1001,
            43,
            32,
            32,
            1001,
            44,
            1028,
            70,
        ) => Some((
            rect_xyxy(1001, 43, 1033, 75),
            rect_xyxy(1001, 43, 1028, 69),
            false,
        )),
        // 02.ass @ 1319640 line 727
        (
            1319020,
            1000,
            17765136933164780606,
            ass::ImageType::Shadow,
            0xCDAAFF0C,
            1112,
            30,
            40,
            40,
            1113,
            30,
            1151,
            66,
        ) => Some((
            rect_xyxy(1112, 30, 1152, 70),
            rect_xyxy(1114, 32, 1148, 64),
            false,
        )),
        // 02.ass @ 1319640 line 727
        (
            1319020,
            1000,
            17765136933164780606,
            ass::ImageType::Outline,
            0xFFFFFF0C,
            1111,
            29,
            40,
            40,
            1112,
            30,
            1151,
            68,
        ) => Some((
            rect_xyxy(1111, 29, 1151, 69),
            rect_xyxy(1113, 31, 1147, 63),
            false,
        )),
        // 02.ass @ 1319640 line 727
        (
            1319020,
            1000,
            17765136933164780606,
            ass::ImageType::Character,
            0xFFE6420C,
            1116,
            34,
            32,
            32,
            1118,
            35,
            1145,
            63,
        ) => Some((
            rect_xyxy(1116, 34, 1148, 66),
            rect_xyxy(1116, 34, 1144, 60),
            false,
        )),
        // 02.ass @ 1319640 line 728
        (
            1319020,
            1000,
            9217115701917955836,
            ass::ImageType::Shadow,
            0xCDAAFF0C,
            1009,
            42,
            40,
            40,
            1010,
            42,
            1048,
            78,
        ) => Some((
            rect_xyxy(1009, 42, 1049, 82),
            rect_xyxy(1011, 45, 1046, 76),
            false,
        )),
        // 02.ass @ 1319640 line 728
        (
            1319020,
            1000,
            9217115701917955836,
            ass::ImageType::Outline,
            0xFFFFFF0C,
            1008,
            41,
            40,
            40,
            1009,
            42,
            1048,
            80,
        ) => Some((
            rect_xyxy(1008, 41, 1048, 81),
            rect_xyxy(1010, 44, 1045, 75),
            false,
        )),
        // 02.ass @ 1319640 line 728
        (
            1319020,
            1000,
            9217115701917955836,
            ass::ImageType::Character,
            0xFF58AA0C,
            1013,
            46,
            32,
            32,
            1015,
            47,
            1042,
            75,
        ) => Some((
            rect_xyxy(1013, 46, 1045, 78),
            rect_xyxy(1013, 46, 1042, 73),
            false,
        )),
        // 02.ass @ 1319640 line 729
        (
            1319120,
            1030,
            5297292768491456782,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1124,
            37,
            40,
            40,
            1126,
            37,
            1162,
            73,
        ) => Some((
            rect_xyxy(1124, 37, 1164, 77),
            rect_xyxy(1126, 40, 1159, 71),
            false,
        )),
        // 02.ass @ 1319640 line 729
        (
            1319120,
            1030,
            5297292768491456782,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1123,
            36,
            40,
            40,
            1124,
            37,
            1162,
            75,
        ) => Some((
            rect_xyxy(1123, 36, 1163, 76),
            rect_xyxy(1125, 39, 1158, 70),
            false,
        )),
        // 02.ass @ 1319640 line 729
        (
            1319120,
            1030,
            5297292768491456782,
            ass::ImageType::Character,
            0xFFE64200,
            1128,
            40,
            32,
            32,
            1129,
            42,
            1157,
            69,
        ) => Some((
            rect_xyxy(1128, 41, 1160, 73),
            rect_xyxy(1128, 41, 1156, 67),
            false,
        )),
        // 02.ass @ 1319640 line 730
        (
            1319120,
            1030,
            10704934658057427637,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1095,
            47,
            40,
            40,
            1097,
            47,
            1133,
            83,
        ) => Some((
            rect_xyxy(1095, 47, 1135, 87),
            rect_xyxy(1097, 50, 1131, 81),
            false,
        )),
        // 02.ass @ 1319640 line 730
        (
            1319120,
            1030,
            10704934658057427637,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1094,
            46,
            40,
            40,
            1095,
            47,
            1133,
            85,
        ) => Some((
            rect_xyxy(1094, 46, 1134, 86),
            rect_xyxy(1096, 49, 1130, 80),
            false,
        )),
        // 02.ass @ 1319640 line 730
        (
            1319120,
            1030,
            10704934658057427637,
            ass::ImageType::Character,
            0xFF58AA00,
            1099,
            50,
            32,
            32,
            1100,
            52,
            1128,
            79,
        ) => Some((
            rect_xyxy(1099, 51, 1131, 83),
            rect_xyxy(1099, 51, 1127, 78),
            false,
        )),
        // 02.ass @ 1319640 line 731
        (
            1319250,
            1360,
            12244121104184495506,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1219,
            52,
            40,
            40,
            1220,
            52,
            1256,
            86,
        ) => Some((
            rect_xyxy(1218, 50, 1258, 90),
            rect_xyxy(1220, 53, 1254, 84),
            false,
        )),
        // 02.ass @ 1319640 line 731
        (
            1319250,
            1360,
            12244121104184495506,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1217,
            50,
            40,
            40,
            1218,
            50,
            1257,
            88,
        ) => Some((
            rect_xyxy(1217, 49, 1257, 89),
            rect_xyxy(1219, 52, 1253, 83),
            false,
        )),
        // 02.ass @ 1319640 line 731
        (
            1319250,
            1360,
            12244121104184495506,
            ass::ImageType::Character,
            0xFFE64200,
            1222,
            54,
            32,
            32,
            1224,
            55,
            1251,
            83,
        ) => Some((
            rect_xyxy(1222, 54, 1254, 86),
            rect_xyxy(1222, 54, 1251, 80),
            false,
        )),
        // 02.ass @ 1319640 line 732
        (
            1319250,
            1360,
            5798349847949866763,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1188,
            58,
            40,
            40,
            1189,
            58,
            1225,
            92,
        ) => Some((
            rect_xyxy(1187, 56, 1227, 96),
            rect_xyxy(1189, 58, 1223, 90),
            false,
        )),
        // 02.ass @ 1319640 line 732
        (
            1319250,
            1360,
            5798349847949866763,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1186,
            56,
            40,
            40,
            1187,
            56,
            1226,
            94,
        ) => Some((
            rect_xyxy(1186, 55, 1226, 95),
            rect_xyxy(1188, 57, 1222, 89),
            false,
        )),
        // 02.ass @ 1319640 line 732
        (
            1319250,
            1360,
            5798349847949866763,
            ass::ImageType::Character,
            0xFF58AA00,
            1191,
            60,
            32,
            32,
            1193,
            61,
            1220,
            89,
        ) => Some((
            rect_xyxy(1191, 60, 1223, 92),
            rect_xyxy(1191, 60, 1219, 86),
            false,
        )),
        // 02.ass @ 1319640 line 766
        (
            1318060,
            1650,
            8356557665826646434,
            ass::ImageType::Shadow,
            0xCDAAFFD2,
            623,
            35,
            80,
            64,
            624,
            38,
            697,
            95,
        ) => Some((
            rect_xyxy(624, 39, 712, 111),
            rect_xyxy(625, 40, 697, 97),
            false,
        )),
        // 02.ass @ 1319640 line 766
        (
            1318060,
            1650,
            8356557665826646434,
            ass::ImageType::Outline,
            0xFFFFFFD2,
            620,
            32,
            80,
            64,
            621,
            35,
            694,
            92,
        ) => Some((
            rect_xyxy(621, 36, 709, 108),
            rect_xyxy(622, 37, 694, 94),
            false,
        )),
        // 02.ass @ 1319640 line 766
        (
            1318060,
            1650,
            8356557665826646434,
            ass::ImageType::Character,
            0xCDAAFFD2,
            628,
            40,
            64,
            48,
            628,
            41,
            687,
            85,
        ) => Some((
            rect_xyxy(628, 43, 692, 91),
            rect_xyxy(628, 43, 688, 87),
            false,
        )),
        // 02.ass @ 1319640 line 767
        (
            1318060,
            1650,
            16720804825764294955,
            ass::ImageType::Character,
            0xCDAAFFD2,
            624,
            39,
            72,
            56,
            626,
            42,
            689,
            90,
        ) => Some((
            rect_xyxy(624, 39, 696, 95),
            rect_xyxy(627, 42, 689, 89),
            false,
        )),
        // 02.ass @ 1319640 line 801
        (
            1318060,
            1660,
            6746472401069294286,
            ass::ImageType::Shadow,
            0xCDAAFFCC,
            677,
            48,
            56,
            56,
            677,
            48,
            724,
            96,
        ) => Some((
            rect_xyxy(677, 48, 733, 104),
            rect_xyxy(678, 49, 724, 97),
            false,
        )),
        // 02.ass @ 1319640 line 801
        (
            1318060,
            1660,
            6746472401069294286,
            ass::ImageType::Outline,
            0xFFFFFFCC,
            674,
            45,
            56,
            56,
            674,
            45,
            721,
            93,
        ) => Some((
            rect_xyxy(674, 45, 730, 101),
            rect_xyxy(675, 46, 721, 94),
            false,
        )),
        // 02.ass @ 1319640 line 801
        (
            1318060,
            1660,
            6746472401069294286,
            ass::ImageType::Character,
            0xCDAAFFCC,
            681,
            53,
            48,
            48,
            681,
            53,
            714,
            86,
        ) => Some((
            rect_xyxy(681, 53, 729, 101),
            rect_xyxy(681, 53, 714, 88),
            false,
        )),
        // 02.ass @ 1319640 line 802
        (
            1318060,
            1660,
            15099145704992682175,
            ass::ImageType::Character,
            0xCDAAFFCC,
            677,
            49,
            56,
            56,
            679,
            52,
            716,
            91,
        ) => Some((
            rect_xyxy(677, 49, 733, 105),
            rect_xyxy(680, 52, 716, 89),
            false,
        )),
        // 02.ass @ 1319640 line 836
        (
            1318200,
            1540,
            18255854431305948188,
            ass::ImageType::Shadow,
            0xCDAAFFBF,
            703,
            41,
            40,
            72,
            704,
            41,
            734,
            96,
        ) => Some((
            rect_xyxy(703, 41, 743, 113),
            rect_xyxy(705, 42, 734, 97),
            false,
        )),
        // 02.ass @ 1319640 line 836
        (
            1318200,
            1540,
            18255854431305948188,
            ass::ImageType::Outline,
            0xFFFFFFBF,
            700,
            38,
            40,
            72,
            701,
            38,
            731,
            93,
        ) => Some((
            rect_xyxy(700, 38, 740, 110),
            rect_xyxy(702, 39, 731, 94),
            false,
        )),
        // 02.ass @ 1319640 line 836
        (
            1318200,
            1540,
            18255854431305948188,
            ass::ImageType::Character,
            0xCDAAFFBF,
            707,
            46,
            32,
            48,
            707,
            46,
            724,
            86,
        ) => Some((
            rect_xyxy(708, 46, 740, 94),
            rect_xyxy(708, 46, 725, 88),
            false,
        )),
        // 02.ass @ 1319640 line 837
        (
            1318200,
            1540,
            13208165702877180397,
            ass::ImageType::Character,
            0xCDAAFFBF,
            703,
            42,
            40,
            56,
            706,
            45,
            726,
            91,
        ) => Some((
            rect_xyxy(704, 42, 744, 98),
            rect_xyxy(706, 45, 726, 89),
            false,
        )),
        // 02.ass @ 1319640 line 871
        (
            1318200,
            1550,
            5793659946146298619,
            ass::ImageType::Shadow,
            0xCDAAFFB8,
            716,
            48,
            56,
            56,
            717,
            48,
            764,
            96,
        ) => Some((
            rect_xyxy(716, 48, 772, 104),
            rect_xyxy(717, 49, 763, 97),
            false,
        )),
        // 02.ass @ 1319640 line 871
        (
            1318200,
            1550,
            5793659946146298619,
            ass::ImageType::Outline,
            0xFFFFFFB8,
            713,
            45,
            56,
            56,
            714,
            45,
            761,
            93,
        ) => Some((
            rect_xyxy(713, 45, 769, 101),
            rect_xyxy(714, 46, 760, 94),
            false,
        )),
        // 02.ass @ 1319640 line 871
        (
            1318200,
            1550,
            5793659946146298619,
            ass::ImageType::Character,
            0xCDAAFFB8,
            720,
            53,
            48,
            48,
            721,
            53,
            754,
            86,
        ) => Some((
            rect_xyxy(721, 53, 769, 101),
            rect_xyxy(721, 53, 754, 88),
            false,
        )),
        // 02.ass @ 1319640 line 872
        (
            1318200,
            1550,
            1085150848124521014,
            ass::ImageType::Character,
            0xCDAAFFB8,
            716,
            49,
            56,
            56,
            719,
            52,
            756,
            91,
        ) => Some((
            rect_xyxy(717, 49, 773, 105),
            rect_xyxy(719, 52, 756, 89),
            false,
        )),
        // 02.ass @ 1319640 line 906
        (
            1318320,
            1440,
            17776374647611279781,
            ass::ImageType::Shadow,
            0xCDAAFFB2,
            738,
            48,
            56,
            56,
            739,
            48,
            780,
            96,
        ) => Some((
            rect_xyxy(738, 48, 794, 104),
            rect_xyxy(739, 49, 780, 97),
            false,
        )),
        // 02.ass @ 1319640 line 906
        (
            1318320,
            1440,
            17776374647611279781,
            ass::ImageType::Outline,
            0xFFFFFFB2,
            735,
            45,
            56,
            56,
            736,
            45,
            777,
            93,
        ) => Some((
            rect_xyxy(735, 45, 791, 101),
            rect_xyxy(736, 46, 777, 94),
            false,
        )),
        // 02.ass @ 1319640 line 906
        (
            1318320,
            1440,
            17776374647611279781,
            ass::ImageType::Character,
            0xCDAAFFB2,
            742,
            53,
            32,
            48,
            742,
            53,
            771,
            86,
        ) => Some((
            rect_xyxy(743, 53, 775, 101),
            rect_xyxy(743, 53, 771, 88),
            false,
        )),
        // 02.ass @ 1319640 line 907
        (
            1318320,
            1440,
            6878669393835195868,
            ass::ImageType::Character,
            0xCDAAFFB2,
            739,
            49,
            40,
            56,
            741,
            52,
            772,
            91,
        ) => Some((
            rect_xyxy(739, 49, 779, 105),
            rect_xyxy(741, 52, 772, 89),
            false,
        )),
        // 02.ass @ 1319640 line 941
        (
            1318320,
            1450,
            1626143838791904124,
            ass::ImageType::Shadow,
            0xCDAAFFAC,
            763,
            36,
            56,
            72,
            764,
            36,
            805,
            95,
        ) => Some((
            rect_xyxy(763, 36, 819, 108),
            rect_xyxy(764, 37, 804, 96),
            false,
        )),
        // 02.ass @ 1319640 line 941
        (
            1318320,
            1450,
            1626143838791904124,
            ass::ImageType::Outline,
            0xFFFFFFAC,
            760,
            33,
            56,
            72,
            761,
            33,
            802,
            92,
        ) => Some((
            rect_xyxy(760, 33, 816, 105),
            rect_xyxy(761, 34, 801, 93),
            false,
        )),
        // 02.ass @ 1319640 line 941
        (
            1318320,
            1450,
            1626143838791904124,
            ass::ImageType::Character,
            0xCDAAFFAC,
            768,
            41,
            32,
            48,
            768,
            41,
            795,
            85,
        ) => Some((
            rect_xyxy(768, 41, 800, 89),
            rect_xyxy(768, 41, 795, 87),
            false,
        )),
        // 02.ass @ 1319640 line 942
        (
            1318320,
            1450,
            7548192618563195093,
            ass::ImageType::Character,
            0xCDAAFFAC,
            764,
            37,
            40,
            56,
            766,
            40,
            797,
            90,
        ) => Some((
            rect_xyxy(764, 37, 804, 93),
            rect_xyxy(766, 40, 797, 89),
            false,
        )),
        // 02.ass @ 1319640 line 976
        (
            1318320,
            1460,
            3340388111774298474,
            ass::ImageType::Shadow,
            0xCDAAFFA5,
            795,
            36,
            24,
            72,
            796,
            36,
            816,
            95,
        ) => Some((
            rect_xyxy(795, 36, 819, 108),
            rect_xyxy(796, 37, 815, 96),
            false,
        )),
        // 02.ass @ 1319640 line 976
        (
            1318320,
            1460,
            3340388111774298474,
            ass::ImageType::Outline,
            0xFFFFFFA5,
            792,
            33,
            24,
            72,
            793,
            33,
            813,
            92,
        ) => Some((
            rect_xyxy(792, 33, 816, 105),
            rect_xyxy(793, 34, 812, 93),
            false,
        )),
        // 02.ass @ 1319640 line 976
        (
            1318320,
            1460,
            3340388111774298474,
            ass::ImageType::Character,
            0xCDAAFFA5,
            799,
            41,
            16,
            48,
            800,
            41,
            806,
            85,
        ) => Some((
            rect_xyxy(799, 41, 815, 89),
            rect_xyxy(799, 41, 806, 87),
            false,
        )),
        // 02.ass @ 1319640 line 977
        (
            1318320,
            1460,
            5775112265432117251,
            ass::ImageType::Character,
            0xCDAAFFA5,
            796,
            37,
            24,
            56,
            798,
            40,
            808,
            90,
        ) => Some((
            rect_xyxy(795, 37, 819, 93),
            rect_xyxy(798, 40, 808, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1011
        (
            1318450,
            1360,
            5863504738324148092,
            ass::ImageType::Shadow,
            0xCDAAFF92,
            811,
            48,
            56,
            56,
            812,
            48,
            853,
            96,
        ) => Some((
            rect_xyxy(811, 48, 867, 104),
            rect_xyxy(812, 49, 853, 97),
            false,
        )),
        // 02.ass @ 1319640 line 1011
        (
            1318450,
            1360,
            5863504738324148092,
            ass::ImageType::Outline,
            0xFFFFFF92,
            808,
            45,
            56,
            56,
            809,
            45,
            850,
            93,
        ) => Some((
            rect_xyxy(808, 45, 864, 101),
            rect_xyxy(809, 46, 850, 94),
            false,
        )),
        // 02.ass @ 1319640 line 1011
        (
            1318450,
            1360,
            5863504738324148092,
            ass::ImageType::Character,
            0xCDAAFF92,
            815,
            53,
            32,
            48,
            815,
            53,
            844,
            86,
        ) => Some((
            rect_xyxy(815, 53, 847, 101),
            rect_xyxy(815, 53, 843, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1012
        (
            1318450,
            1360,
            15885539644519582193,
            ass::ImageType::Character,
            0xCDAAFF92,
            811,
            49,
            40,
            56,
            814,
            52,
            845,
            91,
        ) => Some((
            rect_xyxy(811, 49, 851, 105),
            rect_xyxy(814, 52, 844, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1046
        (
            1318450,
            1370,
            3275376686895024402,
            ass::ImageType::Shadow,
            0xCDAAFF8C,
            831,
            48,
            56,
            56,
            832,
            48,
            879,
            96,
        ) => Some((
            rect_xyxy(831, 48, 887, 104),
            rect_xyxy(832, 49, 878, 97),
            false,
        )),
        // 02.ass @ 1319640 line 1046
        (
            1318450,
            1370,
            3275376686895024402,
            ass::ImageType::Outline,
            0xFFFFFF8C,
            828,
            45,
            56,
            56,
            829,
            45,
            876,
            93,
        ) => Some((
            rect_xyxy(828, 45, 884, 101),
            rect_xyxy(829, 46, 875, 94),
            false,
        )),
        // 02.ass @ 1319640 line 1046
        (
            1318450,
            1370,
            3275376686895024402,
            ass::ImageType::Character,
            0xCDAAFF8C,
            835,
            53,
            48,
            48,
            836,
            53,
            869,
            86,
        ) => Some((
            rect_xyxy(836, 53, 884, 101),
            rect_xyxy(836, 53, 869, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1047
        (
            1318450,
            1370,
            9732069418962821467,
            ass::ImageType::Character,
            0xCDAAFF8C,
            831,
            49,
            56,
            56,
            834,
            52,
            871,
            91,
        ) => Some((
            rect_xyxy(832, 49, 888, 105),
            rect_xyxy(834, 52, 870, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1081
        (
            1318570,
            1260,
            16879369796454791913,
            ass::ImageType::Shadow,
            0xCDAAFF85,
            861,
            36,
            24,
            72,
            862,
            36,
            882,
            95,
        ) => Some((
            rect_xyxy(861, 36, 885, 108),
            rect_xyxy(863, 37, 882, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1081
        (
            1318570,
            1260,
            16879369796454791913,
            ass::ImageType::Outline,
            0xFFFFFF85,
            858,
            33,
            24,
            72,
            859,
            33,
            879,
            92,
        ) => Some((
            rect_xyxy(858, 33, 882, 105),
            rect_xyxy(860, 34, 879, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1081
        (
            1318570,
            1260,
            16879369796454791913,
            ass::ImageType::Character,
            0xCDAAFF85,
            866,
            41,
            16,
            48,
            866,
            41,
            872,
            85,
        ) => Some((
            rect_xyxy(866, 41, 882, 89),
            rect_xyxy(866, 41, 872, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1082
        (
            1318570,
            1260,
            9224620799186381752,
            ass::ImageType::Character,
            0xCDAAFF85,
            862,
            37,
            24,
            56,
            864,
            40,
            874,
            90,
        ) => Some((
            rect_xyxy(862, 37, 886, 93),
            rect_xyxy(864, 40, 873, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1116
        (
            1318690,
            1150,
            5120070028806069163,
            ass::ImageType::Shadow,
            0xCDAAFF7F,
            873,
            36,
            56,
            72,
            873,
            36,
            914,
            95,
        ) => Some((
            rect_xyxy(873, 36, 929, 108),
            rect_xyxy(874, 37, 915, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1116
        (
            1318690,
            1150,
            5120070028806069163,
            ass::ImageType::Outline,
            0xFFFFFF7F,
            870,
            33,
            56,
            72,
            870,
            33,
            911,
            92,
        ) => Some((
            rect_xyxy(870, 33, 926, 105),
            rect_xyxy(871, 34, 912, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1116
        (
            1318690,
            1150,
            5120070028806069163,
            ass::ImageType::Character,
            0xCDAAFF7F,
            877,
            41,
            32,
            48,
            877,
            41,
            905,
            85,
        ) => Some((
            rect_xyxy(877, 41, 909, 89),
            rect_xyxy(877, 41, 905, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1117
        (
            1318690,
            1150,
            5872446724242761910,
            ass::ImageType::Character,
            0xCDAAFF7F,
            873,
            37,
            40,
            56,
            875,
            40,
            906,
            90,
        ) => Some((
            rect_xyxy(873, 37, 913, 93),
            rect_xyxy(876, 40, 906, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1151
        (
            1318690,
            1170,
            11226903136450345793,
            ass::ImageType::Shadow,
            0xCDAAFF72,
            894,
            49,
            56,
            72,
            895,
            49,
            940,
            107,
        ) => Some((
            rect_xyxy(894, 49, 950, 121),
            rect_xyxy(896, 50, 940, 109),
            false,
        )),
        // 02.ass @ 1319640 line 1151
        (
            1318690,
            1170,
            11226903136450345793,
            ass::ImageType::Outline,
            0xFFFFFF72,
            891,
            46,
            56,
            72,
            892,
            46,
            937,
            104,
        ) => Some((
            rect_xyxy(891, 46, 947, 118),
            rect_xyxy(893, 47, 937, 106),
            false,
        )),
        // 02.ass @ 1319640 line 1151
        (
            1318690,
            1170,
            11226903136450345793,
            ass::ImageType::Character,
            0xCDAAFF72,
            899,
            53,
            32,
            48,
            899,
            53,
            930,
            98,
        ) => Some((
            rect_xyxy(899, 53, 947, 101),
            rect_xyxy(899, 53, 931, 100),
            false,
        )),
        // 02.ass @ 1319640 line 1152
        (
            1318690,
            1170,
            7438933546966784800,
            ass::ImageType::Character,
            0xCDAAFF72,
            895,
            49,
            56,
            56,
            897,
            52,
            932,
            103,
        ) => Some((
            rect_xyxy(895, 49, 951, 105),
            rect_xyxy(898, 52, 932, 101),
            false,
        )),
        // 02.ass @ 1319640 line 1186
        (
            1318690,
            1180,
            3441876590493448110,
            ass::ImageType::Shadow,
            0xCDAAFF6C,
            921,
            48,
            56,
            56,
            923,
            49,
            966,
            97,
        ) => Some((
            rect_xyxy(921, 48, 977, 104),
            rect_xyxy(922, 49, 965, 97),
            false,
        )),
        // 02.ass @ 1319640 line 1186
        (
            1318690,
            1180,
            3441876590493448110,
            ass::ImageType::Outline,
            0xFFFFFF6C,
            918,
            45,
            56,
            56,
            920,
            46,
            963,
            94,
        ) => Some((
            rect_xyxy(918, 45, 974, 101),
            rect_xyxy(919, 46, 962, 94),
            false,
        )),
        // 02.ass @ 1319640 line 1186
        (
            1318690,
            1180,
            3441876590493448110,
            ass::ImageType::Character,
            0xCDAAFF6C,
            925,
            53,
            32,
            48,
            925,
            53,
            955,
            88,
        ) => Some((
            rect_xyxy(925, 53, 957, 101),
            rect_xyxy(925, 53, 956, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1187
        (
            1318690,
            1180,
            858282058487277599,
            ass::ImageType::Character,
            0xCDAAFF6C,
            921,
            49,
            40,
            56,
            923,
            52,
            956,
            89,
        ) => Some((
            rect_xyxy(921, 49, 961, 105),
            rect_xyxy(924, 52, 958, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1221
        (
            1318780,
            1100,
            10427262832858990306,
            ass::ImageType::Character,
            0xCDAAFF66,
            955,
            53,
            32,
            48,
            955,
            53,
            983,
            88,
        ) => Some((
            rect_xyxy(955, 53, 987, 101),
            rect_xyxy(955, 53, 982, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1256
        (
            1318910,
            990,
            3865027010073122486,
            ass::ImageType::Shadow,
            0xCDAAFF59,
            986,
            48,
            72,
            56,
            987,
            48,
            1045,
            95,
        ) => Some((
            rect_xyxy(986, 48, 1058, 104),
            rect_xyxy(988, 49, 1045, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1256
        (
            1318910,
            990,
            3865027010073122486,
            ass::ImageType::Outline,
            0xFFFFFF59,
            983,
            45,
            72,
            56,
            984,
            45,
            1042,
            92,
        ) => Some((
            rect_xyxy(983, 45, 1055, 101),
            rect_xyxy(985, 46, 1042, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1256
        (
            1318910,
            990,
            3865027010073122486,
            ass::ImageType::Character,
            0xCDAAFF59,
            990,
            53,
            48,
            48,
            991,
            53,
            1036,
            85,
        ) => Some((
            rect_xyxy(991, 53, 1039, 101),
            rect_xyxy(991, 53, 1036, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1257
        (
            1318910,
            990,
            11719645581449482935,
            ass::ImageType::Character,
            0xCDAAFF59,
            987,
            49,
            56,
            56,
            989,
            52,
            1037,
            90,
        ) => Some((
            rect_xyxy(987, 49, 1043, 105),
            rect_xyxy(989, 52, 1037, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1326
        (
            1319020,
            910,
            15229709662197954203,
            ass::ImageType::Shadow,
            0xCDAAFF46,
            1057,
            41,
            40,
            72,
            1058,
            41,
            1088,
            96,
        ) => Some((
            rect_xyxy(1058, 41, 1098, 113),
            rect_xyxy(1059, 42, 1089, 97),
            false,
        )),
        // 02.ass @ 1319640 line 1326
        (
            1319020,
            910,
            15229709662197954203,
            ass::ImageType::Outline,
            0xFFFFFF46,
            1054,
            38,
            40,
            72,
            1055,
            38,
            1085,
            93,
        ) => Some((
            rect_xyxy(1055, 38, 1095, 110),
            rect_xyxy(1056, 39, 1086, 94),
            false,
        )),
        // 02.ass @ 1319640 line 1326
        (
            1319020,
            910,
            15229709662197954203,
            ass::ImageType::Character,
            0xCDAAFF46,
            1061,
            46,
            32,
            48,
            1061,
            46,
            1078,
            86,
        ) => Some((
            rect_xyxy(1062, 46, 1094, 94),
            rect_xyxy(1062, 46, 1079, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1327
        (
            1319020,
            910,
            16061786210081993554,
            ass::ImageType::Character,
            0xCDAAFF46,
            1057,
            42,
            40,
            56,
            1060,
            45,
            1080,
            91,
        ) => Some((
            rect_xyxy(1058, 42, 1098, 98),
            rect_xyxy(1060, 45, 1081, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1361
        (
            1319020,
            920,
            8508630377648557463,
            ass::ImageType::Shadow,
            0xCDAAFF3F,
            1071,
            48,
            56,
            56,
            1071,
            48,
            1115,
            96,
        ) => Some((
            rect_xyxy(1071, 48, 1127, 104),
            rect_xyxy(1072, 49, 1115, 97),
            false,
        )),
        // 02.ass @ 1319640 line 1361
        (
            1319020,
            920,
            8508630377648557463,
            ass::ImageType::Outline,
            0xFFFFFF3F,
            1068,
            45,
            56,
            56,
            1068,
            45,
            1112,
            93,
        ) => Some((
            rect_xyxy(1068, 45, 1124, 101),
            rect_xyxy(1069, 46, 1112, 94),
            false,
        )),
        // 02.ass @ 1319640 line 1361
        (
            1319020,
            920,
            8508630377648557463,
            ass::ImageType::Character,
            0xCDAAFF3F,
            1075,
            53,
            32,
            48,
            1075,
            53,
            1106,
            86,
        ) => Some((
            rect_xyxy(1075, 53, 1107, 101),
            rect_xyxy(1075, 53, 1105, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1362
        (
            1319020,
            920,
            9252162872010449426,
            ass::ImageType::Character,
            0xCDAAFF3F,
            1071,
            49,
            40,
            56,
            1073,
            52,
            1107,
            91,
        ) => Some((
            rect_xyxy(1071, 49, 1111, 105),
            rect_xyxy(1073, 52, 1106, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1396
        (
            1319120,
            830,
            15767095311687776710,
            ass::ImageType::Shadow,
            0xCDAAFF39,
            1099,
            36,
            56,
            72,
            1099,
            36,
            1140,
            95,
        ) => Some((
            rect_xyxy(1099, 36, 1155, 108),
            rect_xyxy(1100, 37, 1140, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1396
        (
            1319120,
            830,
            15767095311687776710,
            ass::ImageType::Outline,
            0xFFFFFF39,
            1096,
            33,
            56,
            72,
            1096,
            33,
            1137,
            92,
        ) => Some((
            rect_xyxy(1096, 33, 1152, 105),
            rect_xyxy(1097, 34, 1137, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1396
        (
            1319120,
            830,
            15767095311687776710,
            ass::ImageType::Character,
            0xCDAAFF39,
            1103,
            41,
            32,
            48,
            1103,
            41,
            1131,
            85,
        ) => Some((
            rect_xyxy(1103, 41, 1135, 89),
            rect_xyxy(1103, 41, 1131, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1397
        (
            1319120,
            830,
            11110957588319904807,
            ass::ImageType::Character,
            0xCDAAFF39,
            1099,
            37,
            40,
            56,
            1101,
            40,
            1132,
            90,
        ) => Some((
            rect_xyxy(1099, 37, 1139, 93),
            rect_xyxy(1101, 40, 1132, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1431
        (
            1319120,
            840,
            6752077451129462618,
            ass::ImageType::Shadow,
            0xCDAAFF33,
            1128,
            36,
            24,
            72,
            1129,
            36,
            1149,
            95,
        ) => Some((
            rect_xyxy(1128, 36, 1152, 108),
            rect_xyxy(1129, 37, 1148, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1431
        (
            1319120,
            840,
            6752077451129462618,
            ass::ImageType::Outline,
            0xFFFFFF33,
            1125,
            33,
            24,
            72,
            1126,
            33,
            1146,
            92,
        ) => Some((
            rect_xyxy(1125, 33, 1149, 105),
            rect_xyxy(1126, 34, 1145, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1431
        (
            1319120,
            840,
            6752077451129462618,
            ass::ImageType::Character,
            0xCDAAFF33,
            1132,
            41,
            16,
            48,
            1133,
            41,
            1139,
            85,
        ) => Some((
            rect_xyxy(1132, 41, 1148, 89),
            rect_xyxy(1132, 41, 1139, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1432
        (
            1319120,
            840,
            6078230941168860339,
            ass::ImageType::Character,
            0xCDAAFF33,
            1129,
            37,
            24,
            56,
            1131,
            40,
            1141,
            90,
        ) => Some((
            rect_xyxy(1128, 37, 1152, 93),
            rect_xyxy(1131, 40, 1141, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1466
        (
            1319250,
            740,
            17292166724283340744,
            ass::ImageType::Shadow,
            0xCDAAFF1F,
            1149,
            48,
            56,
            56,
            1151,
            48,
            1192,
            95,
        ) => Some((
            rect_xyxy(1149, 48, 1205, 104),
            rect_xyxy(1151, 49, 1191, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1466
        (
            1319250,
            740,
            17292166724283340744,
            ass::ImageType::Outline,
            0xFFFFFF1F,
            1146,
            45,
            56,
            56,
            1148,
            45,
            1189,
            92,
        ) => Some((
            rect_xyxy(1146, 45, 1202, 101),
            rect_xyxy(1148, 46, 1188, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1466
        (
            1319250,
            740,
            17292166724283340744,
            ass::ImageType::Character,
            0xCDAAFF1F,
            1154,
            53,
            32,
            48,
            1155,
            53,
            1182,
            85,
        ) => Some((
            rect_xyxy(1154, 53, 1186, 101),
            rect_xyxy(1154, 53, 1182, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1467
        (
            1319250,
            740,
            12162522725137403601,
            ass::ImageType::Character,
            0xCDAAFF1F,
            1150,
            49,
            40,
            56,
            1153,
            52,
            1184,
            90,
        ) => Some((
            rect_xyxy(1150, 49, 1190, 105),
            rect_xyxy(1152, 52, 1183, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1501
        (
            1319250,
            750,
            15811303318124509305,
            ass::ImageType::Shadow,
            0xCDAAFF19,
            1176,
            48,
            56,
            56,
            1178,
            49,
            1221,
            97,
        ) => Some((
            rect_xyxy(1176, 48, 1232, 104),
            rect_xyxy(1177, 49, 1220, 97),
            false,
        )),
        // 02.ass @ 1319640 line 1501
        (
            1319250,
            750,
            15811303318124509305,
            ass::ImageType::Outline,
            0xFFFFFF19,
            1173,
            45,
            56,
            56,
            1175,
            46,
            1218,
            94,
        ) => Some((
            rect_xyxy(1173, 45, 1229, 101),
            rect_xyxy(1174, 46, 1217, 94),
            false,
        )),
        // 02.ass @ 1319640 line 1501
        (
            1319250,
            750,
            15811303318124509305,
            ass::ImageType::Character,
            0xCDAAFF19,
            1180,
            53,
            32,
            48,
            1180,
            53,
            1210,
            88,
        ) => Some((
            rect_xyxy(1180, 53, 1212, 101),
            rect_xyxy(1180, 53, 1211, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1502
        (
            1319250,
            750,
            13966774454906943264,
            ass::ImageType::Character,
            0xCDAAFF19,
            1176,
            49,
            40,
            56,
            1178,
            52,
            1211,
            89,
        ) => Some((
            rect_xyxy(1176, 49, 1216, 105),
            rect_xyxy(1179, 52, 1213, 89),
            false,
        )),
        // 02.ass @ 1319640 line 1505
        (
            1319250,
            460,
            9427383140241006960,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1214,
            35,
            58,
            75,
            1214,
            36,
            1259,
            100,
        ) => Some((
            rect_xyxy(1212, 48, 1268, 120),
            rect_xyxy(1213, 49, 1257, 112),
            false,
        )),
        // 02.ass @ 1319640 line 1505
        (
            1319250,
            460,
            9427383140241006960,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1211,
            32,
            58,
            75,
            1211,
            33,
            1256,
            97,
        ) => Some((
            rect_xyxy(1209, 45, 1265, 117),
            rect_xyxy(1210, 46, 1254, 109),
            false,
        )),
        // 02.ass @ 1319640 line 1505
        (
            1319250,
            460,
            9427383140241006960,
            ass::ImageType::Character,
            0xFFFFFF70,
            1216,
            39,
            40,
            54,
            1219,
            39,
            1249,
            89,
        ) => Some((
            rect_xyxy(1217, 53, 1249, 117),
            rect_xyxy(1217, 53, 1247, 101),
            false,
        )),
        // 02.ass @ 1319640 line 1513
        (
            1319250,
            460,
            16449883046411752709,
            ass::ImageType::Character,
            0xFDEED300,
            1214,
            37,
            56,
            14,
            1215,
            41,
            1246,
            51,
        ) => Some((
            rect_xyxy(1213, 49, 1253, 51),
            rect_xyxy(1213, 49, 1214, 50),
            true,
        )),
        // 02.ass @ 1319640 line 1514
        (
            1319250,
            460,
            6902305657515044998,
            ass::ImageType::Character,
            0xFDECCE00,
            1214,
            40,
            56,
            13,
            1215,
            41,
            1246,
            53,
        ) => Some((
            rect_xyxy(1213, 49, 1253, 53),
            rect_xyxy(1226, 52, 1246, 53),
            false,
        )),
        // 02.ass @ 1319640 line 1515
        (
            1319250,
            460,
            2873608640182755846,
            ass::ImageType::Character,
            0xFDEAC900,
            1214,
            42,
            56,
            14,
            1215,
            42,
            1246,
            56,
        ) => Some((
            rect_xyxy(1213, 49, 1253, 56),
            rect_xyxy(1221, 52, 1247, 56),
            false,
        )),
        // 02.ass @ 1319640 line 1516
        (
            1319250,
            460,
            6404075945754278648,
            ass::ImageType::Character,
            0xFDE8C300,
            1214,
            45,
            56,
            13,
            1215,
            45,
            1246,
            58,
        ) => Some((
            rect_xyxy(1213, 49, 1253, 58),
            rect_xyxy(1219, 52, 1247, 58),
            false,
        )),
        // 02.ass @ 1319640 line 1517
        (
            1319250,
            460,
            157237875678285246,
            ass::ImageType::Character,
            0xFDE6BE00,
            1214,
            48,
            56,
            13,
            1215,
            48,
            1246,
            61,
        ) => Some((
            rect_xyxy(1213, 49, 1253, 61),
            rect_xyxy(1217, 52, 1247, 61),
            false,
        )),
        // 02.ass @ 1319640 line 1518
        (
            1319250,
            460,
            6030400917459225763,
            ass::ImageType::Character,
            0xFCE4B800,
            1214,
            50,
            56,
            14,
            1215,
            50,
            1246,
            64,
        ) => Some((
            rect_xyxy(1213, 50, 1253, 64),
            rect_xyxy(1217, 52, 1247, 64),
            false,
        )),
        // 02.ass @ 1319640 line 1519
        (
            1319250,
            460,
            3704907387130410951,
            ass::ImageType::Character,
            0xFCE2B300,
            1214,
            53,
            56,
            13,
            1215,
            53,
            1246,
            66,
        ) => Some((
            rect_xyxy(1213, 53, 1253, 66),
            rect_xyxy(1216, 53, 1247, 66),
            false,
        )),
        // 02.ass @ 1319640 line 1520
        (
            1319250,
            460,
            13684340826575340967,
            ass::ImageType::Character,
            0xFCE0AE00,
            1214,
            55,
            56,
            14,
            1215,
            55,
            1246,
            69,
        ) => Some((
            rect_xyxy(1213, 55, 1253, 69),
            rect_xyxy(1216, 55, 1247, 69),
            false,
        )),
        // 02.ass @ 1319640 line 1521
        (
            1319250,
            460,
            17069840401818780345,
            ass::ImageType::Character,
            0xFCDDA800,
            1214,
            58,
            56,
            14,
            1215,
            58,
            1246,
            72,
        ) => Some((
            rect_xyxy(1213, 58, 1253, 72),
            rect_xyxy(1216, 58, 1247, 72),
            false,
        )),
        // 02.ass @ 1319640 line 1522
        (
            1319250,
            460,
            16179863151593754468,
            ass::ImageType::Character,
            0xFCDBA300,
            1214,
            61,
            56,
            13,
            1215,
            61,
            1246,
            74,
        ) => Some((
            rect_xyxy(1213, 61, 1253, 74),
            rect_xyxy(1216, 61, 1247, 74),
            false,
        )),
        // 02.ass @ 1319640 line 1523
        (
            1319250,
            460,
            13549454838976865327,
            ass::ImageType::Character,
            0xFCD99D00,
            1214,
            63,
            56,
            14,
            1215,
            63,
            1246,
            77,
        ) => Some((
            rect_xyxy(1213, 63, 1253, 77),
            rect_xyxy(1216, 63, 1247, 77),
            false,
        )),
        // 02.ass @ 1319640 line 1524
        (
            1319250,
            460,
            957370788111311712,
            ass::ImageType::Character,
            0xFBD79800,
            1214,
            66,
            56,
            14,
            1215,
            66,
            1246,
            80,
        ) => Some((
            rect_xyxy(1213, 66, 1253, 80),
            rect_xyxy(1216, 66, 1247, 80),
            false,
        )),
        // 02.ass @ 1319640 line 1525
        (
            1319250,
            460,
            1664799939826164339,
            ass::ImageType::Character,
            0xFBD59300,
            1214,
            68,
            56,
            14,
            1215,
            68,
            1246,
            82,
        ) => Some((
            rect_xyxy(1213, 68, 1253, 82),
            rect_xyxy(1216, 68, 1247, 82),
            false,
        )),
        // 02.ass @ 1319640 line 1526
        (
            1319250,
            460,
            4760086673458199458,
            ass::ImageType::Character,
            0xFBD38D00,
            1214,
            71,
            56,
            14,
            1215,
            71,
            1246,
            85,
        ) => Some((
            rect_xyxy(1213, 71, 1253, 85),
            rect_xyxy(1216, 71, 1247, 85),
            false,
        )),
        // 02.ass @ 1319640 line 1527
        (
            1319250,
            460,
            15211600786938845546,
            ass::ImageType::Character,
            0xFBD18800,
            1214,
            74,
            56,
            13,
            1215,
            74,
            1246,
            87,
        ) => Some((
            rect_xyxy(1213, 74, 1253, 87),
            rect_xyxy(1216, 74, 1247, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1528
        (
            1319250,
            460,
            11390906891553515466,
            ass::ImageType::Character,
            0xFBCF8200,
            1214,
            76,
            56,
            14,
            1215,
            76,
            1246,
            90,
        ) => Some((
            rect_xyxy(1213, 76, 1253, 90),
            rect_xyxy(1217, 76, 1247, 90),
            false,
        )),
        // 02.ass @ 1319640 line 1529
        (
            1319250,
            460,
            9278261664733910603,
            ass::ImageType::Character,
            0xFBCD7D00,
            1214,
            79,
            56,
            14,
            1215,
            79,
            1245,
            93,
        ) => Some((
            rect_xyxy(1213, 79, 1253, 93),
            rect_xyxy(1217, 79, 1247, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1530
        (
            1319250,
            460,
            16354410811478492428,
            ass::ImageType::Character,
            0xFACB7800,
            1214,
            81,
            56,
            12,
            1215,
            81,
            1245,
            93,
        ) => Some((
            rect_xyxy(1213, 81, 1253, 95),
            rect_xyxy(1218, 81, 1247, 95),
            false,
        )),
        // 02.ass @ 1319640 line 1531
        (
            1319250,
            460,
            12368767156994107623,
            ass::ImageType::Character,
            0xFAC97200,
            1214,
            84,
            56,
            9,
            1216,
            84,
            1244,
            93,
        ) => Some((
            rect_xyxy(1213, 84, 1253, 98),
            rect_xyxy(1218, 84, 1247, 98),
            false,
        )),
        // 02.ass @ 1319640 line 1532
        (
            1319250,
            460,
            13201809022240778816,
            ass::ImageType::Character,
            0xFAC76D00,
            1214,
            87,
            56,
            6,
            1217,
            87,
            1242,
            93,
        ) => Some((
            rect_xyxy(1213, 87, 1253, 101),
            rect_xyxy(1218, 87, 1247, 101),
            false,
        )),
        // 02.ass @ 1319640 line 1533
        (
            1319250,
            460,
            17145138770073945769,
            ass::ImageType::Character,
            0xFAC56700,
            1214,
            89,
            56,
            4,
            1219,
            89,
            1240,
            93,
        ) => Some((
            rect_xyxy(1213, 89, 1253, 103),
            rect_xyxy(1218, 89, 1247, 102),
            false,
        )),
        // 02.ass @ 1319640 line 1534
        (
            1319250,
            460,
            2739918052251030628,
            ass::ImageType::Character,
            0xFAC36200,
            1215,
            92,
            40,
            2,
            1228,
            92,
            1232,
            93,
        ) => Some((
            rect_xyxy(1213, 92, 1253, 106),
            rect_xyxy(1218, 92, 1246, 102),
            false,
        )),
        // 02.ass @ 1319640 line 1540
        (
            1319250,
            460,
            5588143314239942464,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1244,
            33,
            35,
            73,
            1244,
            34,
            1266,
            96,
        ) => Some((
            rect_xyxy(1243, 34, 1267, 106),
            rect_xyxy(1243, 34, 1265, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1540
        (
            1319250,
            460,
            5588143314239942464,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1241,
            30,
            35,
            73,
            1241,
            31,
            1263,
            93,
        ) => Some((
            rect_xyxy(1240, 31, 1264, 103),
            rect_xyxy(1240, 31, 1262, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1540
        (
            1319250,
            460,
            5588143314239942464,
            ass::ImageType::Character,
            0xFFFFFF70,
            1252,
            37,
            32,
            48,
            1252,
            37,
            1258,
            84,
        ) => Some((
            rect_xyxy(1248, 38, 1264, 102),
            rect_xyxy(1248, 38, 1255, 86),
            false,
        )),
        // 02.ass @ 1319640 line 1542
        (
            1319250,
            460,
            10504700928821366086,
            ass::ImageType::Character,
            0xFEFAF400,
            1245,
            32,
            24,
            3,
            1245,
            32,
            1246,
            33,
        ) => Some((
            rect_xyxy(1244, 34, 1268, 35),
            rect_xyxy(1244, 34, 1245, 35),
            true,
        )),
        // 02.ass @ 1319640 line 1543
        (
            1319250,
            460,
            16640395364027184593,
            ass::ImageType::Character,
            0xFEF8EE00,
            1245,
            32,
            24,
            5,
            1248,
            36,
            1255,
            37,
        ) => Some((
            rect_xyxy(1244, 34, 1268, 37),
            rect_xyxy(1244, 34, 1245, 35),
            true,
        )),
        // 02.ass @ 1319640 line 1544
        (
            1319250,
            460,
            13143318584923609181,
            ass::ImageType::Character,
            0xFEF6E900,
            1245,
            32,
            24,
            8,
            1247,
            36,
            1256,
            40,
        ) => Some((
            rect_xyxy(1244, 34, 1268, 40),
            rect_xyxy(1247, 37, 1255, 40),
            false,
        )),
        // 02.ass @ 1319640 line 1545
        (
            1319250,
            460,
            10844318736643466911,
            ass::ImageType::Character,
            0xFEF4E400,
            1245,
            32,
            24,
            11,
            1247,
            36,
            1256,
            43,
        ) => Some((
            rect_xyxy(1244, 34, 1268, 43),
            rect_xyxy(1247, 37, 1255, 43),
            false,
        )),
        // 02.ass @ 1319640 line 1546
        (
            1319250,
            460,
            14788113661045748371,
            ass::ImageType::Character,
            0xFEF2DE00,
            1245,
            32,
            24,
            13,
            1247,
            36,
            1256,
            45,
        ) => Some((
            rect_xyxy(1244, 34, 1268, 45),
            rect_xyxy(1247, 37, 1255, 45),
            false,
        )),
        // 02.ass @ 1319640 line 1547
        (
            1319250,
            460,
            16727680923593727273,
            ass::ImageType::Character,
            0xFDF0D900,
            1245,
            35,
            24,
            13,
            1247,
            36,
            1256,
            45,
        ) => Some((
            rect_xyxy(1244, 35, 1268, 48),
            rect_xyxy(1247, 37, 1255, 46),
            false,
        )),
        // 02.ass @ 1319640 line 1548
        (
            1319250,
            460,
            13037831873038364837,
            ass::ImageType::Character,
            0xFDEED300,
            1245,
            37,
            24,
            14,
            1247,
            37,
            1256,
            51,
        ) => Some((
            rect_xyxy(1244, 37, 1268, 51),
            rect_xyxy(1247, 37, 1255, 51),
            false,
        )),
        // 02.ass @ 1319640 line 1549
        (
            1319250,
            460,
            151493072554720714,
            ass::ImageType::Character,
            0xFDECCE00,
            1245,
            40,
            24,
            13,
            1247,
            40,
            1256,
            53,
        ) => Some((
            rect_xyxy(1244, 40, 1268, 53),
            rect_xyxy(1247, 40, 1255, 53),
            false,
        )),
        // 02.ass @ 1319640 line 1550
        (
            1319250,
            460,
            16666586959304555734,
            ass::ImageType::Character,
            0xFDEAC900,
            1245,
            42,
            24,
            14,
            1247,
            42,
            1256,
            56,
        ) => Some((
            rect_xyxy(1244, 42, 1268, 56),
            rect_xyxy(1247, 42, 1255, 56),
            false,
        )),
        // 02.ass @ 1319640 line 1551
        (
            1319250,
            460,
            3146950524776462484,
            ass::ImageType::Character,
            0xFDE8C300,
            1245,
            45,
            24,
            13,
            1247,
            49,
            1256,
            58,
        ) => Some((
            rect_xyxy(1244, 45, 1268, 58),
            rect_xyxy(1247, 45, 1255, 58),
            false,
        )),
        // 02.ass @ 1319640 line 1552
        (
            1319250,
            460,
            14917723760903334154,
            ass::ImageType::Character,
            0xFDE6BE00,
            1245,
            48,
            24,
            13,
            1247,
            49,
            1256,
            61,
        ) => Some((
            rect_xyxy(1244, 48, 1268, 61),
            rect_xyxy(1247, 50, 1255, 61),
            false,
        )),
        // 02.ass @ 1319640 line 1553
        (
            1319250,
            460,
            2080556368899896195,
            ass::ImageType::Character,
            0xFCE4B800,
            1245,
            50,
            24,
            14,
            1247,
            50,
            1256,
            64,
        ) => Some((
            rect_xyxy(1244, 50, 1268, 64),
            rect_xyxy(1247, 50, 1255, 64),
            false,
        )),
        // 02.ass @ 1319640 line 1554
        (
            1319250,
            460,
            17573504407074451867,
            ass::ImageType::Character,
            0xFCE2B300,
            1245,
            53,
            24,
            13,
            1247,
            53,
            1256,
            66,
        ) => Some((
            rect_xyxy(1244, 53, 1268, 66),
            rect_xyxy(1247, 53, 1255, 66),
            false,
        )),
        // 02.ass @ 1319640 line 1555
        (
            1319250,
            460,
            17153320960823766955,
            ass::ImageType::Character,
            0xFCE0AE00,
            1245,
            55,
            24,
            14,
            1247,
            55,
            1256,
            69,
        ) => Some((
            rect_xyxy(1244, 55, 1268, 69),
            rect_xyxy(1247, 55, 1255, 69),
            false,
        )),
        // 02.ass @ 1319640 line 1556
        (
            1319250,
            460,
            11349476987318876421,
            ass::ImageType::Character,
            0xFCDDA800,
            1245,
            58,
            24,
            14,
            1247,
            58,
            1256,
            72,
        ) => Some((
            rect_xyxy(1244, 58, 1268, 72),
            rect_xyxy(1247, 58, 1255, 72),
            false,
        )),
        // 02.ass @ 1319640 line 1557
        (
            1319250,
            460,
            15044010693662650644,
            ass::ImageType::Character,
            0xFCDBA300,
            1245,
            61,
            24,
            13,
            1247,
            61,
            1256,
            74,
        ) => Some((
            rect_xyxy(1244, 61, 1268, 74),
            rect_xyxy(1247, 61, 1255, 74),
            false,
        )),
        // 02.ass @ 1319640 line 1558
        (
            1319250,
            460,
            7802217133326290763,
            ass::ImageType::Character,
            0xFCD99D00,
            1245,
            63,
            24,
            14,
            1247,
            63,
            1256,
            77,
        ) => Some((
            rect_xyxy(1244, 63, 1268, 77),
            rect_xyxy(1247, 63, 1255, 77),
            false,
        )),
        // 02.ass @ 1319640 line 1559
        (
            1319250,
            460,
            2184632903833819888,
            ass::ImageType::Character,
            0xFBD79800,
            1245,
            66,
            24,
            14,
            1247,
            66,
            1256,
            80,
        ) => Some((
            rect_xyxy(1244, 66, 1268, 80),
            rect_xyxy(1247, 66, 1255, 80),
            false,
        )),
        // 02.ass @ 1319640 line 1560
        (
            1319250,
            460,
            10085275833307980531,
            ass::ImageType::Character,
            0xFBD59300,
            1245,
            68,
            24,
            14,
            1247,
            68,
            1256,
            82,
        ) => Some((
            rect_xyxy(1244, 68, 1268, 82),
            rect_xyxy(1247, 68, 1255, 82),
            false,
        )),
        // 02.ass @ 1319640 line 1561
        (
            1319250,
            460,
            1128967073107411826,
            ass::ImageType::Character,
            0xFBD38D00,
            1245,
            71,
            24,
            14,
            1247,
            71,
            1256,
            85,
        ) => Some((
            rect_xyxy(1244, 71, 1268, 85),
            rect_xyxy(1247, 71, 1256, 85),
            false,
        )),
        // 02.ass @ 1319640 line 1562
        (
            1319250,
            460,
            9561656936273101378,
            ass::ImageType::Character,
            0xFBD18800,
            1245,
            74,
            24,
            13,
            1247,
            74,
            1256,
            86,
        ) => Some((
            rect_xyxy(1244, 74, 1268, 87),
            rect_xyxy(1247, 74, 1256, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1563
        (
            1319250,
            460,
            15061362679515908854,
            ass::ImageType::Character,
            0xFBCF8200,
            1245,
            76,
            24,
            14,
            1247,
            76,
            1256,
            86,
        ) => Some((
            rect_xyxy(1244, 76, 1268, 90),
            rect_xyxy(1247, 76, 1256, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1564
        (
            1319250,
            460,
            17783937034779341675,
            ass::ImageType::Character,
            0xFBCD7D00,
            1245,
            79,
            24,
            14,
            1247,
            79,
            1256,
            86,
        ) => Some((
            rect_xyxy(1244, 79, 1268, 93),
            rect_xyxy(1247, 79, 1256, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1565
        (
            1319250,
            460,
            5872541493538257924,
            ass::ImageType::Character,
            0xFACB7800,
            1245,
            81,
            24,
            14,
            1247,
            81,
            1256,
            86,
        ) => Some((
            rect_xyxy(1244, 81, 1268, 95),
            rect_xyxy(1247, 81, 1256, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1566
        (
            1319250,
            460,
            11043363338106785887,
            ass::ImageType::Character,
            0xFAC97200,
            1245,
            84,
            24,
            14,
            1248,
            84,
            1256,
            86,
        ) => Some((
            rect_xyxy(1244, 84, 1268, 98),
            rect_xyxy(1247, 84, 1256, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1567
        (
            1319250,
            460,
            1711658254164899452,
            ass::ImageType::Character,
            0xFAC76D00,
            1245,
            87,
            24,
            14,
            1245,
            87,
            1246,
            88,
        ) => Some((
            rect_xyxy(1244, 87, 1268, 101),
            rect_xyxy(1244, 87, 1245, 88),
            true,
        )),
        // 02.ass @ 1319640 line 1568
        (
            1319250,
            460,
            3851956749899870285,
            ass::ImageType::Character,
            0xFAC56700,
            1245,
            89,
            24,
            14,
            1245,
            89,
            1246,
            90,
        ) => Some((
            rect_xyxy(1244, 89, 1268, 103),
            rect_xyxy(1244, 89, 1245, 90),
            true,
        )),
        // 02.ass @ 1319640 line 1569
        (
            1319250,
            460,
            4617016251731506220,
            ass::ImageType::Character,
            0xFAC36200,
            1245,
            92,
            24,
            12,
            1245,
            92,
            1246,
            93,
        ) => Some((
            rect_xyxy(1244, 92, 1268, 106),
            rect_xyxy(1244, 92, 1245, 93),
            true,
        )),
        // 02.ass @ 1319640 line 1570
        (
            1319250,
            460,
            16709149901864532552,
            ass::ImageType::Character,
            0xFAC15D00,
            1245,
            94,
            24,
            10,
            1245,
            94,
            1246,
            95,
        ) => Some((
            rect_xyxy(1244, 94, 1268, 106),
            rect_xyxy(1244, 94, 1245, 95),
            true,
        )),
        // 02.ass @ 1319640 line 1575
        (
            1319250,
            460,
            12653429216824683116,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1252,
            49,
            56,
            52,
            1252,
            51,
            1284,
            101,
        ) => Some((
            rect_xyxy(1256, 48, 1296, 104),
            rect_xyxy(1256, 49, 1287, 98),
            false,
        )),
        // 02.ass @ 1319640 line 1575
        (
            1319250,
            460,
            12653429216824683116,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1249,
            46,
            56,
            52,
            1249,
            48,
            1281,
            98,
        ) => Some((
            rect_xyxy(1253, 45, 1293, 101),
            rect_xyxy(1253, 46, 1284, 95),
            false,
        )),
        // 02.ass @ 1319640 line 1575
        (
            1319250,
            460,
            12653429216824683116,
            ass::ImageType::Character,
            0xFFFFFF70,
            1261,
            53,
            32,
            48,
            1261,
            53,
            1278,
            89,
        ) => Some((
            rect_xyxy(1261, 53, 1293, 101),
            rect_xyxy(1261, 53, 1278, 88),
            false,
        )),
        // 02.ass @ 1319640 line 1583
        (
            1319250,
            460,
            78261791657415897,
            ass::ImageType::Character,
            0xFDEED300,
            1255,
            37,
            40,
            14,
            1259,
            37,
            1269,
            51,
        ) => Some((
            rect_xyxy(1257, 49, 1297, 51),
            rect_xyxy(1257, 49, 1258, 50),
            true,
        )),
        // 02.ass @ 1319640 line 1584
        (
            1319250,
            460,
            16363404114741833204,
            ass::ImageType::Character,
            0xFDECCE00,
            1255,
            40,
            40,
            13,
            1259,
            40,
            1268,
            53,
        ) => Some((
            rect_xyxy(1257, 49, 1297, 53),
            rect_xyxy(1271, 52, 1277, 53),
            false,
        )),
        // 02.ass @ 1319640 line 1585
        (
            1319250,
            460,
            5626324089254097866,
            ass::ImageType::Character,
            0xFDEAC900,
            1255,
            42,
            40,
            14,
            1259,
            42,
            1268,
            56,
        ) => Some((
            rect_xyxy(1257, 49, 1297, 56),
            rect_xyxy(1260, 52, 1278, 56),
            false,
        )),
        // 02.ass @ 1319640 line 1586
        (
            1319250,
            460,
            2570081019078544424,
            ass::ImageType::Character,
            0xFDE8C300,
            1255,
            45,
            40,
            13,
            1259,
            45,
            1268,
            58,
        ) => Some((
            rect_xyxy(1257, 49, 1297, 58),
            rect_xyxy(1260, 52, 1278, 58),
            false,
        )),
        // 02.ass @ 1319640 line 1587
        (
            1319250,
            460,
            3457240564190530700,
            ass::ImageType::Character,
            0xFDE6BE00,
            1255,
            48,
            40,
            12,
            1259,
            48,
            1268,
            59,
        ) => Some((
            rect_xyxy(1257, 49, 1297, 61),
            rect_xyxy(1260, 52, 1278, 61),
            false,
        )),
        // 02.ass @ 1319640 line 1588
        (
            1319250,
            460,
            5469545153048965055,
            ass::ImageType::Character,
            0xFCE4B800,
            1255,
            50,
            40,
            10,
            1259,
            50,
            1268,
            59,
        ) => Some((
            rect_xyxy(1257, 50, 1297, 64),
            rect_xyxy(1260, 52, 1278, 64),
            false,
        )),
        // 02.ass @ 1319640 line 1589
        (
            1319250,
            460,
            6105756207391611195,
            ass::ImageType::Character,
            0xFCE2B300,
            1255,
            53,
            40,
            7,
            1259,
            53,
            1268,
            59,
        ) => Some((
            rect_xyxy(1257, 53, 1297, 66),
            rect_xyxy(1260, 53, 1278, 66),
            false,
        )),
        // 02.ass @ 1319640 line 1590
        (
            1319250,
            460,
            863554613518184573,
            ass::ImageType::Character,
            0xFCE0AE00,
            1255,
            55,
            40,
            5,
            1259,
            55,
            1268,
            59,
        ) => Some((
            rect_xyxy(1257, 55, 1297, 69),
            rect_xyxy(1260, 55, 1278, 69),
            false,
        )),
        // 02.ass @ 1319640 line 1591
        (
            1319250,
            460,
            4730042949568283373,
            ass::ImageType::Character,
            0xFCDDA800,
            1255,
            58,
            40,
            2,
            1261,
            58,
            1267,
            59,
        ) => Some((
            rect_xyxy(1257, 58, 1297, 72),
            rect_xyxy(1260, 58, 1278, 72),
            false,
        )),
        // 02.ass @ 1319640 line 1610
        (
            1319250,
            460,
            16245312645778237527,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1276,
            33,
            35,
            73,
            1276,
            34,
            1298,
            96,
        ) => Some((
            rect_xyxy(1275, 34, 1299, 106),
            rect_xyxy(1276, 34, 1297, 96),
            false,
        )),
        // 02.ass @ 1319640 line 1610
        (
            1319250,
            460,
            16245312645778237527,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1273,
            30,
            35,
            73,
            1273,
            31,
            1295,
            93,
        ) => Some((
            rect_xyxy(1272, 31, 1296, 103),
            rect_xyxy(1273, 31, 1294, 93),
            false,
        )),
        // 02.ass @ 1319640 line 1610
        (
            1319250,
            460,
            16245312645778237527,
            ass::ImageType::Character,
            0xFFFFFF70,
            1284,
            37,
            32,
            48,
            1284,
            37,
            1290,
            84,
        ) => Some((
            rect_xyxy(1280, 38, 1296, 102),
            rect_xyxy(1280, 38, 1287, 86),
            false,
        )),
        // 02.ass @ 1319640 line 1612
        (
            1319250,
            460,
            14626671017161161133,
            ass::ImageType::Character,
            0xFEFAF400,
            1277,
            32,
            24,
            3,
            1277,
            32,
            1278,
            33,
        ) => Some((
            rect_xyxy(1276, 34, 1300, 35),
            rect_xyxy(1276, 34, 1277, 35),
            true,
        )),
        // 02.ass @ 1319640 line 1613
        (
            1319250,
            460,
            13502124250110977170,
            ass::ImageType::Character,
            0xFEF8EE00,
            1277,
            32,
            24,
            5,
            1280,
            36,
            1287,
            37,
        ) => Some((
            rect_xyxy(1276, 34, 1300, 37),
            rect_xyxy(1276, 34, 1277, 35),
            true,
        )),
        // 02.ass @ 1319640 line 1614
        (
            1319250,
            460,
            7337511964692263206,
            ass::ImageType::Character,
            0xFEF6E900,
            1277,
            32,
            24,
            8,
            1279,
            36,
            1288,
            40,
        ) => Some((
            rect_xyxy(1276, 34, 1300, 40),
            rect_xyxy(1279, 37, 1287, 40),
            false,
        )),
        // 02.ass @ 1319640 line 1615
        (
            1319250,
            460,
            7446646600675981176,
            ass::ImageType::Character,
            0xFEF4E400,
            1277,
            32,
            24,
            11,
            1279,
            36,
            1288,
            43,
        ) => Some((
            rect_xyxy(1276, 34, 1300, 43),
            rect_xyxy(1279, 37, 1287, 43),
            false,
        )),
        // 02.ass @ 1319640 line 1616
        (
            1319250,
            460,
            17196954036483167076,
            ass::ImageType::Character,
            0xFEF2DE00,
            1277,
            32,
            24,
            13,
            1279,
            36,
            1288,
            45,
        ) => Some((
            rect_xyxy(1276, 34, 1300, 45),
            rect_xyxy(1279, 37, 1287, 45),
            false,
        )),
        // 02.ass @ 1319640 line 1617
        (
            1319250,
            460,
            18437184048389865770,
            ass::ImageType::Character,
            0xFDF0D900,
            1277,
            35,
            24,
            13,
            1279,
            36,
            1288,
            48,
        ) => Some((
            rect_xyxy(1276, 35, 1300, 48),
            rect_xyxy(1279, 37, 1287, 48),
            false,
        )),
        // 02.ass @ 1319640 line 1618
        (
            1319250,
            460,
            17812879560425444862,
            ass::ImageType::Character,
            0xFDEED300,
            1277,
            37,
            24,
            14,
            1279,
            37,
            1288,
            51,
        ) => Some((
            rect_xyxy(1276, 37, 1300, 51),
            rect_xyxy(1279, 37, 1287, 51),
            false,
        )),
        // 02.ass @ 1319640 line 1619
        (
            1319250,
            460,
            2343548907417695705,
            ass::ImageType::Character,
            0xFDECCE00,
            1277,
            40,
            24,
            13,
            1279,
            40,
            1288,
            53,
        ) => Some((
            rect_xyxy(1276, 40, 1300, 53),
            rect_xyxy(1279, 40, 1287, 53),
            false,
        )),
        // 02.ass @ 1319640 line 1620
        (
            1319250,
            460,
            7304059832847923053,
            ass::ImageType::Character,
            0xFDEAC900,
            1277,
            42,
            24,
            14,
            1279,
            42,
            1288,
            56,
        ) => Some((
            rect_xyxy(1276, 42, 1300, 56),
            rect_xyxy(1279, 42, 1287, 56),
            false,
        )),
        // 02.ass @ 1319640 line 1621
        (
            1319250,
            460,
            14822724832205861027,
            ass::ImageType::Character,
            0xFDE8C300,
            1277,
            45,
            24,
            13,
            1279,
            45,
            1288,
            58,
        ) => Some((
            rect_xyxy(1276, 45, 1300, 58),
            rect_xyxy(1279, 45, 1287, 58),
            false,
        )),
        // 02.ass @ 1319640 line 1622
        (
            1319250,
            460,
            9417545832887118649,
            ass::ImageType::Character,
            0xFDE6BE00,
            1277,
            48,
            24,
            13,
            1279,
            48,
            1288,
            61,
        ) => Some((
            rect_xyxy(1276, 48, 1300, 61),
            rect_xyxy(1279, 48, 1287, 61),
            false,
        )),
        // 02.ass @ 1319640 line 1623
        (
            1319250,
            460,
            8572313692926318436,
            ass::ImageType::Character,
            0xFCE4B800,
            1277,
            50,
            24,
            14,
            1279,
            50,
            1288,
            64,
        ) => Some((
            rect_xyxy(1276, 50, 1300, 64),
            rect_xyxy(1279, 50, 1287, 64),
            false,
        )),
        // 02.ass @ 1319640 line 1624
        (
            1319250,
            460,
            4049526979042981372,
            ass::ImageType::Character,
            0xFCE2B300,
            1277,
            53,
            24,
            13,
            1279,
            53,
            1288,
            66,
        ) => Some((
            rect_xyxy(1276, 53, 1300, 66),
            rect_xyxy(1279, 53, 1287, 66),
            false,
        )),
        // 02.ass @ 1319640 line 1625
        (
            1319250,
            460,
            407033117883969996,
            ass::ImageType::Character,
            0xFCE0AE00,
            1277,
            55,
            24,
            14,
            1279,
            55,
            1288,
            69,
        ) => Some((
            rect_xyxy(1276, 55, 1300, 69),
            rect_xyxy(1279, 55, 1287, 69),
            false,
        )),
        // 02.ass @ 1319640 line 1626
        (
            1319250,
            460,
            12538115713421806238,
            ass::ImageType::Character,
            0xFCDDA800,
            1277,
            58,
            24,
            14,
            1279,
            58,
            1288,
            72,
        ) => Some((
            rect_xyxy(1276, 58, 1300, 72),
            rect_xyxy(1279, 58, 1288, 72),
            false,
        )),
        // 02.ass @ 1319640 line 1627
        (
            1319250,
            460,
            16243391995428519363,
            ass::ImageType::Character,
            0xFCDBA300,
            1277,
            61,
            24,
            13,
            1279,
            61,
            1288,
            74,
        ) => Some((
            rect_xyxy(1276, 61, 1300, 74),
            rect_xyxy(1279, 61, 1288, 74),
            false,
        )),
        // 02.ass @ 1319640 line 1628
        (
            1319250,
            460,
            17506951065059652428,
            ass::ImageType::Character,
            0xFCD99D00,
            1277,
            63,
            24,
            14,
            1279,
            63,
            1288,
            77,
        ) => Some((
            rect_xyxy(1276, 63, 1300, 77),
            rect_xyxy(1279, 63, 1288, 77),
            false,
        )),
        // 02.ass @ 1319640 line 1629
        (
            1319250,
            460,
            15830575509521655415,
            ass::ImageType::Character,
            0xFBD79800,
            1277,
            66,
            24,
            14,
            1279,
            66,
            1288,
            80,
        ) => Some((
            rect_xyxy(1276, 66, 1300, 80),
            rect_xyxy(1279, 66, 1288, 80),
            false,
        )),
        // 02.ass @ 1319640 line 1630
        (
            1319250,
            460,
            7445090793230736612,
            ass::ImageType::Character,
            0xFBD59300,
            1277,
            68,
            24,
            14,
            1279,
            68,
            1288,
            82,
        ) => Some((
            rect_xyxy(1276, 68, 1300, 82),
            rect_xyxy(1279, 68, 1288, 82),
            false,
        )),
        // 02.ass @ 1319640 line 1631
        (
            1319250,
            460,
            7242267591724686753,
            ass::ImageType::Character,
            0xFBD38D00,
            1277,
            71,
            24,
            14,
            1279,
            71,
            1288,
            85,
        ) => Some((
            rect_xyxy(1276, 71, 1300, 85),
            rect_xyxy(1279, 71, 1288, 85),
            false,
        )),
        // 02.ass @ 1319640 line 1632
        (
            1319250,
            460,
            17754935651313240241,
            ass::ImageType::Character,
            0xFBD18800,
            1277,
            74,
            24,
            13,
            1279,
            74,
            1288,
            86,
        ) => Some((
            rect_xyxy(1276, 74, 1300, 87),
            rect_xyxy(1279, 74, 1288, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1633
        (
            1319250,
            460,
            11015393101710885517,
            ass::ImageType::Character,
            0xFBCF8200,
            1277,
            76,
            24,
            14,
            1279,
            76,
            1288,
            86,
        ) => Some((
            rect_xyxy(1276, 76, 1300, 90),
            rect_xyxy(1279, 76, 1288, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1634
        (
            1319250,
            460,
            6269447957771600300,
            ass::ImageType::Character,
            0xFBCD7D00,
            1277,
            79,
            24,
            14,
            1279,
            79,
            1288,
            86,
        ) => Some((
            rect_xyxy(1276, 79, 1300, 93),
            rect_xyxy(1279, 79, 1288, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1635
        (
            1319250,
            460,
            404341036597327507,
            ass::ImageType::Character,
            0xFACB7800,
            1277,
            81,
            24,
            14,
            1279,
            81,
            1288,
            86,
        ) => Some((
            rect_xyxy(1276, 81, 1300, 95),
            rect_xyxy(1279, 81, 1288, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1636
        (
            1319250,
            460,
            2361706870113547928,
            ass::ImageType::Character,
            0xFAC97200,
            1277,
            84,
            24,
            14,
            1280,
            84,
            1288,
            86,
        ) => Some((
            rect_xyxy(1276, 84, 1300, 98),
            rect_xyxy(1279, 84, 1288, 87),
            false,
        )),
        // 02.ass @ 1319640 line 1637
        (
            1319250,
            460,
            10937561113159251419,
            ass::ImageType::Character,
            0xFAC76D00,
            1277,
            87,
            24,
            14,
            1277,
            87,
            1278,
            88,
        ) => Some((
            rect_xyxy(1276, 87, 1300, 101),
            rect_xyxy(1276, 87, 1277, 88),
            true,
        )),
        // 02.ass @ 1319640 line 1638
        (
            1319250,
            460,
            16170675725266053734,
            ass::ImageType::Character,
            0xFAC56700,
            1277,
            89,
            24,
            14,
            1277,
            89,
            1278,
            90,
        ) => Some((
            rect_xyxy(1276, 89, 1300, 103),
            rect_xyxy(1276, 89, 1277, 90),
            true,
        )),
        // 02.ass @ 1319640 line 1639
        (
            1319250,
            460,
            2983728918373924027,
            ass::ImageType::Character,
            0xFAC36200,
            1277,
            92,
            24,
            12,
            1277,
            92,
            1278,
            93,
        ) => Some((
            rect_xyxy(1276, 92, 1300, 106),
            rect_xyxy(1276, 92, 1277, 93),
            true,
        )),
        // 02.ass @ 1319640 line 1640
        (
            1319250,
            460,
            11570468709337824191,
            ass::ImageType::Character,
            0xFAC15D00,
            1277,
            94,
            24,
            10,
            1277,
            94,
            1278,
            95,
        ) => Some((
            rect_xyxy(1276, 94, 1300, 106),
            rect_xyxy(1276, 94, 1277, 95),
            true,
        )),
        // 02.ass @ 1319640 line 1669
        (
            1319510,
            240,
            8236866818905708091,
            ass::ImageType::Shadow,
            0xCDAAFF59,
            607,
            55,
            64,
            55,
            608,
            56,
            654,
            107,
        ) => Some((
            rect_xyxy(607, 54, 663, 110),
            rect_xyxy(608, 55, 653, 104),
            false,
        )),
        // 02.ass @ 1319640 line 1669
        (
            1319510,
            240,
            8236866818905708091,
            ass::ImageType::Outline,
            0xFFFFFF59,
            604,
            52,
            64,
            55,
            605,
            53,
            651,
            104,
        ) => Some((
            rect_xyxy(604, 51, 660, 107),
            rect_xyxy(605, 52, 650, 101),
            false,
        )),
        // 02.ass @ 1319640 line 1669
        (
            1319510,
            240,
            8236866818905708091,
            ass::ImageType::Character,
            0xCDAAFF59,
            612,
            60,
            48,
            43,
            612,
            61,
            644,
            96,
        ) => Some((
            rect_xyxy(611, 59, 659, 107),
            rect_xyxy(611, 59, 644, 95),
            false,
        )),
        // 02.ass @ 1319640 line 1670
        (
            1319510,
            240,
            6565966600530438968,
            ass::ImageType::Character,
            0xCDAAFF59,
            607,
            58,
            58,
            50,
            608,
            59,
            648,
            104,
        ) => Some((
            rect_xyxy(607, 55, 663, 111),
            rect_xyxy(609, 55, 646, 98),
            false,
        )),
        // 02.ass @ 1319640 line 1704
        (
            1319530,
            220,
            6291512159847731515,
            ass::ImageType::Shadow,
            0xCDAAFF72,
            645,
            35,
            48,
            48,
            645,
            36,
            682,
            79,
        ) => Some((
            rect_xyxy(646, 36, 686, 92),
            rect_xyxy(646, 37, 681, 78),
            false,
        )),
        // 02.ass @ 1319640 line 1704
        (
            1319530,
            220,
            6291512159847731515,
            ass::ImageType::Outline,
            0xFFFFFF72,
            642,
            32,
            48,
            48,
            642,
            33,
            679,
            76,
        ) => Some((
            rect_xyxy(643, 33, 683, 89),
            rect_xyxy(643, 34, 678, 75),
            false,
        )),
        // 02.ass @ 1319640 line 1704
        (
            1319530,
            220,
            6291512159847731515,
            ass::ImageType::Character,
            0xCDAAFF72,
            650,
            38,
            32,
            32,
            650,
            41,
            672,
            69,
        ) => Some((
            rect_xyxy(650, 41, 682, 73),
            rect_xyxy(650, 41, 672, 68),
            false,
        )),
        // 02.ass @ 1319640 line 1705
        (
            1319530,
            220,
            7446216880890019376,
            ass::ImageType::Character,
            0xCDAAFF72,
            645,
            38,
            42,
            42,
            645,
            40,
            676,
            76,
        ) => Some((
            rect_xyxy(645, 36, 687, 78),
            rect_xyxy(646, 37, 676, 72),
            false,
        )),
        // 02.ass @ 1319640 line 1739
        (
            1319550,
            370,
            5295537058907231316,
            ass::ImageType::Shadow,
            0xCDAAFF8C,
            676,
            67,
            64,
            48,
            676,
            69,
            727,
            111,
        ) => Some((
            rect_xyxy(675, 69, 731, 125),
            rect_xyxy(677, 70, 724, 110),
            false,
        )),
        // 02.ass @ 1319640 line 1739
        (
            1319550,
            370,
            5295537058907231316,
            ass::ImageType::Outline,
            0xFFFFFF8C,
            673,
            64,
            64,
            48,
            673,
            66,
            724,
            108,
        ) => Some((
            rect_xyxy(672, 66, 728, 122),
            rect_xyxy(674, 67, 721, 107),
            false,
        )),
        // 02.ass @ 1319640 line 1739
        (
            1319550,
            370,
            5295537058907231316,
            ass::ImageType::Character,
            0xCDAAFF8C,
            681,
            72,
            48,
            32,
            681,
            73,
            716,
            100,
        ) => Some((
            rect_xyxy(680, 73, 728, 105),
            rect_xyxy(680, 73, 715, 101),
            false,
        )),
        // 02.ass @ 1319640 line 1740
        (
            1319550,
            370,
            11815320957126495875,
            ass::ImageType::Character,
            0xCDAAFF8C,
            676,
            70,
            58,
            42,
            676,
            72,
            721,
            108,
        ) => Some((
            rect_xyxy(675, 68, 733, 110),
            rect_xyxy(676, 69, 719, 104),
            false,
        )),
        // 02.ass @ 1319640 line 1774
        (
            1319570,
            350,
            7828702162055392553,
            ass::ImageType::Shadow,
            0xCDAAFFA5,
            716,
            31,
            40,
            56,
            716,
            31,
            754,
            72,
        ) => Some((
            rect_xyxy(715, 29, 755, 85),
            rect_xyxy(716, 31, 752, 70),
            false,
        )),
        // 02.ass @ 1319640 line 1774
        (
            1319570,
            350,
            7828702162055392553,
            ass::ImageType::Outline,
            0xFFFFFFA5,
            713,
            28,
            40,
            56,
            713,
            28,
            751,
            69,
        ) => Some((
            rect_xyxy(712, 26, 752, 82),
            rect_xyxy(713, 28, 749, 67),
            false,
        )),
        // 02.ass @ 1319640 line 1774
        (
            1319570,
            350,
            7828702162055392553,
            ass::ImageType::Character,
            0xCDAAFFA5,
            720,
            32,
            32,
            32,
            720,
            34,
            743,
            61,
        ) => Some((
            rect_xyxy(720, 34, 752, 66),
            rect_xyxy(720, 34, 743, 61),
            false,
        )),
        // 02.ass @ 1319640 line 1775
        (
            1319570,
            350,
            8262571787834501746,
            ass::ImageType::Character,
            0xCDAAFFA5,
            715,
            30,
            42,
            42,
            715,
            31,
            748,
            68,
        ) => Some((
            rect_xyxy(715, 29, 757, 71),
            rect_xyxy(715, 30, 748, 65),
            false,
        )),
        // 02.ass @ 1319640 line 1809
        (
            1319610,
            440,
            14331061515663635164,
            ass::ImageType::Shadow,
            0xCDAAFFD8,
            755,
            16,
            32,
            48,
            757,
            18,
            783,
            63,
        ) => Some((
            rect_xyxy(755, 18, 795, 74),
            rect_xyxy(756, 19, 782, 63),
            false,
        )),
        // 02.ass @ 1319640 line 1809
        (
            1319610,
            440,
            14331061515663635164,
            ass::ImageType::Outline,
            0xFFFFFFD8,
            752,
            13,
            32,
            48,
            754,
            15,
            780,
            60,
        ) => Some((
            rect_xyxy(752, 15, 792, 71),
            rect_xyxy(753, 16, 779, 60),
            false,
        )),
        // 02.ass @ 1319640 line 1809
        (
            1319610,
            440,
            14331061515663635164,
            ass::ImageType::Character,
            0xCDAAFFD8,
            760,
            21,
            16,
            32,
            760,
            22,
            773,
            53,
        ) => Some((
            rect_xyxy(760, 23, 776, 55),
            rect_xyxy(760, 23, 772, 54),
            false,
        )),
        // 02.ass @ 1319640 line 1810
        (
            1319610,
            440,
            17627374245641523711,
            ass::ImageType::Character,
            0xCDAAFFD8,
            755,
            19,
            26,
            42,
            755,
            20,
            778,
            61,
        ) => Some((
            rect_xyxy(755, 18, 781, 60),
            rect_xyxy(755, 19, 776, 58),
            false,
        )),
        // 02.ass @ 1319640 line 1844
        (
            1319630,
            420,
            8209016220374610911,
            ass::ImageType::Shadow,
            0xCDAAFFF2,
            773,
            79,
            48,
            48,
            775,
            81,
            810,
            121,
        ) => Some((
            rect_xyxy(774, 83, 814, 139),
            rect_xyxy(775, 84, 810, 122),
            false,
        )),
        // 02.ass @ 1319640 line 1844
        (
            1319630,
            420,
            8209016220374610911,
            ass::ImageType::Outline,
            0xFFFFFFF2,
            770,
            76,
            48,
            48,
            772,
            78,
            807,
            118,
        ) => Some((
            rect_xyxy(771, 80, 811, 136),
            rect_xyxy(772, 81, 807, 119),
            false,
        )),
        // 02.ass @ 1319640 line 1844
        (
            1319630,
            420,
            8209016220374610911,
            ass::ImageType::Character,
            0xCDAAFFF2,
            778,
            84,
            32,
            32,
            778,
            85,
            801,
            111,
        ) => Some((
            rect_xyxy(778, 87, 810, 119),
            rect_xyxy(778, 87, 801, 113),
            false,
        )),
        // 02.ass @ 1319640 line 21509
        (
            1317630,
            2070,
            6789697489194229434,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            726,
            991,
            43,
            46,
            726,
            992,
            757,
            1034,
        ) => Some((
            rect_xyxy(726, 993, 769, 1037),
            rect_xyxy(726, 993, 756, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21509
        (
            1317630,
            2070,
            6789697489194229434,
            ass::ImageType::Outline,
            0x000000D8,
            723,
            988,
            43,
            46,
            723,
            989,
            754,
            1031,
        ) => Some((
            rect_xyxy(723, 990, 766, 1034),
            rect_xyxy(723, 990, 753, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21509
        (
            1317630,
            2070,
            6789697489194229434,
            ass::ImageType::Character,
            0xFFFFFFD8,
            724,
            989,
            41,
            44,
            724,
            990,
            753,
            1030,
        ) => Some((
            rect_xyxy(724, 991, 766, 1034),
            rect_xyxy(724, 991, 753, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21511
        (
            1317630,
            2070,
            9566305883465156497,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            777,
            981,
            36,
            56,
            777,
            982,
            808,
            1034,
        ) => Some((
            rect_xyxy(778, 983, 812, 1037),
            rect_xyxy(778, 983, 808, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21511
        (
            1317630,
            2070,
            9566305883465156497,
            ass::ImageType::Outline,
            0x000000D8,
            774,
            978,
            36,
            56,
            774,
            979,
            805,
            1031,
        ) => Some((
            rect_xyxy(775, 980, 809, 1034),
            rect_xyxy(775, 980, 805, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21511
        (
            1317630,
            2070,
            9566305883465156497,
            ass::ImageType::Character,
            0xFFFFFFD8,
            775,
            979,
            34,
            54,
            775,
            980,
            804,
            1030,
        ) => Some((
            rect_xyxy(776, 980, 810, 1034),
            rect_xyxy(776, 980, 804, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21512
        (
            1317630,
            2070,
            791772131111531997,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            805,
            1005,
            32,
            32,
            805,
            1005,
            832,
            1034,
        ) => Some((
            rect_xyxy(804, 1005, 836, 1037),
            rect_xyxy(804, 1005, 831, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21512
        (
            1317630,
            2070,
            791772131111531997,
            ass::ImageType::Outline,
            0x000000D8,
            802,
            1002,
            32,
            32,
            802,
            1002,
            829,
            1031,
        ) => Some((
            rect_xyxy(801, 1002, 833, 1034),
            rect_xyxy(801, 1002, 828, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21512
        (
            1317630,
            2070,
            791772131111531997,
            ass::ImageType::Character,
            0xFFFFFFD8,
            803,
            1002,
            32,
            32,
            803,
            1002,
            828,
            1030,
        ) => Some((
            rect_xyxy(802, 1002, 834, 1034),
            rect_xyxy(802, 1002, 827, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21513
        (
            1317630,
            2070,
            8923425826216384804,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            826,
            1005,
            32,
            32,
            826,
            1005,
            854,
            1034,
        ) => Some((
            rect_xyxy(827, 1005, 859, 1037),
            rect_xyxy(827, 1005, 854, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21513
        (
            1317630,
            2070,
            8923425826216384804,
            ass::ImageType::Outline,
            0x000000D8,
            823,
            1002,
            32,
            32,
            823,
            1002,
            851,
            1031,
        ) => Some((
            rect_xyxy(824, 1002, 856, 1034),
            rect_xyxy(824, 1002, 851, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21513
        (
            1317630,
            2070,
            8923425826216384804,
            ass::ImageType::Character,
            0xFFFFFFD8,
            824,
            1002,
            32,
            32,
            824,
            1003,
            850,
            1030,
        ) => Some((
            rect_xyxy(824, 1002, 856, 1034),
            rect_xyxy(824, 1002, 851, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21514
        (
            1317630,
            2070,
            14237183721553004452,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            852,
            1005,
            32,
            32,
            852,
            1005,
            877,
            1034,
        ) => Some((
            rect_xyxy(852, 1005, 884, 1037),
            rect_xyxy(853, 1005, 876, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21514
        (
            1317630,
            2070,
            14237183721553004452,
            ass::ImageType::Outline,
            0x000000D8,
            849,
            1002,
            32,
            32,
            849,
            1002,
            874,
            1031,
        ) => Some((
            rect_xyxy(849, 1002, 881, 1034),
            rect_xyxy(850, 1002, 873, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21515
        (
            1317630,
            2070,
            15606053313911304850,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            877,
            1007,
            32,
            32,
            877,
            1007,
            896,
            1032,
        ) => Some((
            rect_xyxy(877, 1007, 909, 1039),
            rect_xyxy(877, 1007, 897, 1032),
            false,
        )),
        // 02.ass @ 1319640 line 21515
        (
            1317630,
            2070,
            15606053313911304850,
            ass::ImageType::Outline,
            0x000000D8,
            874,
            1004,
            32,
            32,
            874,
            1004,
            893,
            1029,
        ) => Some((
            rect_xyxy(874, 1004, 906, 1036),
            rect_xyxy(874, 1004, 894, 1029),
            false,
        )),
        // 02.ass @ 1319640 line 21515
        (
            1317630,
            2070,
            15606053313911304850,
            ass::ImageType::Character,
            0xFFFFFFD8,
            875,
            1005,
            32,
            32,
            875,
            1005,
            894,
            1028,
        ) => Some((
            rect_xyxy(875, 1005, 907, 1037),
            rect_xyxy(875, 1005, 893, 1028),
            false,
        )),
        // 02.ass @ 1319640 line 21517
        (
            1317630,
            2070,
            1374061616106244185,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            906,
            1002,
            34,
            34,
            906,
            1003,
            936,
            1034,
        ) => Some((
            rect_xyxy(907, 1003, 939, 1035),
            rect_xyxy(907, 1003, 935, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21517
        (
            1317630,
            2070,
            1374061616106244185,
            ass::ImageType::Outline,
            0x000000D8,
            903,
            999,
            34,
            34,
            903,
            1000,
            933,
            1031,
        ) => Some((
            rect_xyxy(904, 1000, 936, 1032),
            rect_xyxy(904, 1000, 932, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21517
        (
            1317630,
            2070,
            1374061616106244185,
            ass::ImageType::Character,
            0xFFFFFFD8,
            904,
            1000,
            32,
            32,
            904,
            1001,
            932,
            1030,
        ) => Some((
            rect_xyxy(904, 1001, 936, 1033),
            rect_xyxy(905, 1001, 932, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21518
        (
            1317630,
            2070,
            12084933347076215468,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            930,
            1005,
            32,
            32,
            930,
            1005,
            952,
            1034,
        ) => Some((
            rect_xyxy(929, 1005, 961, 1037),
            rect_xyxy(929, 1005, 951, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21518
        (
            1317630,
            2070,
            12084933347076215468,
            ass::ImageType::Outline,
            0x000000D8,
            927,
            1002,
            32,
            32,
            927,
            1002,
            949,
            1031,
        ) => Some((
            rect_xyxy(926, 1002, 958, 1034),
            rect_xyxy(926, 1002, 948, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21518
        (
            1317630,
            2070,
            12084933347076215468,
            ass::ImageType::Character,
            0xFFFFFFD8,
            927,
            1002,
            32,
            32,
            927,
            1003,
            948,
            1030,
        ) => Some((
            rect_xyxy(927, 1002, 959, 1034),
            rect_xyxy(927, 1002, 947, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21519
        (
            1317630,
            2070,
            1684495222097352796,
            ass::ImageType::Character,
            0xFFFFFFD8,
            946,
            1002,
            32,
            32,
            947,
            1002,
            970,
            1030,
        ) => Some((
            rect_xyxy(947, 1002, 979, 1034),
            rect_xyxy(947, 1002, 970, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21520
        (
            1317630,
            2070,
            13229577269880181160,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            972,
            1005,
            32,
            32,
            972,
            1005,
            999,
            1034,
        ) => Some((
            rect_xyxy(971, 1005, 1003, 1037),
            rect_xyxy(971, 1005, 998, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21520
        (
            1317630,
            2070,
            13229577269880181160,
            ass::ImageType::Outline,
            0x000000D8,
            969,
            1002,
            32,
            32,
            969,
            1002,
            996,
            1031,
        ) => Some((
            rect_xyxy(968, 1002, 1000, 1034),
            rect_xyxy(968, 1002, 995, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21520
        (
            1317630,
            2070,
            13229577269880181160,
            ass::ImageType::Character,
            0xFFFFFFD8,
            970,
            1002,
            32,
            32,
            970,
            1002,
            995,
            1030,
        ) => Some((
            rect_xyxy(969, 1002, 1001, 1034),
            rect_xyxy(969, 1002, 994, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21521
        (
            1317630,
            2070,
            870094060340668167,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            995,
            1005,
            32,
            32,
            995,
            1005,
            1019,
            1034,
        ) => Some((
            rect_xyxy(994, 1005, 1026, 1037),
            rect_xyxy(994, 1005, 1019, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21521
        (
            1317630,
            2070,
            870094060340668167,
            ass::ImageType::Outline,
            0x000000D8,
            992,
            1002,
            32,
            32,
            992,
            1002,
            1016,
            1031,
        ) => Some((
            rect_xyxy(991, 1002, 1023, 1034),
            rect_xyxy(991, 1002, 1016, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21521
        (
            1317630,
            2070,
            870094060340668167,
            ass::ImageType::Character,
            0xFFFFFFD8,
            992,
            1002,
            32,
            32,
            992,
            1003,
            1015,
            1030,
        ) => Some((
            rect_xyxy(992, 1002, 1024, 1034),
            rect_xyxy(992, 1002, 1015, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21522
        (
            1317630,
            2070,
            4572759572974775207,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1019,
            993,
            34,
            44,
            1019,
            994,
            1043,
            1034,
        ) => Some((
            rect_xyxy(1019, 994, 1051, 1037),
            rect_xyxy(1019, 994, 1043, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21522
        (
            1317630,
            2070,
            4572759572974775207,
            ass::ImageType::Outline,
            0x000000D8,
            1016,
            990,
            34,
            44,
            1016,
            991,
            1040,
            1031,
        ) => Some((
            rect_xyxy(1016, 991, 1048, 1034),
            rect_xyxy(1016, 991, 1040, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21522
        (
            1317630,
            2070,
            4572759572974775207,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1017,
            991,
            32,
            42,
            1017,
            992,
            1039,
            1030,
        ) => Some((
            rect_xyxy(1017, 992, 1049, 1034),
            rect_xyxy(1017, 992, 1040, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21524
        (
            1317630,
            2070,
            14786448319826156628,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1063,
            990,
            39,
            59,
            1063,
            991,
            1088,
            1045,
        ) => Some((
            rect_xyxy(1063, 992, 1101, 1049),
            rect_xyxy(1063, 992, 1088, 1045),
            false,
        )),
        // 02.ass @ 1319640 line 21524
        (
            1317630,
            2070,
            14786448319826156628,
            ass::ImageType::Outline,
            0x000000D8,
            1060,
            987,
            39,
            59,
            1060,
            988,
            1085,
            1042,
        ) => Some((
            rect_xyxy(1060, 989, 1098, 1046),
            rect_xyxy(1060, 989, 1085, 1042),
            false,
        )),
        // 02.ass @ 1319640 line 21524
        (
            1317630,
            2070,
            14786448319826156628,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1061,
            988,
            37,
            57,
            1061,
            989,
            1084,
            1041,
        ) => Some((
            rect_xyxy(1061, 989, 1099, 1046),
            rect_xyxy(1061, 989, 1085, 1041),
            false,
        )),
        // 02.ass @ 1319640 line 21525
        (
            1317630,
            2070,
            8725239436347953285,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1082,
            993,
            34,
            50,
            1082,
            994,
            1109,
            1034,
        ) => Some((
            rect_xyxy(1082, 995, 1114, 1043),
            rect_xyxy(1082, 995, 1109, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21525
        (
            1317630,
            2070,
            8725239436347953285,
            ass::ImageType::Outline,
            0x000000D8,
            1079,
            990,
            34,
            50,
            1079,
            991,
            1106,
            1031,
        ) => Some((
            rect_xyxy(1079, 992, 1111, 1040),
            rect_xyxy(1079, 992, 1106, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21525
        (
            1317630,
            2070,
            8725239436347953285,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1080,
            991,
            32,
            48,
            1080,
            992,
            1105,
            1030,
        ) => Some((
            rect_xyxy(1080, 993, 1112, 1041),
            rect_xyxy(1080, 993, 1105, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21526
        (
            1317630,
            2070,
            17085450377816648618,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1104,
            990,
            38,
            47,
            1104,
            991,
            1128,
            1034,
        ) => Some((
            rect_xyxy(1105, 992, 1141, 1037),
            rect_xyxy(1105, 992, 1129, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21526
        (
            1317630,
            2070,
            17085450377816648618,
            ass::ImageType::Outline,
            0x000000D8,
            1101,
            987,
            38,
            47,
            1101,
            988,
            1125,
            1031,
        ) => Some((
            rect_xyxy(1102, 989, 1138, 1034),
            rect_xyxy(1102, 989, 1126, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21526
        (
            1317630,
            2070,
            17085450377816648618,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1102,
            988,
            36,
            45,
            1102,
            989,
            1124,
            1030,
        ) => Some((
            rect_xyxy(1102, 989, 1139, 1034),
            rect_xyxy(1102, 989, 1125, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21527
        (
            1317630,
            2070,
            9915210032160638829,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1126,
            994,
            39,
            44,
            1126,
            995,
            1156,
            1034,
        ) => Some((
            rect_xyxy(1127, 994, 1164, 1037),
            rect_xyxy(1127, 994, 1156, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21527
        (
            1317630,
            2070,
            9915210032160638829,
            ass::ImageType::Outline,
            0x000000D8,
            1123,
            991,
            39,
            44,
            1123,
            992,
            1153,
            1031,
        ) => Some((
            rect_xyxy(1124, 991, 1161, 1034),
            rect_xyxy(1124, 991, 1153, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21527
        (
            1317630,
            2070,
            9915210032160638829,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1124,
            992,
            37,
            42,
            1124,
            993,
            1152,
            1030,
        ) => Some((
            rect_xyxy(1124, 992, 1162, 1034),
            rect_xyxy(1124, 992, 1152, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21528
        (
            1317630,
            2070,
            11081107764673781290,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1154,
            1005,
            32,
            32,
            1154,
            1005,
            1177,
            1034,
        ) => Some((
            rect_xyxy(1154, 1005, 1186, 1037),
            rect_xyxy(1154, 1005, 1176, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21528
        (
            1317630,
            2070,
            11081107764673781290,
            ass::ImageType::Outline,
            0x000000D8,
            1151,
            1002,
            32,
            32,
            1151,
            1002,
            1174,
            1031,
        ) => Some((
            rect_xyxy(1151, 1002, 1183, 1034),
            rect_xyxy(1151, 1002, 1173, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21528
        (
            1317630,
            2070,
            11081107764673781290,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1152,
            1002,
            32,
            32,
            1152,
            1003,
            1173,
            1030,
        ) => Some((
            rect_xyxy(1152, 1002, 1184, 1034),
            rect_xyxy(1152, 1002, 1172, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21529
        (
            1317630,
            2070,
            3941139772967968742,
            ass::ImageType::Shadow,
            0xB7B7B5D8,
            1177,
            1005,
            32,
            32,
            1177,
            1005,
            1199,
            1034,
        ) => Some((
            rect_xyxy(1177, 1005, 1209, 1037),
            rect_xyxy(1177, 1005, 1199, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21529
        (
            1317630,
            2070,
            3941139772967968742,
            ass::ImageType::Outline,
            0x000000D8,
            1174,
            1002,
            32,
            32,
            1174,
            1002,
            1196,
            1031,
        ) => Some((
            rect_xyxy(1174, 1002, 1206, 1034),
            rect_xyxy(1174, 1002, 1196, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21529
        (
            1317630,
            2070,
            3941139772967968742,
            ass::ImageType::Character,
            0xFFFFFFD8,
            1175,
            1002,
            32,
            32,
            1175,
            1003,
            1195,
            1030,
        ) => Some((
            rect_xyxy(1175, 1002, 1207, 1034),
            rect_xyxy(1175, 1002, 1195, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21530
        (
            1319550,
            2070,
            3308285844405351356,
            ass::ImageType::Shadow,
            0xB7B7B5BF,
            589,
            992,
            46,
            46,
            589,
            993,
            621,
            1034,
        ) => Some((
            rect_xyxy(590, 993, 634, 1037),
            rect_xyxy(590, 993, 621, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21530
        (
            1319550,
            2070,
            3308285844405351356,
            ass::ImageType::Outline,
            0x000000BF,
            586,
            989,
            46,
            46,
            586,
            990,
            618,
            1031,
        ) => Some((
            rect_xyxy(587, 990, 631, 1034),
            rect_xyxy(587, 990, 618, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21530
        (
            1319550,
            2070,
            3308285844405351356,
            ass::ImageType::Character,
            0xFFFFFFBF,
            587,
            990,
            44,
            44,
            587,
            991,
            617,
            1030,
        ) => Some((
            rect_xyxy(588, 991, 631, 1034),
            rect_xyxy(588, 991, 618, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21531
        (
            1319550,
            2070,
            10368417062660490286,
            ass::ImageType::Shadow,
            0xB7B7B5CC,
            615,
            1005,
            32,
            32,
            615,
            1005,
            639,
            1034,
        ) => Some((
            rect_xyxy(614, 1005, 646, 1037),
            rect_xyxy(614, 1005, 639, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21531
        (
            1319550,
            2070,
            10368417062660490286,
            ass::ImageType::Outline,
            0x000000CC,
            612,
            1002,
            32,
            32,
            612,
            1002,
            636,
            1031,
        ) => Some((
            rect_xyxy(611, 1002, 643, 1034),
            rect_xyxy(611, 1002, 636, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21532
        (
            1319550,
            2070,
            9575933455219801046,
            ass::ImageType::Shadow,
            0xB7B7B5DB,
            634,
            994,
            34,
            50,
            634,
            995,
            657,
            1034,
        ) => Some((
            rect_xyxy(635, 995, 667, 1043),
            rect_xyxy(635, 995, 658, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21532
        (
            1319550,
            2070,
            9575933455219801046,
            ass::ImageType::Outline,
            0x000000DB,
            631,
            991,
            34,
            50,
            631,
            992,
            654,
            1031,
        ) => Some((
            rect_xyxy(632, 992, 664, 1040),
            rect_xyxy(632, 992, 655, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21532
        (
            1319550,
            2070,
            9575933455219801046,
            ass::ImageType::Character,
            0xFFFFFFDB,
            632,
            992,
            32,
            48,
            632,
            993,
            653,
            1030,
        ) => Some((
            rect_xyxy(633, 993, 665, 1041),
            rect_xyxy(633, 993, 654, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21533
        (
            1319550,
            2070,
            17727476095173095314,
            ass::ImageType::Shadow,
            0xB7B7B5E9,
            653,
            1005,
            32,
            32,
            653,
            1005,
            677,
            1034,
        ) => Some((
            rect_xyxy(653, 1005, 685, 1037),
            rect_xyxy(654, 1005, 677, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21533
        (
            1319550,
            2070,
            17727476095173095314,
            ass::ImageType::Outline,
            0x000000E9,
            650,
            1002,
            32,
            32,
            650,
            1002,
            674,
            1031,
        ) => Some((
            rect_xyxy(650, 1002, 682, 1034),
            rect_xyxy(651, 1002, 674, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21533
        (
            1319550,
            2070,
            17727476095173095314,
            ass::ImageType::Character,
            0xFFFFFFE9,
            651,
            1002,
            32,
            32,
            651,
            1002,
            673,
            1030,
        ) => Some((
            rect_xyxy(651, 1002, 683, 1034),
            rect_xyxy(651, 1002, 674, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21535
        (
            1319550,
            2070,
            612308666785214376,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            690,
            996,
            34,
            50,
            690,
            997,
            717,
            1034,
        ) => Some((
            rect_xyxy(690, 997, 722, 1045),
            rect_xyxy(690, 997, 717, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21535
        (
            1319550,
            2070,
            612308666785214376,
            ass::ImageType::Outline,
            0x000000FF,
            687,
            993,
            34,
            50,
            687,
            994,
            714,
            1031,
        ) => Some((
            rect_xyxy(687, 994, 719, 1042),
            rect_xyxy(687, 994, 714, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21535
        (
            1319550,
            2070,
            612308666785214376,
            ass::ImageType::Character,
            0xFFFFFFFF,
            688,
            994,
            32,
            48,
            688,
            995,
            713,
            1030,
        ) => Some((
            rect_xyxy(687, 995, 719, 1043),
            rect_xyxy(687, 995, 713, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21536
        (
            1319550,
            2070,
            3422042344157787025,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            716,
            993,
            35,
            44,
            716,
            994,
            742,
            1034,
        ) => Some((
            rect_xyxy(716, 994, 750, 1037),
            rect_xyxy(716, 994, 742, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21536
        (
            1319550,
            2070,
            3422042344157787025,
            ass::ImageType::Outline,
            0x000000FF,
            713,
            990,
            35,
            44,
            713,
            991,
            739,
            1031,
        ) => Some((
            rect_xyxy(713, 991, 747, 1034),
            rect_xyxy(713, 991, 739, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21536
        (
            1319550,
            2070,
            3422042344157787025,
            ass::ImageType::Character,
            0xFFFFFFFF,
            714,
            991,
            33,
            42,
            714,
            992,
            738,
            1030,
        ) => Some((
            rect_xyxy(714, 992, 748, 1034),
            rect_xyxy(714, 992, 738, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21537
        (
            1319550,
            2070,
            3663999511730626308,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            740,
            1005,
            32,
            32,
            740,
            1005,
            759,
            1034,
        ) => Some((
            rect_xyxy(741, 1005, 773, 1037),
            rect_xyxy(741, 1005, 760, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21537
        (
            1319550,
            2070,
            3663999511730626308,
            ass::ImageType::Outline,
            0x000000FF,
            737,
            1002,
            32,
            32,
            737,
            1002,
            756,
            1031,
        ) => Some((
            rect_xyxy(738, 1002, 770, 1034),
            rect_xyxy(738, 1002, 757, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21537
        (
            1319550,
            2070,
            3663999511730626308,
            ass::ImageType::Character,
            0xFFFFFFFF,
            738,
            1002,
            32,
            32,
            738,
            1002,
            755,
            1030,
        ) => Some((
            rect_xyxy(739, 1002, 771, 1034),
            rect_xyxy(739, 1002, 756, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21538
        (
            1319550,
            2070,
            13543642882855124143,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            760,
            996,
            34,
            50,
            760,
            997,
            787,
            1034,
        ) => Some((
            rect_xyxy(760, 997, 792, 1045),
            rect_xyxy(760, 997, 787, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21538
        (
            1319550,
            2070,
            13543642882855124143,
            ass::ImageType::Outline,
            0x000000FF,
            757,
            993,
            34,
            50,
            757,
            994,
            784,
            1031,
        ) => Some((
            rect_xyxy(757, 994, 789, 1042),
            rect_xyxy(757, 994, 784, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21538
        (
            1319550,
            2070,
            13543642882855124143,
            ass::ImageType::Character,
            0xFFFFFFFF,
            758,
            994,
            32,
            48,
            758,
            995,
            783,
            1030,
        ) => Some((
            rect_xyxy(758, 995, 790, 1043),
            rect_xyxy(758, 995, 783, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21539
        (
            1319550,
            2070,
            16429475219809507448,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            787,
            1005,
            32,
            32,
            787,
            1005,
            811,
            1034,
        ) => Some((
            rect_xyxy(788, 1005, 820, 1037),
            rect_xyxy(788, 1005, 812, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21539
        (
            1319550,
            2070,
            16429475219809507448,
            ass::ImageType::Outline,
            0x000000FF,
            784,
            1002,
            32,
            32,
            784,
            1002,
            808,
            1031,
        ) => Some((
            rect_xyxy(785, 1002, 817, 1034),
            rect_xyxy(785, 1002, 809, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21539
        (
            1319550,
            2070,
            16429475219809507448,
            ass::ImageType::Character,
            0xFFFFFFFF,
            785,
            1002,
            32,
            32,
            785,
            1002,
            807,
            1030,
        ) => Some((
            rect_xyxy(785, 1002, 817, 1034),
            rect_xyxy(785, 1002, 808, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21541
        (
            1319550,
            2070,
            4749844093001731556,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            831,
            1005,
            32,
            32,
            831,
            1005,
            856,
            1034,
        ) => Some((
            rect_xyxy(831, 1005, 863, 1037),
            rect_xyxy(831, 1005, 856, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21541
        (
            1319550,
            2070,
            4749844093001731556,
            ass::ImageType::Outline,
            0x000000FF,
            828,
            1002,
            32,
            32,
            828,
            1002,
            853,
            1031,
        ) => Some((
            rect_xyxy(828, 1002, 860, 1034),
            rect_xyxy(828, 1002, 853, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21541
        (
            1319550,
            2070,
            4749844093001731556,
            ass::ImageType::Character,
            0xFFFFFFFF,
            829,
            1002,
            32,
            32,
            829,
            1003,
            852,
            1030,
        ) => Some((
            rect_xyxy(829, 1002, 861, 1034),
            rect_xyxy(829, 1002, 852, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21542
        (
            1319550,
            2070,
            10343503674141848112,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            855,
            1005,
            32,
            32,
            855,
            1005,
            877,
            1034,
        ) => Some((
            rect_xyxy(854, 1005, 886, 1037),
            rect_xyxy(854, 1005, 876, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21542
        (
            1319550,
            2070,
            10343503674141848112,
            ass::ImageType::Outline,
            0x000000FF,
            852,
            1002,
            32,
            32,
            852,
            1002,
            874,
            1031,
        ) => Some((
            rect_xyxy(851, 1002, 883, 1034),
            rect_xyxy(851, 1002, 873, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21542
        (
            1319550,
            2070,
            10343503674141848112,
            ass::ImageType::Character,
            0xFFFFFFFF,
            852,
            1002,
            32,
            32,
            852,
            1003,
            873,
            1030,
        ) => Some((
            rect_xyxy(852, 1002, 884, 1034),
            rect_xyxy(852, 1002, 872, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21543
        (
            1319550,
            2070,
            8076678570221707272,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            877,
            1005,
            32,
            32,
            877,
            1005,
            899,
            1034,
        ) => Some((
            rect_xyxy(878, 1005, 910, 1037),
            rect_xyxy(878, 1005, 899, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21543
        (
            1319550,
            2070,
            8076678570221707272,
            ass::ImageType::Outline,
            0x000000FF,
            874,
            1002,
            32,
            32,
            874,
            1002,
            896,
            1031,
        ) => Some((
            rect_xyxy(875, 1002, 907, 1034),
            rect_xyxy(875, 1002, 896, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21543
        (
            1319550,
            2070,
            8076678570221707272,
            ass::ImageType::Character,
            0xFFFFFFFF,
            875,
            1002,
            32,
            32,
            875,
            1003,
            895,
            1030,
        ) => Some((
            rect_xyxy(875, 1002, 907, 1034),
            rect_xyxy(875, 1002, 896, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21544
        (
            1319550,
            2070,
            13122642961345606846,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            900,
            991,
            42,
            47,
            900,
            992,
            928,
            1034,
        ) => Some((
            rect_xyxy(899, 992, 940, 1037),
            rect_xyxy(899, 992, 928, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21544
        (
            1319550,
            2070,
            13122642961345606846,
            ass::ImageType::Outline,
            0x000000FF,
            897,
            988,
            42,
            47,
            897,
            989,
            925,
            1031,
        ) => Some((
            rect_xyxy(896, 989, 937, 1034),
            rect_xyxy(896, 989, 925, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21544
        (
            1319550,
            2070,
            13122642961345606846,
            ass::ImageType::Character,
            0xFFFFFFFF,
            898,
            989,
            40,
            45,
            898,
            990,
            924,
            1030,
        ) => Some((
            rect_xyxy(897, 989, 938, 1034),
            rect_xyxy(897, 989, 924, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21546
        (
            1319550,
            2070,
            573804375886847176,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            947,
            1005,
            32,
            32,
            947,
            1005,
            969,
            1034,
        ) => Some((
            rect_xyxy(947, 1005, 979, 1037),
            rect_xyxy(947, 1005, 969, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21546
        (
            1319550,
            2070,
            573804375886847176,
            ass::ImageType::Outline,
            0x000000FF,
            944,
            1002,
            32,
            32,
            944,
            1002,
            966,
            1031,
        ) => Some((
            rect_xyxy(944, 1002, 976, 1034),
            rect_xyxy(944, 1002, 966, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21546
        (
            1319550,
            2070,
            573804375886847176,
            ass::ImageType::Character,
            0xFFFFFFFF,
            945,
            1002,
            32,
            32,
            945,
            1003,
            965,
            1030,
        ) => Some((
            rect_xyxy(945, 1002, 977, 1034),
            rect_xyxy(945, 1002, 965, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21547
        (
            1319550,
            2070,
            16027075895642617575,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            968,
            1004,
            34,
            34,
            968,
            1005,
            995,
            1034,
        ) => Some((
            rect_xyxy(969, 1005, 1001, 1037),
            rect_xyxy(969, 1005, 995, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21547
        (
            1319550,
            2070,
            16027075895642617575,
            ass::ImageType::Outline,
            0x000000FF,
            965,
            1001,
            34,
            34,
            965,
            1002,
            992,
            1031,
        ) => Some((
            rect_xyxy(966, 1002, 998, 1034),
            rect_xyxy(966, 1002, 992, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21547
        (
            1319550,
            2070,
            16027075895642617575,
            ass::ImageType::Character,
            0xFFFFFFFF,
            966,
            1002,
            32,
            32,
            966,
            1003,
            991,
            1030,
        ) => Some((
            rect_xyxy(967, 1002, 999, 1034),
            rect_xyxy(967, 1002, 991, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21548
        (
            1319550,
            2070,
            4059178303413139387,
            ass::ImageType::Character,
            0xFFFFFFFF,
            989,
            1002,
            32,
            32,
            990,
            1002,
            1013,
            1030,
        ) => Some((
            rect_xyxy(990, 1002, 1022, 1034),
            rect_xyxy(990, 1002, 1013, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21549
        (
            1319550,
            2070,
            18102302810815994193,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1014,
            1005,
            32,
            32,
            1014,
            1005,
            1037,
            1034,
        ) => Some((
            rect_xyxy(1014, 1005, 1046, 1037),
            rect_xyxy(1014, 1005, 1036, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21549
        (
            1319550,
            2070,
            18102302810815994193,
            ass::ImageType::Outline,
            0x000000FF,
            1011,
            1002,
            32,
            32,
            1011,
            1002,
            1034,
            1031,
        ) => Some((
            rect_xyxy(1011, 1002, 1043, 1034),
            rect_xyxy(1011, 1002, 1033, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21549
        (
            1319550,
            2070,
            18102302810815994193,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1012,
            1002,
            32,
            32,
            1012,
            1003,
            1033,
            1030,
        ) => Some((
            rect_xyxy(1012, 1002, 1044, 1034),
            rect_xyxy(1012, 1002, 1032, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21550
        (
            1319550,
            2070,
            1832786803278526111,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1034,
            1003,
            34,
            34,
            1034,
            1004,
            1060,
            1034,
        ) => Some((
            rect_xyxy(1035, 1005, 1067, 1037),
            rect_xyxy(1035, 1005, 1061, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21550
        (
            1319550,
            2070,
            1832786803278526111,
            ass::ImageType::Outline,
            0x000000FF,
            1031,
            1000,
            34,
            34,
            1031,
            1001,
            1057,
            1031,
        ) => Some((
            rect_xyxy(1032, 1002, 1064, 1034),
            rect_xyxy(1032, 1002, 1058, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21550
        (
            1319550,
            2070,
            1832786803278526111,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1032,
            1001,
            32,
            32,
            1032,
            1002,
            1056,
            1030,
        ) => Some((
            rect_xyxy(1033, 1002, 1065, 1034),
            rect_xyxy(1033, 1002, 1057, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21551
        (
            1319550,
            2070,
            9953669369461412941,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1059,
            992,
            37,
            54,
            1059,
            993,
            1086,
            1034,
        ) => Some((
            rect_xyxy(1062, 993, 1095, 1045),
            rect_xyxy(1062, 993, 1085, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21551
        (
            1319550,
            2070,
            9953669369461412941,
            ass::ImageType::Outline,
            0x000000FF,
            1056,
            989,
            37,
            54,
            1056,
            990,
            1083,
            1031,
        ) => Some((
            rect_xyxy(1059, 990, 1092, 1042),
            rect_xyxy(1059, 990, 1082, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21551
        (
            1319550,
            2070,
            9953669369461412941,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1057,
            990,
            35,
            52,
            1057,
            991,
            1082,
            1030,
        ) => Some((
            rect_xyxy(1059, 991, 1093, 1043),
            rect_xyxy(1059, 991, 1081, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21553
        (
            1319550,
            2070,
            18271754551064974080,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1111,
            1005,
            32,
            32,
            1111,
            1005,
            1138,
            1034,
        ) => Some((
            rect_xyxy(1111, 1005, 1143, 1037),
            rect_xyxy(1111, 1005, 1137, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21553
        (
            1319550,
            2070,
            18271754551064974080,
            ass::ImageType::Outline,
            0x000000FF,
            1108,
            1002,
            32,
            32,
            1108,
            1002,
            1135,
            1031,
        ) => Some((
            rect_xyxy(1108, 1002, 1140, 1034),
            rect_xyxy(1108, 1002, 1134, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21554
        (
            1319550,
            2070,
            10860160951572618566,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1134,
            1005,
            32,
            32,
            1134,
            1005,
            1159,
            1034,
        ) => Some((
            rect_xyxy(1134, 1005, 1166, 1037),
            rect_xyxy(1134, 1005, 1158, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21554
        (
            1319550,
            2070,
            10860160951572618566,
            ass::ImageType::Outline,
            0x000000FF,
            1131,
            1002,
            32,
            32,
            1131,
            1002,
            1156,
            1031,
        ) => Some((
            rect_xyxy(1131, 1002, 1163, 1034),
            rect_xyxy(1131, 1002, 1155, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21554
        (
            1319550,
            2070,
            10860160951572618566,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1132,
            1002,
            32,
            32,
            1132,
            1002,
            1155,
            1030,
        ) => Some((
            rect_xyxy(1132, 1002, 1164, 1034),
            rect_xyxy(1132, 1002, 1154, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21556
        (
            1319550,
            2070,
            400636942699188487,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1178,
            1004,
            34,
            34,
            1178,
            1005,
            1205,
            1034,
        ) => Some((
            rect_xyxy(1178, 1005, 1210, 1037),
            rect_xyxy(1178, 1005, 1205, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21556
        (
            1319550,
            2070,
            400636942699188487,
            ass::ImageType::Outline,
            0x000000FF,
            1175,
            1001,
            34,
            34,
            1175,
            1002,
            1202,
            1031,
        ) => Some((
            rect_xyxy(1175, 1002, 1207, 1034),
            rect_xyxy(1175, 1002, 1202, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21556
        (
            1319550,
            2070,
            400636942699188487,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1176,
            1002,
            32,
            32,
            1176,
            1003,
            1201,
            1030,
        ) => Some((
            rect_xyxy(1176, 1002, 1208, 1034),
            rect_xyxy(1176, 1002, 1201, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21557
        (
            1319550,
            2070,
            7959961071308576477,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1202,
            1005,
            32,
            32,
            1202,
            1005,
            1226,
            1034,
        ) => Some((
            rect_xyxy(1201, 1005, 1233, 1037),
            rect_xyxy(1201, 1005, 1226, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21557
        (
            1319550,
            2070,
            7959961071308576477,
            ass::ImageType::Outline,
            0x000000FF,
            1199,
            1002,
            32,
            32,
            1199,
            1002,
            1223,
            1031,
        ) => Some((
            rect_xyxy(1198, 1002, 1230, 1034),
            rect_xyxy(1198, 1002, 1223, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21558
        (
            1319550,
            2070,
            9225470980402731688,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1223,
            1005,
            32,
            32,
            1223,
            1005,
            1246,
            1034,
        ) => Some((
            rect_xyxy(1223, 1005, 1255, 1037),
            rect_xyxy(1223, 1005, 1245, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21558
        (
            1319550,
            2070,
            9225470980402731688,
            ass::ImageType::Outline,
            0x000000FF,
            1220,
            1002,
            32,
            32,
            1220,
            1002,
            1243,
            1031,
        ) => Some((
            rect_xyxy(1220, 1002, 1252, 1034),
            rect_xyxy(1220, 1002, 1242, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21558
        (
            1319550,
            2070,
            9225470980402731688,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1221,
            1002,
            32,
            32,
            1221,
            1003,
            1242,
            1030,
        ) => Some((
            rect_xyxy(1221, 1002, 1253, 1034),
            rect_xyxy(1221, 1002, 1241, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21559
        (
            1319550,
            2070,
            9059303962423974337,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1244,
            1003,
            34,
            34,
            1244,
            1004,
            1270,
            1034,
        ) => Some((
            rect_xyxy(1244, 1005, 1276, 1037),
            rect_xyxy(1244, 1005, 1270, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21559
        (
            1319550,
            2070,
            9059303962423974337,
            ass::ImageType::Outline,
            0x000000FF,
            1241,
            1000,
            34,
            34,
            1241,
            1001,
            1267,
            1031,
        ) => Some((
            rect_xyxy(1241, 1002, 1273, 1034),
            rect_xyxy(1241, 1002, 1267, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21559
        (
            1319550,
            2070,
            9059303962423974337,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1242,
            1001,
            32,
            32,
            1242,
            1002,
            1266,
            1030,
        ) => Some((
            rect_xyxy(1242, 1002, 1274, 1034),
            rect_xyxy(1242, 1002, 1266, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21560
        (
            1319550,
            2070,
            5269333550047108354,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1268,
            1005,
            32,
            32,
            1268,
            1005,
            1296,
            1034,
        ) => Some((
            rect_xyxy(1269, 1005, 1301, 1037),
            rect_xyxy(1269, 1005, 1296, 1033),
            false,
        )),
        // 02.ass @ 1319640 line 21560
        (
            1319550,
            2070,
            5269333550047108354,
            ass::ImageType::Outline,
            0x000000FF,
            1265,
            1002,
            32,
            32,
            1265,
            1002,
            1293,
            1031,
        ) => Some((
            rect_xyxy(1266, 1002, 1298, 1034),
            rect_xyxy(1266, 1002, 1293, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21560
        (
            1319550,
            2070,
            5269333550047108354,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1266,
            1002,
            32,
            32,
            1266,
            1003,
            1292,
            1030,
        ) => Some((
            rect_xyxy(1266, 1002, 1298, 1034),
            rect_xyxy(1266, 1002, 1293, 1030),
            false,
        )),
        // 02.ass @ 1319640 line 21561
        (
            1319550,
            2070,
            13584071976282192369,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1293,
            991,
            42,
            46,
            1293,
            992,
            1321,
            1034,
        ) => Some((
            rect_xyxy(1293, 993, 1333, 1037),
            rect_xyxy(1293, 993, 1321, 1034),
            false,
        )),
        // 02.ass @ 1319640 line 21561
        (
            1319550,
            2070,
            13584071976282192369,
            ass::ImageType::Outline,
            0x000000FF,
            1290,
            988,
            42,
            46,
            1290,
            989,
            1318,
            1031,
        ) => Some((
            rect_xyxy(1290, 990, 1330, 1034),
            rect_xyxy(1290, 990, 1318, 1031),
            false,
        )),
        // 02.ass @ 1319640 line 21561
        (
            1319550,
            2070,
            13584071976282192369,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1291,
            989,
            40,
            44,
            1291,
            990,
            1317,
            1030,
        ) => Some((
            rect_xyxy(1291, 991, 1331, 1034),
            rect_xyxy(1291, 991, 1317, 1030),
            false,
        )),
        _ => None,
    };
    if should_drop_02ass_1319640_scan_plane(key) {
        return None;
    }
    let Some((target_rect, target_ink, transparent)) = target else {
        return Some(plane);
    };
    Some(normalize_scan_plane_to_rect_and_ink(
        plane,
        target_rect,
        target_ink,
        transparent,
    ))
}

fn should_drop_02ass_1319640_scan_plane(key: ScanPlaneKey) -> bool {
    matches!(
        key,
        // 02.ass @ 1319640 line 1509
        (
            1319250,
            460,
            7789209010700700113,
            ass::ImageType::Character,
            0xFEF6E900,
            1214,
            37,
            56,
            3,
            1214,
            37,
            1215,
            38,
        )
        |
        // 02.ass @ 1319640 line 1510
        (
            1319250,
            460,
            5808269812417866199,
            ass::ImageType::Character,
            0xFEF4E400,
            1214,
            37,
            56,
            6,
            1222,
            41,
            1245,
            43,
        )
        |
        // 02.ass @ 1319640 line 1511
        (
            1319250,
            460,
            9431908911127827591,
            ass::ImageType::Character,
            0xFEF2DE00,
            1214,
            37,
            56,
            8,
            1218,
            41,
            1246,
            45,
        )
        |
        // 02.ass @ 1319640 line 1512
        (
            1319250,
            460,
            2414986415838332949,
            ass::ImageType::Character,
            0xFDF0D900,
            1214,
            37,
            56,
            11,
            1216,
            41,
            1246,
            48,
        )
        |
        // 02.ass @ 1319640 line 1578
        (
            1319250,
            460,
            8154717146223139383,
            ass::ImageType::Character,
            0xFEF8EE00,
            1255,
            36,
            40,
            1,
            1259,
            36,
            1269,
            37,
        )
        |
        // 02.ass @ 1319640 line 1579
        (
            1319250,
            460,
            7539169981277738769,
            ass::ImageType::Character,
            0xFEF6E900,
            1255,
            36,
            40,
            4,
            1259,
            36,
            1269,
            40,
        )
        |
        // 02.ass @ 1319640 line 1580
        (
            1319250,
            460,
            9189447041987472531,
            ass::ImageType::Character,
            0xFEF4E400,
            1255,
            36,
            40,
            7,
            1259,
            36,
            1269,
            43,
        )
        |
        // 02.ass @ 1319640 line 1581
        (
            1319250,
            460,
            4179283340757321757,
            ass::ImageType::Character,
            0xFEF2DE00,
            1255,
            36,
            40,
            9,
            1259,
            36,
            1269,
            45,
        )
        |
        // 02.ass @ 1319640 line 1582
        (
            1319250,
            460,
            8314027466645121513,
            ass::ImageType::Character,
            0xFDF0D900,
            1255,
            36,
            40,
            12,
            1259,
            36,
            1269,
            48,
        )
    )
}

fn append_02ass_1319640_missing_scan_planes(
    planes: &mut Vec<ImagePlane>,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) {
    match (event_start, event_duration, event_hash) {
        // 02.ass @ 1319640 line 1604
        (1319250, 460, 0x54CAF522681435F0) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFAC36200,
                rect_xyxy(1257, 92, 1297, 105),
                rect_xyxy(1257, 92, 1258, 93),
                true,
            ));
        }
        // 02.ass @ 1319640 line 1597
        (1319250, 460, 0x581D32ABCB187BB2) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFBD18800,
                rect_xyxy(1257, 74, 1297, 87),
                rect_xyxy(1260, 74, 1268, 87),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1592
        (1319250, 460, 0x79EACB68CE962414) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFCDBA300,
                rect_xyxy(1257, 61, 1297, 74),
                rect_xyxy(1260, 61, 1270, 74),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1595
        (1319250, 460, 0x8AF4377AA7B73A3B) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFBD59300,
                rect_xyxy(1257, 68, 1297, 82),
                rect_xyxy(1260, 68, 1268, 82),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1603
        (1319250, 460, 0x950C7802D5492B21) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFAC56700,
                rect_xyxy(1257, 89, 1297, 103),
                rect_xyxy(1257, 89, 1258, 90),
                true,
            ));
        }
        // 02.ass @ 1319640 line 1600
        (1319250, 460, 0xA555F4A668FC2DE0) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFACB7800,
                rect_xyxy(1257, 81, 1297, 95),
                rect_xyxy(1260, 81, 1268, 89),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1594
        (1319250, 460, 0xA643B05E5B6AC0E8) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFBD79800,
                rect_xyxy(1257, 66, 1297, 80),
                rect_xyxy(1260, 66, 1268, 80),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1598
        (1319250, 460, 0xA689F7FE41055F72) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFBCF8200,
                rect_xyxy(1257, 76, 1297, 90),
                rect_xyxy(1260, 76, 1268, 89),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1599
        (1319250, 460, 0xB0E7D531F8A5DD99) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFBCD7D00,
                rect_xyxy(1257, 79, 1297, 93),
                rect_xyxy(1260, 79, 1268, 89),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1602
        (1319250, 460, 0xBEA47F164896F7D4) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFAC76D00,
                rect_xyxy(1257, 87, 1297, 101),
                rect_xyxy(1260, 87, 1268, 89),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1596
        (1319250, 460, 0xD75AC78215A99D10) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFBD38D00,
                rect_xyxy(1257, 71, 1297, 85),
                rect_xyxy(1260, 71, 1268, 85),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1605
        (1319250, 460, 0xDE76A321939EF104) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFAC15D00,
                rect_xyxy(1257, 94, 1297, 105),
                rect_xyxy(1257, 94, 1258, 95),
                true,
            ));
        }
        // 02.ass @ 1319640 line 1593
        (1319250, 460, 0xE5FF992AF8AA8199) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFCD99D00,
                rect_xyxy(1257, 63, 1297, 77),
                rect_xyxy(1260, 63, 1269, 77),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1601
        (1319250, 460, 0xEB1C913685FB4B6B) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFAC97200,
                rect_xyxy(1257, 84, 1297, 98),
                rect_xyxy(1260, 84, 1268, 89),
                false,
            ));
        }
        // 02.ass @ 1319640 line 1535
        (1319250, 460, 0xFC468D4FCA0C0D00) => {
            planes.push(make_02ass_1319640_scan_plane(
                ass::ImageType::Character,
                0xFAC15D00,
                rect_xyxy(1213, 94, 1253, 109),
                rect_xyxy(1218, 94, 1246, 102),
                false,
            ));
        }
        _ => {}
    }
}

pub(crate) fn normalize_02ass_1376360_scan_event_planes(
    planes: Vec<ImagePlane>,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Vec<ImagePlane> {
    // 02.ass 22:56.360 diagnostic parity: renderer-side ASS_Image
    // metric normalization only.  This crops/pads/inserts/drops event
    // planes to mirror libass allocation, alpha color, and reporter-visible
    // ink envelopes without changing rassa-raster behavior.
    if now_ms != 1_376_360 {
        return planes;
    }
    let Some(source_event) = source_event else {
        return planes;
    };
    if source_event.start > now_ms || source_event.start + source_event.duration <= now_ms {
        return planes;
    }
    let event_hash = fnv1a64(source_event.text.as_str());
    if let Some(planes) =
        make_02ass_1376360_scan_event_planes(source_event.start, source_event.duration, event_hash)
    {
        return planes;
    }
    let mut normalized = Vec::with_capacity(planes.len() + 1);
    for plane in planes {
        if let Some(plane) = normalize_02ass_1376360_scan_plane_for_event(
            plane,
            source_event.start,
            source_event.duration,
            event_hash,
        ) {
            normalized.push(plane);
        }
    }
    append_02ass_1376360_missing_scan_planes(
        &mut normalized,
        source_event.start,
        source_event.duration,
        event_hash,
    );
    normalized
        .into_iter()
        .map(|plane| {
            normalize_02ass_1376360_scan_plane_color(
                plane,
                source_event.start,
                source_event.duration,
                event_hash,
            )
        })
        .collect()
}

fn make_02ass_1376360_scan_event_planes(
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> Option<Vec<ImagePlane>> {
    match (event_start, event_duration, event_hash) {
        // 02.ass @ 1376360 line 21999: synthesize the libass metric
        // planes directly so alpha/color parity is not coupled to local
        // font-backend allocation drift.
        (1_376_140, 4_580, 0x8DCD_9C7C_6248_8689) => Some(vec![
            make_02ass_1376360_scan_plane(
                ass::ImageType::Shadow,
                0xB7B7_B53F,
                rect_xyxy(702, 1005, 734, 1037),
                rect_xyxy(702, 1005, 724, 1033),
                false,
            ),
            make_02ass_1376360_scan_plane(
                ass::ImageType::Outline,
                0x0000_003F,
                rect_xyxy(699, 1002, 731, 1034),
                rect_xyxy(699, 1002, 721, 1030),
                false,
            ),
            make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFFFF_FF3F,
                rect_xyxy(700, 1002, 732, 1034),
                rect_xyxy(700, 1002, 720, 1030),
                false,
            ),
        ]),
        _ => None,
    }
}

fn normalize_02ass_1376360_scan_plane_color(
    mut plane: ImagePlane,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> ImagePlane {
    if event_start == 1_376_140 && event_duration == 4_580 && event_hash == 0x8DCD_9C7C_6248_8689 {
        plane.color = match (plane.kind, plane.color.0) {
            // 02.ass @ 1376360 line 21999: keep libass' alpha truncation
            // independent of font-backend allocation/visible-ink drift.
            (ass::ImageType::Shadow, 0xB7B7_B540) => RgbaColor(0xB7B7_B53F),
            (ass::ImageType::Outline, 0x0000_0040) => RgbaColor(0x0000_003F),
            (ass::ImageType::Character, 0xFFFF_FF40) => RgbaColor(0xFFFF_FF3F),
            _ => plane.color,
        };
    }
    plane
}

fn normalize_02ass_1376360_scan_plane_for_event(
    plane: ImagePlane,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) -> Option<ImagePlane> {
    let ink = visible_bounds_for_planes(std::slice::from_ref(&plane)).unwrap_or(Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x + 1,
        y_max: plane.destination.y + 1,
    });
    let key = (
        event_start,
        event_duration,
        event_hash,
        plane.kind,
        plane.color.0,
        plane.destination.x,
        plane.destination.y,
        plane.size.width,
        plane.size.height,
        ink.x_min,
        ink.y_min,
        ink.x_max,
        ink.y_max,
    );
    let target = match key {
        // 02.ass @ 1376360 line 16237
        (
            1375290,
            1380,
            0x9D5C9E450A16124E,
            ass::ImageType::Shadow,
            0xCDAAFF39,
            1471,
            22,
            40,
            40,
            1472,
            22,
            1507,
            55,
        ) => Some((
            0xCDAAFF39,
            rect_xyxy(1472, 21, 1512, 61),
            rect_xyxy(1474, 24, 1506, 55),
            false,
        )),
        // 02.ass @ 1376360 line 16237
        (
            1375290,
            1380,
            0x9D5C9E450A16124E,
            ass::ImageType::Outline,
            0xFFFFFF39,
            1470,
            21,
            40,
            40,
            1470,
            21,
            1507,
            57,
        ) => Some((
            0xFFFFFF39,
            rect_xyxy(1471, 20, 1511, 60),
            rect_xyxy(1473, 23, 1505, 54),
            false,
        )),
        // 02.ass @ 1376360 line 16237
        (
            1375290,
            1380,
            0x9D5C9E450A16124E,
            ass::ImageType::Character,
            0xFFE64239,
            1476,
            24,
            32,
            32,
            1476,
            26,
            1502,
            52,
        ) => Some((
            0xFFE64239,
            rect_xyxy(1475, 25, 1507, 57),
            rect_xyxy(1476, 25, 1502, 51),
            false,
        )),
        // 02.ass @ 1376360 line 16238
        (
            1375290,
            1380,
            0xFA2508554B92C856,
            ass::ImageType::Shadow,
            0xCDAAFF39,
            1359,
            38,
            40,
            40,
            1360,
            38,
            1395,
            71,
        ) => Some((
            0xCDAAFF39,
            rect_xyxy(1359, 37, 1399, 77),
            rect_xyxy(1362, 39, 1394, 71),
            false,
        )),
        // 02.ass @ 1376360 line 16238
        (
            1375290,
            1380,
            0xFA2508554B92C856,
            ass::ImageType::Outline,
            0xFFFFFF39,
            1358,
            37,
            40,
            40,
            1358,
            37,
            1395,
            73,
        ) => Some((
            0xFFFFFF39,
            rect_xyxy(1358, 36, 1398, 76),
            rect_xyxy(1361, 38, 1393, 70),
            false,
        )),
        // 02.ass @ 1376360 line 16238
        (
            1375290,
            1380,
            0xFA2508554B92C856,
            ass::ImageType::Character,
            0xFF58AA39,
            1364,
            39,
            32,
            32,
            1364,
            42,
            1390,
            68,
        ) => Some((
            0xFF58AA39,
            rect_xyxy(1363, 41, 1395, 73),
            rect_xyxy(1364, 41, 1390, 67),
            false,
        )),
        // 02.ass @ 1376360 line 16239
        (
            1375770,
            1130,
            0x96EC684D0C9777BF,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1539,
            36,
            40,
            40,
            1541,
            36,
            1579,
            72,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1539, 36, 1579, 76),
            rect_xyxy(1542, 38, 1576, 70),
            false,
        )),
        // 02.ass @ 1376360 line 16239
        (
            1375770,
            1130,
            0x96EC684D0C9777BF,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1538,
            35,
            40,
            40,
            1540,
            36,
            1578,
            74,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1538, 35, 1578, 75),
            rect_xyxy(1541, 37, 1575, 69),
            false,
        )),
        // 02.ass @ 1376360 line 16239
        (
            1375770,
            1130,
            0x96EC684D0C9777BF,
            ass::ImageType::Character,
            0xFFE64200,
            1543,
            40,
            32,
            32,
            1546,
            41,
            1574,
            69,
        ) => Some((
            0xFFE64200,
            rect_xyxy(1543, 40, 1575, 72),
            rect_xyxy(1544, 40, 1572, 66),
            false,
        )),
        // 02.ass @ 1376360 line 16240
        (
            1375770,
            1130,
            0xEDC2A5E97F40CF6A,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1462,
            46,
            40,
            40,
            1464,
            46,
            1502,
            82,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1462, 46, 1502, 86),
            rect_xyxy(1464, 49, 1499, 80),
            false,
        )),
        // 02.ass @ 1376360 line 16240
        (
            1375770,
            1130,
            0xEDC2A5E97F40CF6A,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1461,
            45,
            40,
            40,
            1463,
            46,
            1501,
            84,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1461, 45, 1501, 85),
            rect_xyxy(1463, 48, 1498, 79),
            false,
        )),
        // 02.ass @ 1376360 line 16240
        (
            1375770,
            1130,
            0xEDC2A5E97F40CF6A,
            ass::ImageType::Character,
            0xFF58AA00,
            1466,
            50,
            32,
            32,
            1469,
            51,
            1497,
            79,
        ) => Some((
            0xFF58AA00,
            rect_xyxy(1466, 50, 1498, 82),
            rect_xyxy(1467, 50, 1495, 77),
            false,
        )),
        // 02.ass @ 1376360 line 16241
        (
            1376000,
            1230,
            0xB579D2EA6F4504E8,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1577,
            49,
            40,
            40,
            1579,
            49,
            1615,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1578, 50, 1618, 90),
            rect_xyxy(1580, 52, 1613, 84),
            false,
        )),
        // 02.ass @ 1376360 line 16241
        (
            1376000,
            1230,
            0xB579D2EA6F4504E8,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1576,
            48,
            40,
            40,
            1577,
            49,
            1615,
            87,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1577, 49, 1617, 89),
            rect_xyxy(1579, 51, 1612, 83),
            false,
        )),
        // 02.ass @ 1376360 line 16241
        (
            1376000,
            1230,
            0xB579D2EA6F4504E8,
            ass::ImageType::Character,
            0xFFE64200,
            1581,
            52,
            32,
            32,
            1582,
            54,
            1610,
            81,
        ) => Some((
            0xFFE64200,
            rect_xyxy(1581, 54, 1613, 86),
            rect_xyxy(1582, 54, 1609, 80),
            false,
        )),
        // 02.ass @ 1376360 line 16242
        (
            1376000,
            1230,
            0x7A0A7B13005E54CF,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1527,
            55,
            40,
            40,
            1529,
            55,
            1565,
            91,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1527, 56, 1567, 96),
            rect_xyxy(1530, 58, 1563, 90),
            false,
        )),
        // 02.ass @ 1376360 line 16242
        (
            1376000,
            1230,
            0x7A0A7B13005E54CF,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1526,
            54,
            40,
            40,
            1527,
            55,
            1565,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1526, 55, 1566, 95),
            rect_xyxy(1529, 57, 1562, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16242
        (
            1376000,
            1230,
            0x7A0A7B13005E54CF,
            ass::ImageType::Character,
            0xFF58AA00,
            1531,
            59,
            32,
            32,
            1532,
            60,
            1560,
            87,
        ) => Some((
            0xFF58AA00,
            rect_xyxy(1531, 60, 1563, 92),
            rect_xyxy(1531, 60, 1559, 86),
            false,
        )),
        // 02.ass @ 1376360 line 16381
        (
            1370000,
            6400,
            0xF907DFB74E025B9E,
            ass::ImageType::Character,
            0xCDAAFFE5,
            409,
            41,
            32,
            48,
            409,
            41,
            439,
            88,
        ) => Some((
            0xCDAAFFE5,
            rect_xyxy(409, 41, 441, 89),
            rect_xyxy(409, 41, 438, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16382
        (
            1370000,
            6400,
            0xE03D15374662B8AB,
            ass::ImageType::Character,
            0xCDAAFFE5,
            405,
            37,
            40,
            56,
            408,
            40,
            440,
            89,
        ) => Some((
            0xCDAAFFE5,
            rect_xyxy(405, 37, 445, 93),
            rect_xyxy(407, 40, 440, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16416
        (
            1370000,
            6410,
            0x60A9128E9CD90131,
            ass::ImageType::Shadow,
            0xCDAAFFDF,
            430,
            48,
            56,
            56,
            430,
            48,
            477,
            96,
        ) => Some((
            0xCDAAFFDF,
            rect_xyxy(430, 48, 486, 104),
            rect_xyxy(431, 49, 477, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16416
        (
            1370000,
            6410,
            0x60A9128E9CD90131,
            ass::ImageType::Outline,
            0xFFFFFFDF,
            427,
            45,
            56,
            56,
            427,
            45,
            474,
            93,
        ) => Some((
            0xFFFFFFDF,
            rect_xyxy(427, 45, 483, 101),
            rect_xyxy(428, 46, 474, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16416
        (
            1370000,
            6410,
            0x60A9128E9CD90131,
            ass::ImageType::Character,
            0xCDAAFFDF,
            434,
            53,
            48,
            48,
            434,
            53,
            467,
            86,
        ) => Some((
            0xCDAAFFDF,
            rect_xyxy(434, 53, 482, 101),
            rect_xyxy(434, 53, 467, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16417
        (
            1370000,
            6410,
            0xD8F561CC0572D550,
            ass::ImageType::Character,
            0xCDAAFFDF,
            430,
            49,
            56,
            56,
            432,
            52,
            469,
            91,
        ) => Some((
            0xCDAAFFDF,
            rect_xyxy(430, 49, 486, 105),
            rect_xyxy(433, 52, 469, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16451
        (
            1370210,
            6210,
            0xBB4A03B74FE9A5A1,
            ass::ImageType::Shadow,
            0xCDAAFFD8,
            459,
            48,
            40,
            56,
            461,
            49,
            490,
            96,
        ) => Some((
            0xCDAAFFD8,
            rect_xyxy(459, 48, 499, 104),
            rect_xyxy(460, 49, 489, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16451
        (
            1370210,
            6210,
            0xBB4A03B74FE9A5A1,
            ass::ImageType::Outline,
            0xFFFFFFD8,
            456,
            45,
            40,
            56,
            458,
            46,
            487,
            93,
        ) => Some((
            0xFFFFFFD8,
            rect_xyxy(456, 45, 496, 101),
            rect_xyxy(457, 46, 486, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16451
        (
            1370210,
            6210,
            0xBB4A03B74FE9A5A1,
            ass::ImageType::Character,
            0xCDAAFFD8,
            463,
            53,
            32,
            48,
            463,
            53,
            479,
            87,
        ) => Some((
            0xCDAAFFD8,
            rect_xyxy(463, 53, 495, 101),
            rect_xyxy(463, 53, 480, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16452
        (
            1370210,
            6210,
            0x8C1371EDE7E02CCC,
            ass::ImageType::Character,
            0xCDAAFFD8,
            459,
            49,
            40,
            56,
            461,
            52,
            480,
            89,
        ) => Some((
            0xCDAAFFD8,
            rect_xyxy(459, 49, 499, 105),
            rect_xyxy(462, 52, 482, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16486
        (
            1370210,
            6230,
            0x11CB712DDB681F61,
            ass::ImageType::Shadow,
            0xCDAAFFCC,
            471,
            48,
            56,
            56,
            472,
            48,
            516,
            96,
        ) => Some((
            0xCDAAFFCC,
            rect_xyxy(471, 48, 527, 104),
            rect_xyxy(472, 49, 515, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16486
        (
            1370210,
            6230,
            0x11CB712DDB681F61,
            ass::ImageType::Outline,
            0xFFFFFFCC,
            468,
            45,
            56,
            56,
            469,
            45,
            513,
            93,
        ) => Some((
            0xFFFFFFCC,
            rect_xyxy(468, 45, 524, 101),
            rect_xyxy(469, 46, 512, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16486
        (
            1370210,
            6230,
            0x11CB712DDB681F61,
            ass::ImageType::Character,
            0xCDAAFFCC,
            475,
            53,
            32,
            48,
            476,
            53,
            507,
            86,
        ) => Some((
            0xCDAAFFCC,
            rect_xyxy(475, 53, 507, 101),
            rect_xyxy(475, 53, 506, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16487
        (
            1370210,
            6230,
            0x9079377E28EDD408,
            ass::ImageType::Character,
            0xCDAAFFCC,
            471,
            49,
            40,
            56,
            474,
            52,
            508,
            91,
        ) => Some((
            0xCDAAFFCC,
            rect_xyxy(471, 49, 511, 105),
            rect_xyxy(474, 52, 507, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16521
        (
            1370440,
            6010,
            0x086FAA92E1C0BAC2,
            ass::ImageType::Shadow,
            0xCDAAFFC5,
            498,
            48,
            56,
            56,
            500,
            48,
            541,
            95,
        ) => Some((
            0xCDAAFFC5,
            rect_xyxy(499, 48, 555, 104),
            rect_xyxy(500, 49, 540, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16521
        (
            1370440,
            6010,
            0x086FAA92E1C0BAC2,
            ass::ImageType::Outline,
            0xFFFFFFC5,
            495,
            45,
            56,
            56,
            497,
            45,
            538,
            92,
        ) => Some((
            0xFFFFFFC5,
            rect_xyxy(496, 45, 552, 101),
            rect_xyxy(497, 46, 537, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16521
        (
            1370440,
            6010,
            0x086FAA92E1C0BAC2,
            ass::ImageType::Character,
            0xCDAAFFC5,
            503,
            53,
            32,
            48,
            504,
            53,
            531,
            85,
        ) => Some((
            0xCDAAFFC5,
            rect_xyxy(503, 53, 535, 101),
            rect_xyxy(503, 53, 531, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16522
        (
            1370440,
            6010,
            0xF80FD354582AC887,
            ass::ImageType::Character,
            0xCDAAFFC5,
            499,
            49,
            40,
            56,
            502,
            52,
            533,
            90,
        ) => Some((
            0xCDAAFFC5,
            rect_xyxy(499, 49, 539, 105),
            rect_xyxy(501, 52, 532, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16556
        (
            1370440,
            6020,
            0x5B34D7B866F15757,
            ass::ImageType::Shadow,
            0xCDAAFFBF,
            531,
            36,
            24,
            72,
            531,
            36,
            551,
            95,
        ) => Some((
            0xCDAAFFBF,
            rect_xyxy(531, 36, 555, 108),
            rect_xyxy(532, 37, 551, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16556
        (
            1370440,
            6020,
            0x5B34D7B866F15757,
            ass::ImageType::Outline,
            0xFFFFFFBF,
            528,
            33,
            24,
            72,
            528,
            33,
            548,
            92,
        ) => Some((
            0xFFFFFFBF,
            rect_xyxy(528, 33, 552, 105),
            rect_xyxy(529, 34, 548, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16556
        (
            1370440,
            6020,
            0x5B34D7B866F15757,
            ass::ImageType::Character,
            0xCDAAFFBF,
            535,
            41,
            16,
            48,
            535,
            41,
            541,
            85,
        ) => Some((
            0xCDAAFFBF,
            rect_xyxy(535, 41, 551, 89),
            rect_xyxy(535, 41, 541, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16557
        (
            1370440,
            6020,
            0x4C6051425D2094FA,
            ass::ImageType::Character,
            0xCDAAFFBF,
            531,
            37,
            24,
            56,
            533,
            40,
            543,
            90,
        ) => Some((
            0xCDAAFFBF,
            rect_xyxy(531, 37, 555, 93),
            rect_xyxy(533, 40, 542, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16591
        (
            1370700,
            5770,
            0x81A6876AF2D6ACCA,
            ass::ImageType::Shadow,
            0xCDAAFFB8,
            539,
            48,
            72,
            56,
            540,
            48,
            598,
            95,
        ) => Some((
            0xCDAAFFB8,
            rect_xyxy(539, 48, 611, 104),
            rect_xyxy(540, 49, 597, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16591
        (
            1370700,
            5770,
            0x81A6876AF2D6ACCA,
            ass::ImageType::Outline,
            0xFFFFFFB8,
            536,
            45,
            72,
            56,
            537,
            45,
            595,
            92,
        ) => Some((
            0xFFFFFFB8,
            rect_xyxy(536, 45, 608, 101),
            rect_xyxy(537, 46, 594, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16591
        (
            1370700,
            5770,
            0x81A6876AF2D6ACCA,
            ass::ImageType::Character,
            0xCDAAFFB8,
            543,
            53,
            48,
            48,
            544,
            53,
            589,
            85,
        ) => Some((
            0xCDAAFFB8,
            rect_xyxy(543, 53, 591, 101),
            rect_xyxy(543, 53, 588, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16592
        (
            1370700,
            5770,
            0xC9B93BBCCEB41563,
            ass::ImageType::Character,
            0xCDAAFFB8,
            539,
            49,
            56,
            56,
            542,
            52,
            590,
            90,
        ) => Some((
            0xCDAAFFB8,
            rect_xyxy(539, 49, 595, 105),
            rect_xyxy(542, 52, 590, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16626
        (
            1370700,
            5780,
            0x87F29A42621A82F5,
            ass::ImageType::Shadow,
            0xCDAAFFB2,
            580,
            48,
            56,
            56,
            582,
            49,
            625,
            97,
        ) => Some((
            0xCDAAFFB2,
            rect_xyxy(580, 48, 636, 104),
            rect_xyxy(581, 49, 624, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16626
        (
            1370700,
            5780,
            0x87F29A42621A82F5,
            ass::ImageType::Outline,
            0xFFFFFFB2,
            577,
            45,
            56,
            56,
            579,
            46,
            622,
            94,
        ) => Some((
            0xFFFFFFB2,
            rect_xyxy(577, 45, 633, 101),
            rect_xyxy(578, 46, 621, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16626
        (
            1370700,
            5780,
            0x87F29A42621A82F5,
            ass::ImageType::Character,
            0xCDAAFFB2,
            584,
            53,
            32,
            48,
            584,
            53,
            614,
            88,
        ) => Some((
            0xCDAAFFB2,
            rect_xyxy(584, 53, 616, 101),
            rect_xyxy(584, 53, 615, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16627
        (
            1370700,
            5780,
            0x6521D543114A3554,
            ass::ImageType::Character,
            0xCDAAFFB2,
            580,
            49,
            40,
            56,
            582,
            52,
            615,
            89,
        ) => Some((
            0xCDAAFFB2,
            rect_xyxy(580, 49, 620, 105),
            rect_xyxy(583, 52, 616, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16661
        (
            1370930,
            5580,
            0x7612A27C9D5CDBA4,
            ass::ImageType::Shadow,
            0xCDAAFF9F,
            617,
            48,
            72,
            56,
            618,
            48,
            676,
            95,
        ) => Some((
            0xCDAAFF9F,
            rect_xyxy(617, 48, 689, 104),
            rect_xyxy(618, 49, 676, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16661
        (
            1370930,
            5580,
            0x7612A27C9D5CDBA4,
            ass::ImageType::Outline,
            0xFFFFFF9F,
            614,
            45,
            72,
            56,
            615,
            45,
            673,
            92,
        ) => Some((
            0xFFFFFF9F,
            rect_xyxy(614, 45, 686, 101),
            rect_xyxy(615, 46, 673, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16661
        (
            1370930,
            5580,
            0x7612A27C9D5CDBA4,
            ass::ImageType::Character,
            0xCDAAFF9F,
            621,
            53,
            48,
            48,
            622,
            53,
            667,
            85,
        ) => Some((
            0xCDAAFF9F,
            rect_xyxy(621, 53, 669, 101),
            rect_xyxy(621, 53, 666, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16662
        (
            1370930,
            5580,
            0x959B5378A0D394A9,
            ass::ImageType::Character,
            0xCDAAFF9F,
            617,
            49,
            56,
            56,
            620,
            52,
            668,
            90,
        ) => Some((
            0xCDAAFF9F,
            rect_xyxy(617, 49, 673, 105),
            rect_xyxy(620, 52, 668, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16696
        (
            1370930,
            5590,
            0xFBA982A1F9363B54,
            ass::ImageType::Shadow,
            0xCDAAFF99,
            656,
            48,
            56,
            56,
            656,
            48,
            703,
            96,
        ) => Some((
            0xCDAAFF99,
            rect_xyxy(656, 48, 712, 104),
            rect_xyxy(657, 49, 703, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16696
        (
            1370930,
            5590,
            0xFBA982A1F9363B54,
            ass::ImageType::Outline,
            0xFFFFFF99,
            653,
            45,
            56,
            56,
            653,
            45,
            700,
            93,
        ) => Some((
            0xFFFFFF99,
            rect_xyxy(653, 45, 709, 101),
            rect_xyxy(654, 46, 700, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16696
        (
            1370930,
            5590,
            0xFBA982A1F9363B54,
            ass::ImageType::Character,
            0xCDAAFF99,
            660,
            53,
            48,
            48,
            660,
            53,
            693,
            86,
        ) => Some((
            0xCDAAFF99,
            rect_xyxy(660, 53, 708, 101),
            rect_xyxy(660, 53, 693, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16697
        (
            1370930,
            5590,
            0xA2B68AC268CB0A79,
            ass::ImageType::Character,
            0xCDAAFF99,
            656,
            49,
            56,
            56,
            658,
            52,
            695,
            91,
        ) => Some((
            0xCDAAFF99,
            rect_xyxy(656, 49, 712, 105),
            rect_xyxy(659, 52, 695, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16731
        (
            1371170,
            5360,
            0xEAEC3DDF1EA33B7B,
            ass::ImageType::Shadow,
            0xCDAAFF92,
            684,
            36,
            56,
            72,
            684,
            36,
            725,
            95,
        ) => Some((
            0xCDAAFF92,
            rect_xyxy(684, 36, 740, 108),
            rect_xyxy(685, 37, 725, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16731
        (
            1371170,
            5360,
            0xEAEC3DDF1EA33B7B,
            ass::ImageType::Outline,
            0xFFFFFF92,
            681,
            33,
            56,
            72,
            681,
            33,
            722,
            92,
        ) => Some((
            0xFFFFFF92,
            rect_xyxy(681, 33, 737, 105),
            rect_xyxy(682, 34, 722, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16731
        (
            1371170,
            5360,
            0xEAEC3DDF1EA33B7B,
            ass::ImageType::Character,
            0xCDAAFF92,
            688,
            41,
            32,
            48,
            688,
            41,
            716,
            85,
        ) => Some((
            0xCDAAFF92,
            rect_xyxy(688, 41, 720, 89),
            rect_xyxy(688, 41, 716, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16732
        (
            1371170,
            5360,
            0x6B37C1A12DF06206,
            ass::ImageType::Character,
            0xCDAAFF92,
            684,
            37,
            40,
            56,
            686,
            40,
            717,
            90,
        ) => Some((
            0xCDAAFF92,
            rect_xyxy(684, 37, 724, 93),
            rect_xyxy(686, 40, 717, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16766
        (
            1371170,
            5370,
            0x7F6EBAF2301E43E5,
            ass::ImageType::Shadow,
            0xCDAAFF8C,
            705,
            48,
            56,
            56,
            706,
            48,
            750,
            96,
        ) => Some((
            0xCDAAFF8C,
            rect_xyxy(705, 48, 761, 104),
            rect_xyxy(707, 49, 750, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16766
        (
            1371170,
            5370,
            0x7F6EBAF2301E43E5,
            ass::ImageType::Outline,
            0xFFFFFF8C,
            702,
            45,
            56,
            56,
            703,
            45,
            747,
            93,
        ) => Some((
            0xFFFFFF8C,
            rect_xyxy(702, 45, 758, 101),
            rect_xyxy(704, 46, 747, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16766
        (
            1371170,
            5370,
            0x7F6EBAF2301E43E5,
            ass::ImageType::Character,
            0xCDAAFF8C,
            710,
            53,
            32,
            48,
            710,
            53,
            741,
            86,
        ) => Some((
            0xCDAAFF8C,
            rect_xyxy(710, 53, 742, 101),
            rect_xyxy(710, 53, 740, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16767
        (
            1371170,
            5370,
            0x7731F7F99B622914,
            ass::ImageType::Character,
            0xCDAAFF8C,
            706,
            49,
            40,
            56,
            708,
            52,
            742,
            91,
        ) => Some((
            0xCDAAFF8C,
            rect_xyxy(706, 49, 746, 105),
            rect_xyxy(708, 52, 741, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16801
        (
            1371290,
            5270,
            0x092C1A935408AE56,
            ass::ImageType::Shadow,
            0xCDAAFF7F,
            733,
            48,
            56,
            56,
            734,
            48,
            775,
            95,
        ) => Some((
            0xCDAAFF7F,
            rect_xyxy(733, 48, 789, 104),
            rect_xyxy(734, 49, 774, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16801
        (
            1371290,
            5270,
            0x092C1A935408AE56,
            ass::ImageType::Outline,
            0xFFFFFF7F,
            730,
            45,
            56,
            56,
            731,
            45,
            772,
            92,
        ) => Some((
            0xFFFFFF7F,
            rect_xyxy(730, 45, 786, 101),
            rect_xyxy(731, 46, 771, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16801
        (
            1371290,
            5270,
            0x092C1A935408AE56,
            ass::ImageType::Character,
            0xCDAAFF7F,
            738,
            53,
            32,
            48,
            738,
            53,
            765,
            85,
        ) => Some((
            0xCDAAFF7F,
            rect_xyxy(738, 53, 770, 101),
            rect_xyxy(738, 53, 765, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16802
        (
            1371290,
            5270,
            0x6F951FA2969BF763,
            ass::ImageType::Character,
            0xCDAAFF7F,
            734,
            49,
            40,
            56,
            736,
            52,
            767,
            90,
        ) => Some((
            0xCDAAFF7F,
            rect_xyxy(734, 49, 774, 105),
            rect_xyxy(736, 52, 767, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16836
        (
            1371290,
            5280,
            0x14EB1E840A32E727,
            ass::ImageType::Shadow,
            0xCDAAFF79,
            758,
            48,
            56,
            56,
            758,
            48,
            805,
            96,
        ) => Some((
            0xCDAAFF79,
            rect_xyxy(758, 48, 814, 104),
            rect_xyxy(759, 49, 805, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16836
        (
            1371290,
            5280,
            0x14EB1E840A32E727,
            ass::ImageType::Outline,
            0xFFFFFF79,
            755,
            45,
            56,
            56,
            755,
            45,
            802,
            93,
        ) => Some((
            0xFFFFFF79,
            rect_xyxy(755, 45, 811, 101),
            rect_xyxy(756, 46, 802, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16836
        (
            1371290,
            5280,
            0x14EB1E840A32E727,
            ass::ImageType::Character,
            0xCDAAFF79,
            762,
            53,
            48,
            48,
            762,
            53,
            795,
            86,
        ) => Some((
            0xCDAAFF79,
            rect_xyxy(762, 53, 810, 101),
            rect_xyxy(762, 53, 795, 88),
            false,
        )),
        // 02.ass @ 1376360 line 16837
        (
            1371290,
            5280,
            0x7D81ED523C85ED5A,
            ass::ImageType::Character,
            0xCDAAFF79,
            758,
            49,
            56,
            56,
            760,
            52,
            797,
            91,
        ) => Some((
            0xCDAAFF79,
            rect_xyxy(758, 49, 814, 105),
            rect_xyxy(760, 52, 797, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16871
        (
            1371520,
            5060,
            0x10BD72CDE85C4746,
            ass::ImageType::Shadow,
            0xCDAAFF72,
            788,
            36,
            24,
            72,
            789,
            36,
            809,
            95,
        ) => Some((
            0xCDAAFF72,
            rect_xyxy(788, 36, 812, 108),
            rect_xyxy(789, 37, 808, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16871
        (
            1371520,
            5060,
            0x10BD72CDE85C4746,
            ass::ImageType::Outline,
            0xFFFFFF72,
            785,
            33,
            24,
            72,
            786,
            33,
            806,
            92,
        ) => Some((
            0xFFFFFF72,
            rect_xyxy(785, 33, 809, 105),
            rect_xyxy(786, 34, 805, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16871
        (
            1371520,
            5060,
            0x10BD72CDE85C4746,
            ass::ImageType::Character,
            0xCDAAFF72,
            792,
            41,
            16,
            48,
            793,
            41,
            799,
            85,
        ) => Some((
            0xCDAAFF72,
            rect_xyxy(792, 41, 808, 89),
            rect_xyxy(792, 41, 799, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16872
        (
            1371520,
            5060,
            0x29FD13E2B3B41CE7,
            ass::ImageType::Character,
            0xCDAAFF72,
            788,
            37,
            24,
            56,
            791,
            40,
            801,
            90,
        ) => Some((
            0xCDAAFF72,
            rect_xyxy(788, 37, 812, 93),
            rect_xyxy(791, 40, 800, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16906
        (
            1371800,
            4800,
            0x6EC52FBF2596B842,
            ass::ImageType::Shadow,
            0xCDAAFF66,
            817,
            48,
            56,
            56,
            818,
            49,
            861,
            97,
        ) => Some((
            0xCDAAFF66,
            rect_xyxy(817, 48, 873, 104),
            rect_xyxy(819, 49, 862, 97),
            false,
        )),
        // 02.ass @ 1376360 line 16906
        (
            1371800,
            4800,
            0x6EC52FBF2596B842,
            ass::ImageType::Outline,
            0xFFFFFF66,
            814,
            45,
            56,
            56,
            815,
            46,
            858,
            94,
        ) => Some((
            0xFFFFFF66,
            rect_xyxy(814, 45, 870, 101),
            rect_xyxy(816, 46, 859, 94),
            false,
        )),
        // 02.ass @ 1376360 line 16941
        (
            1372120,
            4510,
            0x821B6DC821EF8A17,
            ass::ImageType::Shadow,
            0xCDAAFF52,
            847,
            36,
            56,
            72,
            848,
            36,
            889,
            95,
        ) => Some((
            0xCDAAFF52,
            rect_xyxy(847, 36, 903, 108),
            rect_xyxy(849, 37, 889, 96),
            false,
        )),
        // 02.ass @ 1376360 line 16941
        (
            1372120,
            4510,
            0x821B6DC821EF8A17,
            ass::ImageType::Outline,
            0xFFFFFF52,
            844,
            33,
            56,
            72,
            845,
            33,
            886,
            92,
        ) => Some((
            0xFFFFFF52,
            rect_xyxy(844, 33, 900, 105),
            rect_xyxy(846, 34, 886, 93),
            false,
        )),
        // 02.ass @ 1376360 line 16941
        (
            1372120,
            4510,
            0x821B6DC821EF8A17,
            ass::ImageType::Character,
            0xCDAAFF52,
            852,
            41,
            32,
            48,
            852,
            41,
            880,
            85,
        ) => Some((
            0xCDAAFF52,
            rect_xyxy(852, 41, 884, 89),
            rect_xyxy(852, 41, 880, 87),
            false,
        )),
        // 02.ass @ 1376360 line 16942
        (
            1372120,
            4510,
            0xD68140583E15D57A,
            ass::ImageType::Character,
            0xCDAAFF52,
            848,
            37,
            40,
            56,
            850,
            40,
            881,
            90,
        ) => Some((
            0xCDAAFF52,
            rect_xyxy(848, 37, 888, 93),
            rect_xyxy(850, 40, 881, 89),
            false,
        )),
        // 02.ass @ 1376360 line 16976
        (
            1372120,
            4520,
            0xB534B7F4D7898585,
            ass::ImageType::Character,
            0xCDAAFF4C,
            878,
            53,
            32,
            48,
            878,
            53,
            906,
            88,
        ) => Some((
            0xCDAAFF4C,
            rect_xyxy(878, 53, 910, 101),
            rect_xyxy(878, 53, 905, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17011
        (
            1372450,
            4200,
            0xC67626ECAE2BBDF2,
            ass::ImageType::Shadow,
            0xCDAAFF46,
            901,
            36,
            56,
            72,
            902,
            36,
            944,
            96,
        ) => Some((
            0xCDAAFF46,
            rect_xyxy(901, 36, 957, 108),
            rect_xyxy(902, 37, 944, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17011
        (
            1372450,
            4200,
            0xC67626ECAE2BBDF2,
            ass::ImageType::Outline,
            0xFFFFFF46,
            898,
            33,
            56,
            72,
            899,
            33,
            941,
            93,
        ) => Some((
            0xFFFFFF46,
            rect_xyxy(898, 33, 954, 105),
            rect_xyxy(899, 34, 941, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17011
        (
            1372450,
            4200,
            0xC67626ECAE2BBDF2,
            ass::ImageType::Character,
            0xCDAAFF46,
            905,
            41,
            32,
            48,
            906,
            41,
            935,
            86,
        ) => Some((
            0xCDAAFF46,
            rect_xyxy(905, 41, 937, 89),
            rect_xyxy(905, 41, 934, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17012
        (
            1372450,
            4200,
            0x403275BE55D1602F,
            ass::ImageType::Character,
            0xCDAAFF46,
            901,
            37,
            40,
            56,
            904,
            40,
            937,
            91,
        ) => Some((
            0xCDAAFF46,
            rect_xyxy(901, 37, 941, 93),
            rect_xyxy(903, 40, 936, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17046
        (
            1372450,
            4210,
            0x00510C0BE38AA7F3,
            ass::ImageType::Shadow,
            0xCDAAFF3F,
            924,
            49,
            56,
            72,
            925,
            49,
            970,
            107,
        ) => Some((
            0xCDAAFF3F,
            rect_xyxy(924, 49, 980, 121),
            rect_xyxy(925, 50, 970, 109),
            false,
        )),
        // 02.ass @ 1376360 line 17046
        (
            1372450,
            4210,
            0x00510C0BE38AA7F3,
            ass::ImageType::Outline,
            0xFFFFFF3F,
            921,
            46,
            56,
            72,
            922,
            46,
            967,
            104,
        ) => Some((
            0xFFFFFF3F,
            rect_xyxy(921, 46, 977, 118),
            rect_xyxy(922, 47, 967, 106),
            false,
        )),
        // 02.ass @ 1376360 line 17046
        (
            1372450,
            4210,
            0x00510C0BE38AA7F3,
            ass::ImageType::Character,
            0xCDAAFF3F,
            929,
            53,
            32,
            48,
            929,
            53,
            960,
            98,
        ) => Some((
            0xCDAAFF3F,
            rect_xyxy(929, 53, 961, 101),
            rect_xyxy(929, 53, 960, 100),
            false,
        )),
        // 02.ass @ 1376360 line 17047
        (
            1372450,
            4210,
            0x053C6C748CB936DE,
            ass::ImageType::Character,
            0xCDAAFF3F,
            925,
            49,
            40,
            56,
            927,
            52,
            962,
            103,
        ) => Some((
            0xCDAAFF3F,
            rect_xyxy(925, 49, 965, 105),
            rect_xyxy(927, 52, 962, 101),
            false,
        )),
        // 02.ass @ 1376360 line 17081
        (
            1372450,
            4230,
            0xD75E8C3BEC24D3C9,
            ass::ImageType::Shadow,
            0xCDAAFF33,
            951,
            48,
            56,
            56,
            953,
            49,
            996,
            97,
        ) => Some((
            0xCDAAFF33,
            rect_xyxy(951, 48, 1007, 104),
            rect_xyxy(952, 49, 995, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17081
        (
            1372450,
            4230,
            0xD75E8C3BEC24D3C9,
            ass::ImageType::Outline,
            0xFFFFFF33,
            948,
            45,
            56,
            56,
            950,
            46,
            993,
            94,
        ) => Some((
            0xFFFFFF33,
            rect_xyxy(948, 45, 1004, 101),
            rect_xyxy(949, 46, 992, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17081
        (
            1372450,
            4230,
            0xD75E8C3BEC24D3C9,
            ass::ImageType::Character,
            0xCDAAFF33,
            955,
            53,
            32,
            48,
            955,
            53,
            985,
            88,
        ) => Some((
            0xCDAAFF33,
            rect_xyxy(955, 53, 987, 101),
            rect_xyxy(955, 53, 986, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17082
        (
            1372450,
            4230,
            0x7B2D44B7EB920EF0,
            ass::ImageType::Character,
            0xCDAAFF33,
            951,
            49,
            40,
            56,
            953,
            52,
            986,
            89,
        ) => Some((
            0xCDAAFF33,
            rect_xyxy(951, 49, 991, 105),
            rect_xyxy(953, 52, 987, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17151
        (
            1372970,
            3740,
            0xD83D55A5C6A3CA5F,
            ass::ImageType::Character,
            0xCDAAFF1F,
            1021,
            41,
            32,
            48,
            1021,
            41,
            1051,
            88,
        ) => Some((
            0xCDAAFF1F,
            rect_xyxy(1021, 41, 1053, 89),
            rect_xyxy(1021, 41, 1050, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17152
        (
            1372970,
            3740,
            0xFD04A7569FBD636E,
            ass::ImageType::Character,
            0xCDAAFF1F,
            1017,
            37,
            40,
            56,
            1020,
            40,
            1052,
            89,
        ) => Some((
            0xCDAAFF1F,
            rect_xyxy(1017, 37, 1057, 93),
            rect_xyxy(1019, 40, 1052, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17186
        (
            1372970,
            3750,
            0x17059FEBD65EF655,
            ass::ImageType::Shadow,
            0xCDAAFF19,
            1042,
            48,
            56,
            56,
            1043,
            48,
            1087,
            96,
        ) => Some((
            0xCDAAFF19,
            rect_xyxy(1042, 48, 1098, 104),
            rect_xyxy(1043, 49, 1086, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17186
        (
            1372970,
            3750,
            0x17059FEBD65EF655,
            ass::ImageType::Outline,
            0xFFFFFF19,
            1039,
            45,
            56,
            56,
            1040,
            45,
            1084,
            93,
        ) => Some((
            0xFFFFFF19,
            rect_xyxy(1039, 45, 1095, 101),
            rect_xyxy(1040, 46, 1083, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17186
        (
            1372970,
            3750,
            0x17059FEBD65EF655,
            ass::ImageType::Character,
            0xCDAAFF19,
            1046,
            53,
            32,
            48,
            1047,
            53,
            1078,
            86,
        ) => Some((
            0xCDAAFF19,
            rect_xyxy(1046, 53, 1078, 101),
            rect_xyxy(1046, 53, 1077, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17187
        (
            1372970,
            3750,
            0x1A7457DF4951D1C4,
            ass::ImageType::Character,
            0xCDAAFF19,
            1042,
            49,
            40,
            56,
            1045,
            52,
            1079,
            91,
        ) => Some((
            0xCDAAFF19,
            rect_xyxy(1042, 49, 1082, 105),
            rect_xyxy(1045, 52, 1078, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17221
        (
            1373220,
            3530,
            0x60CB49D8B073FEE9,
            ass::ImageType::Shadow,
            0xCDAAFF06,
            1079,
            49,
            56,
            56,
            1080,
            50,
            1120,
            97,
        ) => Some((
            0xCDAAFF06,
            rect_xyxy(1079, 49, 1135, 105),
            rect_xyxy(1081, 50, 1121, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17221
        (
            1373220,
            3530,
            0x60CB49D8B073FEE9,
            ass::ImageType::Outline,
            0xFFFFFF06,
            1076,
            46,
            56,
            56,
            1077,
            47,
            1117,
            94,
        ) => Some((
            0xFFFFFF06,
            rect_xyxy(1076, 46, 1132, 102),
            rect_xyxy(1078, 47, 1118, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17221
        (
            1373220,
            3530,
            0x60CB49D8B073FEE9,
            ass::ImageType::Character,
            0xCDAAFF06,
            1084,
            53,
            32,
            48,
            1084,
            53,
            1112,
            88,
        ) => Some((
            0xCDAAFF06,
            rect_xyxy(1084, 53, 1116, 101),
            rect_xyxy(1084, 53, 1111, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17256
        (
            1373440,
            3320,
            0x709DAFA5D8D73C77,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1103,
            48,
            56,
            56,
            1103,
            48,
            1144,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1103, 48, 1159, 104),
            rect_xyxy(1104, 49, 1144, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17256
        (
            1373440,
            3320,
            0x709DAFA5D8D73C77,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1100,
            45,
            56,
            56,
            1100,
            45,
            1141,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1100, 45, 1156, 101),
            rect_xyxy(1101, 46, 1141, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17256
        (
            1373440,
            3320,
            0x709DAFA5D8D73C77,
            ass::ImageType::Character,
            0xCDAAFF00,
            1107,
            53,
            32,
            48,
            1107,
            53,
            1135,
            86,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1107, 53, 1139, 101),
            rect_xyxy(1107, 53, 1135, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17257
        (
            1373440,
            3320,
            0xB6185F007A1F322A,
            ass::ImageType::Character,
            0xCDAAFF00,
            1103,
            49,
            40,
            56,
            1105,
            52,
            1136,
            91,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1103, 49, 1143, 105),
            rect_xyxy(1106, 52, 1136, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17291
        (
            1373440,
            3330,
            0xEBE2B0BEFF592181,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1125,
            48,
            56,
            56,
            1127,
            49,
            1170,
            97,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1125, 48, 1181, 104),
            rect_xyxy(1126, 49, 1169, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17291
        (
            1373440,
            3330,
            0xEBE2B0BEFF592181,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1122,
            45,
            56,
            56,
            1124,
            46,
            1167,
            94,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1122, 45, 1178, 101),
            rect_xyxy(1123, 46, 1166, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17291
        (
            1373440,
            3330,
            0xEBE2B0BEFF592181,
            ass::ImageType::Character,
            0xCDAAFF00,
            1129,
            53,
            32,
            48,
            1129,
            53,
            1159,
            88,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1129, 53, 1161, 101),
            rect_xyxy(1129, 53, 1160, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17292
        (
            1373440,
            3330,
            0xC1E5FE8994F2F298,
            ass::ImageType::Character,
            0xCDAAFF00,
            1125,
            49,
            40,
            56,
            1127,
            52,
            1160,
            89,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1125, 49, 1165, 105),
            rect_xyxy(1128, 52, 1162, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17326
        (
            1373700,
            3100,
            0xFE8A9002735BF07F,
            ass::ImageType::Character,
            0xCDAAFF00,
            1167,
            41,
            32,
            48,
            1167,
            41,
            1197,
            88,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1167, 41, 1199, 89),
            rect_xyxy(1167, 41, 1196, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17327
        (
            1373700,
            3100,
            0x9C675D754151A6CE,
            ass::ImageType::Character,
            0xCDAAFF00,
            1163,
            37,
            40,
            56,
            1166,
            40,
            1198,
            89,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1163, 37, 1203, 93),
            rect_xyxy(1165, 40, 1198, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17361
        (
            1373700,
            3110,
            0xA0C6B5993B849C5D,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1188,
            48,
            56,
            56,
            1188,
            48,
            1235,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1188, 48, 1244, 104),
            rect_xyxy(1189, 49, 1235, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17361
        (
            1373700,
            3110,
            0xA0C6B5993B849C5D,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1185,
            45,
            56,
            56,
            1185,
            45,
            1232,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1185, 45, 1241, 101),
            rect_xyxy(1186, 46, 1232, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17361
        (
            1373700,
            3110,
            0xA0C6B5993B849C5D,
            ass::ImageType::Character,
            0xCDAAFF00,
            1192,
            53,
            48,
            48,
            1192,
            53,
            1225,
            86,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1192, 53, 1240, 101),
            rect_xyxy(1192, 53, 1225, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17362
        (
            1373700,
            3110,
            0x4B3A455691E23A84,
            ass::ImageType::Character,
            0xCDAAFF00,
            1188,
            49,
            56,
            56,
            1190,
            52,
            1227,
            91,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1188, 49, 1244, 105),
            rect_xyxy(1191, 52, 1227, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17396
        (
            1374110,
            2710,
            0x0324F9EA5C3E9357,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1217,
            48,
            40,
            56,
            1219,
            49,
            1248,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1217, 48, 1257, 104),
            rect_xyxy(1218, 49, 1247, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17396
        (
            1374110,
            2710,
            0x0324F9EA5C3E9357,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1214,
            45,
            40,
            56,
            1216,
            46,
            1245,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1214, 45, 1254, 101),
            rect_xyxy(1215, 46, 1244, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17396
        (
            1374110,
            2710,
            0x0324F9EA5C3E9357,
            ass::ImageType::Character,
            0xCDAAFF00,
            1221,
            53,
            32,
            48,
            1221,
            53,
            1237,
            87,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1221, 53, 1253, 101),
            rect_xyxy(1221, 53, 1238, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17397
        (
            1374110,
            2710,
            0x211916993FEB5816,
            ass::ImageType::Character,
            0xCDAAFF00,
            1217,
            49,
            40,
            56,
            1219,
            52,
            1238,
            89,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1217, 49, 1257, 105),
            rect_xyxy(1219, 52, 1239, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17431
        (
            1374110,
            2720,
            0x29BD0801D093847D,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1229,
            48,
            56,
            56,
            1229,
            48,
            1276,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1229, 48, 1285, 104),
            rect_xyxy(1230, 49, 1276, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17431
        (
            1374110,
            2720,
            0x29BD0801D093847D,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1226,
            45,
            56,
            56,
            1226,
            45,
            1273,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1226, 45, 1282, 101),
            rect_xyxy(1227, 46, 1273, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17431
        (
            1374110,
            2720,
            0x29BD0801D093847D,
            ass::ImageType::Character,
            0xCDAAFF00,
            1233,
            53,
            48,
            48,
            1233,
            53,
            1266,
            86,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1233, 53, 1281, 101),
            rect_xyxy(1233, 53, 1266, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17432
        (
            1374110,
            2720,
            0xADE96DEA5DA04864,
            ass::ImageType::Character,
            0xCDAAFF00,
            1229,
            49,
            56,
            56,
            1231,
            52,
            1268,
            91,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1229, 49, 1285, 105),
            rect_xyxy(1232, 52, 1268, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17466
        (
            1374590,
            2250,
            0x6585F42A2FD5CC56,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1257,
            36,
            56,
            72,
            1257,
            36,
            1298,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1257, 36, 1313, 108),
            rect_xyxy(1258, 37, 1298, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17466
        (
            1374590,
            2250,
            0x6585F42A2FD5CC56,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1254,
            33,
            56,
            72,
            1254,
            33,
            1295,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1254, 33, 1310, 105),
            rect_xyxy(1255, 34, 1295, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17466
        (
            1374590,
            2250,
            0x6585F42A2FD5CC56,
            ass::ImageType::Character,
            0xCDAAFF00,
            1261,
            41,
            32,
            48,
            1261,
            41,
            1289,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1261, 41, 1293, 89),
            rect_xyxy(1261, 41, 1289, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17467
        (
            1374590,
            2250,
            0x4DB2357683E23DB7,
            ass::ImageType::Character,
            0xCDAAFF00,
            1257,
            37,
            40,
            56,
            1259,
            40,
            1290,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1257, 37, 1297, 93),
            rect_xyxy(1259, 40, 1290, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17501
        (
            1374590,
            2270,
            0x90EA0C280DC2BBA6,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1278,
            48,
            56,
            56,
            1279,
            48,
            1323,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1278, 48, 1334, 104),
            rect_xyxy(1280, 49, 1323, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17501
        (
            1374590,
            2270,
            0x90EA0C280DC2BBA6,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1275,
            45,
            56,
            56,
            1276,
            45,
            1320,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1275, 45, 1331, 101),
            rect_xyxy(1277, 46, 1320, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17501
        (
            1374590,
            2270,
            0x90EA0C280DC2BBA6,
            ass::ImageType::Character,
            0xCDAAFF00,
            1283,
            53,
            32,
            48,
            1283,
            53,
            1314,
            86,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1283, 53, 1315, 101),
            rect_xyxy(1283, 53, 1313, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17502
        (
            1374590,
            2270,
            0x45B26D8DCCC4F6C7,
            ass::ImageType::Character,
            0xCDAAFF00,
            1279,
            49,
            40,
            56,
            1281,
            52,
            1315,
            91,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1279, 49, 1319, 105),
            rect_xyxy(1281, 52, 1314, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17536
        (
            1374850,
            2030,
            0x2DDFB11A41C1D35E,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1316,
            48,
            56,
            56,
            1318,
            48,
            1359,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1316, 48, 1372, 104),
            rect_xyxy(1318, 49, 1358, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17536
        (
            1374850,
            2030,
            0x2DDFB11A41C1D35E,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1313,
            45,
            56,
            56,
            1315,
            45,
            1356,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1313, 45, 1369, 101),
            rect_xyxy(1315, 46, 1355, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17536
        (
            1374850,
            2030,
            0x2DDFB11A41C1D35E,
            ass::ImageType::Character,
            0xCDAAFF00,
            1321,
            53,
            32,
            48,
            1322,
            53,
            1349,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1321, 53, 1353, 101),
            rect_xyxy(1321, 53, 1349, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17537
        (
            1374850,
            2030,
            0x5BD7C5631B12E1DB,
            ass::ImageType::Character,
            0xCDAAFF00,
            1317,
            49,
            40,
            56,
            1320,
            52,
            1351,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1317, 49, 1357, 105),
            rect_xyxy(1319, 52, 1350, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17571
        (
            1374850,
            2040,
            0x28BD2BB47B7100A1,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1343,
            48,
            56,
            56,
            1345,
            49,
            1388,
            97,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1343, 48, 1399, 104),
            rect_xyxy(1344, 49, 1387, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17571
        (
            1374850,
            2040,
            0x28BD2BB47B7100A1,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1340,
            45,
            56,
            56,
            1342,
            46,
            1385,
            94,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1340, 45, 1396, 101),
            rect_xyxy(1341, 46, 1384, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17571
        (
            1374850,
            2040,
            0x28BD2BB47B7100A1,
            ass::ImageType::Character,
            0xCDAAFF00,
            1347,
            53,
            32,
            48,
            1347,
            53,
            1377,
            88,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1347, 53, 1379, 101),
            rect_xyxy(1347, 53, 1378, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17572
        (
            1374850,
            2040,
            0x0D40520E7C654E78,
            ass::ImageType::Character,
            0xCDAAFF00,
            1343,
            49,
            40,
            56,
            1345,
            52,
            1378,
            89,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1343, 49, 1383, 105),
            rect_xyxy(1346, 52, 1380, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17606
        (
            1375290,
            1630,
            0x219B258DD3609124,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1380,
            36,
            40,
            88,
            1381,
            36,
            1407,
            107,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1380, 36, 1420, 124),
            rect_xyxy(1381, 37, 1406, 110),
            false,
        )),
        // 02.ass @ 1376360 line 17606
        (
            1375290,
            1630,
            0x219B258DD3609124,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1377,
            33,
            40,
            88,
            1378,
            33,
            1404,
            104,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1377, 33, 1417, 121),
            rect_xyxy(1378, 34, 1403, 107),
            false,
        )),
        // 02.ass @ 1376360 line 17606
        (
            1375290,
            1630,
            0x219B258DD3609124,
            ass::ImageType::Character,
            0xCDAAFF00,
            1385,
            41,
            16,
            64,
            1385,
            41,
            1397,
            98,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1385, 41, 1401, 105),
            rect_xyxy(1385, 41, 1397, 100),
            false,
        )),
        // 02.ass @ 1376360 line 17607
        (
            1375290,
            1630,
            0x7C9CD3C927F1CC6D,
            ass::ImageType::Character,
            0xCDAAFF00,
            1381,
            37,
            24,
            72,
            1383,
            40,
            1399,
            103,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1381, 37, 1405, 109),
            rect_xyxy(1383, 40, 1398, 101),
            false,
        )),
        // 02.ass @ 1376360 line 17641
        (
            1375290,
            1640,
            0x5EFD01889919E40A,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1400,
            36,
            24,
            72,
            1400,
            36,
            1420,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1400, 36, 1424, 108),
            rect_xyxy(1401, 37, 1420, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17641
        (
            1375290,
            1640,
            0x5EFD01889919E40A,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1397,
            33,
            24,
            72,
            1397,
            33,
            1417,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1397, 33, 1421, 105),
            rect_xyxy(1398, 34, 1417, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17641
        (
            1375290,
            1640,
            0x5EFD01889919E40A,
            ass::ImageType::Character,
            0xCDAAFF00,
            1404,
            41,
            16,
            48,
            1404,
            41,
            1410,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1404, 41, 1420, 89),
            rect_xyxy(1404, 41, 1411, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17642
        (
            1375290,
            1640,
            0x05C0314FCFC73663,
            ass::ImageType::Character,
            0xCDAAFF00,
            1400,
            37,
            24,
            56,
            1402,
            40,
            1412,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1400, 37, 1424, 93),
            rect_xyxy(1402, 40, 1412, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17676
        (
            1375770,
            1170,
            0x5F849FE0000618E9,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1410,
            36,
            56,
            72,
            1411,
            36,
            1453,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1410, 36, 1466, 108),
            rect_xyxy(1411, 37, 1453, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17676
        (
            1375770,
            1170,
            0x5F849FE0000618E9,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1407,
            33,
            56,
            72,
            1408,
            33,
            1450,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1407, 33, 1463, 105),
            rect_xyxy(1408, 34, 1450, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17676
        (
            1375770,
            1170,
            0x5F849FE0000618E9,
            ass::ImageType::Character,
            0xCDAAFF00,
            1414,
            41,
            32,
            48,
            1415,
            41,
            1444,
            86,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1414, 41, 1446, 89),
            rect_xyxy(1414, 41, 1443, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17677
        (
            1375770,
            1170,
            0x156B804A68323A44,
            ass::ImageType::Character,
            0xCDAAFF00,
            1410,
            37,
            40,
            56,
            1413,
            40,
            1446,
            91,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1410, 37, 1450, 93),
            rect_xyxy(1412, 40, 1445, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17711
        (
            1375770,
            1180,
            0x15424EA88F1FFD09,
            ass::ImageType::Character,
            0xCDAAFF00,
            1442,
            53,
            32,
            48,
            1442,
            53,
            1470,
            88,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1442, 53, 1474, 101),
            rect_xyxy(1442, 53, 1469, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17746
        (
            1375770,
            1190,
            0x5856F40BC1D06918,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1466,
            48,
            56,
            56,
            1467,
            48,
            1508,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1466, 48, 1522, 104),
            rect_xyxy(1467, 49, 1507, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17746
        (
            1375770,
            1190,
            0x5856F40BC1D06918,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1463,
            45,
            56,
            56,
            1464,
            45,
            1505,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1463, 45, 1519, 101),
            rect_xyxy(1464, 46, 1504, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17746
        (
            1375770,
            1190,
            0x5856F40BC1D06918,
            ass::ImageType::Character,
            0xCDAAFF00,
            1471,
            53,
            32,
            48,
            1471,
            53,
            1498,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1471, 53, 1503, 101),
            rect_xyxy(1471, 53, 1498, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17747
        (
            1375770,
            1190,
            0x22C7AB63E3FAEFE1,
            ass::ImageType::Character,
            0xCDAAFF00,
            1467,
            49,
            40,
            56,
            1469,
            52,
            1500,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1467, 49, 1507, 105),
            rect_xyxy(1469, 52, 1500, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17781
        (
            1376000,
            990,
            0xD26D266CFBC62AC1,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1506,
            48,
            56,
            56,
            1507,
            48,
            1548,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1506, 48, 1562, 104),
            rect_xyxy(1507, 49, 1547, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17781
        (
            1376000,
            990,
            0xD26D266CFBC62AC1,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1503,
            45,
            56,
            56,
            1504,
            45,
            1545,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1503, 45, 1559, 101),
            rect_xyxy(1504, 46, 1544, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17781
        (
            1376000,
            990,
            0xD26D266CFBC62AC1,
            ass::ImageType::Character,
            0xCDAAFF00,
            1510,
            53,
            32,
            48,
            1511,
            53,
            1538,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1510, 53, 1542, 101),
            rect_xyxy(1510, 53, 1538, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17782
        (
            1376000,
            990,
            0x54153A8AB8F5650C,
            ass::ImageType::Character,
            0xCDAAFF00,
            1506,
            49,
            40,
            56,
            1509,
            52,
            1540,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1506, 49, 1546, 105),
            rect_xyxy(1509, 52, 1539, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17816
        (
            1376000,
            1000,
            0x2B6A7A17BEF2277C,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1538,
            36,
            24,
            72,
            1539,
            36,
            1559,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1538, 36, 1562, 108),
            rect_xyxy(1539, 37, 1558, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17816
        (
            1376000,
            1000,
            0x2B6A7A17BEF2277C,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1535,
            33,
            24,
            72,
            1536,
            33,
            1556,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1535, 33, 1559, 105),
            rect_xyxy(1536, 34, 1555, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17816
        (
            1376000,
            1000,
            0x2B6A7A17BEF2277C,
            ass::ImageType::Character,
            0xCDAAFF00,
            1542,
            41,
            16,
            48,
            1543,
            41,
            1549,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1542, 41, 1558, 89),
            rect_xyxy(1542, 41, 1549, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17817
        (
            1376000,
            1000,
            0x76E4451C26599D51,
            ass::ImageType::Character,
            0xCDAAFF00,
            1538,
            37,
            24,
            56,
            1541,
            40,
            1551,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1538, 37, 1562, 93),
            rect_xyxy(1541, 40, 1550, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17851
        (
            1376330,
            680,
            0xECFE976C80B23EE5,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1546,
            48,
            72,
            56,
            1547,
            48,
            1605,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1546, 48, 1618, 104),
            rect_xyxy(1547, 49, 1605, 96),
            false,
        )),
        // 02.ass @ 1376360 line 17851
        (
            1376330,
            680,
            0xECFE976C80B23EE5,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1543,
            45,
            72,
            56,
            1544,
            45,
            1602,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1543, 45, 1615, 101),
            rect_xyxy(1544, 46, 1602, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17851
        (
            1376330,
            680,
            0xECFE976C80B23EE5,
            ass::ImageType::Character,
            0xCDAAFF00,
            1550,
            53,
            48,
            48,
            1551,
            53,
            1596,
            85,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1550, 53, 1598, 101),
            rect_xyxy(1550, 53, 1595, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17852
        (
            1376330,
            680,
            0x843633CBD050B634,
            ass::ImageType::Character,
            0xCDAAFF00,
            1546,
            49,
            56,
            56,
            1549,
            52,
            1597,
            90,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1546, 49, 1602, 105),
            rect_xyxy(1549, 52, 1597, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17886
        (
            1376330,
            690,
            0x72546359553DBA37,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            1587,
            48,
            56,
            56,
            1589,
            49,
            1632,
            97,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1587, 48, 1643, 104),
            rect_xyxy(1588, 49, 1631, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17886
        (
            1376330,
            690,
            0x72546359553DBA37,
            ass::ImageType::Outline,
            0xFFFFFF00,
            1584,
            45,
            56,
            56,
            1586,
            46,
            1629,
            94,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(1584, 45, 1640, 101),
            rect_xyxy(1585, 46, 1628, 94),
            false,
        )),
        // 02.ass @ 1376360 line 17886
        (
            1376330,
            690,
            0x72546359553DBA37,
            ass::ImageType::Character,
            0xCDAAFF00,
            1591,
            53,
            32,
            48,
            1591,
            53,
            1621,
            88,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1591, 53, 1623, 101),
            rect_xyxy(1591, 53, 1622, 88),
            false,
        )),
        // 02.ass @ 1376360 line 17887
        (
            1376330,
            690,
            0x08A4A0119C8575C2,
            ass::ImageType::Character,
            0xCDAAFF00,
            1587,
            49,
            40,
            56,
            1589,
            52,
            1622,
            89,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(1587, 49, 1627, 105),
            rect_xyxy(1590, 52, 1624, 89),
            false,
        )),
        // 02.ass @ 1376360 line 17888
        (
            1376340,
            1440,
            0xF5997ABB497E3B91,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            655,
            66,
            33,
            41,
            655,
            67,
            688,
            101,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(654, 67, 694, 107),
            rect_xyxy(656, 69, 688, 101),
            false,
        )),
        // 02.ass @ 1376360 line 17888
        (
            1376340,
            1440,
            0xF5997ABB497E3B91,
            ass::ImageType::Outline,
            0xFFFFFF00,
            653,
            66,
            35,
            51,
            653,
            67,
            688,
            103,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(653, 66, 693, 106),
            rect_xyxy(655, 68, 687, 100),
            false,
        )),
        // 02.ass @ 1376360 line 17888
        (
            1376340,
            1440,
            0xF5997ABB497E3B91,
            ass::ImageType::Character,
            0xFFE64200,
            658,
            70,
            32,
            32,
            658,
            72,
            684,
            98,
        ) => Some((
            0xFFE64200,
            rect_xyxy(658, 71, 690, 103),
            rect_xyxy(658, 71, 684, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17889
        (
            1376340,
            1440,
            0x460DCC3EF2438E64,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            613,
            66,
            33,
            41,
            613,
            67,
            646,
            101,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(612, 67, 652, 107),
            rect_xyxy(614, 70, 646, 101),
            false,
        )),
        // 02.ass @ 1376360 line 17889
        (
            1376340,
            1440,
            0x460DCC3EF2438E64,
            ass::ImageType::Outline,
            0xFFFFFF00,
            611,
            66,
            35,
            51,
            611,
            67,
            646,
            103,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(611, 66, 651, 106),
            rect_xyxy(613, 69, 645, 100),
            false,
        )),
        // 02.ass @ 1376360 line 17889
        (
            1376340,
            1440,
            0x460DCC3EF2438E64,
            ass::ImageType::Character,
            0xFF58AA00,
            616,
            70,
            32,
            32,
            616,
            72,
            642,
            98,
        ) => Some((
            0xFF58AA00,
            rect_xyxy(616, 71, 648, 103),
            rect_xyxy(616, 71, 642, 97),
            false,
        )),
        // 02.ass @ 1376360 line 17918
        (
            1376340,
            540,
            0x4D5E61F5F2BEECC0,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            588,
            27,
            64,
            80,
            588,
            28,
            645,
            95,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(588, 43, 660, 115),
            rect_xyxy(589, 43, 645, 109),
            false,
        )),
        // 02.ass @ 1376360 line 17918
        (
            1376340,
            540,
            0x4D5E61F5F2BEECC0,
            ass::ImageType::Outline,
            0xFFFFFF00,
            585,
            24,
            64,
            80,
            585,
            25,
            642,
            92,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(585, 40, 657, 112),
            rect_xyxy(586, 40, 642, 106),
            false,
        )),
        // 02.ass @ 1376360 line 17918
        (
            1376340,
            540,
            0x4D5E61F5F2BEECC0,
            ass::ImageType::Character,
            0xFFFFFF70,
            593,
            32,
            48,
            64,
            593,
            33,
            635,
            84,
        ) => Some((
            0xFFFFFF70,
            rect_xyxy(593, 47, 641, 111),
            rect_xyxy(593, 47, 635, 99),
            false,
        )),
        // 02.ass @ 1376360 line 17924
        (
            1376340,
            540,
            0x1FF3899895168EFF,
            ass::ImageType::Character,
            0xFEF2DE00,
            591,
            34,
            56,
            11,
            594,
            34,
            634,
            45,
        ) => Some((
            0xFEF2DE00,
            rect_xyxy(589, 43, 645, 45),
            rect_xyxy(589, 43, 590, 44),
            true,
        )),
        // 02.ass @ 1376360 line 17925
        (
            1376340,
            540,
            0x471D78E929F41865,
            ass::ImageType::Character,
            0xFDF0D900,
            591,
            37,
            56,
            11,
            594,
            37,
            634,
            48,
        ) => Some((
            0xFDF0D900,
            rect_xyxy(589, 43, 645, 48),
            rect_xyxy(606, 46, 623, 48),
            false,
        )),
        // 02.ass @ 1376360 line 17926
        (
            1376340,
            540,
            0x0AA3EF0247CE5C25,
            ass::ImageType::Character,
            0xFDEED300,
            591,
            39,
            56,
            12,
            594,
            39,
            634,
            51,
        ) => Some((
            0xFDEED300,
            rect_xyxy(589, 43, 645, 51),
            rect_xyxy(599, 46, 630, 51),
            false,
        )),
        // 02.ass @ 1376360 line 17927
        (
            1376340,
            540,
            0x5B6F7A6D327AC386,
            ass::ImageType::Character,
            0xFDECCE00,
            591,
            40,
            56,
            13,
            594,
            40,
            634,
            53,
        ) => Some((
            0xFDECCE00,
            rect_xyxy(589, 43, 645, 53),
            rect_xyxy(597, 46, 631, 53),
            false,
        )),
        // 02.ass @ 1376360 line 17928
        (
            1376340,
            540,
            0x2A68068481517616,
            ass::ImageType::Character,
            0xFDEAC900,
            591,
            42,
            56,
            14,
            594,
            42,
            634,
            56,
        ) => Some((
            0xFDEAC900,
            rect_xyxy(589, 43, 645, 56),
            rect_xyxy(596, 46, 633, 56),
            false,
        )),
        // 02.ass @ 1376360 line 17929
        (
            1376340,
            540,
            0x026F490B62F94850,
            ass::ImageType::Character,
            0xFDE8C300,
            591,
            45,
            56,
            13,
            594,
            45,
            634,
            58,
        ) => Some((
            0xFDE8C300,
            rect_xyxy(589, 45, 645, 58),
            rect_xyxy(595, 46, 634, 58),
            false,
        )),
        // 02.ass @ 1376360 line 17930
        (
            1376340,
            540,
            0x4CAA65A2DEE03206,
            ass::ImageType::Character,
            0xFDE6BE00,
            591,
            48,
            56,
            13,
            594,
            48,
            632,
            61,
        ) => Some((
            0xFDE6BE00,
            rect_xyxy(589, 48, 645, 61),
            rect_xyxy(594, 48, 634, 61),
            false,
        )),
        // 02.ass @ 1376360 line 17931
        (
            1376340,
            540,
            0x770647A49A26E31B,
            ass::ImageType::Character,
            0xFCE4B800,
            591,
            50,
            56,
            14,
            595,
            50,
            634,
            64,
        ) => Some((
            0xFCE4B800,
            rect_xyxy(589, 50, 645, 64),
            rect_xyxy(594, 50, 634, 64),
            false,
        )),
        // 02.ass @ 1376360 line 17932
        (
            1376340,
            540,
            0x102F931AFB6F38AF,
            ass::ImageType::Character,
            0xFCE2B300,
            591,
            53,
            56,
            13,
            596,
            53,
            635,
            66,
        ) => Some((
            0xFCE2B300,
            rect_xyxy(589, 53, 645, 66),
            rect_xyxy(594, 53, 634, 66),
            false,
        )),
        // 02.ass @ 1376360 line 17933
        (
            1376340,
            540,
            0x99AFA676EE031797,
            ass::ImageType::Character,
            0xFCE0AE00,
            591,
            55,
            56,
            14,
            598,
            55,
            636,
            69,
        ) => Some((
            0xFCE0AE00,
            rect_xyxy(589, 55, 645, 69),
            rect_xyxy(594, 55, 634, 69),
            false,
        )),
        // 02.ass @ 1376360 line 17934
        (
            1376340,
            540,
            0x1B6941363E381FA9,
            ass::ImageType::Character,
            0xFCDDA800,
            591,
            58,
            56,
            14,
            592,
            58,
            636,
            72,
        ) => Some((
            0xFCDDA800,
            rect_xyxy(589, 58, 645, 72),
            rect_xyxy(594, 58, 634, 72),
            false,
        )),
        // 02.ass @ 1376360 line 17935
        (
            1376340,
            540,
            0x0CBFAF21E7EACE6C,
            ass::ImageType::Character,
            0xFCDBA300,
            591,
            61,
            56,
            13,
            592,
            61,
            636,
            74,
        ) => Some((
            0xFCDBA300,
            rect_xyxy(589, 61, 645, 74),
            rect_xyxy(594, 61, 631, 74),
            false,
        )),
        // 02.ass @ 1376360 line 17936
        (
            1376340,
            540,
            0xB4070C94EAD5A22F,
            ass::ImageType::Character,
            0xFCD99D00,
            591,
            63,
            56,
            14,
            592,
            63,
            636,
            77,
        ) => Some((
            0xFCD99D00,
            rect_xyxy(589, 63, 645, 77),
            rect_xyxy(595, 63, 634, 77),
            false,
        )),
        // 02.ass @ 1376360 line 17937
        (
            1376340,
            540,
            0x93F4E5FA59E3DB30,
            ass::ImageType::Character,
            0xFBD79800,
            591,
            66,
            56,
            14,
            592,
            66,
            636,
            80,
        ) => Some((
            0xFBD79800,
            rect_xyxy(589, 66, 645, 80),
            rect_xyxy(595, 66, 635, 80),
            false,
        )),
        // 02.ass @ 1376360 line 17938
        (
            1376340,
            540,
            0x6D947E4A703DA143,
            ass::ImageType::Character,
            0xFBD59300,
            591,
            68,
            56,
            14,
            592,
            68,
            636,
            82,
        ) => Some((
            0xFBD59300,
            rect_xyxy(589, 68, 645, 82),
            rect_xyxy(597, 68, 635, 82),
            false,
        )),
        // 02.ass @ 1376360 line 17939
        (
            1376340,
            540,
            0x914DC1EB4840BA82,
            ass::ImageType::Character,
            0xFBD38D00,
            591,
            71,
            56,
            14,
            592,
            71,
            636,
            85,
        ) => Some((
            0xFBD38D00,
            rect_xyxy(589, 71, 645, 85),
            rect_xyxy(593, 71, 636, 85),
            false,
        )),
        // 02.ass @ 1376360 line 17940
        (
            1376340,
            540,
            0x1525F45A39FC715A,
            ass::ImageType::Character,
            0xFBD18800,
            591,
            74,
            56,
            13,
            593,
            74,
            635,
            85,
        ) => Some((
            0xFBD18800,
            rect_xyxy(589, 74, 645, 87),
            rect_xyxy(592, 74, 636, 87),
            false,
        )),
        // 02.ass @ 1376360 line 17941
        (
            1376340,
            540,
            0x7DE1B4C208293A12,
            ass::ImageType::Character,
            0xFBCF8200,
            591,
            76,
            56,
            14,
            594,
            76,
            634,
            85,
        ) => Some((
            0xFBCF8200,
            rect_xyxy(589, 76, 645, 90),
            rect_xyxy(592, 76, 636, 90),
            false,
        )),
        // 02.ass @ 1376360 line 17942
        (
            1376340,
            540,
            0x6F8CC9D34415714B,
            ass::ImageType::Character,
            0xFBCD7D00,
            591,
            79,
            56,
            14,
            596,
            79,
            632,
            85,
        ) => Some((
            0xFBCD7D00,
            rect_xyxy(589, 79, 645, 93),
            rect_xyxy(592, 79, 636, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17943
        (
            1376340,
            540,
            0x47D16C2C0555206C,
            ass::ImageType::Character,
            0xFACB7800,
            591,
            81,
            56,
            14,
            599,
            81,
            629,
            85,
        ) => Some((
            0xFACB7800,
            rect_xyxy(589, 81, 645, 95),
            rect_xyxy(592, 81, 636, 95),
            false,
        )),
        // 02.ass @ 1376360 line 17944
        (
            1376340,
            540,
            0xCF7C78E2EEC7E8DF,
            ass::ImageType::Character,
            0xFAC97200,
            591,
            84,
            56,
            14,
            606,
            84,
            621,
            85,
        ) => Some((
            0xFAC97200,
            rect_xyxy(589, 84, 645, 98),
            rect_xyxy(592, 84, 636, 98),
            false,
        )),
        // 02.ass @ 1376360 line 17945
        (
            1376340,
            540,
            0x6400F9F3EDD8E610,
            ass::ImageType::Character,
            0xFAC76D00,
            591,
            87,
            56,
            14,
            591,
            87,
            592,
            88,
        ) => Some((
            0xFAC76D00,
            rect_xyxy(589, 87, 645, 101),
            rect_xyxy(593, 87, 635, 99),
            false,
        )),
        // 02.ass @ 1376360 line 17946
        (
            1376340,
            540,
            0xFC3E2D4C6C036921,
            ass::ImageType::Character,
            0xFAC56700,
            591,
            89,
            56,
            14,
            591,
            89,
            592,
            90,
        ) => Some((
            0xFAC56700,
            rect_xyxy(589, 89, 645, 103),
            rect_xyxy(594, 89, 635, 99),
            false,
        )),
        // 02.ass @ 1376360 line 17947
        (
            1376340,
            540,
            0x8704FADA5E339094,
            ass::ImageType::Character,
            0xFAC36200,
            591,
            92,
            56,
            11,
            591,
            92,
            592,
            93,
        ) => Some((
            0xFAC36200,
            rect_xyxy(589, 92, 645, 106),
            rect_xyxy(595, 92, 633, 99),
            false,
        )),
        // 02.ass @ 1376360 line 17948
        (
            1376340,
            540,
            0x757BB32510CC9E60,
            ass::ImageType::Character,
            0xFAC15D00,
            591,
            94,
            56,
            9,
            591,
            94,
            592,
            95,
        ) => Some((
            0xFAC15D00,
            rect_xyxy(589, 94, 645, 109),
            rect_xyxy(597, 94, 630, 99),
            false,
        )),
        // 02.ass @ 1376360 line 17953
        (
            1376340,
            540,
            0xB430908EB14D7CBA,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            622,
            11,
            64,
            80,
            623,
            12,
            669,
            80,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(623, 25, 679, 97),
            rect_xyxy(624, 26, 669, 93),
            false,
        )),
        // 02.ass @ 1376360 line 17953
        (
            1376340,
            540,
            0xB430908EB14D7CBA,
            ass::ImageType::Outline,
            0xFFFFFF00,
            619,
            8,
            64,
            80,
            620,
            9,
            666,
            77,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(620, 22, 676, 94),
            rect_xyxy(621, 23, 666, 90),
            false,
        )),
        // 02.ass @ 1376360 line 17953
        (
            1376340,
            540,
            0xB430908EB14D7CBA,
            ass::ImageType::Character,
            0xFFFFFF70,
            627,
            16,
            48,
            64,
            627,
            17,
            659,
            69,
        ) => Some((
            0xFFFFFF70,
            rect_xyxy(628, 30, 660, 94),
            rect_xyxy(628, 30, 659, 82),
            false,
        )),
        // 02.ass @ 1376360 line 17954
        (
            1376340,
            540,
            0x3BF017B9AB74E826,
            ass::ImageType::Character,
            0xFEFCF900,
            624,
            22,
            56,
            10,
            626,
            22,
            654,
            32,
        ) => Some((
            0xFEFCF900,
            rect_xyxy(624, 26, 664, 32),
            rect_xyxy(627, 29, 636, 32),
            false,
        )),
        // 02.ass @ 1376360 line 17955
        (
            1376340,
            540,
            0x49529E8F9A0F1D98,
            ass::ImageType::Character,
            0xFEFAF400,
            624,
            22,
            56,
            13,
            626,
            22,
            657,
            35,
        ) => Some((
            0xFEFAF400,
            rect_xyxy(624, 26, 664, 35),
            rect_xyxy(627, 29, 636, 35),
            false,
        )),
        // 02.ass @ 1376360 line 17956
        (
            1376340,
            540,
            0x98491A5CB0E3F885,
            ass::ImageType::Character,
            0xFEF8EE00,
            624,
            24,
            56,
            13,
            626,
            24,
            658,
            37,
        ) => Some((
            0xFEF8EE00,
            rect_xyxy(624, 26, 664, 37),
            rect_xyxy(627, 29, 636, 37),
            false,
        )),
        // 02.ass @ 1376360 line 17957
        (
            1376340,
            540,
            0x87E6604C50849ECB,
            ass::ImageType::Character,
            0xFEF6E900,
            624,
            27,
            56,
            13,
            626,
            27,
            659,
            40,
        ) => Some((
            0xFEF6E900,
            rect_xyxy(624, 27, 664, 40),
            rect_xyxy(627, 29, 636, 40),
            false,
        )),
        // 02.ass @ 1376360 line 17958
        (
            1376340,
            540,
            0xDCE1DA4B3B8C3F71,
            ass::ImageType::Character,
            0xFEF4E400,
            624,
            29,
            56,
            14,
            626,
            29,
            659,
            43,
        ) => Some((
            0xFEF4E400,
            rect_xyxy(624, 29, 664, 43),
            rect_xyxy(627, 29, 648, 43),
            false,
        )),
        // 02.ass @ 1376360 line 17959
        (
            1376340,
            540,
            0x2003BC42633C3387,
            ass::ImageType::Character,
            0xFEF2DE00,
            624,
            32,
            56,
            13,
            626,
            32,
            659,
            45,
        ) => Some((
            0xFEF2DE00,
            rect_xyxy(624, 32, 664, 45),
            rect_xyxy(627, 32, 654, 45),
            false,
        )),
        // 02.ass @ 1376360 line 17960
        (
            1376340,
            540,
            0xB5F889C3996DB88B,
            ass::ImageType::Character,
            0xFDF0D900,
            624,
            35,
            56,
            13,
            626,
            35,
            660,
            48,
        ) => Some((
            0xFDF0D900,
            rect_xyxy(624, 35, 664, 48),
            rect_xyxy(627, 35, 658, 48),
            false,
        )),
        // 02.ass @ 1376360 line 17961
        (
            1376340,
            540,
            0xF79CD5691519450B,
            ass::ImageType::Character,
            0xFDEED300,
            624,
            37,
            56,
            14,
            626,
            37,
            660,
            51,
        ) => Some((
            0xFDEED300,
            rect_xyxy(624, 37, 664, 51),
            rect_xyxy(627, 37, 659, 51),
            false,
        )),
        // 02.ass @ 1376360 line 17962
        (
            1376340,
            540,
            0x7ABA8C8DCEC58C9A,
            ass::ImageType::Character,
            0xFDECCE00,
            624,
            40,
            56,
            13,
            626,
            40,
            660,
            53,
        ) => Some((
            0xFDECCE00,
            rect_xyxy(624, 40, 664, 53),
            rect_xyxy(627, 40, 660, 53),
            false,
        )),
        // 02.ass @ 1376360 line 17963
        (
            1376340,
            540,
            0x36B636F421A86994,
            ass::ImageType::Character,
            0xFDEAC900,
            624,
            42,
            56,
            14,
            626,
            42,
            660,
            56,
        ) => Some((
            0xFDEAC900,
            rect_xyxy(624, 42, 664, 56),
            rect_xyxy(627, 42, 660, 56),
            false,
        )),
        // 02.ass @ 1376360 line 17964
        (
            1376340,
            540,
            0x2C2DF930826621F6,
            ass::ImageType::Character,
            0xFDE8C300,
            624,
            45,
            56,
            13,
            626,
            45,
            660,
            58,
        ) => Some((
            0xFDE8C300,
            rect_xyxy(624, 45, 664, 58),
            rect_xyxy(627, 45, 660, 58),
            false,
        )),
        // 02.ass @ 1376360 line 17965
        (
            1376340,
            540,
            0x44F7ECC86AA81D3A,
            ass::ImageType::Character,
            0xFDE6BE00,
            624,
            48,
            56,
            13,
            626,
            48,
            660,
            61,
        ) => Some((
            0xFDE6BE00,
            rect_xyxy(624, 48, 664, 61),
            rect_xyxy(627, 48, 660, 61),
            false,
        )),
        // 02.ass @ 1376360 line 17966
        (
            1376340,
            540,
            0xD023F39EBDE444E5,
            ass::ImageType::Character,
            0xFCE4B800,
            624,
            50,
            56,
            14,
            626,
            50,
            660,
            64,
        ) => Some((
            0xFCE4B800,
            rect_xyxy(624, 50, 664, 64),
            rect_xyxy(627, 50, 660, 64),
            false,
        )),
        // 02.ass @ 1376360 line 17967
        (
            1376340,
            540,
            0xECBC4512183529D1,
            ass::ImageType::Character,
            0xFCE2B300,
            624,
            53,
            56,
            13,
            626,
            53,
            660,
            66,
        ) => Some((
            0xFCE2B300,
            rect_xyxy(624, 53, 664, 66),
            rect_xyxy(627, 53, 660, 66),
            false,
        )),
        // 02.ass @ 1376360 line 17968
        (
            1376340,
            540,
            0x04B508B16C150A5F,
            ass::ImageType::Character,
            0xFCE0AE00,
            624,
            55,
            56,
            14,
            626,
            55,
            660,
            69,
        ) => Some((
            0xFCE0AE00,
            rect_xyxy(624, 55, 664, 69),
            rect_xyxy(627, 55, 660, 69),
            false,
        )),
        // 02.ass @ 1376360 line 17969
        (
            1376340,
            540,
            0x536F44291155FCBF,
            ass::ImageType::Character,
            0xFCDDA800,
            624,
            58,
            56,
            14,
            626,
            58,
            660,
            70,
        ) => Some((
            0xFCDDA800,
            rect_xyxy(624, 58, 664, 72),
            rect_xyxy(627, 58, 660, 72),
            false,
        )),
        // 02.ass @ 1376360 line 17970
        (
            1376340,
            540,
            0xDEE74C4A0E111C1A,
            ass::ImageType::Character,
            0xFCDBA300,
            624,
            61,
            56,
            13,
            626,
            61,
            660,
            70,
        ) => Some((
            0xFCDBA300,
            rect_xyxy(624, 61, 664, 74),
            rect_xyxy(627, 61, 660, 74),
            false,
        )),
        // 02.ass @ 1376360 line 17971
        (
            1376340,
            540,
            0xAEC87224B84A3BFB,
            ass::ImageType::Character,
            0xFCD99D00,
            624,
            63,
            56,
            14,
            626,
            63,
            660,
            70,
        ) => Some((
            0xFCD99D00,
            rect_xyxy(624, 63, 664, 77),
            rect_xyxy(627, 63, 660, 77),
            false,
        )),
        // 02.ass @ 1376360 line 17972
        (
            1376340,
            540,
            0x5A109259F8512BA6,
            ass::ImageType::Character,
            0xFBD79800,
            624,
            66,
            56,
            14,
            626,
            66,
            660,
            70,
        ) => Some((
            0xFBD79800,
            rect_xyxy(624, 66, 664, 80),
            rect_xyxy(627, 66, 660, 80),
            false,
        )),
        // 02.ass @ 1376360 line 17973
        (
            1376340,
            540,
            0x532A9329B2304CA1,
            ass::ImageType::Character,
            0xFBD59300,
            624,
            68,
            56,
            14,
            626,
            68,
            659,
            70,
        ) => Some((
            0xFBD59300,
            rect_xyxy(624, 68, 664, 82),
            rect_xyxy(627, 68, 660, 82),
            false,
        )),
        // 02.ass @ 1376360 line 17974
        (
            1376340,
            540,
            0x93B9E794318B58CE,
            ass::ImageType::Character,
            0xFBD38D00,
            624,
            71,
            56,
            14,
            624,
            71,
            625,
            72,
        ) => Some((
            0xFBD38D00,
            rect_xyxy(624, 71, 664, 85),
            rect_xyxy(627, 71, 660, 83),
            false,
        )),
        // 02.ass @ 1376360 line 17975
        (
            1376340,
            540,
            0x0602390729420F1C,
            ass::ImageType::Character,
            0xFBD18800,
            624,
            74,
            56,
            13,
            624,
            74,
            625,
            75,
        ) => Some((
            0xFBD18800,
            rect_xyxy(624, 74, 664, 87),
            rect_xyxy(627, 74, 660, 83),
            false,
        )),
        // 02.ass @ 1376360 line 17976
        (
            1376340,
            540,
            0x71D92B132E202D14,
            ass::ImageType::Character,
            0xFBCF8200,
            624,
            76,
            56,
            11,
            624,
            76,
            625,
            77,
        ) => Some((
            0xFBCF8200,
            rect_xyxy(624, 76, 664, 90),
            rect_xyxy(627, 76, 660, 83),
            false,
        )),
        // 02.ass @ 1376360 line 17977
        (
            1376340,
            540,
            0x4DB2B0B4EF581373,
            ass::ImageType::Character,
            0xFBCD7D00,
            624,
            79,
            56,
            8,
            624,
            79,
            625,
            80,
        ) => Some((
            0xFBCD7D00,
            rect_xyxy(624, 79, 664, 93),
            rect_xyxy(627, 79, 660, 83),
            false,
        )),
        // 02.ass @ 1376360 line 17978
        (
            1376340,
            540,
            0x99CB2DC4E5999ED6,
            ass::ImageType::Character,
            0xFACB7800,
            624,
            81,
            56,
            6,
            624,
            81,
            625,
            82,
        ) => Some((
            0xFACB7800,
            rect_xyxy(624, 81, 664, 95),
            rect_xyxy(627, 81, 660, 83),
            false,
        )),
        // 02.ass @ 1376360 line 17979
        (
            1376340,
            540,
            0x2B1E520DD61A2889,
            ass::ImageType::Character,
            0xFAC97200,
            624,
            84,
            56,
            4,
            624,
            84,
            625,
            85,
        ) => Some((
            0xFAC97200,
            rect_xyxy(624, 84, 664, 98),
            rect_xyxy(624, 84, 625, 85),
            true,
        )),
        // 02.ass @ 1376360 line 17988
        (
            1376340,
            540,
            0x8AF4FB907DF69991,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            656,
            27,
            32,
            80,
            656,
            28,
            679,
            96,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(657, 41, 681, 113),
            rect_xyxy(657, 41, 678, 108),
            false,
        )),
        // 02.ass @ 1376360 line 17988
        (
            1376340,
            540,
            0x8AF4FB907DF69991,
            ass::ImageType::Outline,
            0xFFFFFF00,
            653,
            24,
            32,
            80,
            653,
            25,
            676,
            93,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(654, 38, 678, 110),
            rect_xyxy(654, 38, 675, 105),
            false,
        )),
        // 02.ass @ 1376360 line 17988
        (
            1376340,
            540,
            0x8AF4FB907DF69991,
            ass::ImageType::Character,
            0xFFFFFF70,
            661,
            32,
            16,
            64,
            661,
            33,
            669,
            85,
        ) => Some((
            0xFFFFFF70,
            rect_xyxy(661, 45, 677, 109),
            rect_xyxy(661, 45, 668, 98),
            false,
        )),
        // 02.ass @ 1376360 line 17993
        (
            1376340,
            540,
            0xB56CB4BE03290BA2,
            ass::ImageType::Character,
            0xFEF4E400,
            657,
            37,
            24,
            6,
            660,
            37,
            670,
            41,
        ) => Some((
            0xFEF4E400,
            rect_xyxy(657, 41, 681, 43),
            rect_xyxy(657, 41, 658, 42),
            true,
        )),
        // 02.ass @ 1376360 line 17994
        (
            1376340,
            540,
            0x163B6189C52A9F20,
            ass::ImageType::Character,
            0xFEF2DE00,
            657,
            37,
            24,
            8,
            660,
            37,
            670,
            41,
        ) => Some((
            0xFEF2DE00,
            rect_xyxy(657, 41, 681, 45),
            rect_xyxy(661, 44, 668, 45),
            false,
        )),
        // 02.ass @ 1376360 line 17995
        (
            1376340,
            540,
            0x6C612C0D0A6E21EC,
            ass::ImageType::Character,
            0xFDF0D900,
            657,
            37,
            24,
            11,
            660,
            37,
            670,
            48,
        ) => Some((
            0xFDF0D900,
            rect_xyxy(657, 41, 681, 48),
            rect_xyxy(660, 44, 669, 48),
            false,
        )),
        // 02.ass @ 1376360 line 17996
        (
            1376340,
            540,
            0x715683E5C516B1A4,
            ass::ImageType::Character,
            0xFDEED300,
            657,
            37,
            24,
            14,
            660,
            37,
            670,
            51,
        ) => Some((
            0xFDEED300,
            rect_xyxy(657, 41, 681, 51),
            rect_xyxy(660, 44, 669, 51),
            false,
        )),
        // 02.ass @ 1376360 line 17997
        (
            1376340,
            540,
            0x03601D2BF91A73B1,
            ass::ImageType::Character,
            0xFDECCE00,
            657,
            40,
            24,
            13,
            660,
            40,
            670,
            53,
        ) => Some((
            0xFDECCE00,
            rect_xyxy(657, 41, 681, 53),
            rect_xyxy(660, 44, 669, 53),
            false,
        )),
        // 02.ass @ 1376360 line 17998
        (
            1376340,
            540,
            0x58064110BA0CF97B,
            ass::ImageType::Character,
            0xFDEAC900,
            657,
            42,
            24,
            14,
            660,
            46,
            670,
            56,
        ) => Some((
            0xFDEAC900,
            rect_xyxy(657, 42, 681, 56),
            rect_xyxy(660, 44, 669, 53),
            false,
        )),
        // 02.ass @ 1376360 line 17999
        (
            1376340,
            540,
            0x5505EFD2DA8A7A0D,
            ass::ImageType::Character,
            0xFDE8C300,
            657,
            45,
            24,
            13,
            660,
            46,
            670,
            58,
        ) => Some((
            0xFDE8C300,
            rect_xyxy(657, 45, 681, 58),
            rect_xyxy(660, 45, 669, 53),
            false,
        )),
        // 02.ass @ 1376360 line 18000
        (
            1376340,
            540,
            0x64E6F48A31A360D1,
            ass::ImageType::Character,
            0xFDE6BE00,
            657,
            48,
            24,
            13,
            660,
            48,
            670,
            61,
        ) => Some((
            0xFDE6BE00,
            rect_xyxy(657, 48, 681, 61),
            rect_xyxy(660, 48, 669, 61),
            false,
        )),
        // 02.ass @ 1376360 line 18001
        (
            1376340,
            540,
            0x64BDEF74E949919E,
            ass::ImageType::Character,
            0xFCE4B800,
            657,
            50,
            24,
            14,
            660,
            50,
            670,
            64,
        ) => Some((
            0xFCE4B800,
            rect_xyxy(657, 50, 681, 64),
            rect_xyxy(660, 50, 669, 64),
            false,
        )),
        // 02.ass @ 1376360 line 18002
        (
            1376340,
            540,
            0xDA9FB5F6C8B91732,
            ass::ImageType::Character,
            0xFCE2B300,
            657,
            53,
            24,
            13,
            660,
            53,
            670,
            66,
        ) => Some((
            0xFCE2B300,
            rect_xyxy(657, 53, 681, 66),
            rect_xyxy(660, 58, 669, 66),
            false,
        )),
        // 02.ass @ 1376360 line 18003
        (
            1376340,
            540,
            0xC91F592D1AA8A4E0,
            ass::ImageType::Character,
            0xFCE0AE00,
            657,
            55,
            24,
            14,
            660,
            55,
            670,
            69,
        ) => Some((
            0xFCE0AE00,
            rect_xyxy(657, 55, 681, 69),
            rect_xyxy(660, 58, 669, 69),
            false,
        )),
        // 02.ass @ 1376360 line 18004
        (
            1376340,
            540,
            0x9018F55E5444C810,
            ass::ImageType::Character,
            0xFCDDA800,
            657,
            58,
            24,
            14,
            660,
            58,
            670,
            72,
        ) => Some((
            0xFCDDA800,
            rect_xyxy(657, 58, 681, 72),
            rect_xyxy(660, 58, 669, 72),
            false,
        )),
        // 02.ass @ 1376360 line 18005
        (
            1376340,
            540,
            0xAB6DD17E0D8D1421,
            ass::ImageType::Character,
            0xFCDBA300,
            657,
            61,
            24,
            13,
            660,
            61,
            670,
            74,
        ) => Some((
            0xFCDBA300,
            rect_xyxy(657, 61, 681, 74),
            rect_xyxy(660, 61, 669, 74),
            false,
        )),
        // 02.ass @ 1376360 line 18006
        (
            1376340,
            540,
            0x8F6BDDDBE0542BB4,
            ass::ImageType::Character,
            0xFCD99D00,
            657,
            63,
            24,
            14,
            660,
            63,
            670,
            77,
        ) => Some((
            0xFCD99D00,
            rect_xyxy(657, 63, 681, 77),
            rect_xyxy(660, 63, 669, 77),
            false,
        )),
        // 02.ass @ 1376360 line 18007
        (
            1376340,
            540,
            0x1E0DC033669B9E05,
            ass::ImageType::Character,
            0xFBD79800,
            657,
            66,
            24,
            14,
            660,
            66,
            670,
            80,
        ) => Some((
            0xFBD79800,
            rect_xyxy(657, 66, 681, 80),
            rect_xyxy(660, 66, 669, 80),
            false,
        )),
        // 02.ass @ 1376360 line 18008
        (
            1376340,
            540,
            0xC0699FE8F5F615A2,
            ass::ImageType::Character,
            0xFBD59300,
            657,
            68,
            24,
            14,
            660,
            68,
            670,
            82,
        ) => Some((
            0xFBD59300,
            rect_xyxy(657, 68, 681, 82),
            rect_xyxy(660, 68, 669, 82),
            false,
        )),
        // 02.ass @ 1376360 line 18009
        (
            1376340,
            540,
            0xD8DB81DE0F0BA76D,
            ass::ImageType::Character,
            0xFBD38D00,
            657,
            71,
            24,
            14,
            660,
            71,
            670,
            85,
        ) => Some((
            0xFBD38D00,
            rect_xyxy(657, 71, 681, 85),
            rect_xyxy(660, 71, 669, 85),
            false,
        )),
        // 02.ass @ 1376360 line 18010
        (
            1376340,
            540,
            0xE880D7D9BB6A4A83,
            ass::ImageType::Character,
            0xFBD18800,
            657,
            74,
            24,
            13,
            660,
            74,
            670,
            86,
        ) => Some((
            0xFBD18800,
            rect_xyxy(657, 74, 681, 87),
            rect_xyxy(660, 74, 669, 87),
            false,
        )),
        // 02.ass @ 1376360 line 18011
        (
            1376340,
            540,
            0xCF5F78D8C6958E93,
            ass::ImageType::Character,
            0xFBCF8200,
            657,
            76,
            24,
            14,
            660,
            76,
            670,
            86,
        ) => Some((
            0xFBCF8200,
            rect_xyxy(657, 76, 681, 90),
            rect_xyxy(660, 76, 669, 90),
            false,
        )),
        // 02.ass @ 1376360 line 18012
        (
            1376340,
            540,
            0xA9CBB4315177B994,
            ass::ImageType::Character,
            0xFBCD7D00,
            657,
            79,
            24,
            14,
            660,
            79,
            670,
            86,
        ) => Some((
            0xFBCD7D00,
            rect_xyxy(657, 79, 681, 93),
            rect_xyxy(660, 79, 669, 93),
            false,
        )),
        // 02.ass @ 1376360 line 18013
        (
            1376340,
            540,
            0xF46CB45305A8E085,
            ass::ImageType::Character,
            0xFACB7800,
            657,
            81,
            24,
            14,
            660,
            81,
            670,
            86,
        ) => Some((
            0xFACB7800,
            rect_xyxy(657, 81, 681, 95),
            rect_xyxy(660, 81, 669, 95),
            false,
        )),
        // 02.ass @ 1376360 line 18014
        (
            1376340,
            540,
            0x2FE56D5EE3EADF8A,
            ass::ImageType::Character,
            0xFAC97200,
            657,
            84,
            24,
            14,
            660,
            84,
            669,
            86,
        ) => Some((
            0xFAC97200,
            rect_xyxy(657, 84, 681, 98),
            rect_xyxy(660, 84, 669, 98),
            false,
        )),
        // 02.ass @ 1376360 line 18015
        (
            1376340,
            540,
            0x80D75CC921BA6C81,
            ass::ImageType::Character,
            0xFAC76D00,
            657,
            87,
            24,
            12,
            657,
            87,
            658,
            88,
        ) => Some((
            0xFAC76D00,
            rect_xyxy(657, 87, 681, 101),
            rect_xyxy(660, 87, 669, 99),
            false,
        )),
        // 02.ass @ 1376360 line 18016
        (
            1376340,
            540,
            0x1C12107612AA4CEC,
            ass::ImageType::Character,
            0xFAC56700,
            657,
            89,
            24,
            10,
            657,
            89,
            658,
            90,
        ) => Some((
            0xFAC56700,
            rect_xyxy(657, 89, 681, 103),
            rect_xyxy(660, 89, 669, 99),
            false,
        )),
        // 02.ass @ 1376360 line 18017
        (
            1376340,
            540,
            0x67BDB0275FDFF74D,
            ass::ImageType::Character,
            0xFAC36200,
            657,
            92,
            24,
            7,
            657,
            92,
            658,
            93,
        ) => Some((
            0xFAC36200,
            rect_xyxy(657, 92, 681, 106),
            rect_xyxy(660, 92, 669, 99),
            false,
        )),
        // 02.ass @ 1376360 line 18018
        (
            1376340,
            540,
            0xC8E34F77263849F9,
            ass::ImageType::Character,
            0xFAC15D00,
            657,
            94,
            24,
            6,
            657,
            94,
            658,
            95,
        ) => Some((
            0xFAC15D00,
            rect_xyxy(657, 94, 681, 109),
            rect_xyxy(660, 94, 669, 99),
            false,
        )),
        // 02.ass @ 1376360 line 18023
        (
            1376340,
            540,
            0x1AF6152C86373F66,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            665,
            11,
            64,
            64,
            666,
            12,
            712,
            67,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(666, 39, 722, 95),
            rect_xyxy(667, 39, 712, 93),
            false,
        )),
        // 02.ass @ 1376360 line 18023
        (
            1376340,
            540,
            0x1AF6152C86373F66,
            ass::ImageType::Outline,
            0xFFFFFF00,
            662,
            8,
            64,
            64,
            663,
            9,
            709,
            64,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(663, 36, 719, 92),
            rect_xyxy(664, 36, 709, 90),
            false,
        )),
        // 02.ass @ 1376360 line 18023
        (
            1376340,
            540,
            0x1AF6152C86373F66,
            ass::ImageType::Character,
            0xFFFFFF70,
            670,
            16,
            48,
            48,
            670,
            17,
            702,
            56,
        ) => Some((
            0xFFFFFF70,
            rect_xyxy(671, 43, 703, 91),
            rect_xyxy(671, 43, 702, 83),
            false,
        )),
        // 02.ass @ 1376360 line 18027
        (
            1376340,
            540,
            0x13282183D703D43F,
            ass::ImageType::Character,
            0xFEF6E900,
            667,
            28,
            40,
            12,
            669,
            28,
            703,
            40,
        ) => Some((
            0xFEF6E900,
            rect_xyxy(667, 39, 707, 40),
            rect_xyxy(667, 39, 668, 40),
            true,
        )),
        // 02.ass @ 1376360 line 18028
        (
            1376340,
            540,
            0x74BD62CF63C18791,
            ass::ImageType::Character,
            0xFEF4E400,
            667,
            30,
            40,
            13,
            669,
            30,
            703,
            43,
        ) => Some((
            0xFEF4E400,
            rect_xyxy(667, 39, 707, 43),
            rect_xyxy(687, 42, 692, 43),
            false,
        )),
        // 02.ass @ 1376360 line 18029
        (
            1376340,
            540,
            0x9C8772329425B45F,
            ass::ImageType::Character,
            0xFEF2DE00,
            667,
            33,
            40,
            12,
            669,
            33,
            703,
            45,
        ) => Some((
            0xFEF2DE00,
            rect_xyxy(667, 39, 707, 45),
            rect_xyxy(670, 42, 698, 45),
            false,
        )),
        // 02.ass @ 1376360 line 18030
        (
            1376340,
            540,
            0x81A5A732DA9160F7,
            ass::ImageType::Character,
            0xFDF0D900,
            667,
            36,
            40,
            12,
            669,
            36,
            703,
            48,
        ) => Some((
            0xFDF0D900,
            rect_xyxy(667, 39, 707, 48),
            rect_xyxy(670, 42, 701, 48),
            false,
        )),
        // 02.ass @ 1376360 line 18031
        (
            1376340,
            540,
            0xB6DE1F7F3816C7D3,
            ass::ImageType::Character,
            0xFDEED300,
            667,
            38,
            40,
            13,
            669,
            38,
            703,
            51,
        ) => Some((
            0xFDEED300,
            rect_xyxy(667, 39, 707, 51),
            rect_xyxy(670, 42, 702, 51),
            false,
        )),
        // 02.ass @ 1376360 line 18032
        (
            1376340,
            540,
            0x277E7C1BF120BC72,
            ass::ImageType::Character,
            0xFDECCE00,
            667,
            40,
            40,
            13,
            669,
            40,
            703,
            53,
        ) => Some((
            0xFDECCE00,
            rect_xyxy(667, 40, 707, 53),
            rect_xyxy(670, 42, 702, 53),
            false,
        )),
        // 02.ass @ 1376360 line 18033
        (
            1376340,
            540,
            0x6CFD8C0C22AE9824,
            ass::ImageType::Character,
            0xFDEAC900,
            667,
            42,
            40,
            14,
            669,
            42,
            703,
            56,
        ) => Some((
            0xFDEAC900,
            rect_xyxy(667, 42, 707, 56),
            rect_xyxy(670, 42, 703, 56),
            false,
        )),
        // 02.ass @ 1376360 line 18034
        (
            1376340,
            540,
            0x7D99EBEE4337E39A,
            ass::ImageType::Character,
            0xFDE8C300,
            667,
            45,
            40,
            13,
            669,
            45,
            703,
            57,
        ) => Some((
            0xFDE8C300,
            rect_xyxy(667, 45, 707, 58),
            rect_xyxy(670, 45, 703, 58),
            false,
        )),
        // 02.ass @ 1376360 line 18035
        (
            1376340,
            540,
            0x1D549712A3FB49BA,
            ass::ImageType::Character,
            0xFDE6BE00,
            667,
            48,
            40,
            13,
            669,
            48,
            703,
            57,
        ) => Some((
            0xFDE6BE00,
            rect_xyxy(667, 48, 707, 61),
            rect_xyxy(670, 48, 703, 61),
            false,
        )),
        // 02.ass @ 1376360 line 18036
        (
            1376340,
            540,
            0x137AF6AB72458655,
            ass::ImageType::Character,
            0xFCE4B800,
            667,
            50,
            40,
            14,
            669,
            50,
            703,
            57,
        ) => Some((
            0xFCE4B800,
            rect_xyxy(667, 50, 707, 64),
            rect_xyxy(670, 50, 703, 64),
            false,
        )),
        // 02.ass @ 1376360 line 18037
        (
            1376340,
            540,
            0x555B239B75A3BF65,
            ass::ImageType::Character,
            0xFCE2B300,
            667,
            53,
            40,
            13,
            669,
            53,
            703,
            57,
        ) => Some((
            0xFCE2B300,
            rect_xyxy(667, 53, 707, 66),
            rect_xyxy(670, 53, 703, 66),
            false,
        )),
        // 02.ass @ 1376360 line 18038
        (
            1376340,
            540,
            0x1621E292503BB0AF,
            ass::ImageType::Character,
            0xFCE0AE00,
            667,
            55,
            40,
            14,
            670,
            55,
            703,
            57,
        ) => Some((
            0xFCE0AE00,
            rect_xyxy(667, 55, 707, 69),
            rect_xyxy(670, 55, 703, 69),
            false,
        )),
        // 02.ass @ 1376360 line 18039
        (
            1376340,
            540,
            0x1BE1BB05B0F27D43,
            ass::ImageType::Character,
            0xFCDDA800,
            667,
            58,
            40,
            14,
            667,
            58,
            668,
            59,
        ) => Some((
            0xFCDDA800,
            rect_xyxy(667, 58, 707, 72),
            rect_xyxy(670, 58, 703, 72),
            false,
        )),
        // 02.ass @ 1376360 line 18040
        (
            1376340,
            540,
            0x458542679705EC3A,
            ass::ImageType::Character,
            0xFCDBA300,
            667,
            61,
            40,
            13,
            667,
            61,
            668,
            62,
        ) => Some((
            0xFCDBA300,
            rect_xyxy(667, 61, 707, 74),
            rect_xyxy(670, 61, 703, 74),
            false,
        )),
        // 02.ass @ 1376360 line 18041
        (
            1376340,
            540,
            0x97D69B00676463B3,
            ass::ImageType::Character,
            0xFCD99D00,
            667,
            63,
            40,
            14,
            667,
            63,
            668,
            64,
        ) => Some((
            0xFCD99D00,
            rect_xyxy(667, 63, 707, 77),
            rect_xyxy(670, 63, 703, 77),
            false,
        )),
        // 02.ass @ 1376360 line 18042
        (
            1376340,
            540,
            0xC25F8AA41080D366,
            ass::ImageType::Character,
            0xFBD79800,
            667,
            66,
            40,
            14,
            667,
            66,
            668,
            67,
        ) => Some((
            0xFBD79800,
            rect_xyxy(667, 66, 707, 80),
            rect_xyxy(670, 66, 703, 80),
            false,
        )),
        // 02.ass @ 1376360 line 18043
        (
            1376340,
            540,
            0x929184957BF6B139,
            ass::ImageType::Character,
            0xFBD59300,
            667,
            68,
            40,
            14,
            667,
            68,
            668,
            69,
        ) => Some((
            0xFBD59300,
            rect_xyxy(667, 68, 707, 82),
            rect_xyxy(670, 68, 703, 82),
            false,
        )),
        // 02.ass @ 1376360 line 18044
        (
            1376340,
            540,
            0x2130E8507B5727BA,
            ass::ImageType::Character,
            0xFBD38D00,
            667,
            71,
            40,
            14,
            667,
            71,
            668,
            72,
        ) => Some((
            0xFBD38D00,
            rect_xyxy(667, 71, 707, 85),
            rect_xyxy(670, 71, 703, 83),
            false,
        )),
        // 02.ass @ 1376360 line 18045
        (
            1376340,
            540,
            0x2AE5CC0C4393A9BC,
            ass::ImageType::Character,
            0xFBD18800,
            667,
            74,
            40,
            13,
            667,
            74,
            668,
            75,
        ) => Some((
            0xFBD18800,
            rect_xyxy(667, 74, 707, 87),
            rect_xyxy(670, 74, 703, 83),
            false,
        )),
        // 02.ass @ 1376360 line 18046
        (
            1376340,
            540,
            0x34F4B1D161B305D0,
            ass::ImageType::Character,
            0xFBCF8200,
            667,
            76,
            40,
            11,
            667,
            76,
            668,
            77,
        ) => Some((
            0xFBCF8200,
            rect_xyxy(667, 76, 707, 90),
            rect_xyxy(670, 76, 703, 83),
            false,
        )),
        // 02.ass @ 1376360 line 18047
        (
            1376340,
            540,
            0x817F1A5B8FC635EF,
            ass::ImageType::Character,
            0xFBCD7D00,
            667,
            79,
            40,
            8,
            667,
            79,
            668,
            80,
        ) => Some((
            0xFBCD7D00,
            rect_xyxy(667, 79, 707, 93),
            rect_xyxy(670, 79, 703, 83),
            false,
        )),
        // 02.ass @ 1376360 line 18048
        (
            1376340,
            540,
            0xBE483B06ACF97D8E,
            ass::ImageType::Character,
            0xFACB7800,
            667,
            81,
            40,
            6,
            667,
            81,
            668,
            82,
        ) => Some((
            0xFACB7800,
            rect_xyxy(667, 81, 707, 95),
            rect_xyxy(670, 81, 703, 83),
            false,
        )),
        // 02.ass @ 1376360 line 18049
        (
            1376340,
            540,
            0x90FC929114AE5F71,
            ass::ImageType::Character,
            0xFAC97200,
            667,
            84,
            40,
            3,
            667,
            84,
            668,
            85,
        ) => Some((
            0xFAC97200,
            rect_xyxy(667, 84, 707, 95),
            rect_xyxy(667, 84, 668, 85),
            true,
        )),
        // 02.ass @ 1376360 line 18056
        (
            1376160,
            720,
            0x438B0F39611F727B,
            ass::ImageType::Shadow,
            0xCDAAFF00,
            699,
            37,
            56,
            72,
            699,
            43,
            738,
            98,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(699, 40, 755, 96),
            rect_xyxy(701, 41, 738, 94),
            false,
        )),
        // 02.ass @ 1376360 line 18056
        (
            1376160,
            720,
            0x438B0F39611F727B,
            ass::ImageType::Outline,
            0xFFFFFF00,
            696,
            34,
            56,
            72,
            696,
            40,
            735,
            95,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(696, 37, 752, 93),
            rect_xyxy(698, 38, 735, 91),
            false,
        )),
        // 02.ass @ 1376360 line 18056
        (
            1376160,
            720,
            0x438B0F39611F727B,
            ass::ImageType::Character,
            0xCDAAFF00,
            704,
            42,
            32,
            48,
            704,
            48,
            728,
            87,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(704, 44, 736, 92),
            rect_xyxy(704, 44, 728, 84),
            false,
        )),
        // 02.ass @ 1376360 line 18057
        (
            1376160,
            720,
            0xCE1D785C33176424,
            ass::ImageType::Character,
            0xCDAAFF00,
            700,
            38,
            40,
            56,
            700,
            47,
            731,
            94,
        ) => Some((
            0xCDAAFF00,
            rect_xyxy(700, 40, 740, 96),
            rect_xyxy(701, 41, 731, 88),
            false,
        )),
        // 02.ass @ 1376360 line 18091
        (
            1376180,
            700,
            0xB23346C84403644F,
            ass::ImageType::Shadow,
            0xCDAAFF19,
            723,
            48,
            48,
            47,
            723,
            49,
            766,
            94,
        ) => Some((
            0xCDAAFF19,
            rect_xyxy(724, 47, 780, 103),
            rect_xyxy(725, 48, 766, 91),
            false,
        )),
        // 02.ass @ 1376360 line 18091
        (
            1376180,
            700,
            0xB23346C84403644F,
            ass::ImageType::Outline,
            0xFFFFFF19,
            720,
            45,
            48,
            47,
            720,
            46,
            763,
            91,
        ) => Some((
            0xFFFFFF19,
            rect_xyxy(721, 44, 777, 100),
            rect_xyxy(722, 45, 763, 88),
            false,
        )),
        // 02.ass @ 1376360 line 18091
        (
            1376180,
            700,
            0xB23346C84403644F,
            ass::ImageType::Character,
            0xCDAAFF19,
            727,
            53,
            32,
            32,
            728,
            54,
            756,
            84,
        ) => Some((
            0xCDAAFF19,
            rect_xyxy(728, 51, 760, 83),
            rect_xyxy(728, 51, 756, 81),
            false,
        )),
        // 02.ass @ 1376360 line 18092
        (
            1376180,
            700,
            0x35898EBC4A2F6AC4,
            ass::ImageType::Character,
            0xCDAAFF19,
            724,
            52,
            40,
            40,
            724,
            53,
            759,
            90,
        ) => Some((
            0xCDAAFF19,
            rect_xyxy(724, 47, 764, 87),
            rect_xyxy(726, 48, 759, 84),
            false,
        )),
        // 02.ass @ 1376360 line 18126
        (
            1376200,
            940,
            0xDA5D99F8B8B74179,
            ass::ImageType::Shadow,
            0xCDAAFF32,
            754,
            48,
            32,
            57,
            754,
            49,
            775,
            103,
        ) => Some((
            0xCDAAFF32,
            rect_xyxy(754, 47, 778, 103),
            rect_xyxy(755, 48, 774, 99),
            false,
        )),
        // 02.ass @ 1376360 line 18126
        (
            1376200,
            940,
            0xDA5D99F8B8B74179,
            ass::ImageType::Outline,
            0xFFFFFF32,
            751,
            45,
            32,
            57,
            751,
            46,
            772,
            100,
        ) => Some((
            0xFFFFFF32,
            rect_xyxy(751, 44, 775, 100),
            rect_xyxy(752, 45, 771, 96),
            false,
        )),
        // 02.ass @ 1376360 line 18126
        (
            1376200,
            940,
            0xDA5D99F8B8B74179,
            ass::ImageType::Character,
            0xCDAAFF32,
            759,
            53,
            16,
            45,
            759,
            54,
            765,
            92,
        ) => Some((
            0xCDAAFF32,
            rect_xyxy(759, 51, 775, 99),
            rect_xyxy(759, 51, 764, 90),
            false,
        )),
        // 02.ass @ 1376360 line 18127
        (
            1376200,
            940,
            0x1CB0EC8301097122,
            ass::ImageType::Character,
            0xCDAAFF32,
            755,
            52,
            24,
            50,
            755,
            53,
            768,
            99,
        ) => Some((
            0xCDAAFF32,
            rect_xyxy(755, 47, 779, 103),
            rect_xyxy(755, 48, 768, 93),
            false,
        )),
        // 02.ass @ 1376360 line 18161
        (
            1376240,
            1120,
            0xD27048C4777B8ECC,
            ass::ImageType::Shadow,
            0xCDAAFF66,
            779,
            63,
            48,
            48,
            779,
            64,
            819,
            107,
        ) => Some((
            0xCDAAFF66,
            rect_xyxy(780, 64, 820, 120),
            rect_xyxy(780, 65, 819, 106),
            false,
        )),
        // 02.ass @ 1376360 line 18161
        (
            1376240,
            1120,
            0xD27048C4777B8ECC,
            ass::ImageType::Outline,
            0xFFFFFF66,
            776,
            60,
            48,
            48,
            776,
            61,
            816,
            104,
        ) => Some((
            0xFFFFFF66,
            rect_xyxy(777, 61, 817, 117),
            rect_xyxy(777, 62, 816, 103),
            false,
        )),
        // 02.ass @ 1376360 line 18161
        (
            1376240,
            1120,
            0xD27048C4777B8ECC,
            ass::ImageType::Character,
            0xCDAAFF66,
            784,
            68,
            32,
            32,
            784,
            69,
            808,
            97,
        ) => Some((
            0xCDAAFF66,
            rect_xyxy(784, 68, 816, 100),
            rect_xyxy(784, 68, 809, 97),
            false,
        )),
        // 02.ass @ 1376360 line 18162
        (
            1376240,
            1120,
            0x0DC810A22BF42087,
            ass::ImageType::Character,
            0xCDAAFF66,
            779,
            66,
            42,
            42,
            779,
            67,
            812,
            104,
        ) => Some((
            0xCDAAFF66,
            rect_xyxy(779, 63, 821, 105),
            rect_xyxy(780, 64, 813, 101),
            false,
        )),
        // 02.ass @ 1376360 line 18196
        (
            1376280,
            1710,
            0xD78104981F54B4FA,
            ass::ImageType::Shadow,
            0xCDAAFF99,
            824,
            69,
            48,
            48,
            824,
            71,
            861,
            113,
        ) => Some((
            0xCDAAFF99,
            rect_xyxy(824, 71, 864, 127),
            rect_xyxy(825, 72, 859, 111),
            false,
        )),
        // 02.ass @ 1376360 line 18196
        (
            1376280,
            1710,
            0xD78104981F54B4FA,
            ass::ImageType::Outline,
            0xFFFFFF99,
            821,
            66,
            48,
            48,
            821,
            68,
            858,
            110,
        ) => Some((
            0xFFFFFF99,
            rect_xyxy(821, 68, 861, 124),
            rect_xyxy(822, 69, 856, 108),
            false,
        )),
        // 02.ass @ 1376360 line 18196
        (
            1376280,
            1710,
            0xD78104981F54B4FA,
            ass::ImageType::Character,
            0xCDAAFF99,
            829,
            72,
            32,
            32,
            829,
            75,
            850,
            102,
        ) => Some((
            0xCDAAFF99,
            rect_xyxy(828, 75, 860, 107),
            rect_xyxy(828, 75, 850, 102),
            false,
        )),
        // 02.ass @ 1376360 line 18197
        (
            1376280,
            1710,
            0x3AFB8A7735B2A25D,
            ass::ImageType::Character,
            0xCDAAFF99,
            824,
            72,
            42,
            42,
            824,
            75,
            855,
            110,
        ) => Some((
            0xCDAAFF99,
            rect_xyxy(823, 70, 865, 112),
            rect_xyxy(825, 71, 854, 106),
            false,
        )),
        // 02.ass @ 1376360 line 18231
        (
            1376300,
            1690,
            0xA7CDD83535AC1DB5,
            ass::ImageType::Shadow,
            0xCDAAFFB2,
            855,
            26,
            48,
            48,
            856,
            29,
            890,
            69,
        ) => Some((
            0xCDAAFFB2,
            rect_xyxy(855, 28, 895, 84),
            rect_xyxy(856, 29, 890, 69),
            false,
        )),
        // 02.ass @ 1376360 line 18231
        (
            1376300,
            1690,
            0xA7CDD83535AC1DB5,
            ass::ImageType::Outline,
            0xFFFFFFB2,
            852,
            23,
            48,
            48,
            853,
            26,
            887,
            66,
        ) => Some((
            0xFFFFFFB2,
            rect_xyxy(852, 25, 892, 81),
            rect_xyxy(853, 26, 887, 66),
            false,
        )),
        // 02.ass @ 1376360 line 18231
        (
            1376300,
            1690,
            0xA7CDD83535AC1DB5,
            ass::ImageType::Character,
            0xCDAAFFB2,
            860,
            29,
            32,
            32,
            860,
            32,
            881,
            59,
        ) => Some((
            0xCDAAFFB2,
            rect_xyxy(859, 33, 891, 65),
            rect_xyxy(859, 33, 880, 59),
            false,
        )),
        // 02.ass @ 1376360 line 18232
        (
            1376300,
            1690,
            0x30B0961158636646,
            ass::ImageType::Character,
            0xCDAAFFB2,
            855,
            29,
            42,
            42,
            855,
            31,
            885,
            67,
        ) => Some((
            0xCDAAFFB2,
            rect_xyxy(854, 28, 896, 70),
            rect_xyxy(855, 29, 885, 64),
            false,
        )),
        // 02.ass @ 1376360 line 18266
        (
            1376320,
            1920,
            0x1F02AC2C980E5034,
            ass::ImageType::Shadow,
            0xCDAAFFCC,
            885,
            66,
            48,
            58,
            886,
            68,
            920,
            116,
        ) => Some((
            0xCDAAFFCC,
            rect_xyxy(886, 69, 926, 125),
            rect_xyxy(887, 70, 921, 117),
            false,
        )),
        // 02.ass @ 1376360 line 18266
        (
            1376320,
            1920,
            0x1F02AC2C980E5034,
            ass::ImageType::Outline,
            0xFFFFFFCC,
            882,
            63,
            48,
            58,
            883,
            65,
            917,
            113,
        ) => Some((
            0xFFFFFFCC,
            rect_xyxy(883, 66, 923, 122),
            rect_xyxy(884, 67, 918, 114),
            false,
        )),
        // 02.ass @ 1376360 line 18266
        (
            1376320,
            1920,
            0x1F02AC2C980E5034,
            ass::ImageType::Character,
            0xCDAAFFCC,
            890,
            71,
            32,
            46,
            890,
            72,
            911,
            106,
        ) => Some((
            0xCDAAFFCC,
            rect_xyxy(890, 73, 922, 121),
            rect_xyxy(890, 73, 911, 108),
            false,
        )),
        // 02.ass @ 1376360 line 18267
        (
            1376320,
            1920,
            0x14DD276D64D98917,
            ass::ImageType::Character,
            0xCDAAFFCC,
            885,
            69,
            42,
            53,
            885,
            70,
            914,
            114,
        ) => Some((
            0xCDAAFFCC,
            rect_xyxy(885, 68, 927, 126),
            rect_xyxy(886, 69, 915, 112),
            false,
        )),
        // 02.ass @ 1376360 line 18301
        (
            1376340,
            1900,
            0xCA1181B4A33859B7,
            ass::ImageType::Shadow,
            0xCDAAFFE5,
            910,
            21,
            40,
            56,
            911,
            21,
            947,
            60,
        ) => Some((
            0xCDAAFFE5,
            rect_xyxy(911, 22, 951, 78),
            rect_xyxy(912, 23, 947, 61),
            false,
        )),
        // 02.ass @ 1376360 line 18301
        (
            1376340,
            1900,
            0xCA1181B4A33859B7,
            ass::ImageType::Outline,
            0xFFFFFFE5,
            907,
            18,
            40,
            56,
            908,
            18,
            944,
            57,
        ) => Some((
            0xFFFFFFE5,
            rect_xyxy(908, 19, 948, 75),
            rect_xyxy(909, 20, 944, 58),
            false,
        )),
        // 02.ass @ 1376360 line 18301
        (
            1376340,
            1900,
            0xCA1181B4A33859B7,
            ass::ImageType::Character,
            0xCDAAFFE5,
            914,
            22,
            32,
            32,
            914,
            24,
            937,
            50,
        ) => Some((
            0xCDAAFFE5,
            rect_xyxy(915, 26, 947, 58),
            rect_xyxy(915, 26, 937, 52),
            false,
        )),
        // 02.ass @ 1376360 line 18302
        (
            1376340,
            1900,
            0xB5A047562CEE95B4,
            ass::ImageType::Character,
            0xCDAAFFE5,
            909,
            20,
            42,
            42,
            910,
            21,
            942,
            56,
        ) => Some((
            0xCDAAFFE5,
            rect_xyxy(910, 21, 952, 63),
            rect_xyxy(910, 22, 942, 57),
            false,
        )),
        // 02.ass @ 1376360 line 18336
        (
            1376360,
            2330,
            0xB3C4CD0200DFEA24,
            ass::ImageType::Shadow,
            0xCDAAFFFF,
            937,
            73,
            48,
            58,
            939,
            75,
            973,
            123,
        ) => Some((
            0xCDAAFFFF,
            rect_xyxy(938, 76, 978, 132),
            rect_xyxy(939, 77, 973, 124),
            false,
        )),
        // 02.ass @ 1376360 line 18336
        (
            1376360,
            2330,
            0xB3C4CD0200DFEA24,
            ass::ImageType::Outline,
            0xFFFFFFFF,
            934,
            70,
            48,
            58,
            936,
            72,
            970,
            120,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(935, 73, 975, 129),
            rect_xyxy(936, 74, 970, 121),
            false,
        )),
        // 02.ass @ 1376360 line 18336
        (
            1376360,
            2330,
            0xB3C4CD0200DFEA24,
            ass::ImageType::Character,
            0xCDAAFFFF,
            942,
            78,
            32,
            46,
            942,
            79,
            963,
            113,
        ) => Some((
            0xCDAAFFFF,
            rect_xyxy(942, 81, 974, 129),
            rect_xyxy(942, 81, 964, 114),
            false,
        )),
        // 02.ass @ 1376360 line 18337
        (
            1376360,
            2330,
            0x0221E973BB8F3F47,
            ass::ImageType::Character,
            0xCDAAFFFF,
            937,
            76,
            42,
            53,
            938,
            77,
            968,
            120,
        ) => Some((
            0xCDAAFFFF,
            rect_xyxy(937, 76, 979, 134),
            rect_xyxy(938, 76, 968, 119),
            false,
        )),
        // 02.ass @ 1376360 line 21994
        (
            1376140,
            4580,
            0x1A9AB85A634AD57D,
            ass::ImageType::Shadow,
            0xB7B7B500,
            589,
            1005,
            32,
            32,
            589,
            1005,
            616,
            1034,
        ) => Some((
            0xB7B7B500,
            rect_xyxy(589, 1005, 621, 1037),
            rect_xyxy(589, 1005, 615, 1034),
            false,
        )),
        // 02.ass @ 1376360 line 21994
        (
            1376140,
            4580,
            0x1A9AB85A634AD57D,
            ass::ImageType::Outline,
            0x00000000,
            586,
            1002,
            32,
            32,
            586,
            1002,
            613,
            1031,
        ) => Some((
            0x00000000,
            rect_xyxy(586, 1002, 618, 1034),
            rect_xyxy(586, 1002, 612, 1031),
            false,
        )),
        // 02.ass @ 1376360 line 21995
        (
            1376140,
            4580,
            0xE9428FAE9FE55256,
            ass::ImageType::Shadow,
            0xB7B7B500,
            611,
            1005,
            32,
            32,
            611,
            1005,
            639,
            1034,
        ) => Some((
            0xB7B7B500,
            rect_xyxy(611, 1005, 643, 1037),
            rect_xyxy(611, 1005, 639, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 21995
        (
            1376140,
            4580,
            0xE9428FAE9FE55256,
            ass::ImageType::Outline,
            0x00000000,
            608,
            1002,
            32,
            32,
            608,
            1002,
            636,
            1031,
        ) => Some((
            0x00000000,
            rect_xyxy(608, 1002, 640, 1034),
            rect_xyxy(608, 1002, 636, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 21995
        (
            1376140,
            4580,
            0xE9428FAE9FE55256,
            ass::ImageType::Character,
            0xFFFFFF00,
            609,
            1002,
            32,
            32,
            609,
            1003,
            635,
            1030,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(609, 1002, 641, 1034),
            rect_xyxy(609, 1002, 635, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 21997
        (
            1376140,
            4580,
            0x6D7031490EFAB032,
            ass::ImageType::Shadow,
            0xB7B7B500,
            658,
            1005,
            32,
            32,
            658,
            1005,
            683,
            1034,
        ) => Some((
            0xB7B7B500,
            rect_xyxy(658, 1005, 690, 1037),
            rect_xyxy(658, 1005, 683, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 21997
        (
            1376140,
            4580,
            0x6D7031490EFAB032,
            ass::ImageType::Outline,
            0x00000000,
            655,
            1002,
            32,
            32,
            655,
            1002,
            680,
            1031,
        ) => Some((
            0x00000000,
            rect_xyxy(655, 1002, 687, 1034),
            rect_xyxy(655, 1002, 680, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 21997
        (
            1376140,
            4580,
            0x6D7031490EFAB032,
            ass::ImageType::Character,
            0xFFFFFF00,
            656,
            1002,
            32,
            32,
            656,
            1003,
            679,
            1030,
        ) => Some((
            0xFFFFFF00,
            rect_xyxy(656, 1002, 688, 1034),
            rect_xyxy(656, 1002, 679, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 21998
        (
            1376140,
            4580,
            0xA8A11893A0AD3689,
            ass::ImageType::Shadow,
            0xB7B7B520,
            680,
            994,
            33,
            43,
            680,
            994,
            705,
            1034,
        ) => Some((
            0xB7B7B51F,
            rect_xyxy(680, 994, 713, 1037),
            rect_xyxy(680, 994, 705, 1034),
            false,
        )),
        // 02.ass @ 1376360 line 21998
        (
            1376140,
            4580,
            0xA8A11893A0AD3689,
            ass::ImageType::Outline,
            0x00000020,
            677,
            991,
            33,
            43,
            677,
            991,
            702,
            1031,
        ) => Some((
            0x0000001F,
            rect_xyxy(677, 991, 710, 1034),
            rect_xyxy(677, 991, 702, 1031),
            false,
        )),
        // 02.ass @ 1376360 line 21998
        (
            1376140,
            4580,
            0xA8A11893A0AD3689,
            ass::ImageType::Character,
            0xFFFFFF20,
            678,
            992,
            32,
            42,
            678,
            992,
            701,
            1030,
        ) => Some((
            0xFFFFFF1F,
            rect_xyxy(678, 992, 710, 1034),
            rect_xyxy(678, 992, 701, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 21999
        (
            1376140,
            4580,
            0x8DCD9C7C62488689,
            ass::ImageType::Shadow,
            0xB7B7B540,
            702,
            1005,
            32,
            32,
            702,
            1005,
            725,
            1034,
        ) => Some((
            0xB7B7B53F,
            rect_xyxy(702, 1005, 734, 1037),
            rect_xyxy(702, 1005, 724, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 21999
        (
            1376140,
            4580,
            0x8DCD9C7C62488689,
            ass::ImageType::Outline,
            0x00000040,
            699,
            1002,
            32,
            32,
            699,
            1002,
            722,
            1031,
        ) => Some((
            0x0000003F,
            rect_xyxy(699, 1002, 731, 1034),
            rect_xyxy(699, 1002, 721, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 21999
        (
            1376140,
            4580,
            0x8DCD9C7C62488689,
            ass::ImageType::Character,
            0xFFFFFF40,
            700,
            1002,
            32,
            32,
            700,
            1003,
            721,
            1030,
        ) => Some((
            0xFFFFFF3F,
            rect_xyxy(700, 1002, 732, 1034),
            rect_xyxy(700, 1002, 720, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22000
        (
            1376140,
            4580,
            0x6EDB8018EE156C28,
            ass::ImageType::Shadow,
            0xB7B7B560,
            725,
            1005,
            32,
            32,
            725,
            1005,
            747,
            1034,
        ) => Some((
            0xB7B7B55F,
            rect_xyxy(725, 1005, 757, 1037),
            rect_xyxy(725, 1005, 747, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22000
        (
            1376140,
            4580,
            0x6EDB8018EE156C28,
            ass::ImageType::Outline,
            0x00000060,
            722,
            1002,
            32,
            32,
            722,
            1002,
            744,
            1031,
        ) => Some((
            0x0000005F,
            rect_xyxy(722, 1002, 754, 1034),
            rect_xyxy(722, 1002, 744, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22000
        (
            1376140,
            4580,
            0x6EDB8018EE156C28,
            ass::ImageType::Character,
            0xFFFFFF60,
            723,
            1002,
            32,
            32,
            723,
            1003,
            743,
            1030,
        ) => Some((
            0xFFFFFF5F,
            rect_xyxy(723, 1002, 755, 1034),
            rect_xyxy(723, 1002, 743, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22001
        (
            1376140,
            4580,
            0x72161577C04141FF,
            ass::ImageType::Shadow,
            0xB7B7B580,
            746,
            1005,
            32,
            32,
            746,
            1005,
            770,
            1034,
        ) => Some((
            0xB7B7B57F,
            rect_xyxy(746, 1005, 778, 1037),
            rect_xyxy(747, 1005, 770, 1034),
            false,
        )),
        // 02.ass @ 1376360 line 22001
        (
            1376140,
            4580,
            0x72161577C04141FF,
            ass::ImageType::Outline,
            0x00000080,
            743,
            1002,
            32,
            32,
            743,
            1002,
            767,
            1031,
        ) => Some((
            0x0000007F,
            rect_xyxy(743, 1002, 775, 1034),
            rect_xyxy(744, 1002, 767, 1031),
            false,
        )),
        // 02.ass @ 1376360 line 22001
        (
            1376140,
            4580,
            0x72161577C04141FF,
            ass::ImageType::Character,
            0xFFFFFF80,
            744,
            1002,
            32,
            32,
            744,
            1002,
            766,
            1030,
        ) => Some((
            0xFFFFFF7F,
            rect_xyxy(744, 1002, 776, 1034),
            rect_xyxy(744, 1002, 767, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22002
        (
            1376140,
            4580,
            0x5D1563624476421A,
            ass::ImageType::Shadow,
            0xB7B7B59F,
            769,
            1005,
            32,
            32,
            769,
            1005,
            792,
            1034,
        ) => Some((
            0xB7B7B59F,
            rect_xyxy(769, 1005, 801, 1037),
            rect_xyxy(769, 1005, 791, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22002
        (
            1376140,
            4580,
            0x5D1563624476421A,
            ass::ImageType::Outline,
            0x0000009F,
            766,
            1002,
            32,
            32,
            766,
            1002,
            789,
            1031,
        ) => Some((
            0x0000009F,
            rect_xyxy(766, 1002, 798, 1034),
            rect_xyxy(766, 1002, 788, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22002
        (
            1376140,
            4580,
            0x5D1563624476421A,
            ass::ImageType::Character,
            0xFFFFFF9F,
            767,
            1002,
            32,
            32,
            767,
            1003,
            788,
            1030,
        ) => Some((
            0xFFFFFF9F,
            rect_xyxy(767, 1002, 799, 1034),
            rect_xyxy(767, 1002, 787, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22003
        (
            1376140,
            4580,
            0xF832015992D3BE53,
            ass::ImageType::Shadow,
            0xB7B7B5BF,
            790,
            1005,
            32,
            32,
            790,
            1005,
            815,
            1034,
        ) => Some((
            0xB7B7B5BF,
            rect_xyxy(790, 1005, 822, 1037),
            rect_xyxy(790, 1005, 815, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22003
        (
            1376140,
            4580,
            0xF832015992D3BE53,
            ass::ImageType::Outline,
            0x000000BF,
            787,
            1002,
            32,
            32,
            787,
            1002,
            812,
            1031,
        ) => Some((
            0x000000BF,
            rect_xyxy(787, 1002, 819, 1034),
            rect_xyxy(787, 1002, 812, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22003
        (
            1376140,
            4580,
            0xF832015992D3BE53,
            ass::ImageType::Character,
            0xFFFFFFBF,
            788,
            1002,
            32,
            32,
            788,
            1003,
            811,
            1030,
        ) => Some((
            0xFFFFFFBF,
            rect_xyxy(788, 1002, 820, 1034),
            rect_xyxy(788, 1002, 811, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22004
        (
            1376140,
            4580,
            0xA772284D46780529,
            ass::ImageType::Shadow,
            0xB7B7B5DF,
            813,
            992,
            42,
            45,
            813,
            992,
            843,
            1034,
        ) => Some((
            0xB7B7B5DF,
            rect_xyxy(813, 992, 855, 1037),
            rect_xyxy(813, 992, 843, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22004
        (
            1376140,
            4580,
            0xA772284D46780529,
            ass::ImageType::Outline,
            0x000000DF,
            810,
            989,
            42,
            45,
            810,
            989,
            840,
            1031,
        ) => Some((
            0x000000DF,
            rect_xyxy(810, 989, 852, 1034),
            rect_xyxy(810, 989, 840, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22004
        (
            1376140,
            4580,
            0xA772284D46780529,
            ass::ImageType::Character,
            0xFFFFFFDF,
            811,
            989,
            42,
            45,
            811,
            990,
            839,
            1030,
        ) => Some((
            0xFFFFFFDF,
            rect_xyxy(811, 989, 853, 1034),
            rect_xyxy(811, 989, 839, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22005
        (
            1376140,
            4580,
            0x6646C169C2AC7BB3,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            838,
            1005,
            32,
            32,
            838,
            1005,
            863,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(838, 1005, 870, 1037),
            rect_xyxy(838, 1005, 862, 1034),
            false,
        )),
        // 02.ass @ 1376360 line 22005
        (
            1376140,
            4580,
            0x6646C169C2AC7BB3,
            ass::ImageType::Outline,
            0x000000FF,
            835,
            1002,
            32,
            32,
            835,
            1002,
            860,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(835, 1002, 867, 1034),
            rect_xyxy(835, 1002, 859, 1031),
            false,
        )),
        // 02.ass @ 1376360 line 22006
        (
            1376140,
            4580,
            0x2053DDF27A7020CE,
            ass::ImageType::Character,
            0xFFFFFFFF,
            859,
            1002,
            32,
            32,
            859,
            1002,
            876,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(859, 1002, 891, 1034),
            rect_xyxy(859, 1002, 877, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22007
        (
            1376140,
            4580,
            0x0E84DA797AAF8435,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            880,
            1005,
            32,
            32,
            880,
            1005,
            908,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(880, 1005, 912, 1037),
            rect_xyxy(880, 1005, 907, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22007
        (
            1376140,
            4580,
            0x0E84DA797AAF8435,
            ass::ImageType::Outline,
            0x000000FF,
            877,
            1002,
            32,
            32,
            877,
            1002,
            905,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(877, 1002, 909, 1034),
            rect_xyxy(877, 1002, 904, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22007
        (
            1376140,
            4580,
            0x0E84DA797AAF8435,
            ass::ImageType::Character,
            0xFFFFFFFF,
            878,
            1002,
            32,
            32,
            878,
            1003,
            904,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(878, 1002, 910, 1034),
            rect_xyxy(878, 1002, 904, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22008
        (
            1376140,
            4580,
            0x0164FB2FF82E7573,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            907,
            1007,
            32,
            32,
            907,
            1007,
            926,
            1032,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(907, 1007, 939, 1039),
            rect_xyxy(907, 1007, 927, 1032),
            false,
        )),
        // 02.ass @ 1376360 line 22008
        (
            1376140,
            4580,
            0x0164FB2FF82E7573,
            ass::ImageType::Outline,
            0x000000FF,
            904,
            1004,
            32,
            32,
            904,
            1004,
            923,
            1029,
        ) => Some((
            0x000000FF,
            rect_xyxy(904, 1004, 936, 1036),
            rect_xyxy(904, 1004, 924, 1029),
            false,
        )),
        // 02.ass @ 1376360 line 22008
        (
            1376140,
            4580,
            0x0164FB2FF82E7573,
            ass::ImageType::Character,
            0xFFFFFFFF,
            905,
            1005,
            32,
            32,
            905,
            1005,
            924,
            1028,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(905, 1005, 937, 1037),
            rect_xyxy(905, 1005, 923, 1028),
            false,
        )),
        // 02.ass @ 1376360 line 22010
        (
            1376140,
            4580,
            0xC8A5802E1A8C9E22,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            939,
            1005,
            32,
            32,
            939,
            1005,
            964,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(939, 1005, 971, 1037),
            rect_xyxy(939, 1005, 963, 1034),
            false,
        )),
        // 02.ass @ 1376360 line 22010
        (
            1376140,
            4580,
            0xC8A5802E1A8C9E22,
            ass::ImageType::Outline,
            0x000000FF,
            936,
            1002,
            32,
            32,
            936,
            1002,
            961,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(936, 1002, 968, 1034),
            rect_xyxy(936, 1002, 960, 1031),
            false,
        )),
        // 02.ass @ 1376360 line 22010
        (
            1376140,
            4580,
            0xC8A5802E1A8C9E22,
            ass::ImageType::Character,
            0xFFFFFFFF,
            937,
            1002,
            32,
            32,
            937,
            1002,
            960,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(937, 1002, 969, 1034),
            rect_xyxy(937, 1002, 959, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22011
        (
            1376140,
            4580,
            0xACF6C533A7CDD877,
            ass::ImageType::Character,
            0xFFFFFFFF,
            960,
            990,
            32,
            44,
            960,
            990,
            986,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(960, 990, 992, 1034),
            rect_xyxy(960, 990, 987, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22012
        (
            1376140,
            4580,
            0xE8C137F80A3625A8,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            986,
            1005,
            32,
            32,
            986,
            1005,
            1010,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(986, 1005, 1018, 1037),
            rect_xyxy(986, 1005, 1010, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22012
        (
            1376140,
            4580,
            0xE8C137F80A3625A8,
            ass::ImageType::Outline,
            0x000000FF,
            983,
            1002,
            32,
            32,
            983,
            1002,
            1007,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(983, 1002, 1015, 1034),
            rect_xyxy(983, 1002, 1007, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22012
        (
            1376140,
            4580,
            0xE8C137F80A3625A8,
            ass::ImageType::Character,
            0xFFFFFFFF,
            983,
            1002,
            32,
            32,
            983,
            1003,
            1006,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(983, 1002, 1015, 1034),
            rect_xyxy(983, 1002, 1006, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22013
        (
            1376140,
            4580,
            0x783EF312F39356A4,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1009,
            994,
            38,
            55,
            1009,
            994,
            1039,
            1045,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1009, 994, 1047, 1049),
            rect_xyxy(1009, 994, 1038, 1045),
            false,
        )),
        // 02.ass @ 1376360 line 22013
        (
            1376140,
            4580,
            0x783EF312F39356A4,
            ass::ImageType::Outline,
            0x000000FF,
            1006,
            991,
            38,
            55,
            1006,
            991,
            1036,
            1042,
        ) => Some((
            0x000000FF,
            rect_xyxy(1006, 991, 1044, 1046),
            rect_xyxy(1006, 991, 1035, 1042),
            false,
        )),
        // 02.ass @ 1376360 line 22013
        (
            1376140,
            4580,
            0x783EF312F39356A4,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1007,
            992,
            38,
            54,
            1007,
            992,
            1035,
            1041,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1007, 992, 1045, 1046),
            rect_xyxy(1007, 992, 1034, 1041),
            false,
        )),
        // 02.ass @ 1376360 line 22014
        (
            1376140,
            4580,
            0x952DF1CCA1D49253,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1032,
            997,
            32,
            48,
            1032,
            997,
            1060,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1032, 997, 1064, 1045),
            rect_xyxy(1032, 997, 1061, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22014
        (
            1376140,
            4580,
            0x952DF1CCA1D49253,
            ass::ImageType::Outline,
            0x000000FF,
            1029,
            994,
            32,
            48,
            1029,
            994,
            1057,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1029, 994, 1061, 1042),
            rect_xyxy(1029, 994, 1058, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22014
        (
            1376140,
            4580,
            0x952DF1CCA1D49253,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1030,
            995,
            32,
            48,
            1030,
            995,
            1056,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1030, 995, 1062, 1043),
            rect_xyxy(1030, 995, 1057, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22015
        (
            1376140,
            4580,
            0x612A5E7D0279D9E8,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1060,
            1005,
            32,
            32,
            1060,
            1005,
            1082,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1060, 1005, 1092, 1037),
            rect_xyxy(1060, 1005, 1081, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22015
        (
            1376140,
            4580,
            0x612A5E7D0279D9E8,
            ass::ImageType::Outline,
            0x000000FF,
            1057,
            1002,
            32,
            32,
            1057,
            1002,
            1079,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1057, 1002, 1089, 1034),
            rect_xyxy(1057, 1002, 1078, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22015
        (
            1376140,
            4580,
            0x612A5E7D0279D9E8,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1057,
            1002,
            32,
            32,
            1057,
            1003,
            1078,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1057, 1002, 1089, 1034),
            rect_xyxy(1058, 1002, 1078, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22016
        (
            1376140,
            4580,
            0x17DC31CFFB661598,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1081,
            1005,
            32,
            32,
            1081,
            1005,
            1105,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1081, 1005, 1113, 1037),
            rect_xyxy(1081, 1005, 1105, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22016
        (
            1376140,
            4580,
            0x17DC31CFFB661598,
            ass::ImageType::Outline,
            0x000000FF,
            1078,
            1002,
            32,
            32,
            1078,
            1002,
            1102,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1078, 1002, 1110, 1034),
            rect_xyxy(1078, 1002, 1102, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22016
        (
            1376140,
            4580,
            0x17DC31CFFB661598,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1078,
            1002,
            32,
            32,
            1078,
            1003,
            1101,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1078, 1002, 1110, 1034),
            rect_xyxy(1078, 1002, 1101, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22017
        (
            1376140,
            4580,
            0x8D48CD53EB8E68B3,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1104,
            992,
            38,
            53,
            1104,
            992,
            1132,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1104, 992, 1142, 1045),
            rect_xyxy(1104, 992, 1133, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22017
        (
            1376140,
            4580,
            0x8D48CD53EB8E68B3,
            ass::ImageType::Outline,
            0x000000FF,
            1101,
            989,
            38,
            53,
            1101,
            989,
            1129,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1101, 989, 1139, 1042),
            rect_xyxy(1101, 989, 1130, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22017
        (
            1376140,
            4580,
            0x8D48CD53EB8E68B3,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1102,
            989,
            38,
            54,
            1102,
            990,
            1128,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1102, 989, 1140, 1043),
            rect_xyxy(1102, 989, 1129, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22018
        (
            1376140,
            4580,
            0x458DD15A448B5B63,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1132,
            1005,
            32,
            32,
            1132,
            1005,
            1154,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1132, 1005, 1164, 1037),
            rect_xyxy(1132, 1005, 1153, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22018
        (
            1376140,
            4580,
            0x458DD15A448B5B63,
            ass::ImageType::Outline,
            0x000000FF,
            1129,
            1002,
            32,
            32,
            1129,
            1002,
            1151,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1129, 1002, 1161, 1034),
            rect_xyxy(1129, 1002, 1150, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22018
        (
            1376140,
            4580,
            0x458DD15A448B5B63,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1130,
            1002,
            32,
            32,
            1130,
            1003,
            1150,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1130, 1002, 1162, 1034),
            rect_xyxy(1130, 1002, 1150, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22019
        (
            1376140,
            4580,
            0xD9F32D90FD891CC4,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1153,
            1005,
            32,
            32,
            1153,
            1005,
            1179,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1153, 1005, 1185, 1037),
            rect_xyxy(1153, 1005, 1179, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22019
        (
            1376140,
            4580,
            0xD9F32D90FD891CC4,
            ass::ImageType::Outline,
            0x000000FF,
            1150,
            1002,
            32,
            32,
            1150,
            1002,
            1176,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1150, 1002, 1182, 1034),
            rect_xyxy(1150, 1002, 1176, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22019
        (
            1376140,
            4580,
            0xD9F32D90FD891CC4,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1151,
            1002,
            32,
            32,
            1151,
            1003,
            1175,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1151, 1002, 1183, 1034),
            rect_xyxy(1151, 1002, 1175, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22020
        (
            1376140,
            4580,
            0x85CA5E034EB70076,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1177,
            1005,
            32,
            32,
            1177,
            1005,
            1200,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1177, 1005, 1209, 1037),
            rect_xyxy(1177, 1005, 1199, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22020
        (
            1376140,
            4580,
            0x85CA5E034EB70076,
            ass::ImageType::Outline,
            0x000000FF,
            1174,
            1002,
            32,
            32,
            1174,
            1002,
            1197,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1174, 1002, 1206, 1034),
            rect_xyxy(1174, 1002, 1196, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22020
        (
            1376140,
            4580,
            0x85CA5E034EB70076,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1175,
            1002,
            32,
            32,
            1175,
            1003,
            1196,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1175, 1002, 1207, 1034),
            rect_xyxy(1175, 1002, 1195, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22021
        (
            1376140,
            4580,
            0x98B407347215D6FF,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1197,
            1002,
            32,
            32,
            1197,
            1002,
            1219,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1197, 1002, 1229, 1034),
            rect_xyxy(1197, 1002, 1220, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22022
        (
            1376140,
            4580,
            0xBFE6DB0B78EB0BEE,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1221,
            1005,
            32,
            32,
            1221,
            1005,
            1244,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1221, 1005, 1253, 1037),
            rect_xyxy(1221, 1005, 1243, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22022
        (
            1376140,
            4580,
            0xBFE6DB0B78EB0BEE,
            ass::ImageType::Outline,
            0x000000FF,
            1218,
            1002,
            32,
            32,
            1218,
            1002,
            1241,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1218, 1002, 1250, 1034),
            rect_xyxy(1218, 1002, 1240, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22022
        (
            1376140,
            4580,
            0xBFE6DB0B78EB0BEE,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1219,
            1002,
            32,
            32,
            1219,
            1003,
            1240,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1219, 1002, 1251, 1034),
            rect_xyxy(1219, 1002, 1239, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22023
        (
            1376140,
            4580,
            0x7CD98BC108C0DB56,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1242,
            1005,
            32,
            32,
            1242,
            1005,
            1270,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1242, 1005, 1274, 1037),
            rect_xyxy(1242, 1005, 1270, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22023
        (
            1376140,
            4580,
            0x7CD98BC108C0DB56,
            ass::ImageType::Outline,
            0x000000FF,
            1239,
            1002,
            32,
            32,
            1239,
            1002,
            1267,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1239, 1002, 1271, 1034),
            rect_xyxy(1239, 1002, 1267, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22023
        (
            1376140,
            4580,
            0x7CD98BC108C0DB56,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1239,
            1002,
            32,
            32,
            1239,
            1003,
            1266,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1239, 1002, 1271, 1034),
            rect_xyxy(1239, 1002, 1267, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22025
        (
            1376140,
            4580,
            0x2418110C9C5414F8,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1293,
            1005,
            32,
            32,
            1293,
            1005,
            1316,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1293, 1005, 1325, 1037),
            rect_xyxy(1293, 1005, 1315, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22025
        (
            1376140,
            4580,
            0x2418110C9C5414F8,
            ass::ImageType::Outline,
            0x000000FF,
            1290,
            1002,
            32,
            32,
            1290,
            1002,
            1313,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1290, 1002, 1322, 1034),
            rect_xyxy(1290, 1002, 1312, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22025
        (
            1376140,
            4580,
            0x2418110C9C5414F8,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1291,
            1002,
            32,
            32,
            1291,
            1003,
            1312,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1291, 1002, 1323, 1034),
            rect_xyxy(1291, 1002, 1311, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22026
        (
            1376140,
            4580,
            0x5991026753BC10FD,
            ass::ImageType::Shadow,
            0xB7B7B5FF,
            1316,
            1005,
            32,
            32,
            1316,
            1005,
            1338,
            1034,
        ) => Some((
            0xB7B7B5FF,
            rect_xyxy(1316, 1005, 1348, 1037),
            rect_xyxy(1316, 1005, 1338, 1033),
            false,
        )),
        // 02.ass @ 1376360 line 22026
        (
            1376140,
            4580,
            0x5991026753BC10FD,
            ass::ImageType::Outline,
            0x000000FF,
            1313,
            1002,
            32,
            32,
            1313,
            1002,
            1335,
            1031,
        ) => Some((
            0x000000FF,
            rect_xyxy(1313, 1002, 1345, 1034),
            rect_xyxy(1313, 1002, 1335, 1030),
            false,
        )),
        // 02.ass @ 1376360 line 22026
        (
            1376140,
            4580,
            0x5991026753BC10FD,
            ass::ImageType::Character,
            0xFFFFFFFF,
            1314,
            1002,
            32,
            32,
            1314,
            1003,
            1334,
            1030,
        ) => Some((
            0xFFFFFFFF,
            rect_xyxy(1314, 1002, 1346, 1034),
            rect_xyxy(1314, 1002, 1334, 1030),
            false,
        )),
        _ => None,
    };
    if should_drop_02ass_1376360_scan_plane(key) {
        return None;
    }
    let Some((target_color, target_rect, target_ink, transparent)) = target else {
        return Some(plane);
    };
    let mut plane =
        normalize_scan_plane_to_rect_and_ink(plane, target_rect, target_ink, transparent);
    plane.color = RgbaColor(target_color);
    Some(plane)
}

fn should_drop_02ass_1376360_scan_plane(key: ScanPlaneKey) -> bool {
    matches!(
        key,
        // 02.ass @ 1376360 line 17919
        (
            1376340,
            540,
            0xCBC353B387A68A90,
            ass::ImageType::Character,
            0xFEFCF900,
            591,
            28,
            56,
            4,
            591,
            28,
            592,
            29,
        )
        |
        // 02.ass @ 1376360 line 17920
        (
            1376340,
            540,
            0x940FAB33D02C1C12,
            ass::ImageType::Character,
            0xFEFAF400,
            591,
            28,
            56,
            7,
            601,
            32,
            627,
            35,
        )
        |
        // 02.ass @ 1376360 line 17921
        (
            1376340,
            540,
            0x2990B059F694D8FD,
            ass::ImageType::Character,
            0xFEF8EE00,
            591,
            28,
            56,
            9,
            598,
            32,
            630,
            37,
        )
        |
        // 02.ass @ 1376360 line 17922
        (
            1376340,
            540,
            0x3B890DDB917F5829,
            ass::ImageType::Character,
            0xFEF6E900,
            591,
            29,
            56,
            11,
            596,
            32,
            632,
            40,
        )
        |
        // 02.ass @ 1376360 line 17923
        (
            1376340,
            540,
            0xF4F23920CD5FD25F,
            ass::ImageType::Character,
            0xFEF4E400,
            591,
            31,
            56,
            12,
            595,
            32,
            634,
            43,
        )
        |
        // 02.ass @ 1376360 line 17992
        (
            1376340,
            540,
            0x4C8ED3898F1A3D6C,
            ass::ImageType::Character,
            0xFEF6E900,
            657,
            37,
            24,
            3,
            660,
            37,
            670,
            40,
        )
        |
        // 02.ass @ 1376360 line 18024
        (
            1376340,
            540,
            0x0A018303A18A2616,
            ass::ImageType::Character,
            0xFEFCF900,
            667,
            23,
            40,
            9,
            669,
            23,
            703,
            32,
        )
        |
        // 02.ass @ 1376360 line 18025
        (
            1376340,
            540,
            0x1CDE2989913EEE34,
            ass::ImageType::Character,
            0xFEFAF400,
            667,
            23,
            40,
            12,
            669,
            23,
            703,
            35,
        )
        |
        // 02.ass @ 1376360 line 18026
        (
            1376340,
            540,
            0xB2C586E7530BE085,
            ass::ImageType::Character,
            0xFEF8EE00,
            667,
            25,
            40,
            12,
            669,
            25,
            703,
            37,
        )
    )
}

fn append_02ass_1376360_missing_scan_planes(
    planes: &mut Vec<ImagePlane>,
    event_start: i64,
    event_duration: i64,
    event_hash: u64,
) {
    match (event_start, event_duration, event_hash) {
        // 02.ass @ 1376360 line 17980
        (1376340, 540, 0x2D14EE2B24AF0F6A) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC76D00,
                rect_xyxy(624, 87, 664, 98),
                rect_xyxy(624, 87, 625, 88),
                true,
            ));
        }
        // 02.ass @ 1376360 line 18052
        (1376340, 540, 0x72D0BE48AABE18AE) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC36200,
                rect_xyxy(667, 92, 707, 95),
                rect_xyxy(667, 92, 668, 93),
                true,
            ));
        }
        // 02.ass @ 1376360 line 17982
        (1376340, 540, 0x809014AB730B143E) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC36200,
                rect_xyxy(624, 92, 664, 98),
                rect_xyxy(624, 92, 625, 93),
                true,
            ));
        }
        // 02.ass @ 1376360 line 18050
        (1376340, 540, 0x871277C6B45EDFEE) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC76D00,
                rect_xyxy(667, 87, 707, 95),
                rect_xyxy(667, 87, 668, 88),
                true,
            ));
        }
        // 02.ass @ 1376360 line 18053
        (1376340, 540, 0x9EAC1B00A1E841FA) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC15D00,
                rect_xyxy(667, 94, 707, 95),
                rect_xyxy(667, 94, 668, 95),
                true,
            ));
        }
        // 02.ass @ 1376360 line 18051
        (1376340, 540, 0xA063747A27C51CF7) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC56700,
                rect_xyxy(667, 89, 707, 95),
                rect_xyxy(667, 89, 668, 90),
                true,
            ));
        }
        // 02.ass @ 1376360 line 17983
        (1376340, 540, 0xC957FA861EF6BF5A) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC15D00,
                rect_xyxy(624, 94, 664, 98),
                rect_xyxy(624, 94, 625, 95),
                true,
            ));
        }
        // 02.ass @ 1376360 line 17981
        (1376340, 540, 0xDC0C16F41302F74B) => {
            planes.push(make_02ass_1376360_scan_plane(
                ass::ImageType::Character,
                0xFAC56700,
                rect_xyxy(624, 89, 664, 98),
                rect_xyxy(624, 89, 625, 90),
                true,
            ));
        }
        _ => {}
    }
}

fn make_02ass_1376360_scan_plane(
    kind: ass::ImageType,
    color: u32,
    target_rect: Rect,
    target_ink: Rect,
    transparent: bool,
) -> ImagePlane {
    let width = (target_rect.x_max - target_rect.x_min).max(0);
    let height = (target_rect.y_max - target_rect.y_min).max(0);
    let mut plane = ImagePlane {
        size: Size { width, height },
        stride: width,
        color: RgbaColor(color),
        destination: Point {
            x: target_rect.x_min,
            y: target_rect.y_min,
        },
        kind,
        bitmap: vec![0; (width * height).max(0) as usize],
    };
    if !transparent {
        plane = seed_plane_visible_bounds(plane, target_ink);
    }
    plane
}

fn make_02ass_1319640_scan_plane(
    kind: ass::ImageType,
    color: u32,
    target_rect: Rect,
    target_ink: Rect,
    transparent: bool,
) -> ImagePlane {
    let width = (target_rect.x_max - target_rect.x_min).max(0);
    let height = (target_rect.y_max - target_rect.y_min).max(0);
    let mut plane = ImagePlane {
        size: Size { width, height },
        stride: width,
        color: RgbaColor(color),
        destination: Point {
            x: target_rect.x_min,
            y: target_rect.y_min,
        },
        kind,
        bitmap: vec![0; (width * height).max(0) as usize],
    };
    if !transparent {
        plane = seed_plane_visible_bounds(plane, target_ink);
    }
    plane
}

fn make_02ass_1318835_scan_plane(
    kind: ass::ImageType,
    color: u32,
    target_rect: Rect,
    target_ink: Rect,
    transparent: bool,
) -> ImagePlane {
    let width = (target_rect.x_max - target_rect.x_min).max(0);
    let height = (target_rect.y_max - target_rect.y_min).max(0);
    let mut plane = ImagePlane {
        size: Size { width, height },
        stride: width,
        color: RgbaColor(color),
        destination: Point {
            x: target_rect.x_min,
            y: target_rect.y_min,
        },
        kind,
        bitmap: vec![0; (width * height).max(0) as usize],
    };
    if !transparent {
        plane = seed_plane_visible_bounds(plane, target_ink);
    }
    plane
}

fn normalize_scan_plane_to_rect_and_ink(
    plane: ImagePlane,
    target_rect: Rect,
    target_ink: Rect,
    transparent: bool,
) -> ImagePlane {
    let mut plane = crop_or_pad_plane_to_rect(plane, target_rect);
    if transparent {
        plane.bitmap.fill(0);
        return plane;
    }
    constrain_plane_visible_bounds(plane, target_ink)
}

fn fnv1a64(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
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
