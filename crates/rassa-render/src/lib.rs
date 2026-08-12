use std::collections::HashMap;

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
use freetype::{Library, ffi};
use rassa_core::{ImagePlane, Point, Rect, RendererConfig, RgbaColor, Size, ass};
use rassa_fonts::{FontMatch, FontProvider, FontconfigProvider};
use rassa_layout::{LayoutEngine, LayoutEvent, LayoutFeatures, LayoutGlyphRun, LayoutWrapScales};
use rassa_parse::{
    LIBASS_OUTLINE_MAX_D6, ParsedAxisTransform, ParsedColourTransform, ParsedDrawing, ParsedEvent,
    ParsedFade, ParsedFontSizeTransform, ParsedKaraokeMode, ParsedLinearTransform, ParsedMovement,
    ParsedMovementExact, ParsedRectF64, ParsedScaleTransform, ParsedSpanStyle, ParsedTrack,
    ParsedVectorClip, dialogue_has_libass_hard_override, libass_drawing_scale_base,
    libass_outline_coordinate_from_f64, libass_outline_point_is_valid,
    parse_dialogue_vector_clip_d6, parse_drawing_polygons_d6,
};
use rassa_raster::{RasterGlyph, RasterOptions, Rasterizer};
use rassa_shape::{GlyphInfo, ShapingMode};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderSelection {
    pub active_event_indices: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PreparedFrame {
    pub now_ms: i64,
    pub active_events: Vec<LayoutEvent>,
}

#[derive(Default)]
pub struct RenderEngine {
    layout: LayoutEngine,
    // libass keeps a per-event ASS_RenderPriv with the rect the event was
    // assigned by fix_collisions; while the event keeps rendering with the
    // same height it stays at that position across frames.  The cache is
    // invalidated when the renderer settings change (libass bumps render_id
    // on every ass_set_*).
    collision_cache: std::sync::Mutex<HashMap<usize, Rect>>,
    collision_render_id: std::sync::Mutex<u64>,
}

mod metrics;
pub(crate) use metrics::*;
mod helpers;
pub use helpers::*;

impl RenderEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select_active_events(&self, track: &ParsedTrack, now_ms: i64) -> RenderSelection {
        let mut active_event_indices = track
            .events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| is_event_active(event, now_ms).then_some(index))
            .collect::<Vec<_>>();
        active_event_indices.sort_by(|left, right| {
            let left_event = &track.events[*left];
            let right_event = &track.events[*right];
            left_event
                .layer
                .cmp(&right_event.layer)
                .then(left_event.read_order.cmp(&right_event.read_order))
                .then(left.cmp(right))
        });

        RenderSelection {
            active_event_indices,
        }
    }

    pub fn prepare_frame<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        provider: &P,
        now_ms: i64,
    ) -> PreparedFrame {
        self.prepare_frame_with_config(track, provider, now_ms, &default_renderer_config(track))
    }

    pub fn prepare_frame_with_config<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        provider: &P,
        now_ms: i64,
        config: &RendererConfig,
    ) -> PreparedFrame {
        let selection = self.select_active_events(track, now_ms);
        let shaping_mode = match config.shaping {
            ass::ShapingLevel::Simple => ShapingMode::Simple,
            ass::ShapingLevel::Complex => ShapingMode::Complex,
        };
        let active_events = selection
            .active_event_indices
            .into_iter()
            .filter_map(|index| {
                let event = track.events.get(index)?;
                let event_is_explicit = transition_effect_disables_collision(event)
                    || dialogue_has_libass_hard_override(&event.text);
                let wrap_scales = renderer_wrap_scales(track, config, event_is_explicit);
                self.layout
                    .layout_track_event_with_features_and_wrap_scales(
                        track,
                        index,
                        provider,
                        shaping_mode,
                        LayoutFeatures {
                            wrap_unicode: config.wrap_unicode,
                            bidi_brackets: config.bidi_brackets,
                            whole_text_layout: config.whole_text_layout,
                        },
                        wrap_scales,
                    )
                    .ok()
            })
            .collect();

        PreparedFrame {
            now_ms,
            active_events,
        }
    }

    pub fn render_frame_with_provider<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        provider: &P,
        now_ms: i64,
    ) -> Vec<ImagePlane> {
        self.render_frame_with_provider_and_config(
            track,
            provider,
            now_ms,
            &default_renderer_config(track),
        )
    }

    pub fn render_frame_with_provider_and_config<P: FontProvider>(
        &self,
        track: &ParsedTrack,
        provider: &P,
        now_ms: i64,
        config: &RendererConfig,
    ) -> Vec<ImagePlane> {
        let prepared = self.prepare_frame_with_config(track, provider, now_ms, config);
        {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            format!("{config:?}").hash(&mut hasher);
            track.play_res_x.hash(&mut hasher);
            track.play_res_y.hash(&mut hasher);
            // libass invalidates per-event collision placement whenever the
            // track's effective render data changes. Counting events alone
            // leaves stale cached rectangles after in-place event/style edits
            // through the C API, so hash every render-observable field.
            format!("{track:?}").hash(&mut hasher);
            let render_id = hasher.finish();
            let mut current = self
                .collision_render_id
                .lock()
                .expect("collision render id mutex poisoned");
            if *current != render_id {
                *current = render_id;
                self.collision_cache
                    .lock()
                    .expect("collision cache mutex poisoned")
                    .clear();
            }
        }
        let mut rendered_events = Vec::new();

        let render_scale_x = output_scale_x(track, config);
        let render_scale_y = output_scale_y(track, config);
        let render_scale_all = RenderScale {
            x: render_scale_x,
            y: render_scale_y,
        };

        for event in &prepared.active_events {
            let source_event = track.events.get(event.event_index);
            let Some(style) = track.styles.get(event.style_index) else {
                continue;
            };
            let mut shadow_planes = Vec::new();
            let mut outline_planes = Vec::new();
            let mut character_planes = Vec::new();
            let effect_disables_collision = source_event
                .map(transition_effect_disables_collision)
                .unwrap_or(false);
            // libass computes "explicit" before parsing override tags:
            // transition effects set evt_type immediately, and all other
            // hard overrides come from ass_event_has_hard_overrides' raw scan.
            let event_is_explicit = event.hard_override || effect_disables_collision;
            let event_font_scale = renderer_font_scale_for_event(config, event_is_explicit);
            let mapping = event_mapping(track, config, event_is_explicit);
            let projection_scale_y =
                renderer_projection_scale_y(track, config, event_font_scale, &mapping);
            let effective_position =
                scale_position(resolve_event_position(track, event, now_ms), &mapping);
            let metrics_context = LineMetricsContext {
                track,
                config,
                source_event,
                now_ms,
                render_scale: render_scale_all,
                font_scale: event_font_scale,
            };
            let line_metrics = event_line_metrics(&event.lines, &metrics_context);
            let vertical_layout = compute_vertical_layout(
                track,
                &line_metrics,
                event.alignment,
                event.margin_v,
                effective_position,
                config,
                &mapping,
            );
            // libass computes one untransformed string bbox before rendering
            // glyphs, then uses its alignment base point as the implicit
            // \frx/\fry/\frz origin for every glyph.  Precompute equivalent
            // advance/line-metric geometry so style and karaoke runs do not
            // each pick a different origin from their own rendered ink.
            let line_widths = event
                .lines
                .iter()
                .map(|line| {
                    rendered_text_alignment_width(
                        line,
                        source_event,
                        now_ms,
                        track,
                        config,
                        render_scale_all,
                        event_font_scale,
                    )
                })
                .collect::<Vec<_>>();
            // libass's align_lines starts max_width at zero, so pathological
            // negative line advances never make the alignment block negative.
            let block_width = line_widths.iter().copied().max().unwrap_or(0).max(0);
            let horizontal_scroll = source_event.is_some_and(|event| {
                event.effect.starts_with("Banner;") && transition_effect_disables_collision(event)
            });
            let line_layout = event
                .lines
                .iter()
                .zip(line_widths)
                .zip(vertical_layout.iter().copied())
                .zip(line_metrics.iter().copied())
                .map(|(((_, scaled_line_width), line_top), line_metric)| {
                    let origin_x = compute_horizontal_origin(
                        track,
                        event,
                        scaled_line_width,
                        block_width,
                        horizontal_scroll,
                        effective_position,
                        &mapping,
                    );
                    (line_top, line_metric, scaled_line_width, origin_x)
                })
                .collect::<Vec<_>>();
            let event_layout_bounds = line_layout.first().map(|first| {
                let mut x_min = first.3;
                let mut x_max = first.3.saturating_add(first.2);
                for (_, _, width, origin_x) in line_layout.iter().skip(1) {
                    x_min = x_min.min(*origin_x);
                    x_max = x_max.max(origin_x.saturating_add(*width));
                }
                let y_min = first.0;
                Rect {
                    x_min,
                    y_min,
                    x_max,
                    y_max: y_min
                        .saturating_add(total_text_height(&line_metrics, config).round() as i32),
                }
            });
            let mut event_left = i32::MAX;
            let mut event_right = i32::MIN;
            let mut event_border_x = 0_i32;
            let mut event_border_top = 0_i32;
            let mut event_border_bottom = 0_i32;
            let mut event_back_colour = style.back_colour;
            let mut event_shadow = (0.0_f64, 0.0_f64);
            for (line_index, (line, (line_top, line_metric, scaled_line_width, origin_x))) in event
                .lines
                .iter()
                .zip(line_layout.iter().copied())
                .enumerate()
            {
                let line_plane_starts = PlaneStarts {
                    shadow: shadow_planes.len(),
                    outline: outline_planes.len(),
                    character: character_planes.len(),
                };
                let _ = line_plane_starts;
                let line_ascender = line_metric.asc.round() as i32;
                let line_height = line_metric.height().round() as i32;
                event_left = event_left.min(origin_x);
                event_right = event_right.max(origin_x + scaled_line_width);
                let origin_x_26_6 = origin_x * 64;
                let mut line_pen_x_26_6 = 0;
                let mut line_border_y = 0_i32;
                let line_is_trimmed_empty = line
                    .runs
                    .iter()
                    .all(|run| run.drawing.is_none() && run.glyphs.is_empty());
                for run in &line.runs {
                    let effective_style = apply_renderer_style_scale(
                        resolve_run_style(run, source_event, now_ms),
                        track,
                        config,
                        event_font_scale,
                        render_scale_all,
                    );
                    let bitmap_blur =
                        effective_bitmap_blur(&effective_style, track, config, event_font_scale);
                    if run.drawing.is_none() && (line_is_trimmed_empty || !run.glyphs.is_empty()) {
                        event_border_x =
                            event_border_x.max(effective_style.border_x.round().max(0.0) as i32);
                        line_border_y =
                            line_border_y.max(effective_style.border_y.round().max(0.0) as i32);
                    }
                    event_back_colour = effective_style.back_colour;
                    event_shadow = (effective_style.shadow_x, effective_style.shadow_y);
                    let run_origin_x_26_6 = origin_x_26_6 + line_pen_x_26_6;
                    let run_origin_x = run_origin_x_26_6 >> 6;
                    let run_shadow_start = shadow_planes.len();
                    let run_outline_start = outline_planes.len();
                    let run_character_start = character_planes.len();
                    let run_transform =
                        style_transform(&effective_style, effective_pixel_aspect(track, config));
                    if style.border_style == 3 && (run.drawing.is_some() || !run.glyphs.is_empty())
                    {
                        // OUTLINE_BOX is produced per style run in libass. In
                        // particular, inline \bord/\xbord/\ybord, \3c,
                        // \xshad/\yshad and \4c overrides must not inherit the
                        // base event style. Compatible adjacent boxes are
                        // merged later, preserving the traditional one-box
                        // result when every run has the same style.
                        let box_padding_x = (effective_style.border_x
                            * style_scale(effective_style.scale_x))
                        .round()
                        .max(0.0) as i32;
                        let box_padding_y = (effective_style.border_y
                            * style_scale(effective_style.scale_y))
                        .round()
                        .max(0.0) as i32;
                        let source_size = run.style.font_size.max(1.0);
                        let run_box_width = (f64::from(run.width)
                            * (effective_style.font_size / source_size)
                            * effective_pixel_aspect(track, config))
                        .round() as i32;
                        // libass OUTLINE_BOX: the opaque box spans the run's
                        // advance horizontally and -asc..desc vertically,
                        // expanded by the border on each side.
                        let rect = Rect {
                            x_min: run_origin_x - box_padding_x,
                            y_min: line_top - box_padding_y,
                            x_max: run_origin_x + run_box_width + box_padding_x,
                            y_max: line_top + line_height + box_padding_y,
                        };
                        if let Some(box_plane) = opaque_box_plane_from_rects(
                            &[rect],
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                            Point { x: 0, y: 0 },
                        ) {
                            outline_planes.push(box_plane);
                        }
                        let box_shadow_x = effective_style.shadow_x.round() as i32;
                        let box_shadow_y = effective_style.shadow_y.round() as i32;
                        if box_shadow_x != 0 || box_shadow_y != 0 {
                            if let Some(shadow_plane) = opaque_box_plane_from_rects(
                                &[rect],
                                effective_style.back_colour,
                                ass::ImageType::Shadow,
                                Point {
                                    x: box_shadow_x,
                                    y: box_shadow_y,
                                },
                            ) {
                                shadow_planes.push(shadow_plane);
                            }
                        }
                    }
                    if let Some(drawing) = &run.drawing {
                        let drawing_polygons = scaled_drawing_polygons(
                            drawing,
                            &run.text,
                            effective_style.scale_x,
                            effective_style.scale_y,
                            render_scale_all.x,
                            render_scale_all.y,
                        );
                        // libass places a drawing's ink box so its bottom sits
                        // at baseline + pbo (drawing asc = height - pbo,
                        // desc = pbo); the plane top is baseline - height + pbo.
                        let drawing_height = drawing_polygons
                            .as_deref()
                            .and_then(drawing_height_from_d6)
                            .unwrap_or_default();
                        let baseline = line_top.saturating_add(line_ascender);
                        let drawing_top = baseline.saturating_sub(drawing_height);
                        let pbo_script = drawing_pbo_script_pixels(&effective_style, drawing)
                            * style_scale(effective_style.scale_y);
                        if let Some(mut plane) = drawing_polygons.as_deref().and_then(|polygons| {
                            image_plane_from_drawing(
                                polygons,
                                DrawingPlaneParams {
                                    origin_x: run_origin_x,
                                    line_top: drawing_top,
                                    color: resolve_run_fill_color(
                                        run,
                                        &effective_style,
                                        source_event,
                                        now_ms,
                                    ),
                                    render_scale_y: render_scale_all.y,
                                    baseline_offset: pbo_script,
                                },
                            )
                        }) {
                            let drawing_fill_blur = if effective_style.border_x > 0.0
                                || effective_style.border_y > 0.0
                                || effective_style.shadow_x.abs() > f64::EPSILON
                                || effective_style.shadow_y.abs() > f64::EPSILON
                            {
                                BitmapBlur::default()
                            } else {
                                bitmap_blur
                            };
                            if !drawing_fill_blur.is_zero() {
                                plane = blur_image_plane_xy(plane, drawing_fill_blur);
                            }
                            if effective_style.border_x > 0.0 || effective_style.border_y > 0.0 {
                                if let Some(outline_glyph) = plane_to_raster_glyph(&plane) {
                                    let rasterizer = Rasterizer::with_options(RasterOptions {
                                        size_26_6: 64,
                                        hinting: config.hinting,
                                    });
                                    let radius_for = |border: f64| {
                                        if border > 0.0 {
                                            border.round().max(1.0) as i32
                                        } else {
                                            0
                                        }
                                    };
                                    let outline_glyphs = rasterizer.outline_glyphs_xy(
                                        &[outline_glyph],
                                        radius_for(effective_style.border_x),
                                        radius_for(effective_style.border_y),
                                    );
                                    outline_planes.extend(
                                        image_planes_from_absolute_glyphs(
                                            &outline_glyphs,
                                            effective_style.outline_colour,
                                            ass::ImageType::Outline,
                                        )
                                        .into_iter()
                                        .map(|plane| blur_image_plane_xy(plane, bitmap_blur)),
                                    );
                                }
                            }
                            character_planes.push(plane);
                            if style.border_style != 4
                                && (effective_style.shadow_x.abs() > f64::EPSILON
                                    || effective_style.shadow_y.abs() > f64::EPSILON)
                            {
                                if let Some(shadow_glyph) = plane_to_raster_glyph(
                                    character_planes.last().expect("drawing plane"),
                                ) {
                                    // libass offsets shadows down-right for
                                    // positive \xshad/\yshad; top here is the
                                    // baseline-relative bitmap top, so moving the
                                    // ink down means lowering it by shadow_y.
                                    shadow_planes.extend(
                                        image_planes_from_absolute_glyphs(
                                            &[RasterGlyph {
                                                left: shadow_glyph.left.saturating_add(
                                                    effective_style.shadow_x.round() as i32,
                                                ),
                                                top: shadow_glyph.top.saturating_add(
                                                    effective_style.shadow_y.round() as i32,
                                                ),
                                                ..shadow_glyph
                                            }],
                                            effective_style.back_colour,
                                            ass::ImageType::Shadow,
                                        )
                                        .into_iter()
                                        .map(|plane| blur_image_plane_xy(plane, bitmap_blur)),
                                    );
                                }
                            }
                        }
                        apply_run_transform_to_recent_planes(
                            &mut shadow_planes,
                            &mut outline_planes,
                            &mut character_planes,
                            PlaneStarts {
                                shadow: run_shadow_start,
                                outline: run_outline_start,
                                character: run_character_start,
                            },
                            RunTransformContext {
                                transform: run_transform,
                                event,
                                effective_position,
                                event_layout_bounds,
                                projection_scale_y,
                                mapping: &mapping,
                                shear_pivot_x: Some(f64::from(run_origin_x_26_6) / 64.0),
                                shear_pivot_y: Some(f64::from(line_top)),
                            },
                        );
                        // run.width already includes \fscx (layout applies the
                        // style scale when measuring the drawing).
                        let drawing_advance_26_6 = (f64::from(run.width) * render_scale_x * 64.0)
                            .round()
                            .max(0.0) as i32;
                        line_pen_x_26_6 += drawing_advance_26_6;
                        continue;
                    }
                    let rasterizer = Rasterizer::with_options(RasterOptions {
                        size_26_6: (effective_style.font_size.max(1.0) * 64.0).round() as i32,
                        hinting: config.hinting,
                    });
                    let position_scale =
                        shaped_position_render_scale(run, &effective_style, render_scale_all);
                    let glyph_infos =
                        scale_glyph_infos(&run.glyphs, position_scale.0, position_scale.1);
                    let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos)
                    else {
                        line_pen_x_26_6 += (run.width * 64.0).round() as i32;
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
                    let run_ascender = Some(line_ascender);
                    let has_outline = style.border_style != 3
                        && (effective_style.border_x > 0.0 || effective_style.border_y > 0.0)
                        && !karaoke_hides_outline(run, source_event, now_ms);
                    // libass render_text skips shadow bitmaps entirely for
                    // BorderStyle 4 (the background box replaces them).
                    let has_shadow = style.border_style != 4
                        && (effective_style.shadow_x.abs() > f64::EPSILON
                            || effective_style.shadow_y.abs() > f64::EPSILON);
                    let fill_blur = if has_outline || has_shadow {
                        BitmapBlur::default()
                    } else {
                        bitmap_blur
                    };
                    let mut outlined_shadow_source_glyphs = None;
                    if has_outline {
                        // libass strokes with independent x/y radii
                        // (\xbord/\ybord); a zero radius keeps that axis
                        // unexpanded.
                        let radius_for = |border: f64| {
                            if border > 0.0 {
                                border.round().max(1.0) as i32
                            } else {
                                0
                            }
                        };
                        let outline_glyphs = rasterizer.outline_glyphs_xy(
                            &raster_glyphs,
                            radius_for(effective_style.border_x),
                            radius_for(effective_style.border_y),
                        );
                        if has_shadow {
                            outlined_shadow_source_glyphs = Some(outline_glyphs.clone());
                        }
                        if let Some(plane) = combined_image_plane_from_glyphs_xy(
                            &outline_glyphs,
                            run_origin_x_26_6,
                            line_top,
                            run_ascender,
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                            bitmap_blur,
                        ) {
                            outline_planes.push(plane);
                        }
                    }
                    let fill_color =
                        resolve_run_fill_color(run, &effective_style, source_event, now_ms);
                    if run.karaoke.is_none() && !bitmap_blur.is_zero() {
                        if let Some(plane) = combined_image_plane_from_glyphs_xy(
                            &raster_glyphs,
                            run_origin_x_26_6,
                            line_top,
                            run_ascender,
                            fill_color,
                            ass::ImageType::Character,
                            fill_blur,
                        ) {
                            character_planes.push(plane);
                        }
                    } else {
                        let maybe_fill_plane = combined_image_plane_from_glyphs_xy(
                            &raster_glyphs,
                            run_origin_x_26_6,
                            line_top,
                            run_ascender,
                            fill_color,
                            ass::ImageType::Character,
                            fill_blur,
                        );
                        if run.karaoke.is_some() {
                            let fill_planes = maybe_fill_plane.into_iter().collect();
                            let quarter_turn_sweep = run.karaoke.is_some_and(|karaoke| {
                                karaoke.mode == ParsedKaraokeMode::Sweep
                                    && (effective_style.rotation_z.abs() % 180.0 - 90.0).abs()
                                        < f64::EPSILON
                            });
                            if quarter_turn_sweep {
                                character_planes.extend(fill_planes);
                            } else {
                                character_planes.extend(apply_karaoke_to_character_planes(
                                    fill_planes,
                                    run,
                                    &effective_style,
                                    source_event,
                                    now_ms,
                                    run_origin_x,
                                    raster_glyphs
                                        .iter()
                                        .map(|glyph| glyph.advance_x_26_6)
                                        .sum::<i32>()
                                        >> 6,
                                ));
                            }
                        } else if let Some(plane) = maybe_fill_plane {
                            character_planes.push(plane);
                        }
                    }
                    let run_advance_26_6 = raster_glyphs
                        .iter()
                        .map(|glyph| glyph.advance_x_26_6)
                        .sum::<i32>();
                    let decoration_bars = text_decoration_bars(
                        &effective_style,
                        &run.font,
                        line_top + line_ascender,
                        run_origin_x,
                        (run_advance_26_6 + 32) >> 6,
                    );
                    for bar in &decoration_bars {
                        character_planes.push(solid_plane_from_rect(
                            *bar,
                            fill_color,
                            ass::ImageType::Character,
                        ));
                        if has_outline {
                            let expand = |border: f64| {
                                if border > 0.0 {
                                    border.round().max(1.0) as i32
                                } else {
                                    0
                                }
                            };
                            outline_planes.push(solid_plane_from_rect(
                                expand_rect_xy(
                                    *bar,
                                    expand(effective_style.border_x),
                                    expand(effective_style.border_y),
                                ),
                                effective_style.outline_colour,
                                ass::ImageType::Outline,
                            ));
                        }
                        if has_shadow {
                            let mut shadow_bar = *bar;
                            shadow_bar.x_min += effective_style.shadow_x.round() as i32;
                            shadow_bar.x_max += effective_style.shadow_x.round() as i32;
                            shadow_bar.y_min += effective_style.shadow_y.round() as i32;
                            shadow_bar.y_max += effective_style.shadow_y.round() as i32;
                            shadow_planes.push(solid_plane_from_rect(
                                shadow_bar,
                                effective_style.back_colour,
                                ass::ImageType::Shadow,
                            ));
                        }
                    }
                    if has_shadow {
                        let shadow_glyphs = outlined_shadow_source_glyphs
                            .as_deref()
                            .unwrap_or(&raster_glyphs);
                        if let Some(plane) = combined_image_plane_from_glyphs_xy(
                            shadow_glyphs,
                            run_origin_x_26_6 + (effective_style.shadow_x * 64.0).round() as i32,
                            line_top + effective_style.shadow_y.round() as i32,
                            run_ascender,
                            effective_style.back_colour,
                            ass::ImageType::Shadow,
                            bitmap_blur,
                        ) {
                            shadow_planes.push(plane);
                        }
                    }
                    apply_run_transform_to_recent_planes(
                        &mut shadow_planes,
                        &mut outline_planes,
                        &mut character_planes,
                        PlaneStarts {
                            shadow: run_shadow_start,
                            outline: run_outline_start,
                            character: run_character_start,
                        },
                        RunTransformContext {
                            transform: run_transform,
                            event,
                            effective_position,
                            event_layout_bounds,
                            projection_scale_y,
                            mapping: &mapping,
                            shear_pivot_x: Some(f64::from(run_origin_x_26_6) / 64.0),
                            shear_pivot_y: Some(f64::from(line_top)),
                        },
                    );
                    if run.karaoke.is_some()
                        && (effective_style.rotation_z.abs() % 180.0 - 90.0).abs() < f64::EPSILON
                    {
                        let transformed = character_planes.split_off(run_character_start);
                        let swept = apply_quarter_turn_karaoke_sweep_after_transform(
                            transformed,
                            run,
                            &effective_style,
                            source_event,
                            now_ms,
                            run_advance_26_6 >> 6,
                        );
                        character_planes.extend(swept);
                    }
                    line_pen_x_26_6 += run_advance_26_6;
                }
                // libass measure_text retains the first line's maximum border
                // as border_top and overwrites border_bottom for every line.
                // A large border on an earlier line must therefore not pad the
                // bottom of a multiline collision rectangle.
                if line_index == 0 {
                    event_border_top = line_border_y;
                }
                event_border_bottom = line_border_y;
            }

            // libass EventImages: top = device_y - lines[0].asc - border_top,
            // height = text height + borders, width = bbox + 2 * border_x.
            let event_line_box = (!event.lines.is_empty() && event_left < event_right).then(|| {
                let total_height = total_text_height(&line_metrics, config).round() as i32;
                Rect {
                    x_min: event_left - event_border_x,
                    y_min: vertical_layout.first().copied().unwrap_or(0) - event_border_top,
                    x_max: event_right + event_border_x,
                    y_max: vertical_layout.first().copied().unwrap_or(0)
                        + total_height
                        + event_border_bottom,
                }
            });
            let mut event_planes = shadow_planes;
            event_planes.extend(outline_planes);
            event_planes.extend(character_planes);
            // Each plane above already represents one libass bitmap-combine
            // run. Do not merge distinct runs here merely because their
            // padded rectangles overlap: blur/filter state is a run key in
            // libass even when type and colour match. Coalescing such runs
            // changes coverage and collapses public ASS_Image nodes.
            let apply_script_clip = event_is_explicit || !config.use_margins;
            if apply_script_clip {
                if let Some((clip_rect, inverse_clip)) =
                    resolve_rect_clip(event, track, source_event, now_ms)
                {
                    let clip_rect = scale_clip_rect_exact(clip_rect, &mapping);
                    // libass render_and_apply_clip uses the exact clip rectangle for
                    // both \clip and \iclip; it does not bleed the inverse region by
                    // the border/shadow extent.
                    event_planes = apply_event_clip(event_planes, clip_rect, inverse_clip);
                }
                if let Some(vector_clip) = &event.vector_clip {
                    // A failed outline transform makes libass skip vector
                    // clipping altogether, for both regular and inverse clips.
                    if let Some(exact_clip) =
                        source_event.and_then(|source| parse_dialogue_vector_clip_d6(&source.text))
                    {
                        if let Some(vector_clip) = scale_vector_clip_d6(&exact_clip, &mapping) {
                            event_planes = apply_vector_clip_d6(
                                event_planes,
                                &vector_clip,
                                event.vector_clip_inverse,
                            );
                        }
                    } else if let Some(vector_clip) = scale_vector_clip(vector_clip, &mapping) {
                        event_planes = apply_vector_clip(
                            event_planes,
                            &vector_clip,
                            event.vector_clip_inverse,
                        );
                    }
                }
            }
            if style.border_style == 4 {
                if let Some(rect) = event_line_box {
                    // libass add_background: the event box expanded by the
                    // positive shadow offsets, clamped to the frame, filled
                    // with the final back colour and drawn first.
                    let size_x = if event_shadow.0 > 0.0 {
                        event_shadow.0.round() as i32
                    } else {
                        0
                    };
                    let size_y = if event_shadow.1 > 0.0 {
                        event_shadow.1.round() as i32
                    } else {
                        0
                    };
                    let frame = frame_clip_rect(track, config, event_is_explicit);
                    let background = Rect {
                        x_min: (rect.x_min - size_x).clamp(frame.x_min, frame.x_max),
                        y_min: (rect.y_min - size_y).clamp(frame.y_min, frame.y_max),
                        x_max: (rect.x_max + size_x).clamp(frame.x_min, frame.x_max),
                        y_max: (rect.y_max + size_y).clamp(frame.y_min, frame.y_max),
                    };
                    if background.width() > 0 && background.height() > 0 {
                        event_planes.insert(
                            0,
                            solid_plane_from_rect(
                                background,
                                event_back_colour,
                                ass::ImageType::Shadow,
                            ),
                        );
                    }
                }
            }
            if let Some(fade) = event.fade {
                event_planes = apply_fade_to_planes(event_planes, fade, source_event, now_ms);
            } else if planes_have_translucent_fill(&event_planes) {
                // libass leaves FILTER_FILL_IN_BORDER clear when the primary or
                // secondary colour is translucent, carving the fill out of the
                // border so the two do not double-composite. The fade path above
                // already does this when a fade is present.
                carve_fill_out_of_outline(&mut event_planes);
            }
            event_planes = apply_effect_to_planes(
                event_planes,
                source_event,
                track,
                config,
                now_ms,
                &mapping,
                event_line_box,
            );
            // Coordinates are already in final screen space: the per-event
            // mapping folds the margin offsets in.
            let collision_rect = event_line_box;
            rendered_events.push(RenderedEvent {
                event_index: event.event_index,
                planes: event_planes,
                collision_rect,
                detect_collisions: effective_position.is_none()
                    && event.origin.is_none()
                    && event.origin_exact.is_none()
                    && !event.transform_disables_collision
                    && !effect_disables_collision,
                shift_direction: if (event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER))
                    == ass::VALIGN_SUB
                {
                    -1
                } else {
                    1
                },
                frame_clip: frame_clip_rect(track, config, event_is_explicit),
            });
        }

        // libass runs fix_collisions independently for each same-layer group,
        // then concatenates the finished image lists and clips to the frame.
        {
            let mut cache = self
                .collision_cache
                .lock()
                .expect("collision cache mutex poisoned");
            fix_collisions_by_layer(&mut cache, &mut rendered_events, track);
        }

        let mut planes = Vec::new();
        for record in rendered_events {
            planes.extend(apply_event_clip(record.planes, record.frame_clip, false));
        }
        // libass 0.17.5 filters fully transparent ASS_Image nodes only after
        // rendering, collision handling, and final clipping.  Preserve all
        // earlier work (including zero-sized clip nodes) but do not expose a
        // plane whose ASS alpha byte is 0xFF to callers.
        planes.retain(|plane| plane.color.0 & 0xFF != 0xFF);
        planes
    }

    pub fn render_frame(&self, track: &ParsedTrack, now_ms: i64) -> Vec<ImagePlane> {
        let provider = FontconfigProvider::new();
        self.render_frame_with_provider(track, &provider, now_ms)
    }
}

#[cfg(test)]
mod tests;
