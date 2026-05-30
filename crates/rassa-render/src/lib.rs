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
}

const LINE_HEIGHT: i32 = 40;

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
        let mut planes = Vec::new();
        let mut occupied_bounds_by_layer = HashMap::<i32, Vec<Rect>>::new();

        let render_scale_x = output_scale_x(track, config);
        let render_scale_y = output_scale_y(track, config);
        let render_scale = (style_scale(render_scale_x) + style_scale(render_scale_y)) / 2.0;

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
            let layer = event_layer(track, event);
            let occupied_bounds = occupied_bounds_by_layer.entry(layer).or_default();
            let effect_disables_collision = source_event
                .map(transition_effect_disables_collision)
                .unwrap_or(false);
            let layout_occupied_bounds = if effect_disables_collision {
                &[][..]
            } else {
                occupied_bounds.as_slice()
            };
            let vertical_layout = resolve_vertical_layout(
                track,
                event,
                effective_position,
                layout_occupied_bounds,
                source_event,
                now_ms,
                config,
                RenderScale {
                    x: render_scale_x,
                    y: render_scale_y,
                    uniform: render_scale,
                },
            );
            let occupied_bound =
                (!effect_disables_collision && effective_position.is_none()).then(|| {
                    event_bounds(
                        track,
                        event,
                        &vertical_layout,
                        effective_position,
                        config,
                        render_scale_x,
                        render_scale_y,
                    )
                });
            for (line_index, (line, line_top)) in event
                .lines
                .iter()
                .zip(vertical_layout.iter().copied())
                .enumerate()
            {
                let line_plane_starts = PlaneStarts {
                    shadow: shadow_planes.len(),
                    outline: outline_planes.len(),
                    character: character_planes.len(),
                };
                let has_scaled_run = line.runs.iter().any(|run| {
                    (run.style.scale_x - 1.0).abs() > f64::EPSILON
                        || (run.style.scale_y - 1.0).abs() > f64::EPSILON
                });
                let has_karaoke_run = line.runs.iter().any(|run| run.karaoke.is_some());
                let center_transformed_position = effective_position.is_some()
                    && positioned_center_line_has_active_projective_transform(
                        line,
                        source_event,
                        now_ms,
                    );
                let text_line_top = if effective_position.is_some() {
                    let border_style_3_y_adjust = if style.border_style == 3 { 1 } else { 0 };
                    line_top
                        + positioned_text_y_correction(
                            line,
                            config,
                            render_scale_y,
                            event.alignment,
                            center_transformed_position,
                        )
                        - border_style_3_y_adjust
                        + if has_karaoke_run { 2 } else { 0 }
                        + if has_scaled_run { 2 } else { 0 }
                } else {
                    line_top
                        + unpositioned_text_y_correction(line, config, render_scale_y)
                        + if has_scaled_run { 2 } else { 0 }
                };
                let scaled_line_width = rendered_text_alignment_width(
                    line,
                    source_event,
                    now_ms,
                    track,
                    config,
                    RenderScale {
                        x: render_scale_x,
                        y: render_scale_y,
                        uniform: render_scale,
                    },
                    effective_position.is_some(),
                    event.alignment,
                );
                let horizontal_anchor_width = if effective_position.is_none()
                    && line_contains_only_ascii_text(line)
                    && !line_has_outline_or_shadow(line)
                    && (event.alignment & 0x3) != ass::HALIGN_LEFT
                {
                    scaled_line_width + 3
                } else {
                    scaled_line_width
                };
                let origin_x = compute_horizontal_origin(
                    track,
                    event,
                    horizontal_anchor_width,
                    effective_position,
                    render_scale_x,
                );
                let text_origin_x = origin_x;
                let positioned_center_metric_anchor = effective_position.is_some()
                    && !center_transformed_position
                    && (event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER))
                        == ass::VALIGN_CENTER
                    && line_contains_only_ascii_text(line);
                let positioned_center_metric_plane_adjust = positioned_center_metric_anchor
                    && style.border_style != 3
                    && line_has_blur(line)
                    && !line_has_outline_or_shadow(line);
                let line_ascender = line_raster_ascender(
                    line,
                    source_event,
                    now_ms,
                    track,
                    config,
                    RenderScale {
                        x: render_scale_x,
                        y: render_scale_y,
                        uniform: render_scale,
                    },
                    positioned_center_metric_anchor,
                );
                let positioned_center_non_blur_metric_y_adjust = if positioned_center_metric_anchor
                    && !line_has_blur(line)
                    && render_scale_y >= 1.0
                {
                    if style.border_style == 3 {
                        -4
                    } else if has_karaoke_run {
                        -9
                    } else {
                        -6
                    }
                } else {
                    0
                };
                let positioned_center_downscale_metric_y_adjust = if positioned_center_metric_anchor
                    && !line_has_blur(line)
                    && render_scale_y < 1.0
                {
                    1
                } else {
                    0
                };
                let line_ascender = line_ascender
                    + if has_karaoke_run { 1 } else { 0 }
                    + positioned_center_non_blur_metric_y_adjust
                    + positioned_center_downscale_metric_y_adjust;
                let line_metric_height = font_metric_height_for_line(line, render_scale_y).max(1);
                let mut line_pen_x = 0;
                let mut line_has_transformed_borderstyle3_box = false;
                for run in &line.runs {
                    let effective_style = apply_renderer_style_scale(
                        resolve_run_style(run, source_event, now_ms),
                        track,
                        config,
                        render_scale,
                    );
                    clip_mask_bleed = clip_mask_bleed.max(style_clip_bleed(&effective_style));
                    let run_origin_x = text_origin_x + line_pen_x;
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
                        let box_visible_height = (effective_style.font_size
                            * style_scale(render_scale_y))
                        .round()
                        .max(1.0) as i32
                            + box_padding * 2;
                        let box_visible_top = if let Some((_, y)) = effective_position {
                            match event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
                                ass::VALIGN_TOP => y,
                                ass::VALIGN_CENTER => y - box_visible_height / 2,
                                _ => y - box_visible_height,
                            }
                        } else {
                            line_top
                        };
                        let run_box_width = (f64::from(run.width) * render_scale_x).round() as i32;
                        let box_vertical_pixel =
                            style_scale(render_scale_y).round().max(1.0) as i32;
                        let rect = Rect {
                            x_min: run_origin_x - box_padding,
                            y_min: box_visible_top - 1 - box_vertical_pixel,
                            x_max: run_origin_x + run_box_width + box_padding,
                            y_max: box_visible_top + box_visible_height + 1 - box_vertical_pixel,
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
                        let positioned_drawing = effective_position.is_some();
                        let drawing_baseline_y =
                            if line.runs.iter().all(|run| run.drawing.is_some()) {
                                line_top
                            } else if positioned_drawing {
                                line_top - style_scale(render_scale_y).round() as i32
                            } else {
                                line_top
                                    + drawing_baseline_ascender(&effective_style, render_scale_y)
                                    - style_scale(render_scale_y).round() as i32
                            };
                        if let Some(mut plane) = image_plane_from_drawing(
                            drawing,
                            DrawingPlaneParams {
                                origin_x: run_origin_x,
                                line_top: drawing_baseline_y,
                                color: resolve_run_fill_color(
                                    run,
                                    &effective_style,
                                    source_event,
                                    now_ms,
                                ),
                                scale_x: effective_style.scale_x,
                                scale_y: effective_style.scale_y,
                                render_scale: RenderScale {
                                    x: render_scale_x,
                                    y: render_scale_y,
                                    uniform: render_scale,
                                },
                                baseline_offset: effective_style.pbo,
                                pad_to_libass_geometry: effective_style
                                    .blur
                                    .max(effective_style.be)
                                    > 0.0
                                    || track
                                        .events
                                        .get(event.event_index)
                                        .map(|source| {
                                            source.text.contains("\\clip")
                                                || source.text.contains("\\iclip")
                                        })
                                        .unwrap_or(false),
                            },
                        ) {
                            let drawing_fill_blur = if effective_style.border > 0.0
                                || effective_style.shadow > 0.0
                            {
                                0
                            } else {
                                renderer_blur_radius(effective_style.blur.max(effective_style.be))
                            };
                            if drawing_fill_blur > 0 {
                                plane = blur_image_plane(plane, drawing_fill_blur);
                            }
                            if effective_style.border > 0.0 {
                                let mut outline_glyph = plane_to_raster_glyph(&plane);
                                let rasterizer = Rasterizer::with_options(RasterOptions {
                                    size_26_6: 64,
                                    hinting: config.hinting,
                                });
                                let mut outline_glyphs = rasterizer.outline_glyphs(
                                    &[outline_glyph.clone()],
                                    effective_style.border.round().max(1.0) as i32,
                                );
                                if effective_style.blur > 0.0 {
                                    outline_glyphs = rasterizer.blur_glyphs(
                                        &outline_glyphs,
                                        renderer_blur_radius(effective_style.blur),
                                    );
                                }
                                outline_planes.extend(image_planes_from_absolute_glyphs(
                                    &outline_glyphs,
                                    effective_style.outline_colour,
                                    ass::ImageType::Outline,
                                ));
                                outline_glyph = plane_to_raster_glyph(&plane);
                                let _ = outline_glyph;
                            }
                            character_planes.push(plane);
                            if effective_style.shadow > 0.0 {
                                let rasterizer = Rasterizer::with_options(RasterOptions {
                                    size_26_6: 64,
                                    hinting: config.hinting,
                                });
                                let mut shadow_glyph = plane_to_raster_glyph(
                                    character_planes.last().expect("drawing plane"),
                                );
                                if effective_style.blur > 0.0 {
                                    shadow_glyph = rasterizer
                                        .blur_glyphs(
                                            &[shadow_glyph],
                                            renderer_blur_radius(effective_style.blur),
                                        )
                                        .into_iter()
                                        .next()
                                        .expect("shadow glyph");
                                }
                                shadow_planes.extend(image_planes_from_absolute_glyphs(
                                    &[RasterGlyph {
                                        left: shadow_glyph.left
                                            + effective_style.shadow.round() as i32,
                                        top: shadow_glyph.top
                                            - effective_style.shadow.round() as i32,
                                        ..shadow_glyph
                                    }],
                                    effective_style.back_colour,
                                    ass::ImageType::Shadow,
                                ));
                            }
                        }
                        let run_plane_starts = PlaneStarts {
                            shadow: run_shadow_start,
                            outline: run_outline_start,
                            character: run_character_start,
                        };
                        apply_run_transform_to_recent_planes(
                            &mut shadow_planes,
                            &mut outline_planes,
                            &mut character_planes,
                            run_plane_starts,
                            RunTransformContext {
                                transform: run_transform,
                                event,
                                effective_position,
                                render_scale: RenderScale {
                                    x: render_scale_x,
                                    y: render_scale_y,
                                    uniform: render_scale,
                                },
                                drawing_run: true,
                                blur: effective_style.blur.max(effective_style.be),
                            },
                        );
                        let drawing_advance = (f64::from(run.width)
                            * style_scale(effective_style.scale_x)
                            * render_scale_x)
                            .round()
                            .max(0.0) as i32;
                        line_pen_x += drawing_advance;
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
                        line_pen_x += run.width.round() as i32;
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
                    let positioned_center_text_anchor_adjust =
                        if positioned_center_metric_plane_adjust
                            && (event.alignment & 0x3) == ass::HALIGN_CENTER
                        {
                            style_scale(render_scale_x).round().max(1.0) as i32
                        } else {
                            0
                        };
                    let glyph_origin_x = run_origin_x + positioned_center_text_anchor_adjust
                        - i32::from(has_scaled_run);
                    let run_line_metrics = Some(TextLineMetrics {
                        ascender: line_ascender,
                        height: Some(line_metric_height),
                        positioned_center_metric_anchor,
                        positioned_center_metric_plane_adjust,
                    });
                    let effective_blur = effective_style.blur.max(effective_style.be);
                    let has_outline = style.border_style != 3
                        && effective_style.border > 0.0
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
                        let outline_radius = effective_style.border.round().max(1.0) as i32;
                        let outline_glyphs =
                            rasterizer.outline_glyphs(&raster_glyphs, outline_radius);
                        if has_shadow {
                            outlined_shadow_source_glyphs = Some(outline_glyphs.clone());
                        }
                        let outline_blur = renderer_blur_radius(effective_blur);
                        if let Some(plane) = combined_image_plane_from_glyphs(
                            &outline_glyphs,
                            glyph_origin_x,
                            text_line_top,
                            run_line_metrics,
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
                            glyph_origin_x,
                            text_line_top,
                            run_line_metrics,
                            fill_color,
                            ass::ImageType::Character,
                            fill_blur,
                        ) {
                            character_planes.push(plane);
                        }
                    } else {
                        let maybe_fill_plane = combined_image_plane_from_glyphs(
                            &raster_glyphs,
                            glyph_origin_x,
                            text_line_top,
                            run_line_metrics,
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
                                glyph_origin_x,
                                raster_glyphs
                                    .iter()
                                    .map(|glyph| glyph.advance_x)
                                    .sum::<i32>(),
                            ));
                        } else if let Some(plane) = maybe_fill_plane {
                            character_planes.push(plane);
                        }
                    }
                    let run_advance = raster_glyphs
                        .iter()
                        .map(|glyph| glyph.advance_x)
                        .sum::<i32>();
                    character_planes.extend(text_decoration_planes(
                        &effective_style,
                        glyph_origin_x,
                        text_line_top,
                        run_advance,
                        fill_color,
                    ));
                    if effective_style.shadow_x.abs() > f64::EPSILON
                        || effective_style.shadow_y.abs() > f64::EPSILON
                    {
                        let shadow_glyphs = outlined_shadow_source_glyphs
                            .as_deref()
                            .unwrap_or(&raster_glyphs);
                        if let Some(plane) = combined_image_plane_from_glyphs(
                            shadow_glyphs,
                            glyph_origin_x + effective_style.shadow_x.round() as i32,
                            text_line_top + effective_style.shadow_y.round() as i32,
                            run_line_metrics,
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
                            render_scale: RenderScale {
                                x: render_scale_x,
                                y: render_scale_y,
                                uniform: render_scale,
                            },
                            drawing_run: false,
                            blur: effective_blur,
                        },
                    );
                    line_pen_x += run_advance;
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
                    let box_visible_height = (style.font_size * style_scale(render_scale_y))
                        .round()
                        .max(1.0) as i32
                        + box_padding * 2;
                    let box_visible_top = if let Some((_, y)) = effective_position {
                        match event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
                            ass::VALIGN_TOP => y,
                            ass::VALIGN_CENTER => y - box_visible_height / 2,
                            _ => y - box_visible_height,
                        }
                    } else {
                        line_top
                    };
                    let box_line_width = if line_pen_x > 0 {
                        line_pen_x
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
                    let box_vertical_pixel = style_scale(render_scale_y).round().max(1.0) as i32;
                    opaque_box_rects.push(Rect {
                        x_min: box_origin_x - box_padding,
                        y_min: box_visible_top - 1 - box_vertical_pixel,
                        x_max: box_origin_x + box_line_width + box_padding,
                        y_max: box_visible_top + box_visible_height + 1 - box_vertical_pixel,
                    });
                }
                align_positioned_text_line_bottom(
                    &mut shadow_planes,
                    &mut outline_planes,
                    &mut character_planes,
                    line_plane_starts,
                    PositionedLineBottomContext {
                        event,
                        line,
                        line_index,
                        line_count: event.lines.len(),
                        effective_position,
                        render_scale_y,
                    },
                );
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
            let coalesce_split_runs = track
                .events
                .get(event.event_index)
                .map(|source| {
                    let override_blocks = source.text.matches('{').count();
                    (source.text.contains("\\t(") && source.text.contains("\\alpha"))
                        || (override_blocks <= 1 && !source.text.contains("\\N"))
                })
                .unwrap_or(false);
            if coalesce_split_runs {
                event_planes = merge_compatible_event_planes(event_planes);
            }
            if let Some(clip_rect) = event.clip_rect {
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
            event_planes = apply_effect_to_planes(
                event_planes,
                source_event,
                track,
                config,
                now_ms,
                render_scale_x,
                render_scale_y,
            );
            let mut render_offset = output_offset(config);
            if style_scale(render_scale_y) > 1.0 {
                render_offset.y += render_scale_y.round() as i32;
            }
            event_planes = translate_planes(event_planes, render_offset);
            event_planes = apply_event_clip(
                event_planes,
                frame_clip_rect(track, config, event, effective_position),
                false,
            );
            if let Some(occupied_bound) = occupied_bound {
                occupied_bounds.push(occupied_bound);
            }
            planes.extend(event_planes);
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
