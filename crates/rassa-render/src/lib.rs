use std::collections::HashMap;

use rassa_core::{ImagePlane, Point, Rect, RendererConfig, RgbaColor, Size, ass};
use rassa_fonts::{FontProvider, FontconfigProvider};
use rassa_layout::{LayoutEngine, LayoutEvent, LayoutGlyphRun};
use rassa_parse::{
    ParsedDrawing, ParsedEvent, ParsedFade, ParsedKaraokeMode, ParsedMovement, ParsedSpanStyle,
    ParsedTrack, ParsedVectorClip,
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

fn layout_line_height(config: &RendererConfig, scale_y: f64) -> i32 {
    let scale_y = style_scale(scale_y);
    let extra_spacing = if config.line_spacing.is_finite() {
        (config.line_spacing * scale_y).round() as i32
    } else {
        0
    };
    ((f64::from(LINE_HEIGHT) * scale_y).round() as i32 + extra_spacing).max(1)
}

fn layout_line_height_for_line(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    let scale_y = style_scale(scale_y);
    let max_font_size = line
        .runs
        .iter()
        .map(|run| run.style.font_size)
        .filter(|size| size.is_finite() && *size > 0.0)
        .fold(0.0_f64, f64::max);
    let font_metric_height = (max_font_size * scale_y * 0.52).round() as i32;
    layout_line_height(config, scale_y).max(font_metric_height)
}

fn renderer_blur_radius(blur: f64) -> u32 {
    if !(blur.is_finite() && blur > 0.0) {
        return 0;
    }
    (blur * 4.0).ceil().max(1.0) as u32
}

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
        let render_scale =
            ((style_scale(render_scale_x) + style_scale(render_scale_y)) / 2.0).max(1.0);

        for event in &prepared.active_events {
            let Some(_style) = track.styles.get(event.style_index) else {
                continue;
            };
            let mut shadow_planes = Vec::new();
            let mut outline_planes = Vec::new();
            let mut character_planes = Vec::new();
            let effective_position = scale_position(
                resolve_event_position(track, event, now_ms),
                render_scale_x,
                render_scale_y,
            );
            let layer = event_layer(track, event);
            let occupied_bounds = occupied_bounds_by_layer.entry(layer).or_default();
            let vertical_layout = resolve_vertical_layout(
                track,
                event,
                effective_position,
                occupied_bounds,
                config,
                render_scale_y,
            );
            let occupied_bound = effective_position.is_none().then(|| {
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
            for (line, line_top) in event.lines.iter().zip(vertical_layout.iter().copied()) {
                let scaled_line_width = (f64::from(line.width) * render_scale_x).round() as i32;
                let origin_x = compute_horizontal_origin(
                    track,
                    event,
                    scaled_line_width,
                    effective_position,
                    render_scale_x,
                );
                let mut line_pen_x = 0;
                for run in &line.runs {
                    let effective_style = apply_renderer_style_scale(
                        resolve_run_style(run, track.events.get(event.event_index), now_ms),
                        track,
                        config,
                        render_scale,
                    );
                    if let Some(drawing) = &run.drawing {
                        if let Some(plane) = image_plane_from_drawing(
                            drawing,
                            origin_x + line_pen_x,
                            line_top,
                            resolve_run_fill_color(
                                run,
                                &effective_style,
                                track.events.get(event.event_index),
                                now_ms,
                            ),
                            effective_style.scale_x,
                            effective_style.scale_y,
                        ) {
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
                        line_pen_x += run.width.round() as i32;
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
                    let raster_glyphs = scale_raster_glyphs(
                        raster_glyphs,
                        effective_style.scale_x,
                        effective_style.scale_y,
                    );
                    let raster_glyphs = apply_text_spacing(raster_glyphs, &effective_style);
                    if effective_style.border > 0.0
                        && !karaoke_hides_outline(run, track.events.get(event.event_index), now_ms)
                    {
                        let mut outline_glyphs = rasterizer.outline_glyphs(
                            &raster_glyphs,
                            effective_style.border.round().max(1.0) as i32,
                        );
                        if effective_style.blur > 0.0 {
                            outline_glyphs = rasterizer.blur_glyphs(
                                &outline_glyphs,
                                renderer_blur_radius(effective_style.blur),
                            );
                        }
                        outline_planes.extend(image_planes_from_glyphs_with_kind(
                            &outline_glyphs,
                            origin_x + line_pen_x,
                            line_top,
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                        ));
                    }
                    let fill_color = resolve_run_fill_color(
                        run,
                        &effective_style,
                        track.events.get(event.event_index),
                        now_ms,
                    );
                    if run.karaoke.is_none() && effective_style.blur > 0.0 {
                        if let Some(plane) = combined_image_plane_from_glyphs(
                            &raster_glyphs,
                            origin_x + line_pen_x,
                            line_top,
                            fill_color,
                            ass::ImageType::Character,
                            renderer_blur_radius(effective_style.blur),
                        ) {
                            character_planes.push(plane);
                        }
                    } else {
                        let fill_glyphs = if effective_style.blur > 0.0 {
                            rasterizer.blur_glyphs(
                                &raster_glyphs,
                                renderer_blur_radius(effective_style.blur),
                            )
                        } else {
                            raster_glyphs.clone()
                        };
                        let fill_planes = image_planes_from_glyphs(
                            &fill_glyphs,
                            origin_x + line_pen_x,
                            line_top,
                            fill_color,
                        );
                        if run.karaoke.is_some() {
                            character_planes.extend(apply_karaoke_to_character_planes(
                                fill_planes,
                                run,
                                &effective_style,
                                track.events.get(event.event_index),
                                now_ms,
                                origin_x + line_pen_x,
                                raster_glyphs
                                    .iter()
                                    .map(|glyph| glyph.advance_x)
                                    .sum::<i32>(),
                            ));
                        } else {
                            character_planes.extend(fill_planes);
                        }
                    }
                    if effective_style.shadow > 0.0 {
                        let mut shadow_glyphs = raster_glyphs.clone();
                        if effective_style.blur > 0.0 {
                            shadow_glyphs = rasterizer.blur_glyphs(
                                &shadow_glyphs,
                                renderer_blur_radius(effective_style.blur),
                            );
                        }
                        shadow_planes.extend(image_planes_from_glyphs_with_kind(
                            &shadow_glyphs,
                            origin_x + line_pen_x + effective_style.shadow.round() as i32,
                            line_top + effective_style.shadow.round() as i32,
                            effective_style.back_colour,
                            ass::ImageType::Shadow,
                        ));
                    }
                    line_pen_x += raster_glyphs
                        .iter()
                        .map(|glyph| glyph.advance_x)
                        .sum::<i32>();
                }
            }

            let mut event_planes = shadow_planes;
            event_planes.extend(outline_planes);
            event_planes.extend(character_planes);
            if let Some(rotation_z) =
                event_rotation_z(event, track.events.get(event.event_index), now_ms)
            {
                event_planes = rotate_event_planes(event_planes, rotation_z);
            }
            if let Some(clip_rect) = event.clip_rect {
                event_planes = apply_event_clip(event_planes, clip_rect, event.inverse_clip);
            } else if let Some(vector_clip) = &event.vector_clip {
                event_planes = apply_vector_clip(event_planes, vector_clip, event.inverse_clip);
            }
            if let Some(fade) = event.fade {
                event_planes = apply_fade_to_planes(
                    event_planes,
                    fade,
                    track.events.get(event.event_index),
                    now_ms,
                );
            }
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

fn apply_fade_to_planes(
    planes: Vec<ImagePlane>,
    fade: ParsedFade,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Vec<ImagePlane> {
    let fade_alpha = compute_fad_alpha(fade, source_event, now_ms);
    planes
        .into_iter()
        .map(|mut plane| {
            plane.color = RgbaColor(with_fade_alpha(plane.color.0, fade_alpha));
            plane
        })
        .collect()
}

fn resolve_run_fill_color(
    run: &LayoutGlyphRun,
    style: &ParsedSpanStyle,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> u32 {
    let Some(karaoke) = run.karaoke else {
        return style.primary_colour;
    };
    let Some(event) = source_event else {
        return style.primary_colour;
    };
    let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0)) as i32;
    if elapsed >= karaoke.start_ms + karaoke.duration_ms {
        style.primary_colour
    } else {
        style.secondary_colour
    }
}

fn karaoke_hides_outline(
    run: &LayoutGlyphRun,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> bool {
    let Some(karaoke) = run.karaoke else {
        return false;
    };
    if karaoke.mode != ParsedKaraokeMode::OutlineToggle {
        return false;
    }
    let Some(event) = source_event else {
        return false;
    };
    let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0)) as i32;
    elapsed < karaoke.start_ms + karaoke.duration_ms
}

fn apply_karaoke_to_character_planes(
    planes: Vec<ImagePlane>,
    run: &LayoutGlyphRun,
    style: &ParsedSpanStyle,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    run_origin_x: i32,
    run_width: i32,
) -> Vec<ImagePlane> {
    let Some(karaoke) = run.karaoke else {
        return planes;
    };
    let Some(event) = source_event else {
        return planes;
    };
    let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0)) as i32;
    let relative = elapsed - karaoke.start_ms;
    match karaoke.mode {
        ParsedKaraokeMode::FillSwap | ParsedKaraokeMode::OutlineToggle => planes
            .into_iter()
            .map(|mut plane| {
                plane.color = rgba_color_from_ass(if relative >= karaoke.duration_ms {
                    style.primary_colour
                } else {
                    style.secondary_colour
                });
                plane
            })
            .collect(),
        ParsedKaraokeMode::Sweep => {
            if relative <= 0 {
                return planes
                    .into_iter()
                    .map(|mut plane| {
                        plane.color = rgba_color_from_ass(style.secondary_colour);
                        plane
                    })
                    .collect();
            }
            if relative >= karaoke.duration_ms {
                return planes
                    .into_iter()
                    .map(|mut plane| {
                        plane.color = rgba_color_from_ass(style.primary_colour);
                        plane
                    })
                    .collect();
            }

            let progress = f64::from(relative) / f64::from(karaoke.duration_ms.max(1));
            let split_x = run_origin_x + (f64::from(run_width.max(0)) * progress).round() as i32;
            let mut result = Vec::new();
            for plane in planes {
                if let Some(mut left) =
                    clip_plane_horizontally(&plane, plane.destination.x, split_x)
                {
                    left.color = rgba_color_from_ass(style.primary_colour);
                    result.push(left);
                }
                if let Some(mut right) =
                    clip_plane_horizontally(&plane, split_x, plane.destination.x + plane.size.width)
                {
                    right.color = rgba_color_from_ass(style.secondary_colour);
                    result.push(right);
                }
            }
            result
        }
    }
}

fn clip_plane_horizontally(
    plane: &ImagePlane,
    clip_left: i32,
    clip_right: i32,
) -> Option<ImagePlane> {
    let plane_left = plane.destination.x;
    let plane_right = plane.destination.x + plane.size.width;
    let left = clip_left.max(plane_left);
    let right = clip_right.min(plane_right);
    if right <= left || plane.size.width <= 0 || plane.size.height <= 0 {
        return None;
    }

    let start_column = (left - plane_left) as usize;
    let end_column = (right - plane_left) as usize;
    let new_width = (right - left) as usize;
    let mut bitmap = vec![0_u8; new_width * plane.size.height as usize];

    for row in 0..plane.size.height as usize {
        let source_row = row * plane.stride as usize;
        let target_row = row * new_width;
        bitmap[target_row..target_row + new_width]
            .copy_from_slice(&plane.bitmap[source_row + start_column..source_row + end_column]);
    }

    Some(ImagePlane {
        size: Size {
            width: new_width as i32,
            height: plane.size.height,
        },
        stride: new_width as i32,
        color: plane.color,
        destination: Point {
            x: left,
            y: plane.destination.y,
        },
        kind: plane.kind,
        bitmap,
    })
}

fn resolve_run_style(
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
            linear.powf(if transform.accel > 0.0 {
                transform.accel
            } else {
                1.0
            })
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
        if let Some(rotation_z) = transform.style.rotation_z {
            style.rotation_z = interpolate_f64(style.rotation_z, rotation_z, progress);
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
        }
        if let Some(blur) = transform.style.blur {
            style.blur = interpolate_f64(style.blur, blur, progress);
        }
        if let Some(shadow) = transform.style.shadow {
            style.shadow = interpolate_f64(style.shadow, shadow, progress);
        }
    }

    style
}

fn apply_renderer_style_scale(
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
        style.shadow *= scale;
        style.blur *= scale;
    }

    if !track.scaled_border_and_shadow {
        let geometry_scale = border_shadow_compensation_scale(track, config);
        if geometry_scale > 0.0 && (geometry_scale - 1.0).abs() >= f64::EPSILON {
            style.border /= geometry_scale;
            style.shadow /= geometry_scale;
            style.blur /= geometry_scale;
        }
    }
    style
}

fn apply_text_spacing(glyphs: Vec<RasterGlyph>, style: &ParsedSpanStyle) -> Vec<RasterGlyph> {
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

fn text_spacing_advance(style: &ParsedSpanStyle) -> i32 {
    if !style.spacing.is_finite() {
        return 0;
    }
    (style.spacing * style_scale(style.scale_x)).round() as i32
}

fn renderer_font_scale(config: &RendererConfig) -> f64 {
    if config.font_scale.is_finite() && config.font_scale > 0.0 {
        config.font_scale
    } else {
        1.0
    }
}

fn border_shadow_compensation_scale(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    let scale_x = output_scale_x(track, config).abs();
    let scale_y = output_scale_y(track, config).abs();
    let scale = (scale_x + scale_y) / 2.0;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn scale_glyph_infos(glyphs: &[GlyphInfo], scale_x: f64, scale_y: f64) -> Vec<GlyphInfo> {
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

fn scale_raster_glyphs(glyphs: Vec<RasterGlyph>, scale_x: f64, scale_y: f64) -> Vec<RasterGlyph> {
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

fn style_scale(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

fn scale_raster_glyph(glyph: RasterGlyph, scale_x: f64, scale_y: f64) -> RasterGlyph {
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

fn interpolate_f64(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress.clamp(0.0, 1.0)
}

fn interpolate_color(from: u32, to: u32, progress: f64) -> u32 {
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

fn compute_fad_alpha(fade: ParsedFade, source_event: Option<&ParsedEvent>, now_ms: i64) -> u8 {
    let Some(event) = source_event else {
        return 0;
    };
    let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0));
    let duration = event.duration.max(0);

    match fade {
        ParsedFade::Simple {
            fade_in_ms,
            fade_out_ms,
        } => {
            if fade_in_ms > 0 && elapsed < i64::from(fade_in_ms) {
                return (255 - ((elapsed * 255) / i64::from(fade_in_ms.max(1)))) as u8;
            }
            if fade_out_ms > 0 && elapsed > duration - i64::from(fade_out_ms) {
                let fade_out_start = duration - i64::from(fade_out_ms);
                let fade_elapsed = (elapsed - fade_out_start).max(0);
                return ((fade_elapsed * 255) / i64::from(fade_out_ms.max(1))) as u8;
            }
            0
        }
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
                t4_ms = duration as i32;
                t3_ms = t4_ms.saturating_sub(t3_ms);
            }
            interpolate_alpha(elapsed, t1_ms, t2_ms, t3_ms, t4_ms, alpha1, alpha2, alpha3)
                .clamp(0, 255) as u8
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn interpolate_alpha(
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
        let cf = (now - i64::from(t1)) as f64 / i64::from((t2 - t1).max(1)) as f64;
        (f64::from(a1) * (1.0 - cf) + f64::from(a2) * cf).round() as i32
    } else if now < i64::from(t3) {
        a2
    } else if now < i64::from(t4) {
        let cf = (now - i64::from(t3)) as f64 / i64::from((t4 - t3).max(1)) as f64;
        (f64::from(a2) * (1.0 - cf) + f64::from(a3) * cf).round() as i32
    } else {
        a3
    }
}

fn with_fade_alpha(color: u32, fade_alpha: u8) -> u32 {
    // ASS_Image/RgbaColor stores RGB in the high three bytes and inverse alpha
    // in the low byte. Fade tags compute an absolute event-level alpha, so
    // apply it directly while preserving RGB channels.
    (color & 0xFFFF_FF00) | u32::from(fade_alpha)
}

fn ass_color_to_rgba(color: u32) -> u32 {
    let alpha = (color >> 24) & 0xff;
    let blue = (color >> 16) & 0xff;
    let green = (color >> 8) & 0xff;
    let red = color & 0xff;
    (red << 24) | (green << 16) | (blue << 8) | alpha
}

fn rgba_color_from_ass(color: u32) -> RgbaColor {
    RgbaColor(ass_color_to_rgba(color))
}

fn event_rotation_z(
    event: &LayoutEvent,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Option<f64> {
    event
        .lines
        .iter()
        .flat_map(|line| line.runs.iter())
        .map(|run| resolve_run_style(run, source_event, now_ms).rotation_z)
        .find(|rotation| rotation.is_finite() && rotation.abs() >= f64::EPSILON)
}

fn rotate_event_planes(planes: Vec<ImagePlane>, rotation_degrees: f64) -> Vec<ImagePlane> {
    if planes.is_empty() || !rotation_degrees.is_finite() || rotation_degrees.abs() < f64::EPSILON {
        return planes;
    }

    let Some(bounds) = planes_bounds(&planes) else {
        return planes;
    };
    let center_x = f64::from(bounds.x_min + bounds.x_max) / 2.0;
    let center_y = f64::from(bounds.y_min + bounds.y_max) / 2.0;
    planes
        .into_iter()
        .filter_map(|plane| rotate_plane(plane, rotation_degrees, center_x, center_y))
        .collect()
}

fn planes_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    let mut iter = planes
        .iter()
        .filter(|plane| plane.size.width > 0 && plane.size.height > 0);
    let first = iter.next()?;
    let mut bounds = Rect {
        x_min: first.destination.x,
        y_min: first.destination.y,
        x_max: first.destination.x + first.size.width,
        y_max: first.destination.y + first.size.height,
    };
    for plane in iter {
        bounds.x_min = bounds.x_min.min(plane.destination.x);
        bounds.y_min = bounds.y_min.min(plane.destination.y);
        bounds.x_max = bounds.x_max.max(plane.destination.x + plane.size.width);
        bounds.y_max = bounds.y_max.max(plane.destination.y + plane.size.height);
    }
    Some(bounds)
}

fn rotate_plane(
    plane: ImagePlane,
    rotation_degrees: f64,
    center_x: f64,
    center_y: f64,
) -> Option<ImagePlane> {
    if plane.size.width <= 0 || plane.size.height <= 0 || plane.bitmap.is_empty() {
        return Some(plane);
    }

    let radians = rotation_degrees.to_radians();
    let sin = radians.sin();
    let cos = radians.cos();
    let corners = [
        (
            f64::from(plane.destination.x),
            f64::from(plane.destination.y),
        ),
        (
            f64::from(plane.destination.x + plane.size.width),
            f64::from(plane.destination.y),
        ),
        (
            f64::from(plane.destination.x),
            f64::from(plane.destination.y + plane.size.height),
        ),
        (
            f64::from(plane.destination.x + plane.size.width),
            f64::from(plane.destination.y + plane.size.height),
        ),
    ];
    let rotated = corners.map(|(x, y)| rotate_point(x, y, center_x, center_y, sin, cos));
    let min_x = rotated
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::INFINITY, f64::min)
        .floor() as i32;
    let min_y = rotated
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min)
        .floor() as i32;
    let max_x = rotated
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as i32;
    let max_y = rotated
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as i32;
    let width = (max_x - min_x).max(1) as usize;
    let height = (max_y - min_y).max(1) as usize;
    let mut bitmap = vec![0_u8; width * height];
    let src_stride = plane.stride.max(0) as usize;
    let src_width = plane.size.width as usize;
    let src_height = plane.size.height as usize;
    let inv_sin = -sin;

    for row in 0..height {
        for column in 0..width {
            let dest_x = f64::from(min_x) + column as f64 + 0.5;
            let dest_y = f64::from(min_y) + row as f64 + 0.5;
            let (src_global_x, src_global_y) =
                rotate_point(dest_x, dest_y, center_x, center_y, inv_sin, cos);
            let src_x = (src_global_x - f64::from(plane.destination.x)).floor() as i32;
            let src_y = (src_global_y - f64::from(plane.destination.y)).floor() as i32;
            if src_x >= 0
                && src_y >= 0
                && (src_x as usize) < src_width
                && (src_y as usize) < src_height
            {
                let value = plane.bitmap[src_y as usize * src_stride + src_x as usize];
                bitmap[row * width + column] = value;
            }
        }
    }

    bitmap.iter().any(|value| *value > 0).then_some(ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        destination: Point { x: min_x, y: min_y },
        bitmap,
        ..plane
    })
}

fn rotate_point(x: f64, y: f64, center_x: f64, center_y: f64, sin: f64, cos: f64) -> (f64, f64) {
    let dx = x - center_x;
    let dy = y - center_y;
    (
        center_x + dx * cos - dy * sin,
        center_y + dx * sin + dy * cos,
    )
}

fn default_renderer_config(track: &ParsedTrack) -> RendererConfig {
    RendererConfig {
        frame: Size {
            width: track.play_res_x,
            height: track.play_res_y,
        },
        ..RendererConfig::default()
    }
}

fn output_scale_x(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    let frame_width = output_mapping_size(track, config).width;
    let base_width = track.play_res_x.max(1);
    let aspect = effective_pixel_aspect(track, config);

    f64::from(frame_width.max(1)) / f64::from(base_width) * aspect
}

fn output_scale_y(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    let frame_height = output_mapping_size(track, config).height;
    let base_height = track.play_res_y.max(1);

    f64::from(frame_height.max(1)) / f64::from(base_height)
}

fn effective_pixel_aspect(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    if layout_resolution(track).is_some()
        || !(config.pixel_aspect.is_finite() && config.pixel_aspect > 0.0)
    {
        return derived_pixel_aspect(track, config).unwrap_or(1.0);
    }

    config.pixel_aspect
}

fn derived_pixel_aspect(track: &ParsedTrack, config: &RendererConfig) -> Option<f64> {
    let layout = layout_resolution(track).or_else(|| storage_resolution(config))?;
    let frame = frame_content_size(track, config);
    if frame.width <= 0 || frame.height <= 0 || layout.width <= 0 || layout.height <= 0 {
        return None;
    }

    let display_aspect = f64::from(frame.width) / f64::from(frame.height);
    let source_aspect = f64::from(layout.width) / f64::from(layout.height);
    (source_aspect > 0.0).then_some(display_aspect / source_aspect)
}

fn layout_resolution(track: &ParsedTrack) -> Option<Size> {
    (track.layout_res_x > 0 && track.layout_res_y > 0).then_some(Size {
        width: track.layout_res_x,
        height: track.layout_res_y,
    })
}

fn storage_resolution(config: &RendererConfig) -> Option<Size> {
    (config.storage.width > 0 && config.storage.height > 0).then_some(config.storage)
}

fn frame_content_size(track: &ParsedTrack, config: &RendererConfig) -> Size {
    let frame_width = if config.frame.width > 0 {
        config.frame.width
    } else {
        track.play_res_x
    };
    let frame_height = if config.frame.height > 0 {
        config.frame.height
    } else {
        track.play_res_y
    };

    Size {
        width: (frame_width - config.margins.left - config.margins.right).max(0),
        height: (frame_height - config.margins.top - config.margins.bottom).max(0),
    }
}

fn output_mapping_size(track: &ParsedTrack, config: &RendererConfig) -> Size {
    if config.use_margins {
        Size {
            width: if config.frame.width > 0 {
                config.frame.width
            } else {
                track.play_res_x
            },
            height: if config.frame.height > 0 {
                config.frame.height
            } else {
                track.play_res_y
            },
        }
    } else {
        frame_content_size(track, config)
    }
}

fn output_offset(config: &RendererConfig) -> Point {
    if config.use_margins {
        Point { x: 0, y: 0 }
    } else {
        Point {
            x: config.margins.left.max(0),
            y: config.margins.top.max(0),
        }
    }
}

fn translate_planes(mut planes: Vec<ImagePlane>, offset: Point) -> Vec<ImagePlane> {
    if offset == Point::default() {
        return planes;
    }
    for plane in &mut planes {
        plane.destination.x += offset.x;
        plane.destination.y += offset.y;
    }
    planes
}

fn frame_clip_rect(
    track: &ParsedTrack,
    config: &RendererConfig,
    event: &LayoutEvent,
    effective_position: Option<(i32, i32)>,
) -> Rect {
    let frame_width = if config.frame.width > 0 {
        config.frame.width
    } else {
        track.play_res_x.max(0)
    };
    let frame_height = if config.frame.height > 0 {
        config.frame.height
    } else {
        track.play_res_y.max(0)
    };
    if config.use_margins
        && effective_position.is_none()
        && event.clip_rect.is_none()
        && event.vector_clip.is_none()
    {
        Rect {
            x_min: config.margins.left.max(0),
            y_min: config.margins.top.max(0),
            x_max: (frame_width - config.margins.right).max(0),
            y_max: (frame_height - config.margins.bottom).max(0),
        }
    } else {
        Rect {
            x_min: 0,
            y_min: 0,
            x_max: frame_width,
            y_max: frame_height,
        }
    }
}

fn compute_horizontal_origin(
    track: &ParsedTrack,
    event: &LayoutEvent,
    line_width: i32,
    effective_position: Option<(i32, i32)>,
    scale_x: f64,
) -> i32 {
    let scale_x = style_scale(scale_x);
    if let Some((x, _)) = effective_position {
        return x;
    }
    let frame_width = (f64::from(track.play_res_x) * scale_x).round() as i32;
    let margin_l = (f64::from(event.margin_l) * scale_x).round() as i32;
    let margin_r = (f64::from(event.margin_r) * scale_x).round() as i32;
    match event.alignment & 0x3 {
        ass::HALIGN_LEFT => margin_l,
        ass::HALIGN_RIGHT => (frame_width - margin_r - line_width).max(0),
        _ => ((frame_width - line_width) / 2).max(0),
    }
}

fn scale_position(position: Option<(i32, i32)>, scale_x: f64, scale_y: f64) -> Option<(i32, i32)> {
    let scale_x = style_scale(scale_x);
    let scale_y = style_scale(scale_y);
    position.map(|(x, y)| {
        (
            (f64::from(x) * scale_x).round() as i32,
            (f64::from(y) * scale_y).round() as i32,
        )
    })
}

fn resolve_event_position(
    track: &ParsedTrack,
    event: &LayoutEvent,
    now_ms: i64,
) -> Option<(i32, i32)> {
    event.position.or_else(|| {
        event
            .movement
            .map(|movement| interpolate_move(movement, track.events.get(event.event_index), now_ms))
    })
}

fn event_layer(track: &ParsedTrack, event: &LayoutEvent) -> i32 {
    track
        .events
        .get(event.event_index)
        .map(|source| source.layer)
        .unwrap_or_default()
}

fn interpolate_move(
    movement: ParsedMovement,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> (i32, i32) {
    let event_duration = source_event
        .map(|event| event.duration)
        .unwrap_or_default()
        .max(0) as i32;
    let event_elapsed = source_event
        .map(|event| (now_ms - event.start).clamp(0, event.duration.max(0)) as i32)
        .unwrap_or_default();

    let (t1_ms, t2_ms) = if movement.t1_ms <= 0 && movement.t2_ms <= 0 {
        (0, event_duration)
    } else {
        (movement.t1_ms.max(0), movement.t2_ms.max(movement.t1_ms))
    };
    let k = if event_elapsed <= t1_ms {
        0.0
    } else if event_elapsed >= t2_ms {
        1.0
    } else {
        let delta = (t2_ms - t1_ms).max(1) as f64;
        f64::from(event_elapsed - t1_ms) / delta
    };

    let x = f64::from(movement.end.0 - movement.start.0) * k + f64::from(movement.start.0);
    let y = f64::from(movement.end.1 - movement.start.1) * k + f64::from(movement.start.1);
    (x.round() as i32, y.round() as i32)
}

fn compute_vertical_layout(
    track: &ParsedTrack,
    lines: &[rassa_layout::LayoutLine],
    alignment: i32,
    margin_v: i32,
    position: Option<(i32, i32)>,
    config: &RendererConfig,
    scale_y: f64,
) -> Vec<i32> {
    let scale_y = style_scale(scale_y);
    if let Some((_, y)) = position {
        let line_height = layout_line_height(config, scale_y);
        let mut positions = Vec::with_capacity(lines.len());
        let mut current_y = y;
        for _ in lines {
            positions.push(current_y);
            current_y += line_height;
        }
        return positions;
    }
    let line_heights = lines
        .iter()
        .map(|line| layout_line_height_for_line(line, config, scale_y))
        .collect::<Vec<_>>();
    let total_height: i32 = line_heights.iter().sum();
    let default_start_y = match alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
        ass::VALIGN_TOP => (f64::from(margin_v) * scale_y).round() as i32,
        ass::VALIGN_CENTER => {
            ((f64::from(track.play_res_y) * scale_y).round() as i32 - total_height) / 2
        }
        _ => ((f64::from(track.play_res_y) * scale_y).round() as i32
            - (f64::from(margin_v) * scale_y).round() as i32
            - total_height)
            .max(0),
    };

    let line_position = config.line_position.clamp(0.0, 100.0);
    let start_y = if (alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER)) == ass::VALIGN_SUB
        && line_position > 0.0
    {
        let bottom_y = f64::from(default_start_y);
        let top_y = 0.0;
        (bottom_y + (top_y - bottom_y) * (line_position / 100.0)).round() as i32
    } else {
        default_start_y
    }
    .max(0);

    let mut positions = Vec::with_capacity(lines.len());
    let mut current_y = start_y;
    for height in line_heights {
        positions.push(current_y);
        current_y += height;
    }
    positions
}

fn resolve_vertical_layout(
    track: &ParsedTrack,
    event: &LayoutEvent,
    effective_position: Option<(i32, i32)>,
    occupied_bounds: &[Rect],
    config: &RendererConfig,
    scale_y: f64,
) -> Vec<i32> {
    let mut vertical_layout = compute_vertical_layout(
        track,
        &event.lines,
        event.alignment,
        event.margin_v,
        effective_position,
        config,
        scale_y,
    );
    if effective_position.is_some() || occupied_bounds.is_empty() {
        return vertical_layout;
    }

    let line_height = layout_line_height(config, scale_y);
    let shift = match event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
        ass::VALIGN_TOP => line_height,
        ass::VALIGN_CENTER => line_height,
        _ => -line_height,
    };

    let mut bounds = event_bounds(
        track,
        event,
        &vertical_layout,
        effective_position,
        config,
        1.0,
        scale_y,
    );
    let frame_height = (f64::from(track.play_res_y) * scale_y).round() as i32;
    while occupied_bounds
        .iter()
        .any(|occupied| bounds.intersect(*occupied).is_some())
    {
        for line_top in &mut vertical_layout {
            *line_top += shift;
        }
        bounds = event_bounds(
            track,
            event,
            &vertical_layout,
            effective_position,
            config,
            1.0,
            scale_y,
        );
        if bounds.y_min < 0 || bounds.y_max > frame_height {
            break;
        }
    }

    vertical_layout
}

fn event_bounds(
    track: &ParsedTrack,
    event: &LayoutEvent,
    vertical_layout: &[i32],
    effective_position: Option<(i32, i32)>,
    config: &RendererConfig,
    scale_x: f64,
    scale_y: f64,
) -> Rect {
    let mut x_min = i32::MAX;
    let mut y_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut y_max = i32::MIN;

    for (line, line_top) in event.lines.iter().zip(vertical_layout.iter().copied()) {
        let line_width = (f64::from(line.width) * style_scale(scale_x)).round() as i32;
        let origin_x =
            compute_horizontal_origin(track, event, line_width, effective_position, scale_x);
        x_min = x_min.min(origin_x);
        y_min = y_min.min(line_top);
        x_max = x_max.max(origin_x + line_width);
        y_max = y_max.max(line_top + layout_line_height(config, scale_y));
    }

    if x_min == i32::MAX {
        Rect::default()
    } else {
        Rect {
            x_min,
            y_min,
            x_max,
            y_max,
        }
    }
}

fn image_planes_from_glyphs(
    glyphs: &[RasterGlyph],
    origin_x: i32,
    line_top: i32,
    color: u32,
) -> Vec<ImagePlane> {
    image_planes_from_glyphs_with_kind(glyphs, origin_x, line_top, color, ass::ImageType::Character)
}

fn image_planes_from_glyphs_with_kind(
    glyphs: &[RasterGlyph],
    origin_x: i32,
    line_top: i32,
    color: u32,
    kind: ass::ImageType,
) -> Vec<ImagePlane> {
    let ascender = glyphs.iter().map(|glyph| glyph.top).max().unwrap_or(0);
    let mut pen_x = 0;
    let mut planes = Vec::new();

    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x;
            continue;
        }

        let baseline_y = line_top + ascender;
        planes.push(ImagePlane {
            size: Size {
                width: glyph.width,
                height: glyph.height,
            },
            stride: glyph.stride,
            color: rgba_color_from_ass(color),
            destination: Point {
                x: origin_x + pen_x + glyph.left + glyph.offset_x,
                y: baseline_y - glyph.top + glyph.offset_y,
            },
            kind,
            bitmap: glyph.bitmap.clone(),
        });
        pen_x += glyph.advance_x;
    }

    planes
}

fn combined_image_plane_from_glyphs(
    glyphs: &[RasterGlyph],
    origin_x: i32,
    line_top: i32,
    color: u32,
    kind: ass::ImageType,
    blur_radius: u32,
) -> Option<ImagePlane> {
    let ascender = glyphs.iter().map(|glyph| glyph.top).max().unwrap_or(0);
    let mut pen_x = 0_i32;
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x;
            continue;
        }
        let x = pen_x + glyph.left + glyph.offset_x;
        let y = ascender - glyph.top + glyph.offset_y;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x + glyph.width);
        max_y = max_y.max(y + glyph.height);
        pen_x += glyph.advance_x;
    }

    if min_x == i32::MAX || min_y == i32::MAX || max_x <= min_x || max_y <= min_y {
        return None;
    }

    let width = (max_x - min_x) as usize;
    let height = (max_y - min_y) as usize;
    let mut bitmap = vec![0_u8; width * height];
    pen_x = 0;
    for glyph in glyphs {
        if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
            pen_x += glyph.advance_x;
            continue;
        }
        let x0 = (pen_x + glyph.left + glyph.offset_x - min_x) as usize;
        let y0 = (ascender - glyph.top + glyph.offset_y - min_y) as usize;
        let glyph_width = glyph.width as usize;
        let glyph_height = glyph.height as usize;
        let glyph_stride = glyph.stride as usize;
        for y in 0..glyph_height {
            for x in 0..glyph_width {
                let src = glyph.bitmap[y * glyph_stride + x];
                let dst = &mut bitmap[(y0 + y) * width + x0 + x];
                *dst = (*dst).max(src);
            }
        }
        pen_x += glyph.advance_x;
    }

    let (bitmap, width, height, pad) = blur_bitmap(bitmap, width, height, blur_radius);
    Some(ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: origin_x + min_x - pad as i32,
            y: line_top + min_y - pad as i32,
        },
        kind,
        bitmap,
    })
}

fn blur_bitmap(
    source: Vec<u8>,
    width: usize,
    height: usize,
    radius: u32,
) -> (Vec<u8>, usize, usize, usize) {
    if radius == 0 || width == 0 || height == 0 || source.is_empty() {
        return (source, width, height, 0);
    }
    let r2 = libass_blur_r2_from_radius(radius);
    let (bitmap, width, height, pad_x, pad_y) =
        libass_gaussian_blur(&source, width, height, r2, r2);
    debug_assert_eq!(pad_x, pad_y);
    (bitmap, width, height, pad_x)
}

#[derive(Clone)]
struct LibassBlurMethod {
    level: usize,
    radius: usize,
    coeff: [i16; 8],
}

fn libass_blur_r2_from_radius(radius: u32) -> f64 {
    const POSITION_PRECISION: f64 = 8.0;
    const BLUR_PRECISION: f64 = 1.0 / 256.0;
    let blur = f64::from(radius) / 4.0;
    let blur_radius_scale = 2.0 / 256.0_f64.ln().sqrt();
    let scale = 64.0 * BLUR_PRECISION / POSITION_PRECISION;
    let qblur = ((1.0 + blur * blur_radius_scale * scale).ln() / BLUR_PRECISION).round();
    let sigma = (BLUR_PRECISION * qblur).exp_m1() / scale;
    sigma * sigma
}

fn libass_gaussian_blur(
    source: &[u8],
    width: usize,
    height: usize,
    r2x: f64,
    r2y: f64,
) -> (Vec<u8>, usize, usize, usize, usize) {
    let blur_x = find_libass_blur_method(r2x);
    let blur_y = if (r2y - r2x).abs() < f64::EPSILON {
        blur_x.clone()
    } else {
        find_libass_blur_method(r2y)
    };

    let offset_x = ((2 * blur_x.radius + 9) << blur_x.level) - 5;
    let offset_y = ((2 * blur_y.radius + 9) << blur_y.level) - 5;
    let mask_x = (1_usize << blur_x.level) - 1;
    let mask_y = (1_usize << blur_y.level) - 1;
    let end_width = ((width + offset_x) & !mask_x).saturating_sub(4);
    let end_height = ((height + offset_y) & !mask_y).saturating_sub(4);
    let pad_x = ((blur_x.radius + 4) << blur_x.level) - 4;
    let pad_y = ((blur_y.radius + 4) << blur_y.level) - 4;

    let mut buffer = unpack_libass_blur(source);
    let mut w = width;
    let mut h = height;

    for _ in 0..blur_y.level {
        let next = shrink_vert_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }
    for _ in 0..blur_x.level {
        let next = shrink_horz_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }

    let next = blur_horz_libass(&buffer, w, h, &blur_x.coeff, blur_x.radius);
    buffer = next.0;
    w = next.1;
    h = next.2;
    let next = blur_vert_libass(&buffer, w, h, &blur_y.coeff, blur_y.radius);
    buffer = next.0;
    w = next.1;
    h = next.2;

    for _ in 0..blur_x.level {
        let next = expand_horz_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }
    for _ in 0..blur_y.level {
        let next = expand_vert_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }

    debug_assert_eq!(w, end_width);
    debug_assert_eq!(h, end_height);
    (pack_libass_blur(&buffer, w, h), w, h, pad_x, pad_y)
}

fn find_libass_blur_method(r2: f64) -> LibassBlurMethod {
    let mut mu = [0.0_f64; 8];
    let (level, radius) = if r2 < 0.5 {
        mu[1] = 0.085 * r2 * r2 * r2;
        mu[0] = 0.5 * r2 - 4.0 * mu[1];
        (0_usize, 4_usize)
    } else {
        let (frac, level) = frexp((0.11569 * r2 + 0.20591047).sqrt());
        let mul = 0.25_f64.powi(level);
        let radius = (8_i32 - ((10.1525 + 0.8335 * mul) * (1.0 - frac)) as i32).max(4) as usize;
        calc_libass_coeff(&mut mu, radius, r2, mul);
        (level.max(0) as usize, radius)
    };
    let mut coeff = [0_i16; 8];
    for i in 0..radius {
        coeff[i] = (65536.0 * mu[i] + 0.5) as i16;
    }
    LibassBlurMethod {
        level,
        radius,
        coeff,
    }
}

fn calc_libass_coeff(mu: &mut [f64; 8], n: usize, r2: f64, mul: f64) {
    let w = 12096.0;
    let kernel = [
        (((3280.0 / w) * mul + 1092.0 / w) * mul + 2520.0 / w) * mul + 5204.0 / w,
        (((-2460.0 / w) * mul - 273.0 / w) * mul - 210.0 / w) * mul + 2943.0 / w,
        (((984.0 / w) * mul - 546.0 / w) * mul - 924.0 / w) * mul + 486.0 / w,
        (((-164.0 / w) * mul + 273.0 / w) * mul - 126.0 / w) * mul + 17.0 / w,
    ];
    let mut mat_freq = [0.0_f64; 17];
    mat_freq[..4].copy_from_slice(&kernel);
    coeff_filter_libass(&mut mat_freq, 7, &kernel);
    let mut vec_freq = [0.0_f64; 12];
    calc_gauss_libass(&mut vec_freq, n + 4, r2 * mul);
    coeff_filter_libass(&mut vec_freq, n + 1, &kernel);
    let mut mat = [[0.0_f64; 8]; 8];
    calc_matrix_libass(&mut mat, &mat_freq, n);
    let mut vec = [0.0_f64; 8];
    for i in 0..n {
        vec[i] = mat_freq[0] - mat_freq[i + 1] - vec_freq[0] + vec_freq[i + 1];
    }
    for i in 0..n {
        let mut res = 0.0;
        for (j, value) in vec.iter().enumerate().take(n) {
            res += mat[i][j] * value;
        }
        mu[i] = res.max(0.0);
    }
}

fn calc_gauss_libass(res: &mut [f64], n: usize, r2: f64) {
    let alpha = 0.5 / r2;
    let mut mul = (-alpha).exp();
    let mul2 = mul * mul;
    let mut cur = (alpha / std::f64::consts::PI).sqrt();
    res[0] = cur;
    cur *= mul;
    res[1] = cur;
    for value in res.iter_mut().take(n).skip(2) {
        mul *= mul2;
        cur *= mul;
        *value = cur;
    }
}

fn coeff_filter_libass(coeff: &mut [f64], n: usize, kernel: &[f64; 4]) {
    let mut prev1 = coeff[1];
    let mut prev2 = coeff[2];
    let mut prev3 = coeff[3];
    for i in 0..n {
        let res = coeff[i] * kernel[0]
            + (prev1 + coeff[i + 1]) * kernel[1]
            + (prev2 + coeff[i + 2]) * kernel[2]
            + (prev3 + coeff[i + 3]) * kernel[3];
        prev3 = prev2;
        prev2 = prev1;
        prev1 = coeff[i];
        coeff[i] = res;
    }
}

fn calc_matrix_libass(mat: &mut [[f64; 8]; 8], mat_freq: &[f64], n: usize) {
    for i in 0..n {
        mat[i][i] = mat_freq[2 * i + 2] + 3.0 * mat_freq[0] - 4.0 * mat_freq[i + 1];
        for j in i + 1..n {
            let v = mat_freq[i + j + 2]
                + mat_freq[j - i]
                + 2.0 * (mat_freq[0] - mat_freq[i + 1] - mat_freq[j + 1]);
            mat[i][j] = v;
            mat[j][i] = v;
        }
    }
    for k in 0..n {
        let z = 1.0 / mat[k][k];
        mat[k][k] = 1.0;
        let pivot_row = mat[k];
        for (i, row) in mat.iter_mut().enumerate().take(n) {
            if i == k {
                continue;
            }
            let mul = row[k] * z;
            row[k] = 0.0;
            for j in 0..n {
                row[j] -= pivot_row[j] * mul;
            }
        }
        for value in mat[k].iter_mut().take(n) {
            *value *= z;
        }
    }
}

fn frexp(value: f64) -> (f64, i32) {
    if value == 0.0 {
        return (0.0, 0);
    }
    let exponent = value.abs().log2().floor() as i32 + 1;
    (value / 2.0_f64.powi(exponent), exponent)
}

#[inline]
fn get_libass_sample(source: &[i16], width: usize, height: usize, x: isize, y: isize) -> i16 {
    if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
        0
    } else {
        source[y as usize * width + x as usize]
    }
}

fn unpack_libass_blur(source: &[u8]) -> Vec<i16> {
    source
        .iter()
        .map(|value| {
            let value = u16::from(*value);
            ((((value << 7) | (value >> 1)) + 1) >> 1) as i16
        })
        .collect()
}

const LIBASS_DITHER_LINE: [i16; 32] = [
    8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 56, 24, 56, 24, 56, 24, 56, 24, 56, 24,
    56, 24, 56, 24, 56, 24,
];

fn pack_libass_blur(source: &[i16], width: usize, height: usize) -> Vec<u8> {
    let mut bitmap = vec![0_u8; width * height];
    for y in 0..height {
        let dither = &LIBASS_DITHER_LINE[16 * (y & 1)..];
        for x in 0..width {
            let sample = i32::from(source[y * width + x]);
            let value = ((sample - (sample >> 8) + i32::from(dither[x & 15])) >> 6).clamp(0, 255);
            bitmap[y * width + x] = value as u8;
        }
    }
    bitmap
}

#[inline]
fn shrink_func_libass(p1p: i16, p1n: i16, z0p: i16, z0n: i16, n1p: i16, n1n: i16) -> i16 {
    let mut r = (i32::from(p1p) + i32::from(p1n) + i32::from(n1p) + i32::from(n1n)) >> 1;
    r = (r + i32::from(z0p) + i32::from(z0n)) >> 1;
    r = (r + i32::from(p1n) + i32::from(n1p)) >> 1;
    ((r + i32::from(z0p) + i32::from(z0n) + 2) >> 2) as i16
}

#[inline]
fn expand_func_libass(p1: i16, z0: i16, n1: i16) -> (i16, i16) {
    let r = ((((p1 as u16).wrapping_add(n1 as u16)) >> 1).wrapping_add(z0 as u16)) >> 1;
    let rp = (((r.wrapping_add(p1 as u16) >> 1)
        .wrapping_add(z0 as u16)
        .wrapping_add(1))
        >> 1) as i16;
    let rn = (((r.wrapping_add(n1 as u16) >> 1)
        .wrapping_add(z0 as u16)
        .wrapping_add(1))
        >> 1) as i16;
    (rp, rn)
}

fn shrink_horz_libass(source: &[i16], width: usize, height: usize) -> (Vec<i16>, usize, usize) {
    let dst_width = (width + 5) >> 1;
    let mut dst = vec![0_i16; dst_width * height];
    for y in 0..height {
        for x in 0..dst_width {
            let sx = (2 * x) as isize;
            dst[y * dst_width + x] = shrink_func_libass(
                get_libass_sample(source, width, height, sx - 4, y as isize),
                get_libass_sample(source, width, height, sx - 3, y as isize),
                get_libass_sample(source, width, height, sx - 2, y as isize),
                get_libass_sample(source, width, height, sx - 1, y as isize),
                get_libass_sample(source, width, height, sx, y as isize),
                get_libass_sample(source, width, height, sx + 1, y as isize),
            );
        }
    }
    (dst, dst_width, height)
}

fn shrink_vert_libass(source: &[i16], width: usize, height: usize) -> (Vec<i16>, usize, usize) {
    let dst_height = (height + 5) >> 1;
    let mut dst = vec![0_i16; width * dst_height];
    for y in 0..dst_height {
        let sy = (2 * y) as isize;
        for x in 0..width {
            dst[y * width + x] = shrink_func_libass(
                get_libass_sample(source, width, height, x as isize, sy - 4),
                get_libass_sample(source, width, height, x as isize, sy - 3),
                get_libass_sample(source, width, height, x as isize, sy - 2),
                get_libass_sample(source, width, height, x as isize, sy - 1),
                get_libass_sample(source, width, height, x as isize, sy),
                get_libass_sample(source, width, height, x as isize, sy + 1),
            );
        }
    }
    (dst, width, dst_height)
}

fn expand_horz_libass(source: &[i16], width: usize, height: usize) -> (Vec<i16>, usize, usize) {
    let dst_width = 2 * width + 4;
    let mut dst = vec![0_i16; dst_width * height];
    for y in 0..height {
        for i in 0..(width + 2) {
            let sx = i as isize;
            let (rp, rn) = expand_func_libass(
                get_libass_sample(source, width, height, sx - 2, y as isize),
                get_libass_sample(source, width, height, sx - 1, y as isize),
                get_libass_sample(source, width, height, sx, y as isize),
            );
            let dx = 2 * i;
            dst[y * dst_width + dx] = rp;
            dst[y * dst_width + dx + 1] = rn;
        }
    }
    (dst, dst_width, height)
}

fn expand_vert_libass(source: &[i16], width: usize, height: usize) -> (Vec<i16>, usize, usize) {
    let dst_height = 2 * height + 4;
    let mut dst = vec![0_i16; width * dst_height];
    for i in 0..(height + 2) {
        let sy = i as isize;
        for x in 0..width {
            let (rp, rn) = expand_func_libass(
                get_libass_sample(source, width, height, x as isize, sy - 2),
                get_libass_sample(source, width, height, x as isize, sy - 1),
                get_libass_sample(source, width, height, x as isize, sy),
            );
            let dy = 2 * i;
            dst[dy * width + x] = rp;
            dst[(dy + 1) * width + x] = rn;
        }
    }
    (dst, width, dst_height)
}

fn blur_horz_libass(
    source: &[i16],
    width: usize,
    height: usize,
    param: &[i16; 8],
    radius: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_width = width + 2 * radius;
    let mut dst = vec![0_i16; dst_width * height];
    for y in 0..height {
        for x in 0..dst_width {
            let center_x = x as isize - radius as isize;
            let center = i32::from(get_libass_sample(
                source, width, height, center_x, y as isize,
            ));
            let mut acc = 0x8000_i32;
            for i in (1..=radius).rev() {
                let coeff = i32::from(param[i - 1]);
                let left = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    center_x - i as isize,
                    y as isize,
                ));
                let right = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    center_x + i as isize,
                    y as isize,
                ));
                acc += ((left - center) as i16 as i32) * coeff;
                acc += ((right - center) as i16 as i32) * coeff;
            }
            dst[y * dst_width + x] = (center + (acc >> 16)) as i16;
        }
    }
    (dst, dst_width, height)
}

fn blur_vert_libass(
    source: &[i16],
    width: usize,
    height: usize,
    param: &[i16; 8],
    radius: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_height = height + 2 * radius;
    let mut dst = vec![0_i16; width * dst_height];
    for y in 0..dst_height {
        let center_y = y as isize - radius as isize;
        for x in 0..width {
            let center = i32::from(get_libass_sample(
                source, width, height, x as isize, center_y,
            ));
            let mut acc = 0x8000_i32;
            for i in (1..=radius).rev() {
                let coeff = i32::from(param[i - 1]);
                let top = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    x as isize,
                    center_y - i as isize,
                ));
                let bottom = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    x as isize,
                    center_y + i as isize,
                ));
                acc += ((top - center) as i16 as i32) * coeff;
                acc += ((bottom - center) as i16 as i32) * coeff;
            }
            dst[y * width + x] = (center + (acc >> 16)) as i16;
        }
    }
    (dst, width, dst_height)
}

fn image_planes_from_absolute_glyphs(
    glyphs: &[RasterGlyph],
    color: u32,
    kind: ass::ImageType,
) -> Vec<ImagePlane> {
    glyphs
        .iter()
        .filter_map(|glyph| {
            if glyph.width <= 0 || glyph.height <= 0 || glyph.bitmap.is_empty() {
                return None;
            }

            Some(ImagePlane {
                size: Size {
                    width: glyph.width,
                    height: glyph.height,
                },
                stride: glyph.stride,
                color: rgba_color_from_ass(color),
                destination: Point {
                    x: glyph.left,
                    y: glyph.top - glyph.height,
                },
                kind,
                bitmap: glyph.bitmap.clone(),
            })
        })
        .collect()
}

fn image_plane_from_drawing(
    drawing: &ParsedDrawing,
    origin_x: i32,
    line_top: i32,
    color: u32,
    scale_x: f64,
    scale_y: f64,
) -> Option<ImagePlane> {
    let polygons = scaled_drawing_polygons(drawing, scale_x, scale_y);
    let bounds = drawing_bounds(&polygons)?;
    let width = bounds.width();
    let height = bounds.height();
    if width <= 0 || height <= 0 {
        return None;
    }

    let stride = width as usize;
    let mut bitmap = vec![0_u8; stride * height as usize];
    let mut any_visible = false;

    for row in 0..height as usize {
        for column in 0..width as usize {
            let x = bounds.x_min + column as i32;
            let y = bounds.y_min + row as i32;
            if polygons
                .iter()
                .any(|polygon| point_in_polygon(x, y, polygon))
            {
                bitmap[row * stride + column] = 255;
                any_visible = true;
            }
        }
    }

    any_visible.then_some(ImagePlane {
        size: Size { width, height },
        stride: width,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: origin_x + bounds.x_min,
            y: line_top + bounds.y_min,
        },
        kind: ass::ImageType::Character,
        bitmap,
    })
}

fn scaled_drawing_polygons(drawing: &ParsedDrawing, scale_x: f64, scale_y: f64) -> Vec<Vec<Point>> {
    let scale_x = style_scale(scale_x);
    let scale_y = style_scale(scale_y);
    if (scale_x - 1.0).abs() < f64::EPSILON && (scale_y - 1.0).abs() < f64::EPSILON {
        return drawing.polygons.clone();
    }

    drawing
        .polygons
        .iter()
        .map(|polygon| {
            polygon
                .iter()
                .map(|point| Point {
                    x: (f64::from(point.x) * scale_x).round() as i32,
                    y: (f64::from(point.y) * scale_y).round() as i32,
                })
                .collect()
        })
        .collect()
}

fn drawing_bounds(polygons: &[Vec<Point>]) -> Option<Rect> {
    let mut points = polygons.iter().flat_map(|polygon| polygon.iter().copied());
    let first = points.next()?;
    let mut x_min = first.x;
    let mut y_min = first.y;
    let mut x_max = first.x;
    let mut y_max = first.y;
    for point in points {
        x_min = x_min.min(point.x);
        y_min = y_min.min(point.y);
        x_max = x_max.max(point.x);
        y_max = y_max.max(point.y);
    }
    Some(Rect {
        x_min,
        y_min,
        x_max: x_max + 1,
        y_max: y_max + 1,
    })
}

fn plane_to_raster_glyph(plane: &ImagePlane) -> RasterGlyph {
    RasterGlyph {
        width: plane.size.width,
        height: plane.size.height,
        stride: plane.stride,
        left: plane.destination.x,
        top: plane.destination.y + plane.size.height,
        bitmap: plane.bitmap.clone(),
        ..RasterGlyph::default()
    }
}

fn apply_event_clip(planes: Vec<ImagePlane>, clip_rect: Rect, inverse: bool) -> Vec<ImagePlane> {
    let mut clipped = Vec::new();
    for plane in planes {
        if inverse {
            clipped.extend(inverse_clip_plane(plane, clip_rect));
        } else if let Some(plane) = clip_plane(plane, clip_rect) {
            clipped.push(plane);
        }
    }
    clipped
}

fn apply_vector_clip(
    planes: Vec<ImagePlane>,
    clip: &ParsedVectorClip,
    inverse: bool,
) -> Vec<ImagePlane> {
    planes
        .into_iter()
        .filter_map(|plane| mask_plane_with_vector_clip(plane, clip, inverse))
        .collect()
}

fn mask_plane_with_vector_clip(
    plane: ImagePlane,
    clip: &ParsedVectorClip,
    inverse: bool,
) -> Option<ImagePlane> {
    let mut bitmap = plane.bitmap.clone();
    let stride = plane.stride as usize;
    let mut any_visible = false;

    for row in 0..plane.size.height as usize {
        for column in 0..plane.size.width as usize {
            let global_x = plane.destination.x + column as i32;
            let global_y = plane.destination.y + row as i32;
            let inside = clip
                .polygons
                .iter()
                .any(|polygon| point_in_polygon(global_x, global_y, polygon));
            let keep = if inverse { !inside } else { inside };
            if !keep {
                bitmap[row * stride + column] = 0;
            } else if bitmap[row * stride + column] > 0 {
                any_visible = true;
            }
        }
    }

    any_visible.then_some(ImagePlane { bitmap, ..plane })
}

fn point_in_polygon(x: i32, y: i32, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];
    let sample_x = x as f64 + 0.5;
    let sample_y = y as f64 + 0.5;

    for &current in polygon {
        let current_y = current.y as f64;
        let previous_y = previous.y as f64;
        let intersects = (current_y > sample_y) != (previous_y > sample_y);
        if intersects {
            let current_x = current.x as f64;
            let previous_x = previous.x as f64;
            let x_intersection = (previous_x - current_x) * (sample_y - current_y)
                / (previous_y - current_y)
                + current_x;
            if sample_x < x_intersection {
                inside = !inside;
            }
        }
        previous = current;
    }

    inside
}

fn clip_plane(plane: ImagePlane, clip_rect: Rect) -> Option<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let intersection = plane_rect.intersect(clip_rect)?;
    crop_plane_to_rect(plane, intersection)
}

fn inverse_clip_plane(plane: ImagePlane, clip_rect: Rect) -> Vec<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let Some(intersection) = plane_rect.intersect(clip_rect) else {
        return vec![plane];
    };

    let mut result = Vec::new();
    let regions = [
        Rect {
            x_min: plane_rect.x_min,
            y_min: plane_rect.y_min,
            x_max: plane_rect.x_max,
            y_max: intersection.y_min,
        },
        Rect {
            x_min: plane_rect.x_min,
            y_min: intersection.y_max,
            x_max: plane_rect.x_max,
            y_max: plane_rect.y_max,
        },
        Rect {
            x_min: plane_rect.x_min,
            y_min: intersection.y_min,
            x_max: intersection.x_min,
            y_max: intersection.y_max,
        },
        Rect {
            x_min: intersection.x_max,
            y_min: intersection.y_min,
            x_max: plane_rect.x_max,
            y_max: intersection.y_max,
        },
    ];
    for region in regions {
        if region.is_empty() {
            continue;
        }
        if let Some(cropped) = crop_plane_to_rect(plane.clone(), region) {
            result.push(cropped);
        }
    }
    result
}

fn plane_rect(plane: &ImagePlane) -> Rect {
    Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x + plane.size.width,
        y_max: plane.destination.y + plane.size.height,
    }
}

fn crop_plane_to_rect(plane: ImagePlane, rect: Rect) -> Option<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let rect = plane_rect.intersect(rect)?;
    let offset_x = (rect.x_min - plane_rect.x_min) as usize;
    let offset_y = (rect.y_min - plane_rect.y_min) as usize;
    let width = rect.width() as usize;
    let height = rect.height() as usize;
    let src_stride = plane.stride as usize;
    let mut bitmap = Vec::with_capacity(width * height);

    for row in 0..height {
        let start = (offset_y + row) * src_stride + offset_x;
        bitmap.extend_from_slice(&plane.bitmap[start..start + width]);
    }

    Some(ImagePlane {
        size: Size {
            width: rect.width(),
            height: rect.height(),
        },
        stride: rect.width(),
        destination: Point {
            x: rect.x_min,
            y: rect.y_min,
        },
        bitmap,
        ..plane
    })
}
fn is_event_active(event: &ParsedEvent, now_ms: i64) -> bool {
    now_ms >= event.start && now_ms < event.start + event.duration
}

#[cfg(test)]
mod tests {
    use super::*;
    use rassa_fonts::{FontconfigProvider, NullFontProvider};
    use rassa_parse::parse_script_text;

    fn config(
        frame_width: i32,
        frame_height: i32,
        margins: rassa_core::Margins,
        use_margins: bool,
    ) -> RendererConfig {
        RendererConfig {
            frame: Size {
                width: frame_width,
                height: frame_height,
            },
            margins,
            use_margins,
            ..RendererConfig::default()
        }
    }

    fn total_plane_area(planes: &[ImagePlane]) -> i32 {
        planes
            .iter()
            .map(|plane| plane.size.width * plane.size.height)
            .sum()
    }

    fn vertical_span(planes: &[ImagePlane]) -> i32 {
        let min_y = planes
            .iter()
            .map(|plane| plane.destination.y)
            .min()
            .expect("plane");
        let max_y = planes
            .iter()
            .map(|plane| plane.destination.y + plane.size.height)
            .max()
            .expect("plane");
        max_y - min_y
    }

    fn character_bounds(planes: &[ImagePlane]) -> Option<Rect> {
        let mut character_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character);
        let first = character_planes.next()?;
        let mut bounds = Rect {
            x_min: first.destination.x,
            y_min: first.destination.y,
            x_max: first.destination.x + first.size.width,
            y_max: first.destination.y + first.size.height,
        };
        for plane in character_planes {
            bounds.x_min = bounds.x_min.min(plane.destination.x);
            bounds.y_min = bounds.y_min.min(plane.destination.y);
            bounds.x_max = bounds.x_max.max(plane.destination.x + plane.size.width);
            bounds.y_max = bounds.y_max.max(plane.destination.y + plane.size.height);
        }
        Some(bounds)
    }

    fn visible_bounds(planes: &[ImagePlane]) -> Option<Rect> {
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

    #[test]
    fn prepare_frame_only_keeps_active_events() {
        let track = parse_script_text("[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,First\nDialogue: 0,0:00:02.00,0:00:03.00,Default,,0000,0000,0000,,Second").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = NullFontProvider;
        let frame = engine.prepare_frame(&track, &provider, 500);

        assert_eq!(frame.active_events.len(), 1);
        assert_eq!(frame.active_events[0].text, "First");
    }

    #[test]
    fn render_frame_produces_image_planes_for_active_text() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(!planes.is_empty());
        assert!(planes.iter().all(|plane| plane.size.width >= 0));
        assert!(planes.iter().all(|plane| plane.size.height >= 0));
    }

    #[test]
    fn render_frame_supports_multiple_override_runs() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fnDejaVu Sans}Hi{\\fnArial} there").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(!planes.is_empty());
    }

    #[test]
    fn render_frame_uses_override_colors_and_shadow_planes() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\1c&H112233&\\4c&H445566&\\shad3}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(
            planes.iter().any(
                |plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100
            )
        );
        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x6655_4400)
        );
    }

    #[test]
    fn render_frame_orders_events_by_layer_then_read_order() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 5,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\1c&H0000FF&}High\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,40)\\1c&H00FF00&}Low").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let first_character = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("character plane");
        assert_eq!(first_character.color.0, 0x00FF_0000);
    }

    #[test]
    fn render_frame_orders_shadow_outline_before_character_within_event() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00111111,&H0000FFFF,&H00222222,&H00333333,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let kinds = planes.iter().map(|plane| plane.kind).collect::<Vec<_>>();

        let first_shadow = kinds
            .iter()
            .position(|kind| *kind == ass::ImageType::Shadow)
            .expect("shadow plane");
        let first_outline = kinds
            .iter()
            .position(|kind| *kind == ass::ImageType::Outline)
            .expect("outline plane");
        let first_character = kinds
            .iter()
            .position(|kind| *kind == ass::ImageType::Character)
            .expect("character plane");

        assert!(first_shadow < first_outline);
        assert!(first_outline < first_character);
    }

    #[test]
    fn render_frame_emits_outline_planes_for_border_override() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord3\\3c&H0A0B0C&}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0C0B_0A00)
        );
    }

    #[test]
    fn render_frame_blurs_outline_and_shadow_layers() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord2\\blur2\\3c&H0A0B0C&\\shad2}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline
                    && plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
        );
        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Shadow
                    && plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
        );
    }

    #[test]
    fn render_frame_applies_rectangular_clip() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(0,0,64,64)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(!planes.is_empty());
        assert!(planes.iter().all(|plane| plane.destination.x >= 0));
        assert!(planes.iter().all(|plane| plane.destination.y >= 0));
        assert!(
            planes
                .iter()
                .all(|plane| plane.destination.x + plane.size.width <= 64)
        );
        assert!(
            planes
                .iter()
                .all(|plane| plane.destination.y + plane.size.height <= 64)
        );
    }

    #[test]
    fn render_frame_accepts_renderer_shaping_mode() {
        let track = parse_script_text("[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,48,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,office").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let simple = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                shaping: ass::ShapingLevel::Simple,
                ..default_renderer_config(&track)
            },
        );
        let complex = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                shaping: ass::ShapingLevel::Complex,
                ..default_renderer_config(&track)
            },
        );

        assert!(!simple.is_empty());
        assert!(!complex.is_empty());
    }

    #[test]
    fn render_frame_applies_inverse_rectangular_clip() {
        let plane = ImagePlane {
            size: Size {
                width: 6,
                height: 4,
            },
            stride: 6,
            color: RgbaColor(0x00FF_FFFF),
            destination: Point { x: 0, y: 0 },
            kind: ass::ImageType::Character,
            bitmap: vec![255; 24],
        };
        let parts = inverse_clip_plane(
            plane,
            Rect {
                x_min: 2,
                y_min: 1,
                x_max: 4,
                y_max: 3,
            },
        );

        assert_eq!(parts.len(), 4);
        assert_eq!(
            parts.iter().map(|plane| plane.bitmap.len()).sum::<usize>(),
            20
        );
    }

    #[test]
    fn render_frame_applies_vector_clip() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(m 0 0 l 32 0 32 32 0 32)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(!planes.is_empty());
        assert!(
            planes
                .iter()
                .all(|plane| plane.bitmap.iter().any(|value| *value > 0))
        );
        assert!(planes.iter().all(|plane| plane.destination.x >= 0));
        assert!(planes.iter().all(|plane| plane.destination.y >= 0));
    }

    #[test]
    fn render_frame_clips_to_frame_bounds() {
        let plane = ImagePlane {
            size: Size {
                width: 20,
                height: 20,
            },
            stride: 20,
            color: RgbaColor(0x00FF_FFFF),
            destination: Point { x: 50, y: 50 },
            kind: ass::ImageType::Character,
            bitmap: vec![255; 400],
        };
        let clipped = apply_event_clip(
            vec![plane],
            Rect {
                x_min: 0,
                y_min: 0,
                x_max: 60,
                y_max: 60,
            },
            false,
        );

        assert_eq!(clipped.len(), 1);
        assert_eq!(clipped[0].size.width, 10);
        assert_eq!(clipped[0].size.height, 10);
    }

    #[test]
    fn render_frame_applies_margin_clip_when_enabled() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &config(
                100,
                100,
                rassa_core::Margins {
                    top: 10,
                    bottom: 10,
                    left: 10,
                    right: 10,
                },
                true,
            ),
        );

        assert!(!planes.is_empty());
        assert!(planes.iter().all(|plane| plane.destination.x >= 10));
        assert!(planes.iter().all(|plane| plane.destination.y >= 10));
        assert!(
            planes
                .iter()
                .all(|plane| plane.destination.x + plane.size.width <= 90)
        );
        assert!(
            planes
                .iter()
                .all(|plane| plane.destination.y + plane.size.height <= 90)
        );
    }

    #[test]
    fn render_frame_maps_into_content_area_when_margins_are_not_used() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &config(
                120,
                120,
                rassa_core::Margins {
                    top: 10,
                    bottom: 10,
                    left: 10,
                    right: 10,
                },
                false,
            ),
        );

        assert!(!planes.is_empty());
        let bounds = visible_bounds(&planes).expect("visible bounds");
        assert!(
            bounds.x_min >= 10,
            "visible bounds should start inside content area: {bounds:?}"
        );
        assert!(
            bounds.y_min >= 9,
            "libass-style antialiasing may allocate one guard row above the content area: {bounds:?}"
        );
        assert!(
            bounds.x_max <= 110,
            "visible bounds should end inside content area: {bounds:?}"
        );
        assert!(
            bounds.y_max <= 110,
            "visible bounds should end inside content area: {bounds:?}"
        );
    }

    #[test]
    fn render_frame_keeps_border_closer_to_device_size_when_scaled_border_is_disabled() {
        let enabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
        let disabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: no\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let config = config(200, 200, rassa_core::Margins::default(), true);
        let enabled_planes =
            engine.render_frame_with_provider_and_config(&enabled, &provider, 500, &config);
        let disabled_planes =
            engine.render_frame_with_provider_and_config(&disabled, &provider, 500, &config);
        let enabled_outline_area: i32 = enabled_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Outline)
            .map(|plane| plane.size.width * plane.size.height)
            .sum();
        let disabled_outline_area: i32 = disabled_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Outline)
            .map(|plane| plane.size.width * plane.size.height)
            .sum();

        assert!(disabled_outline_area > 0);
        assert!(disabled_outline_area < enabled_outline_area);
    }

    #[test]
    fn render_frame_applies_font_scale_to_output() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider(&track, &provider, 500);
        let scaled = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 200,
                    height: 120,
                },
                font_scale: 2.0,
                ..RendererConfig::default()
            },
        );

        assert!(!baseline.is_empty());
        assert!(!scaled.is_empty());
        assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
    }

    #[test]
    fn render_frame_applies_text_scale_overrides() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Scale").expect("script should parse");
        let stretched = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50}Scale").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let baseline = engine.render_frame_with_provider(&track, &provider, 500);
        let scaled = engine.render_frame_with_provider(&stretched, &provider, 500);
        let baseline_width = baseline
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.x + plane.size.width)
            .max()
            .expect("baseline max x")
            - baseline
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character)
                .map(|plane| plane.destination.x)
                .min()
                .expect("baseline min x");
        let scaled_width = scaled
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.x + plane.size.width)
            .max()
            .expect("scaled max x")
            - scaled
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character)
                .map(|plane| plane.destination.x)
                .min()
                .expect("scaled min x");

        assert!(scaled_width > baseline_width);
        assert!(total_plane_area(&scaled) < total_plane_area(&baseline) * 2);
    }

    #[test]
    fn render_frame_applies_drawing_scale_overrides() {
        let baseline = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 10 0 10 10 0 10").expect("script should parse");
        let scaled = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50\\p1}m 0 0 l 10 0 10 10 0 10").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
        let scaled_planes = engine.render_frame_with_provider(&scaled, &provider, 500);
        let baseline_plane = baseline_planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("baseline drawing plane");
        let scaled_plane = scaled_planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("scaled drawing plane");

        assert!(scaled_plane.size.width > baseline_plane.size.width);
        assert!(scaled_plane.size.height < baseline_plane.size.height);
        assert_eq!(scaled_plane.destination, Point { x: 10, y: 10 });
    }

    #[test]
    fn render_frame_applies_text_spacing_override() {
        let baseline = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}IIII").expect("script should parse");
        let spaced = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fsp8}IIII").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
        let spaced_planes = engine.render_frame_with_provider(&spaced, &provider, 500);
        let baseline_width = character_bounds(&baseline_planes)
            .expect("baseline bounds")
            .width();
        let spaced_width = character_bounds(&spaced_planes)
            .expect("spaced bounds")
            .width();

        assert!(spaced_width > baseline_width);
    }

    #[test]
    fn render_frame_scales_output_to_frame_size() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider(&track, &provider, 500);
        let scaled = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 240,
                },
                ..default_renderer_config(&track)
            },
        );

        assert!(total_plane_area(&baseline) > 0);
        assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
    }

    #[test]
    fn render_frame_applies_pixel_aspect_horizontally() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 120,
                },
                ..default_renderer_config(&track)
            },
        );
        let widened = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 120,
                },
                pixel_aspect: 2.0,
                ..default_renderer_config(&track)
            },
        );

        let baseline_bounds = character_bounds(&baseline).expect("baseline character bounds");
        let widened_bounds = character_bounds(&widened).expect("widened character bounds");
        assert!(
            widened_bounds.x_min > baseline_bounds.x_min,
            "pixel aspect should affect horizontal placement: baseline={baseline_bounds:?} widened={widened_bounds:?}"
        );
    }

    #[test]
    fn render_frame_derives_pixel_aspect_from_storage_size_when_unset() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Storage").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 240,
                },
                ..default_renderer_config(&track)
            },
        );
        let storage_adjusted = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 240,
                },
                storage: Size {
                    width: 400,
                    height: 120,
                },
                ..default_renderer_config(&track)
            },
        );

        assert!(total_plane_area(&baseline) > 0);
        assert!(total_plane_area(&storage_adjusted) < total_plane_area(&baseline));
    }

    #[test]
    fn render_frame_layout_resolution_takes_precedence_over_storage_and_explicit_aspect() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\nLayoutResX: 400\nLayoutResY: 240\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Layout").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 240,
                },
                ..default_renderer_config(&track)
            },
        );
        let overridden_inputs = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 400,
                    height: 240,
                },
                storage: Size {
                    width: 400,
                    height: 120,
                },
                pixel_aspect: 2.0,
                ..default_renderer_config(&track)
            },
        );

        assert_eq!(
            total_plane_area(&overridden_inputs),
            total_plane_area(&baseline)
        );
    }

    #[test]
    fn render_frame_applies_line_position_to_subtitles() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Shift").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider(&track, &provider, 500);
        let shifted = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 200,
                    height: 120,
                },
                line_position: 50.0,
                ..RendererConfig::default()
            },
        );

        let baseline_y = baseline
            .iter()
            .map(|plane| plane.destination.y)
            .min()
            .expect("baseline plane");
        let shifted_y = shifted
            .iter()
            .map(|plane| plane.destination.y)
            .min()
            .expect("shifted plane");

        assert!(shifted_y < baseline_y);
    }

    #[test]
    fn render_frame_applies_line_spacing_to_multiline_subtitles() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,One\\NTwo").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();

        let baseline = engine.render_frame_with_provider(&track, &provider, 500);
        let spaced = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 200,
                    height: 140,
                },
                line_spacing: 20.0,
                ..RendererConfig::default()
            },
        );

        assert!(vertical_span(&spaced) > vertical_span(&baseline));
    }

    #[test]
    fn render_frame_avoids_basic_bottom_collision_for_unpositioned_events() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,First\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Second").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let mut ys = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.y)
            .collect::<Vec<_>>();
        ys.sort_unstable();
        ys.dedup();

        assert!(ys.len() >= 2);
        assert!(ys.last().expect("max y") - ys.first().expect("min y") >= 20);
    }

    #[test]
    fn render_frame_allows_basic_collision_across_different_layers() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 1,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H00FF00&}Second").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let layer0_y = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
            .map(|plane| plane.destination.y)
            .min()
            .expect("layer 0 character plane");
        let layer1_y = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
            .map(|plane| plane.destination.y)
            .min()
            .expect("layer 1 character plane");

        assert_eq!(layer0_y, layer1_y);
    }

    #[test]
    fn render_frame_interpolates_move_position() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\move(0,0,100,0,0,1000)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

        let start_x = start_planes
            .iter()
            .map(|plane| plane.destination.x)
            .min()
            .expect("start plane");
        let mid_x = mid_planes
            .iter()
            .map(|plane| plane.destination.x)
            .min()
            .expect("mid plane");
        let end_x = end_planes
            .iter()
            .map(|plane| plane.destination.x)
            .min()
            .expect("end plane");

        assert!(start_x <= mid_x);
        assert!(mid_x <= end_x);
        assert!(end_x - start_x >= 80);
    }

    #[test]
    fn render_frame_applies_z_rotation_to_event_planes() {
        let baseline = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
        let rotated = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\frz90\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
        let rotated_planes = engine.render_frame_with_provider(&rotated, &provider, 500);
        let baseline_bounds = character_bounds(&baseline_planes).expect("baseline bounds");
        let rotated_bounds = character_bounds(&rotated_planes).expect("rotated bounds");

        assert!(baseline_bounds.width() > baseline_bounds.height());
        assert!(rotated_bounds.height() > rotated_bounds.width());
    }

    #[test]
    fn render_frame_interpolates_z_rotation_transform() {
        let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\t(0,1000,\\frz90)\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);
        let start_bounds = character_bounds(&start_planes).expect("start bounds");
        let end_bounds = character_bounds(&end_planes).expect("end bounds");

        assert!(start_bounds.width() > start_bounds.height());
        assert!(end_bounds.height() > end_bounds.width());
    }

    #[test]
    fn render_frame_applies_fad_alpha() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fad(200,200)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

        let start_alpha = start_planes
            .iter()
            .map(|plane| plane.color.0 & 0xFF)
            .max()
            .expect("start alpha");
        let mid_alpha = mid_planes
            .iter()
            .map(|plane| plane.color.0 & 0xFF)
            .max()
            .expect("mid alpha");
        let end_alpha = end_planes
            .iter()
            .map(|plane| plane.color.0 & 0xFF)
            .max()
            .expect("end alpha");

        assert!(start_alpha > mid_alpha);
        assert!(end_alpha > mid_alpha);
    }

    #[test]
    fn render_frame_applies_full_fade_alpha() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fade(255,0,128,0,200,700,1000)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let middle_planes = engine.render_frame_with_provider(&track, &provider, 400);
        let late_planes = engine.render_frame_with_provider(&track, &provider, 850);

        let start_alpha = start_planes
            .iter()
            .map(|plane| plane.color.0 & 0xFF)
            .max()
            .expect("start alpha");
        let middle_alpha = middle_planes
            .iter()
            .map(|plane| plane.color.0 & 0xFF)
            .max()
            .expect("middle alpha");
        let late_alpha = late_planes
            .iter()
            .map(|plane| plane.color.0 & 0xFF)
            .max()
            .expect("late alpha");

        assert!(start_alpha > middle_alpha);
        assert!(late_alpha > middle_alpha);
        assert!(late_alpha < start_alpha);
    }

    #[test]
    fn render_frame_switches_karaoke_fill_after_elapsed_span() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\k50}Ka").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
        let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

        assert!(
            early_planes.iter().any(
                |plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x6655_4400
            )
        );
        assert!(
            late_planes.iter().any(
                |plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100
            )
        );
    }

    #[test]
    fn render_frame_sweeps_karaoke_fill_during_active_span() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\K100}Kara").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(
            mid_planes.iter().any(
                |plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100
            )
        );
        assert!(
            mid_planes.iter().any(
                |plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x6655_4400
            )
        );
    }

    #[test]
    fn render_frame_hides_outline_for_ko_until_span_ends() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H000A0B0C,&H00000000,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\ko50}Ko").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
        let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

        assert!(
            !early_planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline)
        );
        assert!(
            late_planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline)
        );
    }

    #[test]
    fn render_frame_renders_drawing_plane() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(
            planes.iter().any(
                |plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100
            )
        );
        let plane = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("drawing plane");
        assert_eq!(plane.destination.x, 10);
        assert_eq!(plane.destination.y, 10);
        assert!(plane.bitmap.contains(&255));
    }

    #[test]
    fn render_frame_renders_bezier_drawing_plane() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 b 10 0 10 10 0 10").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let plane = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("drawing plane");
        assert!(plane.bitmap.contains(&255));
        assert!(plane.size.width >= 8);
        assert!(plane.size.height >= 8);
    }

    #[test]
    fn render_frame_emits_outline_and_shadow_for_drawings() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H000A0B0C,&H00445566,0,0,0,0,100,100,0,0,1,2,3,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0C0B_0A00)
        );
        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x6655_4400)
        );
    }

    #[test]
    fn render_frame_renders_spline_drawing_plane() {
        let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 s 10 0 10 10 0 10 p -5 5 c").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let plane = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("drawing plane");
        assert!(plane.bitmap.contains(&255));
        assert!(plane.size.width >= 10);
        assert!(plane.size.height >= 10);
    }

    #[test]
    fn render_frame_renders_non_closing_move_subpaths() {
        let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8 n 20 20 l 28 20 28 28 20 28").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let plane = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("drawing plane");
        assert!(plane.bitmap.contains(&255));
        assert!(plane.size.width >= 28);
        assert!(plane.size.height >= 28);
    }

    #[test]
    fn render_frame_applies_timed_transform_style() {
        let track = parse_script_text("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H000000FF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\t(0,1000,\\1c&H00112233&\\fs48\\bord4)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

        assert!(
            !start_planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline)
        );
        assert!(
            mid_planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline)
        );
        assert!(
            end_planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Outline)
        );

        let start_fill = start_planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("start fill")
            .color
            .0;
        let end_fill = end_planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("end fill")
            .color
            .0;
        assert_ne!(start_fill, end_fill);
        assert!(total_plane_area(&end_planes) > total_plane_area(&start_planes));
    }
}
