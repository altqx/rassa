use std::collections::HashMap;

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
use freetype::{Library, ffi};
use rassa_core::{ImagePlane, Point, Rect, RendererConfig, RgbaColor, Size, ass};
use rassa_fonts::{FontMatch, FontProvider, FontconfigProvider};
use rassa_layout::{LayoutEngine, LayoutEvent, LayoutGlyphRun};
use rassa_parse::{
    ParsedDrawing, ParsedEvent, ParsedFade, ParsedKaraokeMode, ParsedMovement, ParsedMovementExact,
    ParsedSpanStyle, ParsedTrack, ParsedVectorClip,
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
                self.layout
                    .layout_track_event_with_mode(track, index, provider, shaping_mode)
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
            track.events.len().hash(&mut hasher);
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
        let render_scale = (style_scale(render_scale_x) + style_scale(render_scale_y)) / 2.0;
        let render_scale_all = RenderScale {
            x: render_scale_x,
            y: render_scale_y,
            uniform: render_scale,
        };

        for event in &prepared.active_events {
            let source_event = track.events.get(event.event_index);
            let Some(style) = track.styles.get(event.style_index) else {
                continue;
            };
            let mut shadow_planes = Vec::new();
            let mut outline_planes = Vec::new();
            let mut character_planes = Vec::new();
            let mut opaque_box_rects = Vec::new();
            let mut clip_mask_bleed = 0;
            let effective_position = scale_position(
                resolve_event_position(track, event, now_ms),
                render_scale_x,
                render_scale_y,
            );
            let effect_disables_collision = source_event
                .map(transition_effect_disables_collision)
                .unwrap_or(false);
            let metrics_context = LineMetricsContext {
                track,
                config,
                source_event,
                now_ms,
                render_scale: render_scale_all,
            };
            let line_metrics = event_line_metrics(&event.lines, &metrics_context);
            let vertical_layout = compute_vertical_layout(
                track,
                &line_metrics,
                event.alignment,
                event.margin_v,
                effective_position,
                config,
                render_scale_all,
            );
            let mut event_left = i32::MAX;
            let mut event_right = i32::MIN;
            let mut event_border_x = 0_i32;
            let mut event_border_y = 0_i32;
            for ((line, line_top), line_metric) in event
                .lines
                .iter()
                .zip(vertical_layout.iter().copied())
                .zip(line_metrics.iter().copied())
            {
                let line_plane_starts = PlaneStarts {
                    shadow: shadow_planes.len(),
                    outline: outline_planes.len(),
                    character: character_planes.len(),
                };
                let _ = line_plane_starts;
                let line_ascender = line_metric.asc.round() as i32;
                let line_height = line_metric.height().round() as i32;
                let scaled_line_width = rendered_text_alignment_width(
                    line,
                    source_event,
                    now_ms,
                    track,
                    config,
                    render_scale_all,
                );
                let origin_x = compute_horizontal_origin(
                    track,
                    event,
                    scaled_line_width,
                    effective_position,
                    render_scale_x,
                );
                event_left = event_left.min(origin_x);
                event_right = event_right.max(origin_x + scaled_line_width);
                let origin_x_26_6 = origin_x * 64;
                let mut line_pen_x_26_6 = 0;
                let mut line_has_transformed_borderstyle3_box = false;
                for run in &line.runs {
                    let effective_style = apply_renderer_style_scale(
                        resolve_run_style(run, source_event, now_ms),
                        track,
                        config,
                        render_scale,
                    );
                    clip_mask_bleed = clip_mask_bleed.max(style_clip_bleed(&effective_style));
                    if run.drawing.is_none() {
                        event_border_x =
                            event_border_x.max(effective_style.border_x.round().max(0.0) as i32);
                        event_border_y =
                            event_border_y.max(effective_style.border_y.round().max(0.0) as i32);
                    }
                    let run_origin_x_26_6 = origin_x_26_6 + line_pen_x_26_6;
                    let run_origin_x = run_origin_x_26_6 >> 6;
                    let run_shadow_start = shadow_planes.len();
                    let run_outline_start = outline_planes.len();
                    let run_character_start = character_planes.len();
                    let run_transform = style_transform(&effective_style);
                    let transformed_borderstyle3_box =
                        style.border_style == 3 && !run_transform.is_identity();
                    if transformed_borderstyle3_box {
                        line_has_transformed_borderstyle3_box = true;
                        let box_scale = renderer_font_scale(config) * style_scale(render_scale);
                        let compensation = if track.scaled_border_and_shadow {
                            1.0
                        } else {
                            border_shadow_compensation_scale(track, config)
                        };
                        let box_padding = (effective_style.border * box_scale / compensation)
                            .round()
                            .max(0.0) as i32;
                        let run_box_width = (f64::from(run.width) * render_scale_x).round() as i32;
                        // libass OUTLINE_BOX: the opaque box spans the run's
                        // advance horizontally and -asc..desc vertically,
                        // expanded by the border on each side.
                        let rect = Rect {
                            x_min: run_origin_x - box_padding,
                            y_min: line_top - box_padding,
                            x_max: run_origin_x + run_box_width + box_padding,
                            y_max: line_top + line_height + box_padding,
                        };
                        if let Some(box_plane) = opaque_box_plane_from_rects(
                            &[rect],
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                            Point { x: 0, y: 0 },
                        ) {
                            outline_planes.push(box_plane);
                        }
                        let box_shadow =
                            (effective_style.shadow * box_scale / compensation).round() as i32;
                        if box_shadow > 0 {
                            if let Some(shadow_plane) = opaque_box_plane_from_rects(
                                &[rect],
                                effective_style.back_colour,
                                ass::ImageType::Shadow,
                                Point {
                                    x: box_shadow,
                                    y: box_shadow,
                                },
                            ) {
                                shadow_planes.push(shadow_plane);
                            }
                        }
                    }
                    if let Some(drawing) = &run.drawing {
                        // libass places a drawing's ink box so its bottom sits
                        // at baseline + pbo (drawing asc = height - pbo,
                        // desc = pbo); the plane top is baseline - height + pbo.
                        let drawing_scale_y = style_scale(effective_style.scale_y)
                            * style_scale(render_scale_y);
                        let drawing_height = drawing
                            .bounds()
                            .map(|bounds| {
                                f64::from((bounds.height() - 1).max(0)) * drawing_scale_y
                            })
                            .unwrap_or_default();
                        let baseline = line_top + line_ascender;
                        let drawing_top = baseline - drawing_height.round() as i32;
                        let pbo_script =
                            drawing_pbo_script_pixels(&effective_style, drawing)
                                * style_scale(effective_style.scale_y);
                        if let Some(mut plane) = image_plane_from_drawing(
                            drawing,
                            DrawingPlaneParams {
                                origin_x: run_origin_x,
                                line_top: drawing_top,
                                color: resolve_run_fill_color(
                                    run,
                                    &effective_style,
                                    source_event,
                                    now_ms,
                                ),
                                scale_x: effective_style.scale_x,
                                scale_y: effective_style.scale_y,
                                render_scale: render_scale_all,
                                baseline_offset: pbo_script,
                            },
                        ) {
                            let drawing_fill_blur = if effective_style.border_x > 0.0
                                || effective_style.border_y > 0.0
                                || effective_style.shadow_x.abs() > f64::EPSILON
                                || effective_style.shadow_y.abs() > f64::EPSILON
                            {
                                0
                            } else {
                                renderer_blur_radius(effective_blur_strength(&effective_style))
                            };
                            if drawing_fill_blur > 0 {
                                plane = blur_image_plane(plane, drawing_fill_blur);
                            }
                            if effective_style.border_x > 0.0 || effective_style.border_y > 0.0
                            {
                                let outline_glyph = plane_to_raster_glyph(&plane);
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
                                let mut outline_glyphs = rasterizer.outline_glyphs_xy(
                                    &[outline_glyph],
                                    radius_for(effective_style.border_x),
                                    radius_for(effective_style.border_y),
                                );
                                let drawing_blur =
                                    effective_blur_strength(&effective_style);
                                if drawing_blur > 0.0 {
                                    outline_glyphs = rasterizer.blur_glyphs(
                                        &outline_glyphs,
                                        renderer_blur_radius(drawing_blur),
                                    );
                                }
                                outline_planes.extend(image_planes_from_absolute_glyphs(
                                    &outline_glyphs,
                                    effective_style.outline_colour,
                                    ass::ImageType::Outline,
                                ));
                            }
                            character_planes.push(plane);
                            if effective_style.shadow_x.abs() > f64::EPSILON
                                || effective_style.shadow_y.abs() > f64::EPSILON
                            {
                                let rasterizer = Rasterizer::with_options(RasterOptions {
                                    size_26_6: 64,
                                    hinting: config.hinting,
                                });
                                let mut shadow_glyph = plane_to_raster_glyph(
                                    character_planes.last().expect("drawing plane"),
                                );
                                let drawing_blur =
                                    effective_blur_strength(&effective_style);
                                if drawing_blur > 0.0 {
                                    shadow_glyph = rasterizer
                                        .blur_glyphs(
                                            &[shadow_glyph],
                                            renderer_blur_radius(drawing_blur),
                                        )
                                        .into_iter()
                                        .next()
                                        .expect("shadow glyph");
                                }
                                // libass offsets shadows down-right for
                                // positive \xshad/\yshad; top here is the
                                // baseline-relative bitmap top, so moving the
                                // ink down means lowering it by shadow_y.
                                shadow_planes.extend(image_planes_from_absolute_glyphs(
                                    &[RasterGlyph {
                                        left: shadow_glyph.left
                                            + effective_style.shadow_x.round() as i32,
                                        top: shadow_glyph.top
                                            + effective_style.shadow_y.round() as i32,
                                        ..shadow_glyph
                                    }],
                                    effective_style.back_colour,
                                    ass::ImageType::Shadow,
                                ));
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
                                render_scale: render_scale_all,
                            },
                        );
                        // run.width already includes \fscx (layout applies the
                        // style scale when measuring the drawing).
                        let drawing_advance_26_6 =
                            (f64::from(run.width) * render_scale_x * 64.0).round().max(0.0) as i32;
                        line_pen_x_26_6 += drawing_advance_26_6;
                        continue;
                    }
                    let rasterizer = Rasterizer::with_options(RasterOptions {
                        size_26_6: (effective_style.font_size.max(1.0) * 64.0).round() as i32,
                        hinting: config.hinting,
                    });
                    let glyph_infos =
                        scale_glyph_infos(&run.glyphs, render_scale_x, render_scale_y);
                    let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &glyph_infos)
                    else {
                        line_pen_x_26_6 += (run.width * 64.0).round() as i32;
                        continue;
                    };
                    let raster_glyphs =
                        apply_vertical_font_raster_advances(raster_glyphs, &effective_style);
                    let raster_glyphs = scale_raster_glyphs(
                        raster_glyphs,
                        effective_style.scale_x,
                        effective_style.scale_y,
                    );
                    let raster_glyphs = apply_text_spacing(raster_glyphs, &effective_style);
                    let run_ascender = Some(line_ascender);
                    let effective_blur = effective_blur_strength(&effective_style);
                    let has_outline = style.border_style != 3
                        && (effective_style.border_x > 0.0 || effective_style.border_y > 0.0)
                        && !karaoke_hides_outline(run, source_event, now_ms);
                    let has_shadow = effective_style.shadow_x.abs() > f64::EPSILON
                        || effective_style.shadow_y.abs() > f64::EPSILON;
                    let fill_blur = if has_outline || has_shadow {
                        0
                    } else {
                        renderer_blur_radius(effective_blur)
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
                        let outline_blur = renderer_blur_radius(effective_blur);
                        if let Some(plane) = combined_image_plane_from_glyphs(
                            &outline_glyphs,
                            run_origin_x_26_6,
                            line_top,
                            run_ascender,
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                            outline_blur,
                        ) {
                            outline_planes.push(plane);
                        }
                    }
                    let fill_color =
                        resolve_run_fill_color(run, &effective_style, source_event, now_ms);
                    if run.karaoke.is_none() && effective_blur > 0.0 {
                        if let Some(plane) = combined_image_plane_from_glyphs(
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
                        let maybe_fill_plane = combined_image_plane_from_glyphs(
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
                    if effective_style.shadow_x.abs() > f64::EPSILON
                        || effective_style.shadow_y.abs() > f64::EPSILON
                    {
                        let shadow_glyphs = outlined_shadow_source_glyphs
                            .as_deref()
                            .unwrap_or(&raster_glyphs);
                        if let Some(plane) = combined_image_plane_from_glyphs(
                            shadow_glyphs,
                            run_origin_x_26_6 + (effective_style.shadow_x * 64.0).round() as i32,
                            line_top + effective_style.shadow_y.round() as i32,
                            run_ascender,
                            effective_style.back_colour,
                            ass::ImageType::Shadow,
                            renderer_blur_radius(effective_blur),
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
                            render_scale: render_scale_all,
                        },
                    );
                    line_pen_x_26_6 += run_advance_26_6;
                }
                if style.border_style == 3 && !line_has_transformed_borderstyle3_box {
                    let box_scale = renderer_font_scale(config) * style_scale(render_scale);
                    let compensation = if track.scaled_border_and_shadow {
                        1.0
                    } else {
                        border_shadow_compensation_scale(track, config)
                    };
                    let box_padding =
                        (style.outline * box_scale / compensation).round().max(0.0) as i32;
                    let box_line_width = if line_pen_x_26_6 > 0 {
                        (line_pen_x_26_6 + 32) >> 6
                    } else {
                        scaled_line_width
                    };
                    let box_origin_x = compute_horizontal_origin(
                        track,
                        event,
                        box_line_width,
                        effective_position,
                        render_scale_x,
                    );
                    opaque_box_rects.push(Rect {
                        x_min: box_origin_x - box_padding,
                        y_min: line_top - box_padding,
                        x_max: box_origin_x + box_line_width + box_padding,
                        y_max: line_top + line_height + box_padding,
                    });
                }
            }

            if style.border_style == 3 {
                let box_scale = renderer_font_scale(config) * style_scale(render_scale);
                let compensation = if track.scaled_border_and_shadow {
                    1.0
                } else {
                    border_shadow_compensation_scale(track, config)
                };
                let box_shadow = (style.shadow * box_scale / compensation).round() as i32;
                if let Some(box_plane) = opaque_box_plane_from_rects(
                    &opaque_box_rects,
                    style.outline_colour,
                    ass::ImageType::Outline,
                    Point { x: 0, y: 0 },
                ) {
                    outline_planes.insert(0, box_plane);
                }
                if box_shadow > 0 {
                    if let Some(shadow_plane) = opaque_box_plane_from_rects(
                        &opaque_box_rects,
                        style.back_colour,
                        ass::ImageType::Shadow,
                        Point {
                            x: box_shadow,
                            y: box_shadow,
                        },
                    ) {
                        shadow_planes.clear();
                        shadow_planes.push(shadow_plane);
                    }
                }
            }

            let mut event_planes = shadow_planes;
            event_planes.extend(outline_planes);
            event_planes.extend(character_planes);
            // libass combines bitmaps of the same type and color within an
            // event (render_and_combine_glyphs); merge unconditionally.
            event_planes = merge_compatible_event_planes(event_planes);
            if let Some(clip_rect) =
                resolve_animated_clip_rect(event, track, source_event, now_ms)
            {
                let clip_rect = scale_clip_rect(clip_rect, render_scale_x, render_scale_y);
                let clip_rect = if event.inverse_clip {
                    expand_rect(clip_rect, clip_mask_bleed)
                } else {
                    clip_rect
                };
                event_planes = apply_event_clip(event_planes, clip_rect, event.inverse_clip);
            } else if let Some(vector_clip) = &event.vector_clip {
                event_planes = apply_vector_clip(event_planes, vector_clip, event.inverse_clip);
            }
            if let Some(fade) = event.fade {
                event_planes = apply_fade_to_planes(event_planes, fade, source_event, now_ms);
            }
            // libass EventImages: top = device_y - lines[0].asc - border_top,
            // height = text height + borders, width = bbox + 2 * border_x.
            let event_line_box = (!event.lines.is_empty() && event_left < event_right).then(|| {
                let total_height = total_text_height(&line_metrics, config).round() as i32;
                Rect {
                    x_min: event_left - event_border_x,
                    y_min: vertical_layout.first().copied().unwrap_or(0) - event_border_y,
                    x_max: event_right + event_border_x,
                    y_max: vertical_layout.first().copied().unwrap_or(0)
                        + total_height
                        + event_border_y,
                }
            });
            event_planes = apply_effect_to_planes(
                event_planes,
                source_event,
                track,
                config,
                now_ms,
                render_scale_x,
                render_scale_y,
                event_line_box,
            );
            let render_offset = output_offset(config);
            event_planes = translate_planes(event_planes, render_offset);
            let collision_rect = event_line_box.map(|rect| Rect {
                x_min: rect.x_min + render_offset.x,
                y_min: rect.y_min + render_offset.y,
                x_max: rect.x_max + render_offset.x,
                y_max: rect.y_max + render_offset.y,
            });
            rendered_events.push(RenderedEvent {
                event_index: event.event_index,
                planes: event_planes,
                collision_rect,
                detect_collisions: effective_position.is_none() && !effect_disables_collision,
                shift_direction: if (event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER))
                    == ass::VALIGN_SUB
                {
                    -1
                } else {
                    1
                },
                frame_clip: frame_clip_rect(track, config, event, effective_position),
            });
        }

        // libass runs fix_collisions over the finished event images of the
        // whole frame (all layers together), then clips to the frame.
        {
            let mut cache = self
                .collision_cache
                .lock()
                .expect("collision cache mutex poisoned");
            fix_collisions(&mut cache, &mut rendered_events);
        }

        let mut planes = Vec::new();
        for record in rendered_events {
            planes.extend(apply_event_clip(record.planes, record.frame_clip, false));
        }
        planes
    }

    pub fn render_frame(&self, track: &ParsedTrack, now_ms: i64) -> Vec<ImagePlane> {
        let provider = FontconfigProvider::new();
        self.render_frame_with_provider(track, &provider, now_ms)
    }
}

#[cfg(test)]
mod tests;
