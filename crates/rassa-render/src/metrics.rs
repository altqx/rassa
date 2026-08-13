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
    pub(crate) font_scale: f64,
    pub(crate) scaled_drawings: &'a [Vec<Option<Vec<Vec<Point>>>>],
}

fn run_is_whitespace_text(run: &LayoutGlyphRun) -> bool {
    run.drawing.is_none() && run.text.chars().all(|character| character == ' ')
}

fn text_run_metrics(run: &LayoutGlyphRun, context: &LineMetricsContext<'_>) -> LineMetrics {
    let effective_style = apply_renderer_style_scale(
        resolve_run_style(run, context.source_event, context.now_ms),
        context.track,
        context.config,
        context.font_scale,
        context.render_scale,
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
        let position_scale =
            shaped_position_render_scale(run, &effective_style, context.render_scale);
        let glyph_infos = scale_glyph_infos(&run.glyphs, position_scale.0, position_scale.1);
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

fn drawing_run_metrics(
    run: &LayoutGlyphRun,
    scaled_polygons: Option<&[Vec<Point>]>,
    context: &LineMetricsContext<'_>,
) -> LineMetrics {
    let Some(drawing) = run.drawing.as_ref() else {
        return LineMetrics::default();
    };
    // Use the same exact 26.6 outline and endpoint rounding as the drawing
    // raster path.  Deriving metrics from ParsedDrawing's integer compatibility
    // polygons makes the line ascender cross whole-pixel thresholds at a
    // different fscy value than the rendered height, which shows up as a
    // one-pixel vertical bounce during slow frame-baked scaling.
    let effective_style = resolve_run_style(run, context.source_event, context.now_ms);
    let Some(height) = scaled_polygons.and_then(drawing_height_exact_from_d6) else {
        return LineMetrics::default();
    };
    let scale_y = style_scale(effective_style.scale_y) * style_scale(context.render_scale.y);
    // libass (ass_render.c get_bitmap_glyph): drawing desc = pbo, asc =
    // bbox height - pbo, both scaled by scale.y which already includes the
    // 1/2^(\p - 1) drawing-scale division (rassa pre-divides the polygon
    // coordinates at parse time, so only pbo needs the division here).
    let height = height.max(0.0);
    let pbo = drawing_pbo_script_pixels(&effective_style, drawing) * scale_y;
    if libass_outline_coordinate_from_f64(height).is_none()
        || libass_outline_coordinate_from_f64(pbo).is_none()
    {
        return LineMetrics::default();
    }
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
    let scale_base = rassa_parse::libass_drawing_scale_base(drawing.scale);
    if scale_base <= 0 {
        return 0.0;
    }
    style.pbo / f64::from(scale_base)
}

fn run_metrics(
    run: &LayoutGlyphRun,
    scaled_polygons: Option<&[Vec<Point>]>,
    context: &LineMetricsContext<'_>,
) -> LineMetrics {
    if run.drawing.is_some() {
        drawing_run_metrics(run, scaled_polygons, context)
    } else {
        text_run_metrics(run, context)
    }
}

pub(crate) fn line_metrics_for_line(
    line: &rassa_layout::LayoutLine,
    line_index: usize,
    context: &LineMetricsContext<'_>,
) -> LineMetrics {
    // libass measure_text ignores the metrics of line-leading/trailing
    // trimmed whitespace, except when the line is empty after trimming;
    // an empty line counts at half height (scale = 0.5/64).
    let content_runs = line
        .runs
        .iter()
        .enumerate()
        .filter(|(_, run)| !run_is_whitespace_text(run))
        .collect::<Vec<_>>();
    let (runs, factor): (Vec<(usize, &LayoutGlyphRun)>, f64) = if content_runs.is_empty() {
        (line.runs.iter().enumerate().collect(), 0.5)
    } else {
        (content_runs, 1.0)
    };
    let mut metrics = LineMetrics::default();
    for (run_index, run) in runs {
        let scaled_polygons = context
            .scaled_drawings
            .get(line_index)
            .and_then(|line| line.get(run_index))
            .and_then(Option::as_deref);
        let run_metrics = run_metrics(run, scaled_polygons, context);
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
        .enumerate()
        .map(|(line_index, line)| line_metrics_for_line(line, line_index, context))
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
    font_scale: f64,
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
            font_scale,
            render_scale,
        );
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: (effective_style.font_size.max(1.0) * 64.0).round() as i32,
            hinting: config.hinting,
        });
        let position_scale = shaped_position_render_scale(run, &effective_style, render_scale);
        let glyph_infos = scale_glyph_infos(&run.glyphs, position_scale.0, position_scale.1);
        let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos) else {
            width += (f64::from(run.width)
                * style_scale(render_scale.y)
                * effective_pixel_aspect(track, config))
            .round() as i32;
            continue;
        };
        let raster_glyphs = apply_vertical_font_raster_advances(
            raster_glyphs,
            &glyph_infos,
            &effective_style,
            &run.font,
        );
        let raster_glyphs = scale_raster_glyphs(
            raster_glyphs,
            effective_style.scale_x * effective_pixel_aspect(track, config),
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
    // Keep a true zero advance: libass uses this width for EventImages and
    // excludes zero-area events from collision placement. A combining-only
    // event can still produce visible ink while having no horizontal advance.
    width
}

/// Unrounded device-space line advance used by positioned text rasterization.
///
/// Collision/layout compatibility still uses the integer helper above.  The
/// exact companion prevents a centred `\an5` line from changing its origin by
/// a whole pixel when a slowly animated `\fscx` crosses a bitmap-size rounding
/// threshold.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rendered_text_alignment_width_exact(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: RenderScale,
    font_scale: f64,
) -> f64 {
    let mut width = 0.0_f64;
    for run in &line.runs {
        if run.drawing.is_some() {
            width += f64::from(run.width) * style_scale(render_scale.x);
            continue;
        }
        if run.glyphs.is_empty() {
            continue;
        }
        let effective_style = apply_renderer_style_scale(
            resolve_run_style(run, source_event, now_ms),
            track,
            config,
            font_scale,
            render_scale,
        );
        let rasterizer = Rasterizer::with_options(RasterOptions {
            size_26_6: (effective_style.font_size.max(1.0) * 64.0).round() as i32,
            hinting: config.hinting,
        });
        let position_scale = shaped_position_render_scale(run, &effective_style, render_scale);
        let glyph_infos = scale_glyph_infos(&run.glyphs, position_scale.0, position_scale.1);
        let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos) else {
            width += f64::from(run.width) * font_scale * style_scale(render_scale.x);
            continue;
        };
        let raster_glyphs = apply_vertical_font_raster_advances(
            raster_glyphs,
            &glyph_infos,
            &effective_style,
            &run.font,
        );
        let scale_x = style_scale(effective_style.scale_x * effective_pixel_aspect(track, config));
        let spacing = f64::from(text_spacing_advance_26_6(&effective_style)) / 64.0;
        width += raster_glyphs
            .iter()
            .map(|glyph| f64::from(glyph.advance_x_26_6) / 64.0 * scale_x + spacing)
            .sum::<f64>();
    }
    if width.is_finite() { width } else { 0.0 }
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
    let scale_libass = |value: i32| ((i64::from(value) * metrics.y_scale + 0x8000) >> 16) as i32;
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

pub(crate) fn effective_bitmap_blur(
    style: &ParsedSpanStyle,
    track: &ParsedTrack,
    config: &RendererConfig,
    font_scale: f64,
    mapping: &EventMapping,
) -> BitmapBlur {
    let blur = if style.blur.is_finite() && style.blur > 0.0 {
        style.blur
    } else {
        0.0
    };
    let be = if style.be.is_finite() && style.be > 0.0 {
        style.be.trunc().clamp(0.0, 127.0) as u32
    } else {
        0
    };
    let (scale_x, scale_y) = renderer_blur_scales(track, config, font_scale, mapping);
    BitmapBlur::from_scaled_blur(blur * scale_x, blur * scale_y, be)
}

pub(crate) fn expand_rect_xy(rect: Rect, amount_x: i32, amount_y: i32) -> Rect {
    Rect {
        x_min: rect.x_min - amount_x.max(0),
        y_min: rect.y_min - amount_y.max(0),
        x_max: rect.x_max + amount_x.max(0),
        y_max: rect.y_max + amount_y.max(0),
    }
}
