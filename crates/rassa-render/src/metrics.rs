use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FontVerticalMetrics {
    pub(crate) ascender_26_6: i32,
    pub(crate) descender_26_6: i32,
    /// Underline (top offset below baseline, thickness), both 26.6, from the
    /// post table the way libass ass_get_glyph_outline derives it:
    /// top = -underlinePosition - thickness/2.
    pub(crate) underline_26_6: Option<(i32, i32)>,
    /// Strikeout (top offset relative baseline, negative above, thickness)
    /// from OS/2 yStrikeoutPosition/ySize.
    pub(crate) strikeout_26_6: Option<(i32, i32)>,
    /// OS/2 sTypoDescender scaled (negative below baseline); libass uses it
    /// to offset rotated @font glyphs (DECO_ROTATE), 0 without an OS/2 table.
    pub(crate) typo_descender_26_6: i32,
}

/// Per-line ascent/descent in device pixels, mirroring libass
/// `measure_text` (ass_render.c): line asc/desc is the max over the line's
/// glyphs of the font's scaled win-metrics (`ass_font_get_asc_desc`),
/// multiplied by \fscy; drawings contribute asc = height - pbo, desc = pbo.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LineMetrics {
    pub(crate) asc: f64,
    pub(crate) desc: f64,
}

impl LineMetrics {
    pub(crate) fn height(self) -> f64 {
        self.asc + self.desc
    }
}

pub(crate) struct LineMetricsContext<'a> {
    pub(crate) track: &'a ParsedTrack,
    pub(crate) config: &'a RendererConfig,
    pub(crate) source_event: Option<&'a ParsedEvent>,
    pub(crate) now_ms: i64,
    pub(crate) render_scale: RenderScale,
}

fn run_is_whitespace_text(run: &LayoutGlyphRun) -> bool {
    run.drawing.is_none() && run.text.chars().all(|character| character == ' ')
}

fn text_run_metrics(run: &LayoutGlyphRun, context: &LineMetricsContext<'_>) -> LineMetrics {
    let effective_style = apply_renderer_style_scale(
        resolve_run_style(run, context.source_event, context.now_ms),
        context.track,
        context.config,
        context.render_scale.uniform,
    );
    if !(effective_style.font_size.is_finite() && effective_style.font_size > 0.0) {
        return LineMetrics::default();
    }
    let font_size = effective_style.font_size.max(1.0);
    let scale_y = style_scale(effective_style.scale_y);
    let size_26_6 = (font_size * 64.0).round() as i32;
    if let Some(metrics) = font_vertical_metrics(&run.font, size_26_6) {
        return LineMetrics {
            asc: f64::from(metrics.ascender_26_6) / 64.0 * scale_y,
            desc: f64::from(metrics.descender_26_6) / 64.0 * scale_y,
        };
    }
    // No face metrics available (unresolved font path / unparsable font
    // data).  FT_SIZE_REQUEST_TYPE_REAL_DIM guarantees asc + desc ==
    // font_size, so recover the split from rendered ink and keep the sum.
    let mut ink_ascender = 0_i32;
    if !run.glyphs.is_empty() {
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6,
            hinting: context.config.hinting,
        });
        let glyph_infos =
            scale_glyph_infos(&run.glyphs, context.render_scale.x, context.render_scale.y);
        if let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos) {
            ink_ascender = raster_glyphs
                .iter()
                .map(|glyph| glyph.top)
                .max()
                .unwrap_or(0);
        }
    }
    let asc = f64::from(ink_ascender).clamp(0.0, font_size) * scale_y;
    LineMetrics {
        asc,
        desc: (font_size * scale_y - asc).max(0.0),
    }
}

fn drawing_run_metrics(run: &LayoutGlyphRun, context: &LineMetricsContext<'_>) -> LineMetrics {
    let Some(drawing) = run.drawing.as_ref() else {
        return LineMetrics::default();
    };
    let Some(bounds) = drawing.bounds() else {
        return LineMetrics::default();
    };
    let effective_style = resolve_run_style(run, context.source_event, context.now_ms);
    let scale_y = style_scale(effective_style.scale_y) * style_scale(context.render_scale.y);
    // libass (ass_render.c get_bitmap_glyph): drawing desc = pbo, asc =
    // bbox height - pbo, both scaled by scale.y which already includes the
    // 1/2^(\p - 1) drawing-scale division (rassa pre-divides the polygon
    // coordinates at parse time, so only pbo needs the division here).
    let height = f64::from((bounds.height() - 1).max(0)) * scale_y;
    let pbo = drawing_pbo_script_pixels(&effective_style, drawing) * scale_y;
    LineMetrics {
        asc: height - pbo,
        desc: pbo,
    }
}

/// \pbo in script pixels: libass keeps pbo in drawing-coordinate units, so
/// it is divided by the same 2^(\p - 1) factor as the drawing itself.
pub(crate) fn drawing_pbo_script_pixels(style: &ParsedSpanStyle, drawing: &ParsedDrawing) -> f64 {
    if !style.pbo.is_finite() {
        return 0.0;
    }
    let scale_base = 1_i32
        .checked_shl(drawing.scale.saturating_sub(1).max(0) as u32)
        .unwrap_or(1)
        .max(1);
    style.pbo / f64::from(scale_base)
}

fn run_metrics(run: &LayoutGlyphRun, context: &LineMetricsContext<'_>) -> LineMetrics {
    if run.drawing.is_some() {
        drawing_run_metrics(run, context)
    } else {
        text_run_metrics(run, context)
    }
}

pub(crate) fn line_metrics_for_line(
    line: &rassa_layout::LayoutLine,
    context: &LineMetricsContext<'_>,
) -> LineMetrics {
    // libass measure_text ignores the metrics of line-leading/trailing
    // trimmed whitespace, except when the line is empty after trimming;
    // an empty line counts at half height (scale = 0.5/64).
    let content_runs = line
        .runs
        .iter()
        .filter(|run| !run_is_whitespace_text(run))
        .collect::<Vec<_>>();
    let (runs, factor): (Vec<&LayoutGlyphRun>, f64) = if content_runs.is_empty() {
        (line.runs.iter().collect(), 0.5)
    } else {
        (content_runs, 1.0)
    };
    let mut metrics = LineMetrics::default();
    for run in runs {
        let run_metrics = run_metrics(run, context);
        metrics.asc = metrics.asc.max(run_metrics.asc * factor);
        metrics.desc = metrics.desc.max(run_metrics.desc * factor);
    }
    metrics
}

pub(crate) fn event_line_metrics(
    lines: &[rassa_layout::LayoutLine],
    context: &LineMetricsContext<'_>,
) -> Vec<LineMetrics> {
    lines
        .iter()
        .map(|line| line_metrics_for_line(line, context))
        .collect()
}

pub(crate) fn line_spacing(config: &RendererConfig) -> f64 {
    if config.line_spacing.is_finite() {
        config.line_spacing
    } else {
        0.0
    }
}

/// Total text height per libass measure_text: sum of per-line asc+desc plus
/// `line_spacing` per line break.
pub(crate) fn total_text_height(metrics: &[LineMetrics], config: &RendererConfig) -> f64 {
    let height: f64 = metrics.iter().map(|line| line.height()).sum();
    height + line_spacing(config) * metrics.len().saturating_sub(1) as f64
}

/// Line advance width in device pixels: sum of scaled glyph advances
/// (including \fsp spacing and \fscx), matching libass compute_string_bbox
/// which measures from cluster advances, never from rendered ink.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rendered_text_alignment_width(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: RenderScale,
) -> i32 {
    let mut width = 0_i32;
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
        let glyph_infos = scale_glyph_infos(&run.glyphs, render_scale.x, render_scale.y);
        let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos) else {
            width += (f64::from(run.width) * style_scale(render_scale.x)).round() as i32;
            continue;
        };
        let raster_glyphs =
            apply_vertical_font_raster_advances(raster_glyphs, &effective_style, &run.font);
        let raster_glyphs = scale_raster_glyphs(
            raster_glyphs,
            effective_style.scale_x,
            effective_style.scale_y,
        );
        let raster_glyphs = apply_text_spacing(raster_glyphs, &effective_style);
        width += (raster_glyphs
            .iter()
            .map(|glyph| glyph.advance_x_26_6)
            .sum::<i32>()
            + 32)
            >> 6;
    }
    width.max(1)
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
    rassa_raster::request_real_dim_size(&mut face, size_26_6.max(64)).ok()?;
    let metrics = face.size_metrics()?;
    let scale = |value: i32| unsafe { ffi::FT_MulFix(value.into(), metrics.y_scale) } as i32;
    let ascender = scale(face.ascender().into());
    let descender = scale((-face.descender()).into());

    // libass ass_get_glyph_outline: underline from the *raw* post table when
    // underlinePosition <= 0 and underlineThickness > 0, scaled with the exact
    // ((val * y_scale + 0x8000) >> 16) arithmetic libass uses (ass_font.c:769).
    // Note: this reads TT_Postscript (the raw post values), NOT
    // FT_FaceRec.underline_position, which FreeType has already recentered by
    // -thickness/2 at face load (sfobjs.c). Using the recentered value here and
    // then applying -size/2 again double-recenters the bar ~thickness/2 lower.
    let scale_libass =
        |value: i32| ((i64::from(value) * metrics.y_scale + 0x8000) >> 16) as i32;
    let post = unsafe {
        ffi::FT_Get_Sfnt_Table(face.raw_mut() as *mut ffi::FT_FaceRec, ffi::ft_sfnt_post)
            as *const ffi::TT_Postscript
    };
    let underline = (!post.is_null())
        .then(|| unsafe { &*post })
        .filter(|ps| ps.underlinePosition <= 0 && ps.underlineThickness > 0)
        .map(|ps| {
            let pos = scale_libass(ps.underlinePosition.into());
            let size = scale_libass(ps.underlineThickness.into());
            (-pos - size / 2, size)
        });
    let os2 = unsafe {
        ffi::FT_Get_Sfnt_Table(face.raw_mut() as *mut ffi::FT_FaceRec, ffi::ft_sfnt_os2)
            as *const ffi::TT_OS2
    };
    let strikeout = (!os2.is_null())
        .then(|| unsafe { &*os2 })
        .filter(|os2| os2.yStrikeoutPosition >= 0 && os2.yStrikeoutSize > 0)
        .map(|os2| {
            // Same exact arithmetic libass uses (ass_font.c:782).
            let pos = scale_libass(os2.yStrikeoutPosition.into());
            let size = scale_libass(os2.yStrikeoutSize.into());
            (-pos - size / 2, size)
        });
    let typo_descender = (!os2.is_null())
        .then(|| unsafe { &*os2 })
        .map(|os2| scale(os2.sTypoDescender.into()))
        .unwrap_or(0);

    Some(FontVerticalMetrics {
        ascender_26_6: ascender,
        descender_26_6: descender,
        underline_26_6: underline,
        strikeout_26_6: strikeout,
        typo_descender_26_6: typo_descender,
    })
}

#[cfg(any(target_os = "macos", target_arch = "wasm32", not(unix)))]
pub(crate) fn font_vertical_metrics(
    font: &FontMatch,
    size_26_6: i32,
) -> Option<FontVerticalMetrics> {
    let font_path = font.path.as_ref()?;
    let data = std::fs::read(font_path).ok()?;
    font_vertical_metrics_from_data(&data, font.face_index.unwrap_or(0), size_26_6)
}

/// FreeType-free `font_vertical_metrics` used on targets without the
/// FreeType rasterizer.  Replicates apply_gdi_font_metrics (win → typo →
/// bbox ascender/descender) and FT_SIZE_REQUEST_TYPE_REAL_DIM scaling with
/// FreeType's FT_DivFix/FT_MulFix rounding so it matches the FreeType path
/// bit for bit (asserted by a test on the FreeType targets).
#[cfg(any(test, target_os = "macos", target_arch = "wasm32", not(unix)))]
pub(crate) fn font_vertical_metrics_from_data(
    data: &[u8],
    face_index: u32,
    size_26_6: i32,
) -> Option<FontVerticalMetrics> {
    let face = ttf_parser::Face::parse(data, face_index).ok()?;
    let tables = face.tables();

    let hhea = tables.hhea;
    let mut ascender = i32::from(hhea.ascender);
    let mut descender = i32::from(hhea.descender);
    let mut height = ascender - descender + i32::from(hhea.line_gap);
    if let Some(os2) = tables.os2 {
        // ttf-parser reads the unsigned spec fields as signed (like libass)
        // and returns the descender already negated.
        let win_ascender = i32::from(os2.windows_ascender());
        let win_descender = i32::from(os2.windows_descender());
        if win_ascender - win_descender != 0 {
            ascender = win_ascender;
            descender = win_descender;
            height = ascender - descender;
        }
    }
    if ascender - descender == 0 || height == 0 {
        if let Some(os2) = tables.os2 {
            let typo_ascender = i32::from(os2.typographic_ascender());
            let typo_descender = i32::from(os2.typographic_descender());
            if typo_ascender - typo_descender != 0 {
                ascender = typo_ascender;
                descender = typo_descender;
                height = ascender - descender;
            }
        }
        if ascender - descender == 0 || height == 0 {
            let bbox = face.global_bounding_box();
            ascender = i32::from(bbox.y_max);
            descender = i32::from(bbox.y_min);
        }
    }

    // REAL_DIM maps ascender - descender onto the requested height:
    // y_scale = FT_DivFix(size, asc - desc), values FT_MulFix'ed by it.
    let units = i64::from(ascender - descender);
    if units <= 0 {
        return None;
    }
    let y_scale = ((i64::from(size_26_6.max(64)) << 16) + (units >> 1)) / units;
    let scale = |value: i32| {
        let product = i64::from(value) * y_scale;
        ((product + 0x8000 - i64::from(product < 0)) >> 16) as i32
    };
    // Decoration bars use libass's exact rounding (ass_font.c:769): a plain
    // arithmetic shift of (val * y_scale + 0x8000), with NO negative-bias
    // adjustment, on the *raw* post/OS-2 values.
    let scale_deco = |value: i32| ((i64::from(value) * y_scale + 0x8000) >> 16) as i32;

    let underline = tables
        .post
        .map(|post| post.underline_metrics)
        .filter(|line| line.position <= 0 && line.thickness > 0)
        .map(|line| {
            // Raw post-table position (not FreeType's recentered face value);
            // libass recenters exactly once via -pos - size/2.
            let pos = scale_deco(line.position.into());
            let size = scale_deco(line.thickness.into());
            (-pos - size / 2, size)
        });
    let strikeout = tables
        .os2
        .map(|os2| os2.strikeout_metrics())
        .filter(|line| line.position >= 0 && line.thickness > 0)
        .map(|line| {
            let pos = scale_deco(line.position.into());
            let size = scale_deco(line.thickness.into());
            (-pos - size / 2, size)
        });
    let typo_descender = tables
        .os2
        .map(|os2| scale(os2.typographic_descender().into()))
        .unwrap_or(0);

    Some(FontVerticalMetrics {
        ascender_26_6: scale(ascender),
        descender_26_6: scale(-descender),
        underline_26_6: underline,
        strikeout_26_6: strikeout,
        typo_descender_26_6: typo_descender,
    })
}

/// Combined \be/\blur strength in \blur units.  libass applies \be as N
/// passes of a [1,2,1] box blur (variance N/2) followed by the \blur
/// gaussian (variance blur^2); sequential blurs add variances.
pub(crate) fn effective_blur_strength(style: &ParsedSpanStyle) -> f64 {
    let blur = if style.blur.is_finite() && style.blur > 0.0 {
        style.blur
    } else {
        0.0
    };
    let be = if style.be.is_finite() && style.be > 0.0 {
        style.be
    } else {
        0.0
    };
    (be / 2.0 + blur * blur).sqrt()
}

pub(crate) fn renderer_blur_radius(blur: f64) -> u32 {
    if !(blur.is_finite() && blur > 0.0) {
        return 0;
    }
    (blur * 4.0).ceil().max(1.0) as u32
}

pub(crate) fn expand_rect_xy(rect: Rect, amount_x: i32, amount_y: i32) -> Rect {
    Rect {
        x_min: rect.x_min - amount_x.max(0),
        y_min: rect.y_min - amount_y.max(0),
        x_max: rect.x_max + amount_x.max(0),
        y_max: rect.y_max + amount_y.max(0),
    }
}

