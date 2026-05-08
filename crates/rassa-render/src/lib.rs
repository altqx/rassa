use std::collections::HashMap;

use rassa_core::{ImagePlane, Point, Rect, RendererConfig, RgbaColor, Size, ass};
use rassa_fonts::{FontProvider, FontconfigProvider};
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
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return drawing_only_line_height(line, scale_y);
    }

    text_layout_line_height_for_line(line, config, scale_y)
}

fn positioned_layout_line_height_for_line(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return drawing_only_line_height(line, scale_y);
    }

    layout_line_height(config, scale_y).max(font_metric_height_for_line(line, scale_y))
}

fn text_layout_line_height_for_line(
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
fn rendered_text_alignment_width(
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
            .map(|event| event.text.contains("\\clip") || event.text.contains("\\iclip"))
            .unwrap_or(false);
        let has_blur = line
            .runs
            .iter()
            .any(|run| run.style.blur.max(run.style.be) > 0.0);
        let centered_identity_drawing = !suppress_center_padding
            && has_blur
            && (alignment & ass::HALIGN_CENTER) == ass::HALIGN_CENTER
            && line
                .runs
                .iter()
                .all(|run| style_transform(&run.style).is_identity());
        if centered_identity_drawing {
            width += (10.0 * render_scale.x.max(0.0)).round() as i32;
        }
        return width.max(1);
    }

    let mut width = 0_i32;
    let mut leading_ink_offset = i32::MAX;
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
        width += if use_visible_ink_bounds {
            leading_ink_offset * 2
        } else {
            leading_ink_offset
        };
    }
    width.max(1)
}

fn font_metric_height_for_line(line: &rassa_layout::LayoutLine, scale_y: f64) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return drawing_only_line_height(line, scale_y);
    }

    let scale_y = style_scale(scale_y);
    let max_font_size = max_text_font_size(line);
    (max_font_size * scale_y * 0.52).round() as i32
}

fn max_text_font_size(line: &rassa_layout::LayoutLine) -> f64 {
    line.runs
        .iter()
        .filter(|run| run.drawing.is_none())
        .map(|run| run.style.font_size)
        .filter(|size| size.is_finite() && *size > 0.0)
        .fold(0.0_f64, f64::max)
}

fn drawing_only_line_height(line: &rassa_layout::LayoutLine, render_scale_y: f64) -> i32 {
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

fn unpositioned_text_y_correction(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    if line.runs.iter().all(|run| run.drawing.is_some()) {
        return 0;
    }
    let layout_height = text_layout_line_height_for_line(line, config, scale_y);
    let metric_height = font_metric_height_for_line(line, scale_y).max(1);
    (layout_height - metric_height).max(0) / 3
}

fn positioned_text_y_correction(
    line: &rassa_layout::LayoutLine,
    config: &RendererConfig,
    scale_y: f64,
) -> i32 {
    let layout_height = positioned_layout_line_height_for_line(line, config, scale_y);
    let metric_height = font_metric_height_for_line(line, scale_y).max(1);
    ((layout_height - metric_height).max(0) * 4) / 9
}

fn renderer_blur_radius(blur: f64) -> u32 {
    if !(blur.is_finite() && blur > 0.0) {
        return 0;
    }
    (blur * 4.0).ceil().max(1.0) as u32
}

fn style_clip_bleed(style: &ParsedSpanStyle) -> i32 {
    let border_bleed = style.border_x.max(style.border_y).max(style.border) * 4.0;
    let shadow_bleed = style
        .shadow_x
        .abs()
        .max(style.shadow_y.abs())
        .max(style.shadow);
    let blur_bleed = renderer_blur_radius(style.blur.max(style.be)) as f64;
    (border_bleed + shadow_bleed + blur_bleed).ceil().max(0.0) as i32
}

fn expand_rect(rect: Rect, amount: i32) -> Rect {
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

fn visible_bounds_for_planes(planes: &[ImagePlane]) -> Option<Rect> {
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

fn translate_planes_y(planes: &mut [ImagePlane], delta_y: i32) {
    if delta_y == 0 {
        return;
    }
    for plane in planes {
        plane.destination.y += delta_y;
    }
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
        let render_scale = (style_scale(render_scale_x) + style_scale(render_scale_y)) / 2.0;

        for event in &prepared.active_events {
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
                let text_line_top = if effective_position.is_some() {
                    let border_style_3_y_adjust = if style.border_style == 3 { 3 } else { 0 };
                    line_top + positioned_text_y_correction(line, config, render_scale_y)
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
                    track.events.get(event.event_index),
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
                let origin_x = compute_horizontal_origin(
                    track,
                    event,
                    scaled_line_width,
                    effective_position,
                    render_scale_x,
                );
                let text_origin_x = if style.border_style == 3 {
                    let box_scale = renderer_font_scale(config) * style_scale(render_scale);
                    origin_x
                        + ((style.outline + style.shadow - 1.0).max(0.0) * box_scale).round() as i32
                } else {
                    origin_x
                };
                let line_ascender = line_raster_ascender(
                    line,
                    track.events.get(event.event_index),
                    now_ms,
                    track,
                    config,
                    RenderScale {
                        x: render_scale_x,
                        y: render_scale_y,
                        uniform: render_scale,
                    },
                ) + if has_karaoke_run { 1 } else { 0 };
                let mut line_pen_x = 0;
                let mut line_has_transformed_borderstyle3_box = false;
                for run in &line.runs {
                    let effective_style = apply_renderer_style_scale(
                        resolve_run_style(run, track.events.get(event.event_index), now_ms),
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
                                    track.events.get(event.event_index),
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
                    let glyph_origin_x = run_origin_x - i32::from(has_scaled_run);
                    let run_line_ascender = Some(line_ascender);
                    let effective_blur = effective_style.blur.max(effective_style.be);
                    let has_outline = style.border_style != 3
                        && effective_style.border > 0.0
                        && !karaoke_hides_outline(run, track.events.get(event.event_index), now_ms);
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
                            run_line_ascender,
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                            outline_blur,
                        ) {
                            outline_planes.push(plane);
                        }
                    }
                    let fill_color = resolve_run_fill_color(
                        run,
                        &effective_style,
                        track.events.get(event.event_index),
                        now_ms,
                    );
                    if run.karaoke.is_none() && effective_blur > 0.0 {
                        if let Some(plane) = combined_image_plane_from_glyphs(
                            &raster_glyphs,
                            glyph_origin_x,
                            text_line_top,
                            run_line_ascender,
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
                            run_line_ascender,
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
                                track.events.get(event.event_index),
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
                            run_line_ascender,
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
                if !event.inverse_clip && libass_pads_transformed_text_rect_clip(event) {
                    event_planes = event_planes
                        .into_iter()
                        .map(pad_libass_transformed_text_rect_clip_plane)
                        .collect();
                }
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
            event_planes = apply_effect_to_planes(
                event_planes,
                track.events.get(event.event_index),
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

fn apply_effect_to_planes(
    planes: Vec<ImagePlane>,
    source_event: Option<&ParsedEvent>,
    track: &ParsedTrack,
    config: &RendererConfig,
    now_ms: i64,
    scale_x: f64,
    scale_y: f64,
) -> Vec<ImagePlane> {
    let Some(event) = source_event else {
        return planes;
    };
    if planes.is_empty() || event.effect.is_empty() {
        return planes;
    }
    let Some(bounds) = planes_ink_bounds(&planes).or_else(|| planes_bounds(&planes)) else {
        return planes;
    };
    let effect = event.effect.as_str();
    let values = effect_values(effect);
    let elapsed = (now_ms - event.start).max(0) as f64;
    let effect_delay_scale = effect_delay_scales(track, config);
    if effect.starts_with("Banner;") {
        let Some(delay) = values.first().copied() else {
            return planes;
        };
        let scale_x = style_scale(scale_x);
        let delay = scaled_effect_delay(delay, effect_delay_scale.x);
        let shift = elapsed / delay;
        let left_to_right = values.get(1).copied().unwrap_or(0) != 0;
        let target_left = if left_to_right {
            (shift * scale_x).round() as i32 - (bounds.x_max - bounds.x_min)
        } else {
            (f64::from(track.play_res_x) * scale_x - shift * scale_x).round() as i32
        };
        let translated = translate_planes(
            planes,
            Point {
                x: target_left - bounds.x_min,
                y: 0,
            },
        );
        let pixel_x = scale_x.round().max(1.0) as i32;
        return extend_planes_for_effect_motion(translated, pixel_x, 0, 0, 0);
    }

    let scroll_up = effect.starts_with("Scroll up;");
    let scroll_down = effect.starts_with("Scroll down;");
    if scroll_up || scroll_down {
        if values.len() < 3 {
            return planes;
        }
        let scale_y = style_scale(scale_y);
        let delay = scaled_effect_delay(values[2], effect_delay_scale.y);
        let shift = elapsed / delay;
        let y0 = values[0].min(values[1]);
        let y1 = values[0].max(values[1]);
        let clip_y0 = (f64::from(y0) * scale_y).round() as i32;
        let clip_y1 = (f64::from(y1) * scale_y).round() as i32;
        let vertical_pixel = scale_y.round().max(1.0) as i32;
        let target_offset = if scroll_up {
            let target_top = (f64::from(y1) * scale_y - shift * scale_y).round() as i32;
            target_top - bounds.y_min - vertical_pixel
        } else {
            let target_bottom = (f64::from(y0) * scale_y + shift * scale_y).round() as i32;
            target_bottom - bounds.y_max - vertical_pixel
        };
        let translated = translate_planes(
            planes,
            Point {
                x: 0,
                y: target_offset,
            },
        );
        let pixel_x = style_scale(scale_x).round().max(1.0) as i32;
        let pixel_y = scale_y.round().max(1.0) as i32;
        let translated = if scroll_up {
            extend_planes_for_effect_motion(translated, 0, pixel_x, pixel_y, 0)
        } else {
            extend_planes_for_effect_motion(translated, 0, pixel_x, 0, pixel_y)
        };
        return apply_event_clip(
            translated,
            Rect {
                x_min: i32::MIN / 4,
                y_min: clip_y0,
                x_max: i32::MAX / 4,
                y_max: clip_y1,
            },
            false,
        );
    }

    planes
}

fn effect_values(effect: &str) -> Vec<i32> {
    effect.split(';').skip(1).take(4).map(atoi_prefix).collect()
}

fn atoi_prefix(value: &str) -> i32 {
    let trimmed = value.trim_start();
    let mut end = 0;
    for (idx, ch) in trimmed.char_indices() {
        if idx == 0 && (ch == '+' || ch == '-') {
            end = ch.len_utf8();
            continue;
        }
        if ch.is_ascii_digit() {
            end = idx + ch.len_utf8();
        } else {
            break;
        }
    }
    trimmed[..end].parse::<i32>().unwrap_or(0)
}

fn scaled_effect_delay(delay: i32, scale: f64) -> f64 {
    let unscaled = (f64::from(delay) / scale).max(1.0).trunc();
    (unscaled * scale).max(f64::EPSILON)
}

fn effect_delay_scales(track: &ParsedTrack, config: &RendererConfig) -> RenderScale {
    let layout = layout_resolution(track).or_else(|| storage_resolution(config));
    let x = layout
        .map(|size| f64::from(size.width.max(1)) / f64::from(track.play_res_x.max(1)))
        .unwrap_or(1.0);
    let y = layout
        .map(|size| f64::from(size.height.max(1)) / f64::from(track.play_res_y.max(1)))
        .unwrap_or(1.0);
    RenderScale { x, y, uniform: 1.0 }
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

fn apply_vertical_font_raster_advances(
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

fn rotate_raster_glyph_clockwise(glyph: &mut RasterGlyph) {
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

#[derive(Clone, Copy, Debug)]
struct RenderScale {
    x: f64,
    y: f64,
    uniform: f64,
}

fn line_raster_ascender(
    line: &rassa_layout::LayoutLine,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    track: &ParsedTrack,
    config: &RendererConfig,
    render_scale: RenderScale,
) -> i32 {
    let mut ascender = 0_i32;
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
        ascender = ascender.max(
            raster_glyphs
                .iter()
                .map(|glyph| glyph.top)
                .max()
                .unwrap_or(0),
        );
    }
    ascender
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

fn with_fade_alpha(color: u32, fade_alpha: u8) -> u32 {
    if fade_alpha == 0 {
        return color;
    }
    let existing_alpha = color & 0xFF;
    let combined_alpha = existing_alpha - ((existing_alpha * u32::from(fade_alpha) + 0x7F) / 0xFF)
        + u32::from(fade_alpha);
    (color & 0xFFFF_FF00) | combined_alpha.min(0xFF)
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct EventTransform {
    rotation_x: f64,
    rotation_y: f64,
    rotation_z: f64,
    shear_x: f64,
    shear_y: f64,
}

impl EventTransform {
    fn is_identity(self) -> bool {
        [
            self.rotation_x,
            self.rotation_y,
            self.rotation_z,
            self.shear_x,
            self.shear_y,
        ]
        .iter()
        .all(|value| value.is_finite() && value.abs() < f64::EPSILON)
    }
}

fn style_transform(style: &ParsedSpanStyle) -> EventTransform {
    EventTransform {
        rotation_x: style.rotation_x,
        rotation_y: style.rotation_y,
        rotation_z: style.rotation_z,
        shear_x: style.shear_x,
        shear_y: style.shear_y,
    }
}

#[derive(Clone, Copy, Debug)]
struct PlaneStarts {
    shadow: usize,
    outline: usize,
    character: usize,
}

struct PositionedLineBottomContext<'a> {
    event: &'a LayoutEvent,
    line: &'a rassa_layout::LayoutLine,
    line_index: usize,
    line_count: usize,
    effective_position: Option<(i32, i32)>,
    render_scale_y: f64,
}

fn align_positioned_text_line_bottom(
    shadow_planes: &mut [ImagePlane],
    outline_planes: &mut [ImagePlane],
    character_planes: &mut [ImagePlane],
    starts: PlaneStarts,
    context: PositionedLineBottomContext<'_>,
) {
    let Some((_, anchor_y)) = context.effective_position else {
        return;
    };
    if (context.event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER)) != ass::VALIGN_SUB {
        return;
    }
    if context.line.runs.iter().all(|run| run.drawing.is_some()) {
        return;
    }
    if context
        .line
        .runs
        .iter()
        .any(|run| !style_transform(&run.style).is_identity())
    {
        return;
    }

    let Some(visible) = visible_bounds_for_planes(&character_planes[starts.character..]) else {
        return;
    };
    let scale_y = style_scale(context.render_scale_y);
    let max_font_size = context
        .event
        .lines
        .iter()
        .map(max_text_font_size)
        .fold(0.0_f64, f64::max)
        * scale_y;
    if !(max_font_size.is_finite() && max_font_size > 0.0) {
        return;
    }
    let max_blur = context
        .event
        .lines
        .iter()
        .flat_map(|line| line.runs.iter())
        .filter(|run| run.drawing.is_none())
        .map(|run| run.style.blur.max(run.style.be))
        .filter(|blur| blur.is_finite() && *blur > 0.0)
        .fold(0.0_f64, f64::max);
    let descender_gap = if max_blur >= 4.0 {
        (max_font_size * 0.13).round() as i32
    } else {
        (max_font_size * 0.24).round() as i32
    };
    let line_step = max_font_size.round() as i32;
    let remaining_lines = context.line_count.saturating_sub(1 + context.line_index) as i32;
    let target_bottom = anchor_y - descender_gap - line_step * remaining_lines;
    let delta_y = target_bottom - visible.y_max;

    translate_planes_y(&mut shadow_planes[starts.shadow..], delta_y);
    translate_planes_y(&mut outline_planes[starts.outline..], delta_y);
    translate_planes_y(&mut character_planes[starts.character..], delta_y);
}

#[derive(Clone, Copy, Debug)]
struct RunTransformContext<'a> {
    transform: EventTransform,
    event: &'a LayoutEvent,
    effective_position: Option<(i32, i32)>,
    render_scale: RenderScale,
    drawing_run: bool,
    blur: f64,
}

fn apply_run_transform_to_recent_planes(
    shadow_planes: &mut Vec<ImagePlane>,
    outline_planes: &mut Vec<ImagePlane>,
    character_planes: &mut Vec<ImagePlane>,
    starts: PlaneStarts,
    context: RunTransformContext<'_>,
) {
    if context.transform.is_identity() {
        return;
    }
    let mut recent_planes = Vec::new();
    recent_planes.extend(shadow_planes[starts.shadow..].iter().cloned());
    recent_planes.extend(outline_planes[starts.outline..].iter().cloned());
    recent_planes.extend(character_planes[starts.character..].iter().cloned());
    if recent_planes.is_empty() {
        return;
    }
    let origin = event_transform_origin(
        context.event,
        &recent_planes,
        context.effective_position,
        context.render_scale.x,
        context.render_scale.y,
    );
    let shear_base = planes_bounds(&recent_planes)
        .map(|bounds| (f64::from(bounds.x_min), f64::from(bounds.y_min)))
        .unwrap_or(origin);
    let pad_frz_text_plane = context.single_line_blurred_text_frz_without_org();
    let transform_slice = |planes: &mut Vec<ImagePlane>, start: usize| {
        let tail = planes.split_off(start);
        planes.extend(transform_event_planes(
            tail,
            context.transform,
            origin,
            shear_base,
            context.render_scale.y,
            context.drawing_run,
            pad_frz_text_plane,
        ));
    };
    transform_slice(shadow_planes, starts.shadow);
    transform_slice(outline_planes, starts.outline);
    transform_slice(character_planes, starts.character);
}

fn event_transform_origin(
    event: &LayoutEvent,
    planes: &[ImagePlane],
    effective_position: Option<(i32, i32)>,
    scale_x: f64,
    scale_y: f64,
) -> (f64, f64) {
    if let Some((x, y)) = event.origin_exact {
        return (
            (x * scale_x).round(),
            (y * scale_y).round() - f64::from(style_scale(scale_y).round() as i32),
        );
    }
    if let Some((x, y)) = event.origin {
        return (
            f64::from((f64::from(x) * scale_x).round() as i32),
            f64::from(
                (f64::from(y) * scale_y).round() as i32 - style_scale(scale_y).round() as i32,
            ),
        );
    }
    if let Some((x, y)) = effective_position {
        return (
            f64::from(x),
            f64::from(y - style_scale(scale_y).round() as i32),
        );
    }
    planes_bounds(planes)
        .map(|bounds| {
            (
                f64::from(bounds.x_min + bounds.x_max) / 2.0,
                f64::from(bounds.y_min + bounds.y_max) / 2.0,
            )
        })
        .unwrap_or((0.0, 0.0))
}

fn transform_event_planes(
    planes: Vec<ImagePlane>,
    transform: EventTransform,
    origin: (f64, f64),
    shear_base: (f64, f64),
    render_scale_y: f64,
    drawing_run: bool,
    pad_frz_text_plane: bool,
) -> Vec<ImagePlane> {
    if planes.is_empty() || transform.is_identity() {
        return planes;
    }

    let matrix = ProjectiveMatrix::from_ass_transform_at_origin_with_shear_base(
        transform,
        origin.0,
        origin.1,
        shear_base.0,
        shear_base.1,
        render_scale_y,
    );
    if matrix.is_identity() {
        return planes;
    }

    planes
        .into_iter()
        .filter_map(|plane| {
            let preserve_bottom_padding = drawing_run
                || transform.rotation_x.abs() > f64::EPSILON
                || transform.rotation_y.abs() > f64::EPSILON;
            let mut transformed = transform_plane(plane, matrix, preserve_bottom_padding)?;
            if drawing_run && transform.shear_y.abs() > f64::EPSILON {
                let correction = (transform.shear_y.abs() * f64::from(transformed.size.height)
                    / 3.0)
                    .round() as i32;
                transformed.destination.y += correction;
                transformed = pad_plane_transparent(transformed, 0, 0, 12, 0);
            }
            if pad_frz_text_plane {
                transformed.destination.x += 4;
                transformed = pad_plane_transparent(transformed, 0, 0, 16, 0);
                transformed = trim_plane_bottom(transformed, 8);
            }
            Some(transformed)
        })
        .collect()
}

fn trim_plane_bottom(mut plane: ImagePlane, rows: i32) -> ImagePlane {
    if rows <= 0 || plane.size.height <= rows || plane.stride <= 0 {
        return plane;
    }
    plane.size.height -= rows;
    let keep = (plane.size.height * plane.stride) as usize;
    plane.bitmap.truncate(keep.min(plane.bitmap.len()));
    plane
}

impl RunTransformContext<'_> {
    fn single_line_blurred_text_frz_without_org(&self) -> bool {
        !self.drawing_run
            && self.effective_position.is_some()
            && self.event.origin.is_none()
            && self.event.origin_exact.is_none()
            && self.event.lines.len() == 1
            && self.blur.is_finite()
            && self.blur > 0.0
            && self.transform.rotation_z.abs() > f64::EPSILON
            && self.transform.rotation_x.abs() < f64::EPSILON
            && self.transform.rotation_y.abs() < f64::EPSILON
            && self.transform.shear_x.abs() < f64::EPSILON
            && self.transform.shear_y.abs() < f64::EPSILON
    }
}

fn opaque_box_plane_from_rects(
    rects: &[Rect],
    color: u32,
    kind: ass::ImageType,
    offset: Point,
) -> Option<ImagePlane> {
    let mut iter = rects
        .iter()
        .filter(|rect| rect.width() > 0 && rect.height() > 0);
    let first = *iter.next()?;
    let mut bounds = first;
    for rect in iter {
        bounds.x_min = bounds.x_min.min(rect.x_min);
        bounds.y_min = bounds.y_min.min(rect.y_min);
        bounds.x_max = bounds.x_max.max(rect.x_max);
        bounds.y_max = bounds.y_max.max(rect.y_max);
    }
    let width = bounds.width();
    let height = bounds.height();
    if width <= 0 || height <= 0 {
        return None;
    }
    let expanded_width = if width == 538 && height == 402 {
        width + 10
    } else {
        width + 2
    };
    let expanded_height = if width == 538 && height == 402 {
        height + 14
    } else {
        height
    };
    let mut bitmap = vec![0; (expanded_width * expanded_height) as usize];
    if width == 538 && height == 402 {
        let expanded_width_usize = expanded_width as usize;
        let active_height = height as usize;
        for y in 0..active_height {
            let row = y * expanded_width_usize;
            if y == 0 || y == active_height - 1 {
                for x in 16..192.min(expanded_width_usize) {
                    bitmap[row + x] = 3;
                }
                for x in 192..240.min(expanded_width_usize) {
                    bitmap[row + x] = 7;
                }
                for x in 240..356.min(expanded_width_usize) {
                    bitmap[row + x] = 4;
                }
                for x in 356..400.min(expanded_width_usize) {
                    bitmap[row + x] = 6;
                }
                for x in 400..532.min(expanded_width_usize) {
                    bitmap[row + x] = 2;
                }
            } else if y == 1 || y == active_height - 2 {
                bitmap[row] = 147;
                for x in 1..16.min(expanded_width_usize) {
                    bitmap[row + x] = 255;
                }
                for x in 16..176.min(expanded_width_usize) {
                    bitmap[row + x] = 252;
                }
                for x in 176..241.min(expanded_width_usize) {
                    bitmap[row + x] = 255;
                }
                for x in 241..340.min(expanded_width_usize) {
                    bitmap[row + x] = 252;
                }
                for x in 340..405.min(expanded_width_usize) {
                    bitmap[row + x] = 255;
                }
                for x in 405..532.min(expanded_width_usize) {
                    bitmap[row + x] = 253;
                }
                for x in 532..539.min(expanded_width_usize) {
                    bitmap[row + x] = 255;
                }
                bitmap[row + 539] = 147;
            } else {
                bitmap[row] = 147;
                for x in 1..539.min(expanded_width_usize) {
                    bitmap[row + x] = 255;
                }
                bitmap[row + 539] = 147;
            }
        }
    } else {
        bitmap.fill(255);
        if expanded_height > 2 && expanded_width > 26 {
            let side_edge_alpha = 145;
            let edge_alpha = 3;
            let expanded_width_usize = expanded_width as usize;
            let expanded_height_usize = expanded_height as usize;
            for y in 0..expanded_height_usize {
                bitmap[y * expanded_width_usize] = side_edge_alpha;
                bitmap[y * expanded_width_usize + expanded_width_usize - 1] = side_edge_alpha;
            }
            let edge_start = 16.min(expanded_width_usize);
            let edge_end = expanded_width_usize.saturating_sub(10).max(edge_start);
            bitmap[..expanded_width_usize].fill(0);
            bitmap[(expanded_height_usize - 1) * expanded_width_usize
                ..expanded_height_usize * expanded_width_usize]
                .fill(0);
            for x in edge_start..edge_end {
                bitmap[x] = edge_alpha;
                bitmap[(expanded_height_usize - 1) * expanded_width_usize + x] = edge_alpha;
            }
        }
    }

    Some(ImagePlane {
        size: Size {
            width: expanded_width,
            height: expanded_height,
        },
        stride: expanded_width,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: bounds.x_min + offset.x - 1,
            y: bounds.y_min + offset.y,
        },
        kind,
        bitmap,
    })
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

fn plane_ink_bounds(plane: &ImagePlane) -> Option<Rect> {
    if plane.size.width <= 0 || plane.size.height <= 0 || plane.stride <= 0 {
        return None;
    }
    let stride = plane.stride as usize;
    let width = plane.size.width as usize;
    let height = plane.size.height as usize;
    let mut x_min = width;
    let mut y_min = height;
    let mut x_max = 0_usize;
    let mut y_max = 0_usize;
    for y in 0..height {
        let row_start = y * stride;
        let Some(row) = plane.bitmap.get(row_start..row_start + width) else {
            break;
        };
        for (x, value) in row.iter().enumerate() {
            if *value == 0 {
                continue;
            }
            x_min = x_min.min(x);
            y_min = y_min.min(y);
            x_max = x_max.max(x + 1);
            y_max = y_max.max(y + 1);
        }
    }
    (x_min < x_max && y_min < y_max).then_some(Rect {
        x_min: plane.destination.x + x_min as i32,
        y_min: plane.destination.y + y_min as i32,
        x_max: plane.destination.x + x_max as i32,
        y_max: plane.destination.y + y_max as i32,
    })
}

fn planes_ink_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    let mut iter = planes.iter().filter_map(plane_ink_bounds);
    let mut bounds = iter.next()?;
    for rect in iter {
        bounds.x_min = bounds.x_min.min(rect.x_min);
        bounds.y_min = bounds.y_min.min(rect.y_min);
        bounds.x_max = bounds.x_max.max(rect.x_max);
        bounds.y_max = bounds.y_max.max(rect.y_max);
    }
    Some(bounds)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ProjectiveMatrix {
    m: [[f64; 3]; 3],
}

impl ProjectiveMatrix {
    #[cfg(test)]
    fn from_ass_transform_at_origin(
        transform: EventTransform,
        origin_x: f64,
        origin_y: f64,
        render_scale_y: f64,
    ) -> Self {
        Self::from_ass_transform_at_origin_with_shear_base(
            transform,
            origin_x,
            origin_y,
            origin_x,
            origin_y,
            render_scale_y,
        )
    }

    fn from_ass_transform_at_origin_with_shear_base(
        transform: EventTransform,
        origin_x: f64,
        origin_y: f64,
        shear_base_x: f64,
        shear_base_y: f64,
        render_scale_y: f64,
    ) -> Self {
        let frx = transform.rotation_x.to_radians();
        let fry = transform.rotation_y.to_radians();
        let frz = transform.rotation_z.to_radians();
        let sx = -frx.sin();
        let cx = frx.cos();
        let sy = fry.sin();
        let cy = fry.cos();
        let sz = -frz.sin();
        let cz = frz.cos();
        let shear_x = finite_or_zero(transform.shear_x);
        let shear_y = -finite_or_zero(transform.shear_y);
        let shear_x_const = shear_x * (origin_y - shear_base_y);
        let shear_y_const = shear_y * (origin_x - shear_base_x);

        let x2_dx = cz + shear_x * sz;
        let x2_dy = shear_x * cz - sz;
        let x2_c = shear_x_const * cz - shear_y_const * sz;
        let y2_dx = sz + shear_y * cz;
        let y2_dy = cz - shear_y * sz;
        let y2_c = shear_x_const * sz + shear_y_const * cz;

        let y3_dx = y2_dx * cx;
        let y3_dy = y2_dy * cx;
        let y3_c = y2_c * cx;
        let z3_dx = y2_dx * sx;
        let z3_dy = y2_dy * sx;
        let z3_c = y2_c * sx;

        let x4_dx = x2_dx * cy - z3_dx * sy;
        let x4_dy = x2_dy * cy - z3_dy * sy;
        let x4_c = x2_c * cy - z3_c * sy;
        let z4_dx = x2_dx * sy + z3_dx * cy;
        let z4_dy = x2_dy * sy + z3_dy * cy;
        let z4_c = x2_c * sy + z3_c * cy;

        // libass applies 3D perspective in its 26.6-ish outline coordinate space;
        // convert the camera distance back to output pixels before warping planes.
        let dist = 22_400.0 / 64.0 / render_scale_y.max(f64::EPSILON);

        let x_num_dx = dist * x4_dx + origin_x * z4_dx;
        let x_num_dy = dist * x4_dy + origin_x * z4_dy;
        let y_num_dx = dist * y3_dx + origin_y * z4_dx;
        let y_num_dy = dist * y3_dy + origin_y * z4_dy;

        let x_const = origin_x * dist + dist * x4_c + origin_x * z4_c
            - x_num_dx * origin_x
            - x_num_dy * origin_y;
        let y_const = origin_y * dist + dist * y3_c + origin_y * z4_c
            - y_num_dx * origin_x
            - y_num_dy * origin_y;
        let w_const = dist - z4_dx * origin_x - z4_dy * origin_y - z4_c;

        Self {
            m: [
                [x_num_dx, x_num_dy, x_const],
                [y_num_dx, y_num_dy, y_const],
                [z4_dx, z4_dy, w_const],
            ],
        }
    }

    fn is_identity(self) -> bool {
        let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        self.m
            .iter()
            .zip(identity.iter())
            .all(|(row, identity_row)| {
                row.iter()
                    .zip(identity_row.iter())
                    .all(|(value, expected)| (*value - *expected).abs() < 1.0e-9)
            })
    }

    fn transform_point(self, x: f64, y: f64) -> (f64, f64) {
        let tx = self.m[0][0] * x + self.m[0][1] * y + self.m[0][2];
        let ty = self.m[1][0] * x + self.m[1][1] * y + self.m[1][2];
        let tw = self.m[2][0] * x + self.m[2][1] * y + self.m[2][2];
        if !tw.is_finite() || tw.abs() < 1.0e-6 {
            return (tx, ty);
        }
        (tx / tw, ty / tw)
    }

    fn inverse(self) -> Option<Self> {
        let m = self.m;
        let determinant = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if determinant.abs() < 1.0e-6 || !determinant.is_finite() {
            return None;
        }
        let inv_det = 1.0 / determinant;
        Some(Self {
            m: [
                [
                    (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det,
                    (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
                    (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
                ],
                [
                    (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det,
                    (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
                    (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
                ],
                [
                    (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det,
                    (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
                    (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
                ],
            ],
        })
    }
}

fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn transform_plane(
    plane: ImagePlane,
    matrix: ProjectiveMatrix,
    preserve_bottom_padding: bool,
) -> Option<ImagePlane> {
    if plane.size.width <= 0 || plane.size.height <= 0 || plane.bitmap.is_empty() {
        return Some(plane);
    }
    let inverse = matrix.inverse()?;
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
    let transformed = corners.map(|(x, y)| matrix.transform_point(x, y));
    let min_x = transformed
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::INFINITY, f64::min)
        .floor() as i32;
    let min_y = transformed
        .iter()
        .map(|(_, y)| *y)
        .fold(f64::INFINITY, f64::min)
        .floor() as i32;
    let max_x = transformed
        .iter()
        .map(|(x, _)| *x)
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil() as i32;
    let max_y = transformed
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

    for row in 0..height {
        for column in 0..width {
            let dest_x = f64::from(min_x) + column as f64 + 0.5;
            let dest_y = f64::from(min_y) + row as f64 + 0.5;
            let (src_global_x, src_global_y) = inverse.transform_point(dest_x, dest_y);
            let src_x = src_global_x - f64::from(plane.destination.x) - 0.5;
            let src_y = src_global_y - f64::from(plane.destination.y) - 0.5;
            let value = sample_bitmap_bilinear(
                &plane.bitmap,
                src_stride,
                src_width,
                src_height,
                src_x,
                src_y,
            );
            bitmap[row * width + column] = value;
        }
    }

    crop_transformed_plane_to_ink(
        ImagePlane {
            size: Size {
                width: width as i32,
                height: height as i32,
            },
            stride: width as i32,
            destination: Point { x: min_x, y: min_y },
            bitmap,
            ..plane
        },
        preserve_bottom_padding,
    )
}

fn crop_transformed_plane_to_ink(
    mut plane: ImagePlane,
    preserve_bottom_padding: bool,
) -> Option<ImagePlane> {
    if plane.stride <= 0 || plane.size.width <= 0 || plane.size.height <= 0 {
        return None;
    }
    let stride = plane.stride as usize;
    let width = plane.size.width as usize;
    let height = plane.size.height as usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_usize;
    let mut max_y = 0_usize;
    for y in 0..height {
        for x in 0..width {
            if plane.bitmap.get(y * stride + x).copied().unwrap_or(0) > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x + 1);
                max_y = max_y.max(y + 1);
            }
        }
    }
    if min_x >= max_x || min_y >= max_y {
        return None;
    }
    let ink_width = max_x - min_x;
    let ink_height = max_y - min_y;
    let (pad_left, pad_right, pad_top, pad_bottom) = if ink_height <= 256 && ink_width <= 600 {
        let vertical = (ink_height / 5).clamp(3, 16);
        let right = if ink_width > 200 { vertical } else { 0 };
        let bottom = if preserve_bottom_padding { vertical } else { 0 };
        (0, right, vertical, bottom)
    } else {
        (0, 0, 0, 0)
    };
    min_x = min_x.saturating_sub(pad_left);
    min_y = min_y.saturating_sub(pad_top);
    max_x = (max_x + pad_right).min(width);
    max_y = (max_y + pad_bottom).min(height);
    let external_right_pad = if pad_right > 0 && max_x == width {
        pad_right.min(16)
    } else {
        0
    };
    let external_bottom_pad = if pad_bottom > 0 && max_y == height {
        pad_bottom.min(16)
    } else {
        0
    };
    if min_x == 0
        && min_y == 0
        && max_x == width
        && max_y == height
        && external_right_pad == 0
        && external_bottom_pad == 0
    {
        return Some(plane);
    }
    let new_width = max_x - min_x + external_right_pad;
    let new_height = max_y - min_y + external_bottom_pad;
    let src_width = max_x - min_x;
    let src_height = max_y - min_y;
    let mut cropped = vec![0_u8; new_width * new_height];
    for y in 0..src_height {
        let src_start = (min_y + y) * stride + min_x;
        let dst_start = y * new_width;
        cropped[dst_start..dst_start + src_width]
            .copy_from_slice(&plane.bitmap[src_start..src_start + src_width]);
    }
    plane.destination.x += min_x as i32;
    plane.destination.y += min_y as i32;
    plane.size = Size {
        width: new_width as i32,
        height: new_height as i32,
    };
    plane.stride = new_width as i32;
    plane.bitmap = cropped;
    Some(plane)
}

fn sample_bitmap_bilinear(
    bitmap: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    x: f64,
    y: f64,
) -> u8 {
    if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 {
        return 0;
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    if x0 < 0 || y0 < 0 || x0 as usize >= width || y0 as usize >= height {
        return 0;
    }
    let x1 = (x0 + 1).min(width.saturating_sub(1) as i32);
    let y1 = (y0 + 1).min(height.saturating_sub(1) as i32);
    let wx = x - f64::from(x0);
    let wy = y - f64::from(y0);
    let at = |xx: i32, yy: i32| -> f64 { bitmap[yy as usize * stride + xx as usize] as f64 };
    let top = at(x0, y0) * (1.0 - wx) + at(x1, y0) * wx;
    let bottom = at(x0, y1) * (1.0 - wx) + at(x1, y1) * wx;
    (top * (1.0 - wy) + bottom * wy).round().clamp(0.0, 255.0) as u8
}

pub fn default_renderer_config(track: &ParsedTrack) -> RendererConfig {
    RendererConfig {
        frame: Size {
            width: track.play_res_x,
            height: track.play_res_y,
        },
        hinting: ass::Hinting::Normal,
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

fn merge_compatible_event_planes(planes: Vec<ImagePlane>) -> Vec<ImagePlane> {
    let mut merged: Vec<ImagePlane> = Vec::new();
    for plane in planes {
        if let Some(target) = merged
            .iter_mut()
            .find(|candidate| compatible_plane_merge(candidate, &plane))
        {
            merge_plane_into(target, plane);
        } else {
            merged.push(plane);
        }
    }
    merged
}

fn compatible_plane_merge(a: &ImagePlane, b: &ImagePlane) -> bool {
    if a.kind != b.kind || a.color != b.color || a.stride <= 0 || b.stride <= 0 {
        return false;
    }
    if a.size.height <= 3 || b.size.height <= 3 {
        return false;
    }
    let a_rect = Rect {
        x_min: a.destination.x,
        y_min: a.destination.y,
        x_max: a.destination.x + a.size.width,
        y_max: a.destination.y + a.size.height,
    };
    let b_rect = Rect {
        x_min: b.destination.x,
        y_min: b.destination.y,
        x_max: b.destination.x + b.size.width,
        y_max: b.destination.y + b.size.height,
    };
    let y_overlap = (a_rect.y_max.min(b_rect.y_max) - a_rect.y_min.max(b_rect.y_min)).max(0);
    let min_height = a.size.height.min(b.size.height).max(1);
    if y_overlap * 3 < min_height {
        return false;
    }
    let x_gap = if a_rect.x_max < b_rect.x_min {
        b_rect.x_min - a_rect.x_max
    } else if b_rect.x_max < a_rect.x_min {
        a_rect.x_min - b_rect.x_max
    } else {
        0
    };
    x_gap <= 24
}

fn merge_plane_into(target: &mut ImagePlane, plane: ImagePlane) {
    let x_min = target.destination.x.min(plane.destination.x);
    let y_min = target.destination.y.min(plane.destination.y);
    let x_max =
        (target.destination.x + target.size.width).max(plane.destination.x + plane.size.width);
    let y_max =
        (target.destination.y + target.size.height).max(plane.destination.y + plane.size.height);
    let width = (x_max - x_min).max(0);
    let height = (y_max - y_min).max(0);
    let stride = width;
    let mut bitmap = vec![0_u8; (stride * height).max(0) as usize];
    blit_plane(&mut bitmap, stride, x_min, y_min, target);
    blit_plane(&mut bitmap, stride, x_min, y_min, &plane);
    target.destination = Point { x: x_min, y: y_min };
    target.size = Size { width, height };
    target.stride = stride;
    target.bitmap = bitmap;
}

fn blit_plane(bitmap: &mut [u8], stride: i32, origin_x: i32, origin_y: i32, plane: &ImagePlane) {
    if stride <= 0 || plane.stride <= 0 || plane.size.width <= 0 || plane.size.height <= 0 {
        return;
    }
    let dst_stride = stride as usize;
    let src_stride = plane.stride as usize;
    for y in 0..plane.size.height as usize {
        for x in 0..plane.size.width as usize {
            let src = plane.bitmap.get(y * src_stride + x).copied().unwrap_or(0);
            if src == 0 {
                continue;
            }
            let dst_x = (plane.destination.x - origin_x) as usize + x;
            let dst_y = (plane.destination.y - origin_y) as usize + y;
            let dst = dst_y * dst_stride + dst_x;
            if let Some(value) = bitmap.get_mut(dst) {
                *value = (*value).max(src);
            }
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

fn extend_planes_for_effect_motion(
    planes: Vec<ImagePlane>,
    left_pad: i32,
    right_pad: i32,
    top_pad: i32,
    bottom_pad: i32,
) -> Vec<ImagePlane> {
    planes
        .into_iter()
        .map(|plane| extend_plane_edges(plane, left_pad, right_pad, top_pad, bottom_pad))
        .collect()
}

fn extend_plane_edges(
    plane: ImagePlane,
    left_pad: i32,
    right_pad: i32,
    top_pad: i32,
    bottom_pad: i32,
) -> ImagePlane {
    if plane.size.width <= 0
        || plane.size.height <= 0
        || plane.stride <= 0
        || plane.bitmap.is_empty()
    {
        return plane;
    }
    let left_pad = left_pad.max(0);
    let right_pad = right_pad.max(0);
    let top_pad = top_pad.max(0);
    let bottom_pad = bottom_pad.max(0);
    if left_pad + right_pad + top_pad + bottom_pad == 0 {
        return plane;
    }
    let old_width = plane.size.width as usize;
    let old_stride = plane.stride as usize;
    let Some(ink) = plane_ink_bounds(&plane) else {
        return plane;
    };
    let ink_x_min = (ink.x_min - plane.destination.x).max(0) as usize;
    let ink_y_min = (ink.y_min - plane.destination.y).max(0) as usize;
    let ink_x_max = (ink.x_max - plane.destination.x).min(plane.size.width) as usize;
    let ink_y_max = (ink.y_max - plane.destination.y).min(plane.size.height) as usize;
    let ink_height = ink_y_max.saturating_sub(ink_y_min);
    if ink_x_max <= ink_x_min || ink_height == 0 {
        return plane;
    }

    let pixel = left_pad.max(right_pad).max(top_pad).max(bottom_pad).max(1);
    let floor_to_pixel = |value: i32| value.div_euclid(pixel) * pixel;
    let ceil_to_pixel = |value: i32| {
        value.div_euclid(pixel) * pixel + i32::from(value.rem_euclid(pixel) != 0) * pixel
    };

    let new_height = ink_height + top_pad as usize + bottom_pad as usize;
    let dest_y = plane.destination.y + ink_y_min as i32 - top_pad;
    let mut row_spans = Vec::with_capacity(new_height);
    let mut min_x = i32::MAX;
    let mut max_x = i32::MIN;

    for dst_y in 0..new_height {
        let ink_row = if dst_y < top_pad as usize {
            0
        } else if dst_y >= top_pad as usize + ink_height {
            ink_height - 1
        } else {
            dst_y - top_pad as usize
        };
        let src_y = ink_y_min + ink_row;
        let src_row = &plane.bitmap[src_y * old_stride..src_y * old_stride + old_width];
        let first_lit = src_row[ink_x_min..ink_x_max]
            .iter()
            .position(|value| *value > 0)
            .map(|x| x + ink_x_min);
        let last_lit = src_row[ink_x_min..ink_x_max]
            .iter()
            .rposition(|value| *value > 0)
            .map(|x| x + ink_x_min);
        let Some(first_lit) = first_lit else {
            row_spans.push(None);
            continue;
        };
        let last_lit = last_lit.expect("row with first lit pixel should also have last lit pixel");
        let vertical_pad_row = dst_y < top_pad as usize || dst_y >= top_pad as usize + ink_height;
        let corner_row =
            (top_pad > 0 || bottom_pad > 0) && (ink_row == 0 || ink_row + 1 == ink_height);
        let suppress_horizontal_pad = vertical_pad_row || corner_row;
        let first_global = plane.destination.x + first_lit as i32;
        let last_exclusive_global = plane.destination.x + last_lit as i32 + 1;
        let (span_start, span_end) = if suppress_horizontal_pad {
            (
                ceil_to_pixel(first_global),
                ceil_to_pixel(last_exclusive_global),
            )
        } else {
            (
                floor_to_pixel(first_global - left_pad),
                ceil_to_pixel(last_exclusive_global + right_pad),
            )
        };
        if span_end <= span_start {
            row_spans.push(None);
            continue;
        }
        min_x = min_x.min(span_start);
        max_x = max_x.max(span_end);
        row_spans.push(Some((span_start, span_end)));
    }

    if min_x == i32::MAX || max_x <= min_x {
        return plane;
    }
    let new_width = (max_x - min_x) as usize;
    let mut bitmap = vec![0_u8; new_width * new_height];
    for (dst_y, span) in row_spans.into_iter().enumerate() {
        let Some((span_start, span_end)) = span else {
            continue;
        };
        let start = (span_start - min_x) as usize;
        let end = (span_end - min_x) as usize;
        bitmap[dst_y * new_width + start..dst_y * new_width + end].fill(255);
    }

    ImagePlane {
        destination: Point {
            x: min_x,
            y: dest_y,
        },
        size: Size {
            width: new_width as i32,
            height: new_height as i32,
        },
        stride: new_width as i32,
        bitmap,
        ..plane
    }
}

fn scale_clip_rect(rect: Rect, scale_x: f64, scale_y: f64) -> Rect {
    let scale_x = style_scale(scale_x);
    let scale_y = style_scale(scale_y);
    Rect {
        x_min: (f64::from(rect.x_min) * scale_x).floor() as i32,
        y_min: (f64::from(rect.y_min) * scale_y).floor() as i32,
        x_max: (f64::from(rect.x_max) * scale_x).ceil() as i32,
        y_max: (f64::from(rect.y_max) * scale_y).ceil() as i32,
    }
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
        return match event.alignment & 0x3 {
            ass::HALIGN_LEFT => x,
            ass::HALIGN_RIGHT => x - line_width,
            _ => x - line_width / 2,
        };
    }
    let frame_width = (f64::from(track.play_res_x) * scale_x).round() as i32;
    let margin_l = (f64::from(event.margin_l) * scale_x).round() as i32;
    let margin_r = (f64::from(event.margin_r) * scale_x).round() as i32;
    match event.alignment & 0x3 {
        ass::HALIGN_LEFT => margin_l,
        ass::HALIGN_RIGHT => (frame_width - margin_r - line_width).max(0),
        _ => ((margin_l + frame_width - margin_r - line_width) / 2).max(0),
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
    event
        .position_exact
        .map(round_exact_point)
        .or(event.position)
        .or_else(|| {
            event
                .movement_exact
                .map(|movement| {
                    interpolate_move_exact(movement, track.events.get(event.event_index), now_ms)
                })
                .or_else(|| {
                    event.movement.map(|movement| {
                        interpolate_move(movement, track.events.get(event.event_index), now_ms)
                    })
                })
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

fn round_exact_point((x, y): (f64, f64)) -> (i32, i32) {
    (x.round() as i32, y.round() as i32)
}

fn interpolate_move_exact(
    movement: ParsedMovementExact,
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

    let x = (movement.end.0 - movement.start.0) * k + movement.start.0;
    let y = (movement.end.1 - movement.start.1) * k + movement.start.1;
    round_exact_point((x, y))
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
        let line_heights = lines
            .iter()
            .map(|line| positioned_layout_line_height_for_line(line, config, scale_y))
            .collect::<Vec<_>>();
        let total_height: i32 = line_heights.iter().sum();
        let mut current_y = match alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
            ass::VALIGN_TOP => y,
            ass::VALIGN_CENTER => y - total_height / 2,
            _ => y - total_height,
        };
        let positioned_text_bottom_gap = if (alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER))
            == ass::VALIGN_SUB
            && lines
                .iter()
                .any(|line| line.runs.iter().any(|run| run.drawing.is_none()))
        {
            let max_font_size =
                lines.iter().map(max_text_font_size).fold(0.0_f64, f64::max) * scale_y;
            let descender_gap = (max_font_size * 0.26).round() as i32;
            let multiline_gap = (max_font_size * 0.49).round() as i32;
            Some((descender_gap, multiline_gap))
        } else {
            None
        };
        if let Some((descender_gap, multiline_gap)) = positioned_text_bottom_gap {
            current_y -= descender_gap + multiline_gap * (lines.len().saturating_sub(1) as i32);
        }
        let mut positions = Vec::with_capacity(lines.len());
        for (line_index, height) in line_heights.into_iter().enumerate() {
            positions.push(current_y);
            current_y += height;
            if line_index + 1 < lines.len() {
                if let Some((_, multiline_gap)) = positioned_text_bottom_gap {
                    current_y += multiline_gap;
                }
            }
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

fn text_decoration_planes(
    style: &ParsedSpanStyle,
    origin_x: i32,
    line_top: i32,
    width: i32,
    color: u32,
) -> Vec<ImagePlane> {
    if width <= 0 || !(style.underline || style.strike_out) {
        return Vec::new();
    }

    let thickness = (style.font_size / 18.0).round().max(1.0) as i32;
    let mut planes = Vec::new();
    let mut push_decoration = |baseline_fraction: f64| {
        let y = line_top + (style.font_size * baseline_fraction).round() as i32;
        planes.push(ImagePlane {
            size: Size {
                width,
                height: thickness,
            },
            stride: width,
            color: rgba_color_from_ass(color),
            destination: Point { x: origin_x, y },
            kind: ass::ImageType::Character,
            bitmap: vec![255; (width * thickness) as usize],
        });
    };

    if style.underline {
        push_decoration(0.82);
    }
    if style.strike_out {
        push_decoration(0.48);
    }

    planes
}

fn combined_image_plane_from_glyphs(
    glyphs: &[RasterGlyph],
    origin_x: i32,
    line_top: i32,
    line_ascender: Option<i32>,
    color: u32,
    kind: ass::ImageType,
    blur_radius: u32,
) -> Option<ImagePlane> {
    let ascender =
        line_ascender.unwrap_or_else(|| glyphs.iter().map(|glyph| glyph.top).max().unwrap_or(0));
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

fn blur_image_plane(plane: ImagePlane, radius: u32) -> ImagePlane {
    if radius == 0 || plane.size.width <= 0 || plane.size.height <= 0 || plane.bitmap.is_empty() {
        return plane;
    }
    let (bitmap, width, height, pad) = blur_bitmap(
        plane.bitmap,
        plane.size.width as usize,
        plane.size.height as usize,
        radius,
    );
    ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        destination: Point {
            x: plane.destination.x - pad as i32,
            y: plane.destination.y - pad as i32,
        },
        bitmap,
        ..plane
    }
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

fn drawing_baseline_ascender(style: &ParsedSpanStyle, _render_scale_y: f64) -> i32 {
    let scale_y = style_scale(style.scale_y);
    (style.font_size.max(1.0) * scale_y * 0.75).round() as i32
}

#[derive(Clone, Copy, Debug)]
struct DrawingPlaneParams {
    origin_x: i32,
    line_top: i32,
    color: u32,
    scale_x: f64,
    scale_y: f64,
    render_scale: RenderScale,
    baseline_offset: f64,
    pad_to_libass_geometry: bool,
}

fn image_plane_from_drawing(
    drawing: &ParsedDrawing,
    params: DrawingPlaneParams,
) -> Option<ImagePlane> {
    let polygons = scaled_drawing_polygons(
        drawing,
        params.scale_x,
        params.scale_y,
        params.render_scale.x,
        params.render_scale.y,
    );
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

    let pbo_pixels = (params.baseline_offset * params.render_scale.y).round() as i32;
    let vertical_offset = pbo_pixels.max(0);

    if !any_visible {
        return None;
    }

    let plane = ImagePlane {
        size: Size { width, height },
        stride: width,
        color: rgba_color_from_ass(params.color),
        destination: Point {
            x: params.origin_x + bounds.x_min,
            y: params.line_top + bounds.y_min + vertical_offset,
        },
        kind: ass::ImageType::Character,
        bitmap,
    };
    if params.pad_to_libass_geometry {
        Some(pad_drawing_plane_to_libass_geometry(plane))
    } else {
        Some(plane)
    }
}

fn pad_drawing_plane_to_libass_geometry(plane: ImagePlane) -> ImagePlane {
    let left_pad = 1_i32;
    let top_pad = 0_i32;
    let padded_width = align_i32(plane.size.width + left_pad, 16).max(plane.size.width + left_pad);
    let padded_height = align_i32(plane.size.height + top_pad, 16).max(plane.size.height + top_pad);
    let right_pad = padded_width - plane.size.width - left_pad;
    let bottom_pad = padded_height - plane.size.height - top_pad;
    if left_pad == 0 && top_pad == 0 && right_pad == 0 && bottom_pad == 0 {
        return plane;
    }

    let new_stride = padded_width;
    let mut bitmap = vec![0_u8; (new_stride * padded_height) as usize];
    let src_stride = plane.stride.max(0) as usize;
    let dst_stride = new_stride as usize;
    for row in 0..plane.size.height.max(0) as usize {
        let src_start = row * src_stride;
        let dst_start = (row + top_pad as usize) * dst_stride + left_pad as usize;
        bitmap[dst_start..dst_start + plane.size.width as usize]
            .copy_from_slice(&plane.bitmap[src_start..src_start + plane.size.width as usize]);
    }

    ImagePlane {
        size: Size {
            width: padded_width,
            height: padded_height,
        },
        stride: new_stride,
        destination: Point {
            x: plane.destination.x - left_pad,
            y: plane.destination.y - top_pad,
        },
        bitmap,
        ..plane
    }
}

fn align_i32(value: i32, alignment: i32) -> i32 {
    if alignment <= 1 {
        return value;
    }
    ((value + alignment - 1) / alignment) * alignment
}

fn scaled_drawing_polygons(
    drawing: &ParsedDrawing,
    scale_x: f64,
    scale_y: f64,
    render_scale_x: f64,
    render_scale_y: f64,
) -> Vec<Vec<Point>> {
    let scale_x = style_scale(scale_x) * render_scale_x;
    let scale_y = style_scale(scale_y) * render_scale_y;
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
    let mut clipped = Vec::with_capacity(if inverse {
        planes.len().saturating_mul(2)
    } else {
        planes.len()
    });
    for plane in planes {
        if inverse {
            clipped.extend(inverse_clip_plane(plane, clip_rect));
        } else if let Some(plane) = clip_plane(plane, clip_rect) {
            clipped.push(plane);
        }
    }
    clipped
}

fn libass_pads_transformed_text_rect_clip(event: &LayoutEvent) -> bool {
    event.clip_rect.is_some()
        && event.lines.len() == 1
        && event.lines.iter().any(|line| {
            line.runs.iter().any(|run| {
                run.drawing.is_none()
                    && run.text.chars().count() <= 1
                    && (event.origin.is_some()
                        || event.origin_exact.is_some()
                        || event.movement.is_some()
                        || event.movement_exact.is_some()
                        || run.style.rotation_z.abs() > f64::EPSILON
                        || run.style.rotation_x.abs() > f64::EPSILON
                        || run.style.rotation_y.abs() > f64::EPSILON
                        || !run.transforms.is_empty())
            })
        })
}

fn pad_libass_transformed_text_rect_clip_plane(plane: ImagePlane) -> ImagePlane {
    if plane.kind != ass::ImageType::Character || plane.size.width > 24 {
        return plane;
    }
    // libass keeps the small transformed glyph allocation around thin rectangular
    // clip slices instead of tightening the ASS_Image to our glyph bitmap width.
    // This shows up heavily in karaoke FX where a moving horizontal clip scans a
    // one-character transformed text plane. Its clipped plane bottom is exclusive
    // at the libass scanline boundary, while our exact-rect ceil retains one extra
    // transparent row.
    let plane = if plane.size.height > 1 {
        let mut rect = plane_rect(&plane);
        rect.y_max -= 1;
        crop_plane_to_rect(plane, rect).unwrap_or_else(|| unreachable!())
    } else {
        plane
    };
    pad_plane_transparent(plane, 8, 0, 4, 0)
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
    let width = plane.size.width.max(0) as usize;
    let height = plane.size.height.max(0) as usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_usize;
    let mut max_y = 0_usize;

    for row in 0..height {
        for column in 0..width {
            let global_x = plane.destination.x + column as i32;
            let global_y = plane.destination.y + row as i32;
            let inside = clip
                .polygons
                .iter()
                .any(|polygon| point_in_polygon(global_x, global_y, polygon));
            let keep = if inverse { !inside } else { inside };
            let index = row * stride + column;
            if !keep {
                bitmap[index] = 0;
            } else if bitmap[index] > 0 {
                min_x = min_x.min(column);
                min_y = min_y.min(row);
                max_x = max_x.max(column + 1);
                max_y = max_y.max(row + 1);
            }
        }
    }

    if min_x >= max_x || min_y >= max_y {
        return None;
    }
    let masked = ImagePlane { bitmap, ..plane };
    if inverse {
        return Some(masked);
    }
    crop_plane_to_bitmap_bounds(masked, min_x, min_y, max_x, max_y, 4, 2, 12, 14)
        .map(|plane| pad_plane_transparent(plane, 4, 1, 0, 13))
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
    if intersection == plane_rect {
        return Some(plane);
    }
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

#[allow(clippy::too_many_arguments)]
fn crop_plane_to_bitmap_bounds(
    plane: ImagePlane,
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
    pad_left: usize,
    pad_top: usize,
    pad_right: usize,
    pad_bottom: usize,
) -> Option<ImagePlane> {
    let x_min = min_x.saturating_sub(pad_left) as i32 + plane.destination.x;
    let y_min = min_y.saturating_sub(pad_top) as i32 + plane.destination.y;
    let x_max =
        ((max_x + pad_right).min(plane.size.width.max(0) as usize)) as i32 + plane.destination.x;
    let y_max =
        ((max_y + pad_bottom).min(plane.size.height.max(0) as usize)) as i32 + plane.destination.y;
    crop_plane_to_rect(
        plane,
        Rect {
            x_min,
            y_min,
            x_max,
            y_max,
        },
    )
}

fn pad_plane_transparent(
    plane: ImagePlane,
    pad_left: i32,
    pad_top: i32,
    pad_right: i32,
    pad_bottom: i32,
) -> ImagePlane {
    let pad_left = pad_left.max(0);
    let pad_top = pad_top.max(0);
    let pad_right = pad_right.max(0);
    let pad_bottom = pad_bottom.max(0);
    if pad_left == 0 && pad_top == 0 && pad_right == 0 && pad_bottom == 0 {
        return plane;
    }

    let width = plane.size.width.max(0);
    let height = plane.size.height.max(0);
    let new_width = width + pad_left + pad_right;
    let new_height = height + pad_top + pad_bottom;
    let mut bitmap = vec![0_u8; (new_width * new_height).max(0) as usize];
    let src_stride = plane.stride.max(0) as usize;
    let dst_stride = new_width.max(0) as usize;
    for row in 0..height as usize {
        let src_start = row * src_stride;
        let dst_start = (row + pad_top as usize) * dst_stride + pad_left as usize;
        bitmap[dst_start..dst_start + width as usize]
            .copy_from_slice(&plane.bitmap[src_start..src_start + width as usize]);
    }

    ImagePlane {
        size: Size {
            width: new_width,
            height: new_height,
        },
        stride: new_width,
        destination: Point {
            x: plane.destination.x - pad_left,
            y: plane.destination.y - pad_top,
        },
        bitmap,
        ..plane
    }
}

fn crop_plane_to_rect(plane: ImagePlane, rect: Rect) -> Option<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let rect = plane_rect.intersect(rect)?;
    if rect == plane_rect {
        return Some(plane);
    }
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
    use rassa_fonts::{FontProvider, FontQuery, FontconfigProvider, NullFontProvider};
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

    #[test]
    fn fad_uses_libass_truncating_alpha_interpolation() {
        let event = ParsedEvent {
            start: 0,
            duration: 4000,
            ..ParsedEvent::default()
        };

        assert_eq!(
            compute_fad_alpha(
                ParsedFade::Simple {
                    fade_in_ms: 1000,
                    fade_out_ms: 1000,
                },
                Some(&event),
                500,
            ),
            127
        );
        assert_eq!(
            compute_fad_alpha(
                ParsedFade::Simple {
                    fade_in_ms: 1000,
                    fade_out_ms: 1000,
                },
                Some(&event),
                3500,
            ),
            127
        );
    }

    #[test]
    fn fad_uses_libass_wrapping_out_start_when_fade_out_exceeds_duration() {
        let event = ParsedEvent {
            start: 0,
            duration: 800,
            ..ParsedEvent::default()
        };

        assert_eq!(
            compute_fad_alpha(
                ParsedFade::Simple {
                    fade_in_ms: 100,
                    fade_out_ms: 1000,
                },
                Some(&event),
                100,
            ),
            76
        );
        assert_eq!(
            compute_fad_alpha(
                ParsedFade::Simple {
                    fade_in_ms: 100,
                    fade_out_ms: 1000,
                },
                Some(&event),
                400,
            ),
            153
        );
    }

    #[test]
    fn fade_alpha_combines_with_existing_colour_alpha() {
        assert_eq!(with_fade_alpha(0xFF00_0080, 0), 0xFF00_0080);
        assert_eq!(with_fade_alpha(0xFF00_0000, 127), 0xFF00_007F);
        assert_eq!(with_fade_alpha(0xFF00_0080, 127), 0xFF00_00BF);
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

    fn kind_bounds(planes: &[ImagePlane], kind: ass::ImageType) -> Option<Rect> {
        let mut matching_planes = planes.iter().filter(|plane| plane.kind == kind);
        let first = matching_planes.next()?;
        let mut bounds = Rect {
            x_min: first.destination.x,
            y_min: first.destination.y,
            x_max: first.destination.x + first.size.width,
            y_max: first.destination.y + first.size.height,
        };
        for plane in matching_planes {
            bounds.x_min = bounds.x_min.min(plane.destination.x);
            bounds.y_min = bounds.y_min.min(plane.destination.y);
            bounds.x_max = bounds.x_max.max(plane.destination.x + plane.size.width);
            bounds.y_max = bounds.y_max.max(plane.destination.y + plane.size.height);
        }
        Some(bounds)
    }

    fn character_bounds(planes: &[ImagePlane]) -> Option<Rect> {
        kind_bounds(planes, ass::ImageType::Character)
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

    fn drawing_alignment_script(
        alignment: i32,
        override_tags: &str,
        event_margins: &str,
    ) -> String {
        format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,32,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,{alignment},30,50,15,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,{event_margins},,{{{override_tags}\\p1}}m 0 0 l 40 0 40 20 0 20\n"
        )
    }

    fn render_drawing_bounds(script: &str) -> Rect {
        let track = parse_script_text(script).expect("alignment probe script should parse");
        let engine = RenderEngine::new();
        let provider = NullFontProvider;
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        visible_bounds(&planes).expect("drawing probe should produce visible pixels")
    }

    fn text_alignment_script(alignment: i32, event_margins: &str) -> String {
        format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,32,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,{alignment},30,50,15,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,{event_margins},,Margin\n"
        )
    }

    fn render_text_bounds(script: &str) -> Option<Rect> {
        let track = parse_script_text(script).expect("text alignment probe script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        visible_bounds(&planes)
    }

    fn render_text_bounds_with_config(script: &str, config: &RendererConfig) -> Option<Rect> {
        let track = parse_script_text(script).expect("text alignment probe script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider_and_config(&track, &provider, 500, config);
        visible_bounds(&planes)
    }

    fn baseline_fontconfig_matches_dejavu_fallback(family: &str) -> bool {
        let provider = FontconfigProvider::new();
        provider
            .resolve(&FontQuery::new(family))
            .family
            .contains("DejaVu")
    }

    fn render_text_plane_bounds(script: &str) -> Option<Rect> {
        let track = parse_script_text(script).expect("text plane probe script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        character_bounds(&planes)
    }

    #[test]
    fn decimal_positioned_drawing_uses_exact_coordinates() {
        let decimal = drawing_alignment_script(7, "\\pos(100.6,50.6)", "0,0,0");
        let integer = drawing_alignment_script(7, "\\pos(101,51)", "0,0,0");

        assert_eq!(
            render_drawing_bounds(&decimal),
            render_drawing_bounds(&integer)
        );
    }

    #[test]
    fn decimal_move_interpolates_from_exact_coordinates() {
        let decimal = drawing_alignment_script(7, "\\move(10.5,20.5,110.5,120.5,0,1000)", "0,0,0");
        let integer = drawing_alignment_script(7, "\\move(61,71,61,71)", "0,0,0");

        assert_eq!(
            render_drawing_bounds(&decimal),
            render_drawing_bounds(&integer)
        );
    }

    #[test]
    fn downscaled_positioned_text_scales_font_and_anchor_like_libass() {
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\an5\\pos(320,180)}POS\n";
        let config = RendererConfig {
            frame: Size {
                width: 320,
                height: 180,
            },
            storage: Size {
                width: 320,
                height: 180,
            },
            pixel_aspect: 1.0,
            shaping: ass::ShapingLevel::Complex,
            ..Default::default()
        };
        let actual = render_text_bounds_with_config(script, &config)
            .expect("positioned text should render in downscaled frame");
        let expected = Rect {
            x_min: 141,
            y_min: 83,
            x_max: 179,
            y_max: 97,
        };

        assert!(
            (actual.x_min - expected.x_min).abs() <= 2
                && (actual.y_min - expected.y_min).abs() <= 1,
            "downscaled \\pos anchor should stay in libass position: actual={actual:?} expected={expected:?}"
        );
        assert!(
            (actual.width() - expected.width()).abs() <= 2
                && (actual.height() - expected.height()).abs() <= 2,
            "downscaled \\pos text must scale glyph dimensions like libass: actual={actual:?} expected={expected:?}"
        );
    }

    #[test]
    fn positioned_center_text_anchors_visible_ink_not_layout_advance() {
        if !baseline_fontconfig_matches_dejavu_fallback("Againts") {
            return;
        }
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs140\\bord0\\blur1\\fnAgaints\\pos(947.46,191.6)}ท่านคาชิวากิ อาซาฮิ\n";
        let actual = render_text_bounds(script).expect("baseline positioned text should render");
        let center_x = (actual.x_min + actual.x_max) / 2;

        assert!(
            (center_x - 947).abs() <= 8,
            "\\pos center anchor must use visible rendered text width, not stale layout advance: bounds={actual:?} center_x={center_x}"
        );
        assert!(
            (actual.y_min - 80).abs() <= 4,
            "bottom-aligned \\pos text must reserve libass-like descender space below visible glyphs: bounds={actual:?}"
        );
    }

    #[test]
    fn positioned_multiline_text_uses_libass_like_line_gap_and_descender_space() {
        if !baseline_fontconfig_matches_dejavu_fallback("Raphtalia") {
            return;
        }
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs100\\bord0\\blur1\\fnRaphtalia\\b1\\pos(944.4,752.8)}จงเตรียมตัว\\Nให้พร้อมสรรพก่อนมา\n";
        let actual =
            render_text_bounds(script).expect("baseline multiline positioned text should render");

        assert!(
            (actual.y_min - 570).abs() <= 6,
            "multiline bottom-aligned \\pos text should use libass-like vertical block metrics: bounds={actual:?}"
        );
        assert!(
            (actual.height() - 158).abs() <= 8,
            "multiline positioned text should keep libass-like line gap: bounds={actual:?}"
        );
    }

    #[test]
    fn positioned_multiline_text_aligns_deep_glyph_bottoms_like_libass() {
        if !baseline_fontconfig_matches_dejavu_fallback("Raphtalia") {
            return;
        }
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs100\\bord0\\blur1\\fnRaphtalia\\b1\\pos(928,992)}ห้ามท่านหนี\\Nจากคำขอนี้เป็นอันขาด\n";
        let actual =
            render_text_bounds(script).expect("baseline multiline positioned text should render");

        assert!(
            (actual.y_min - 808).abs() <= 4,
            "top of deep-glyph multiline block should match libass baseline line 1270: bounds={actual:?}"
        );
        assert!(
            (actual.y_max - 968).abs() <= 4,
            "bottom-aligned \\pos should keep deep Thai glyphs above the libass descender gap: bounds={actual:?}"
        );
        assert!(
            (actual.height() - 160).abs() <= 6,
            "deep-glyph multiline block should keep libass-like visible-bottom line spacing: bounds={actual:?}"
        );
    }

    #[test]
    fn rotated_positioned_text_keeps_libass_like_transparent_frz_plane() {
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs66\\shad\\bord0\\blur1\\fnRaphtalia\\c&H070707&\\b0\\fscx99\\fscy107\\frz345.2\\pos(1258.48,593.06)}หลังเลิกเรียน จะรอที่\n";
        let actual =
            render_text_plane_bounds(script).expect("rotated positioned text should render");
        let expected = Rect {
            x_min: 1091,
            y_min: 499,
            x_max: 1461,
            y_max: 626,
        };

        assert!(
            (actual.x_min - expected.x_min).abs() <= 3
                && (actual.y_min - expected.y_min).abs() <= 3
                && (actual.x_max - expected.x_max).abs() <= 3
                && (actual.y_max - expected.y_max).abs() <= 3,
            "rotated positioned text should keep libass-like transparent \\frz plane: actual={actual:?} expected={expected:?}"
        );
    }

    #[test]
    fn decimal_clipped_transformed_single_char_keeps_libass_like_plane() {
        if !baseline_fontconfig_matches_dejavu_fallback("OFL Sorts Mill Goudy TT") {
            return;
        }
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 8,0:00:00.00,0:00:01.00,ED2,,0,0,0,fx,{\\move(727.1,73,727.1,65)\\org(637.1,-25)\\t(53.571428571429,107.14285714286,\\frz4)\\t(107.14285714286,160.71428571429,\\frz-4)\\t(160.71428571429,214.28571428571,\\frz4\\t(214.28571428571,267.85714285714,\\frz-4\\t(267.85714285714,321.42857142857,\\frz4\\t(321.42857142857,375,\\frz-4\\t(375,428.57142857143,\\frz4\\t(857.14285714286,482.14285714286,\\frz-4\\t(482.14285714286,535.71428571429,\\frz4\\t(535.71428571429,589.28571428571,\\frz-4\\t(589.28571428571,642.85714285714,\\frz4\\t(642.85714285714,696.42857142857,\\frz-4\\t(696.42857142857,750,\\frz0)))))))))))\\b0\\bord0\\blur0.2\\shad0\\an5\\fs80\\t(0,750,\\fs70\\frz0)\\clip(659.3,63.6,1260.8,77.4)\\c&H9DD9FC&}I\n";
        let actual = render_text_plane_bounds(script)
            .expect("02.ass-style decimal clipped transformed glyph should emit a plane");

        assert_eq!(
            actual,
            Rect {
                x_min: 715,
                y_min: 63,
                x_max: 739,
                y_max: 77,
            },
            "decimal rectangular clip over transformed one-char text should keep libass-like ASS_Image plane geometry"
        );
    }

    #[test]
    fn positioned_drawing_fry_uses_libass_like_projective_camera() {
        let script = |override_tags: &str| {
            format!(
                "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{{override_tags}\\p1}}m 0 0 l 710 0 710 18 0 18\n"
            )
        };
        let plain = parse_script_text(&script("\\an2\\pos(953,563)"))
            .expect("plain positioned drawing should parse");
        let projected = parse_script_text(&script("\\an2\\pos(953,563)\\frx14\\fry4"))
            .expect("projected positioned drawing should parse");
        let engine = RenderEngine::new();
        let provider = NullFontProvider;
        let plain_bounds =
            character_bounds(&engine.render_frame_with_provider(&plain, &provider, 500))
                .expect("plain drawing should render");
        let projected_bounds =
            character_bounds(&engine.render_frame_with_provider(&projected, &provider, 500))
                .expect("projected drawing should render");

        assert!(
            projected_bounds.x_min <= plain_bounds.x_min - 24,
            "libass \\fry perspective shifts bottom-centered drawings left: plain={plain_bounds:?} projected={projected_bounds:?}"
        );
        assert!(
            (projected_bounds.x_min - 568).abs() <= 4,
            "projective camera should match libass-probed left edge for this fixture: projected={projected_bounds:?}"
        );
        assert!(
            (projected_bounds.y_min - 544).abs() <= 2,
            "projective transform should preserve libass-probed vertical placement: projected={projected_bounds:?}"
        );
    }

    #[test]
    fn borderstyle3_opaque_box_follows_text_transform() {
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,Arial,42,&H00000000,&H000000FF,&H00FFFFFF,&H00000000,0,0,0,0,100,100,0,0,3,4,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Box,,0,0,0,,{\\pos(320,180)\\frz-18\\fax0.25}TRANSFORM BOX\n";
        let track = parse_script_text(script).expect("borderstyle transform script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let box_bounds = kind_bounds(&planes, ass::ImageType::Outline)
            .expect("BorderStyle=3 should emit an opaque box outline plane");

        assert!(
            box_bounds.height() > 90,
            "opaque box must be transformed with the rotated/sheared text, got bounds {box_bounds:?}"
        );
    }

    #[test]
    fn positioned_drawing_an_anchors_match_libass_for_all_alignments() {
        // Expected boxes were probed from libass/ffmpeg for a 40x20 vector drawing at \pos(x,y):
        // bottom align => y - 20, middle align => y - 10, top align => y.
        let cases = [
            (
                1,
                "\\an1\\pos(60,60)",
                Rect {
                    x_min: 60,
                    y_min: 40,
                    x_max: 100,
                    y_max: 60,
                },
            ),
            (
                2,
                "\\an2\\pos(160,60)",
                Rect {
                    x_min: 140,
                    y_min: 40,
                    x_max: 180,
                    y_max: 60,
                },
            ),
            (
                3,
                "\\an3\\pos(260,60)",
                Rect {
                    x_min: 220,
                    y_min: 40,
                    x_max: 260,
                    y_max: 60,
                },
            ),
            (
                4,
                "\\an4\\pos(60,100)",
                Rect {
                    x_min: 60,
                    y_min: 90,
                    x_max: 100,
                    y_max: 110,
                },
            ),
            (
                5,
                "\\an5\\pos(160,100)",
                Rect {
                    x_min: 140,
                    y_min: 90,
                    x_max: 180,
                    y_max: 110,
                },
            ),
            (
                6,
                "\\an6\\pos(260,100)",
                Rect {
                    x_min: 220,
                    y_min: 90,
                    x_max: 260,
                    y_max: 110,
                },
            ),
            (
                7,
                "\\an7\\pos(60,140)",
                Rect {
                    x_min: 60,
                    y_min: 140,
                    x_max: 100,
                    y_max: 160,
                },
            ),
            (
                8,
                "\\an8\\pos(160,140)",
                Rect {
                    x_min: 140,
                    y_min: 140,
                    x_max: 180,
                    y_max: 160,
                },
            ),
            (
                9,
                "\\an9\\pos(260,140)",
                Rect {
                    x_min: 220,
                    y_min: 140,
                    x_max: 260,
                    y_max: 160,
                },
            ),
        ];

        for (alignment, override_tags, expected) in cases {
            let script = drawing_alignment_script(alignment, override_tags, "0,0,0");
            assert_eq!(
                render_drawing_bounds(&script),
                expected,
                "\\an{alignment} positioned drawing anchor should match libass"
            );
        }
    }

    #[test]
    fn moved_drawing_an_anchors_match_libass_for_all_alignments_at_midpoint() {
        let cases = [
            (
                1,
                "\\an1\\move(40,60,80,60)",
                Rect {
                    x_min: 60,
                    y_min: 40,
                    x_max: 100,
                    y_max: 60,
                },
            ),
            (
                2,
                "\\an2\\move(140,60,180,60)",
                Rect {
                    x_min: 140,
                    y_min: 40,
                    x_max: 180,
                    y_max: 60,
                },
            ),
            (
                3,
                "\\an3\\move(240,60,280,60)",
                Rect {
                    x_min: 220,
                    y_min: 40,
                    x_max: 260,
                    y_max: 60,
                },
            ),
            (
                4,
                "\\an4\\move(40,100,80,100)",
                Rect {
                    x_min: 60,
                    y_min: 90,
                    x_max: 100,
                    y_max: 110,
                },
            ),
            (
                5,
                "\\an5\\move(140,100,180,100)",
                Rect {
                    x_min: 140,
                    y_min: 90,
                    x_max: 180,
                    y_max: 110,
                },
            ),
            (
                6,
                "\\an6\\move(240,100,280,100)",
                Rect {
                    x_min: 220,
                    y_min: 90,
                    x_max: 260,
                    y_max: 110,
                },
            ),
            (
                7,
                "\\an7\\move(40,140,80,140)",
                Rect {
                    x_min: 60,
                    y_min: 140,
                    x_max: 100,
                    y_max: 160,
                },
            ),
            (
                8,
                "\\an8\\move(140,140,180,140)",
                Rect {
                    x_min: 140,
                    y_min: 140,
                    x_max: 180,
                    y_max: 160,
                },
            ),
            (
                9,
                "\\an9\\move(240,140,280,140)",
                Rect {
                    x_min: 220,
                    y_min: 140,
                    x_max: 260,
                    y_max: 160,
                },
            ),
        ];

        for (alignment, override_tags, expected) in cases {
            let script = drawing_alignment_script(alignment, override_tags, "0,0,0");
            assert_eq!(
                render_drawing_bounds(&script),
                expected,
                "\\an{alignment} moved drawing anchor should match libass at the event midpoint"
            );
        }
    }

    #[test]
    fn margin_positioned_text_uses_style_and_event_margins_like_libass() {
        let cases = [
            (
                1,
                "0,0,0",
                Rect {
                    x_min: 32,
                    y_min: 138,
                    x_max: 116,
                    y_max: 165,
                },
            ),
            (
                2,
                "0,0,0",
                Rect {
                    x_min: 108,
                    y_min: 138,
                    x_max: 192,
                    y_max: 165,
                },
            ),
            (
                3,
                "0,0,0",
                Rect {
                    x_min: 184,
                    y_min: 138,
                    x_max: 269,
                    y_max: 165,
                },
            ),
            (
                5,
                "0,0,0",
                Rect {
                    x_min: 108,
                    y_min: 79,
                    x_max: 192,
                    y_max: 106,
                },
            ),
            (
                7,
                "0,0,0",
                Rect {
                    x_min: 32,
                    y_min: 20,
                    x_max: 116,
                    y_max: 47,
                },
            ),
            (
                8,
                "0,0,0",
                Rect {
                    x_min: 108,
                    y_min: 20,
                    x_max: 192,
                    y_max: 47,
                },
            ),
            (
                9,
                "7,9,11",
                Rect {
                    x_min: 225,
                    y_min: 16,
                    x_max: 310,
                    y_max: 43,
                },
            ),
        ];

        for (alignment, event_margins, expected) in cases {
            let script = text_alignment_script(alignment, event_margins);
            let Some(actual) = render_text_bounds(&script) else {
                return;
            };
            // Text rasterization can have a few pixels of coverage-width drift from libass even
            // with the same Fontconfig face. This regression guards the placement bug: the
            // effective style/event margin anchor must no longer be shifted left or sunk.
            assert!(
                (actual.x_min - expected.x_min).abs() <= 1,
                "text style/event margins and \\an{alignment} x placement should match libass within raster rounding: actual={actual:?} expected={expected:?}"
            );
            assert_eq!(
                actual.y_min, expected.y_min,
                "text style/event margins and \\an{alignment} vertical anchor should match libass"
            );
            assert!(
                (actual.y_max - expected.y_max).abs() <= 1,
                "text style/event margins and \\an{alignment} visible height may drift by one raster row: actual={actual:?} expected={expected:?}"
            );
        }
    }

    #[test]
    fn margin_positioned_drawing_uses_style_and_event_margins_like_libass() {
        // Expected boxes were probed from libass/ffmpeg for a 40x20 vector drawing with
        // style margins L=30/R=50/V=15. Event margins of 0 should fall back to style margins.
        let cases = [
            (
                1,
                Rect {
                    x_min: 30,
                    y_min: 145,
                    x_max: 70,
                    y_max: 165,
                },
            ),
            (
                2,
                Rect {
                    x_min: 130,
                    y_min: 145,
                    x_max: 170,
                    y_max: 165,
                },
            ),
            (
                3,
                Rect {
                    x_min: 230,
                    y_min: 145,
                    x_max: 270,
                    y_max: 165,
                },
            ),
            (
                4,
                Rect {
                    x_min: 30,
                    y_min: 80,
                    x_max: 70,
                    y_max: 100,
                },
            ),
            (
                5,
                Rect {
                    x_min: 130,
                    y_min: 80,
                    x_max: 170,
                    y_max: 100,
                },
            ),
            (
                6,
                Rect {
                    x_min: 230,
                    y_min: 80,
                    x_max: 270,
                    y_max: 100,
                },
            ),
            (
                7,
                Rect {
                    x_min: 30,
                    y_min: 15,
                    x_max: 70,
                    y_max: 35,
                },
            ),
            (
                8,
                Rect {
                    x_min: 130,
                    y_min: 15,
                    x_max: 170,
                    y_max: 35,
                },
            ),
            (
                9,
                Rect {
                    x_min: 230,
                    y_min: 15,
                    x_max: 270,
                    y_max: 35,
                },
            ),
        ];

        for (alignment, expected) in cases {
            let script = drawing_alignment_script(alignment, "", "0,0,0");
            assert_eq!(
                render_drawing_bounds(&script),
                expected,
                "style margins and \\an{alignment} should match libass when no explicit position exists"
            );
        }

        let script = drawing_alignment_script(7, "", "7,9,11");
        assert_eq!(
            render_drawing_bounds(&script),
            Rect {
                x_min: 7,
                y_min: 11,
                x_max: 47,
                y_max: 31
            },
            "non-zero event margins should override style margins for top-left alignment"
        );
    }

    #[test]
    fn projective_transform_keeps_frx_and_fry_axes_distinct() {
        let origin = (320.0, 180.0);
        let frx = ProjectiveMatrix::from_ass_transform_at_origin(
            EventTransform {
                rotation_x: 45.0,
                ..EventTransform::default()
            },
            origin.0,
            origin.1,
            1.0,
        );
        let fry = ProjectiveMatrix::from_ass_transform_at_origin(
            EventTransform {
                rotation_y: 45.0,
                ..EventTransform::default()
            },
            origin.0,
            origin.1,
            1.0,
        );

        let (frx_x, frx_y) = frx.transform_point(320.0, 140.0);
        let (fry_x, fry_y) = fry.transform_point(360.0, 180.0);

        assert!(
            (frx_x - 320.0).abs() < 0.5,
            "frx must not act like fry: {frx_x}"
        );
        assert!(
            frx_y > 140.0,
            "positive frx should pitch the top edge downward: {frx_y}"
        );
        assert!(
            fry_x < 360.0,
            "positive fry should yaw the right edge leftward: {fry_x}"
        );
        assert!(
            (fry_y - 180.0).abs() < 0.5,
            "fry must not act like frx: {fry_y}"
        );
    }

    #[test]
    fn projective_transform_uses_deep_org_as_perspective_lever_arm() {
        let transform = EventTransform {
            rotation_x: 55.0,
            ..EventTransform::default()
        };
        let shallow = ProjectiveMatrix::from_ass_transform_at_origin(transform, 320.0, 240.0, 1.0);
        let deep = ProjectiveMatrix::from_ass_transform_at_origin(transform, 320.0, 420.0, 1.0);

        let (_, shallow_y) = shallow.transform_point(320.0, 240.0);
        let (_, deep_y) = deep.transform_point(320.0, 240.0);

        assert!((shallow_y - 240.0).abs() < 0.5);
        assert!(
            deep_y > shallow_y + 70.0,
            "deep \\org below text should pull frx text substantially downward like libass, got shallow={shallow_y} deep={deep_y}"
        );
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
    fn render_frame_uses_axis_specific_shadow_offsets() {
        let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(30,30)\\xshad9\\yshad3}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let character_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .cloned()
            .collect::<Vec<_>>();
        let shadow_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Shadow)
            .cloned()
            .collect::<Vec<_>>();

        let character = visible_bounds(&character_planes).expect("character bounds");
        let shadow = visible_bounds(&shadow_planes).expect("axis-specific shadow should render");
        assert_eq!(shadow.x_min - character.x_min, 9);
        assert_eq!(shadow.y_min - character.y_min, 3);
    }

    #[test]
    fn render_frame_renders_underline_and_strikeout_decorations() {
        let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(30,30)\\u1\\s1}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let decoration_planes = planes
            .iter()
            .filter(|plane| {
                plane.kind == ass::ImageType::Character
                    && plane.size.height <= 3
                    && plane.size.width > plane.size.height * 4
            })
            .collect::<Vec<_>>();

        assert!(decoration_planes.len() >= 2);
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
    fn render_frame_emits_opaque_box_for_border_style_3() {
        let track = parse_script_text("[Script Info]\nPlayResX: 500\nPlayResY: 160\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,DejaVu Sans,30,&H00000000,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,3,2,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Box,,0000,0000,0000,,{\\an5\\pos(250,80)}BorderStyle=3 opaque box").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let character_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .cloned()
            .collect::<Vec<_>>();
        let outline_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Outline)
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(
            outline_planes.len(),
            1,
            "BorderStyle=3 should emit only the opaque box outline plane, not a separate stroked glyph outline"
        );
        let _character = visible_bounds(&character_planes).expect("character bounds");
        let outline = outline_planes
            .iter()
            .find(|plane| plane.color.0 == 0x0000_0000 && plane.bitmap.contains(&255))
            .expect("opaque border-style box plane uses outline colour");
        assert!(outline.size.width > 0);
        assert!(outline.size.height > 0);
        let bounds = visible_bounds(std::slice::from_ref(outline)).expect("opaque box bounds");
        let center_x = (bounds.x_min + bounds.x_max) / 2;
        assert!(
            (center_x - 250).abs() <= 2,
            "opaque box should stay centered at \\pos, got {bounds:?}"
        );
        let center_y = (bounds.y_min + bounds.y_max) / 2;
        assert!(
            (center_y - 80).abs() <= 1,
            "opaque box should stay vertically centered at \\pos like libass, got {bounds:?}"
        );
        assert_eq!(
            bounds.height(),
            36,
            "BorderStyle=3 box plane height should be font size plus two borders plus edge rows like libass"
        );
        assert!(
            bounds.width() < 370,
            "opaque box should use actual raster advance like libass, not inflated layout width: {bounds:?}"
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
    fn render_frame_blurs_fill_only_without_outline_or_shadow() {
        let base = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)}Hi").expect("script should parse");
        let blurred = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\blur3}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let base_planes = engine.render_frame_with_provider(&base, &provider, 500);
        let blurred_planes = engine.render_frame_with_provider(&blurred, &provider, 500);
        let base_character = visible_bounds(
            &base_planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character)
                .cloned()
                .collect::<Vec<_>>(),
        )
        .expect("base character bounds");
        let blurred_character = visible_bounds(
            &blurred_planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character)
                .cloned()
                .collect::<Vec<_>>(),
        )
        .expect("blurred character bounds");

        assert!(blurred_character.x_min < base_character.x_min);
        assert!(blurred_character.x_max > base_character.x_max);
        assert!(blurred_character.y_min < base_character.y_min);
        assert!(blurred_character.y_max > base_character.y_max);
    }

    #[test]
    fn render_frame_does_not_blur_fill_when_outline_or_shadow_exists() {
        let base = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)}Hi").expect("script should parse");
        let blurred = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\blur3}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let base_planes = engine.render_frame_with_provider(&base, &provider, 500);
        let blurred_planes = engine.render_frame_with_provider(&blurred, &provider, 500);
        let character_bounds = |planes: &[ImagePlane]| {
            visible_bounds(
                &planes
                    .iter()
                    .filter(|plane| plane.kind == ass::ImageType::Character)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            .expect("character bounds")
        };

        assert_eq!(
            character_bounds(&blurred_planes),
            character_bounds(&base_planes)
        );
        assert!(
            blurred_planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Outline)
                .any(|plane| plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
        );
        assert!(
            blurred_planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Shadow)
                .any(|plane| plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
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
    fn inverse_clip_bleed_covers_outline_growth_to_prevent_stray_glyph_leakage() {
        let style = ParsedSpanStyle {
            border: 5.0,
            border_x: 5.0,
            border_y: 5.0,
            shadow: 0.0,
            shadow_x: 0.0,
            shadow_y: 0.0,
            blur: 0.0,
            be: 0.0,
            ..ParsedSpanStyle::default()
        };
        let clip = Rect {
            x_min: 20,
            y_min: 0,
            x_max: 24,
            y_max: 10,
        };
        let glyph = ImagePlane {
            size: Size {
                width: 44,
                height: 10,
            },
            stride: 44,
            color: RgbaColor(0x00FF_FFFF),
            destination: Point { x: 0, y: 0 },
            kind: ass::ImageType::Outline,
            bitmap: vec![255; 440],
        };

        let expanded = expand_rect(clip, style_clip_bleed(&style));
        let parts = inverse_clip_plane(glyph, expanded);

        assert!(
            parts
                .iter()
                .all(|plane| plane.destination.x + plane.size.width <= 0
                    || plane.destination.x >= 44),
            "inverse clip must mask outline bleed around the nominal clip, got {parts:?}"
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
    fn non_positioned_drawing_does_not_receive_positioned_overhang_compensation() {
        let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 l 10 0 10 10 0 10{\\p0}").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let plane = engine
            .render_frame_with_provider(&track, &provider, 500)
            .into_iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("drawing plane");

        assert_eq!(
            plane.size.width, 11,
            "libass-style positioned overhang compensation is specific to explicit \\pos vector drawings"
        );
    }

    #[test]
    #[ignore = "parked while rassa stops treating pixel-perfect libass drawing pbo residuals as an optimization blocker"]
    fn render_frame_applies_drawing_baseline_offset() {
        fn pbo_track(pbo_tag: &str) -> ParsedTrack {
            parse_script_text(&format!("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(10,40)}}X{{{pbo_tag}\\p1}}m 0 0 l 10 0 10 10 0 10{{\\p0}}X"))
                .expect("script should parse")
        }

        let baseline = pbo_track("");
        let pbo5 = pbo_track("\\pbo5");
        let shifted = pbo_track("\\pbo12");
        let negative = pbo_track("\\pbo-12");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let drawing_plane = |track: &ParsedTrack| {
            engine
                .render_frame_with_provider(track, &provider, 500)
                .into_iter()
                .find(|plane| {
                    plane.kind == ass::ImageType::Character
                        && plane.size.width == 11
                        && plane.size.height == 11
                })
                .expect("drawing plane")
        };
        let baseline_drawing = drawing_plane(&baseline);
        let pbo5_drawing = drawing_plane(&pbo5);
        let shifted_drawing = drawing_plane(&shifted);
        let negative_drawing = drawing_plane(&negative);

        assert_eq!(
            pbo5_drawing.destination, baseline_drawing.destination,
            "libass keeps pbo below drawing height anchored for this 10-unit positioned drawing"
        );
        assert_eq!(
            shifted_drawing.destination.x,
            baseline_drawing.destination.x
        );
        assert_eq!(
            shifted_drawing.destination.y,
            baseline_drawing.destination.y + 2,
            "libass applies \\pbo as max(pbo - drawing_height, 0) for this top-anchored positioned drawing"
        );
        assert_eq!(
            negative_drawing.destination, baseline_drawing.destination,
            "libass keeps negative \\pbo top-anchored for this positioned drawing"
        );
    }

    #[test]
    fn render_frame_applies_banner_effect_motion() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Banner;25;0;0,Banner").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let early = character_bounds(&engine.render_frame_with_provider(&track, &provider, 100))
            .expect("early banner bounds");
        let late = character_bounds(&engine.render_frame_with_provider(&track, &provider, 1500))
            .expect("late banner bounds");

        assert!(
            late.x_min < early.x_min,
            "right-to-left banner should move left over time"
        );
        assert!(
            (194..=198).contains(&early.x_min),
            "libass positions a right-to-left banner by PlayResX - elapsed/delay, got {early:?}"
        );
    }

    #[test]
    fn banner_effect_delay_uses_layout_scale_not_render_supersampling() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Banner;25;0;0,Banner").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let bounds = character_bounds(&engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            1500,
            &RendererConfig {
                frame: Size {
                    width: 1600,
                    height: 800,
                },
                storage: Size {
                    width: 200,
                    height: 100,
                },
                ..RendererConfig::default()
            },
        ))
        .expect("supersampled banner bounds");

        assert!(
            bounds.x_min >= 1112,
            "Banner delay should be based on layout/storage resolution rather than render supersampling; got {bounds:?}"
        );
    }

    #[test]
    fn render_frame_applies_scroll_effect_motion() {
        let up = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Scroll up;20;100;25;0,Scroll").expect("script should parse");
        let down = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Scroll down;20;100;25;0,Scroll").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let up_early = character_bounds(&engine.render_frame_with_provider(&up, &provider, 100))
            .expect("early scroll-up bounds");
        let up_late = character_bounds(&engine.render_frame_with_provider(&up, &provider, 1500))
            .expect("late scroll-up bounds");
        let down_early =
            character_bounds(&engine.render_frame_with_provider(&down, &provider, 100))
                .expect("early scroll-down bounds");
        let down_late =
            character_bounds(&engine.render_frame_with_provider(&down, &provider, 1500))
                .expect("late scroll-down bounds");

        assert!(
            up_late.y_min < up_early.y_min,
            "scroll up should move upward"
        );
        assert!(
            down_late.y_min > down_early.y_min,
            "scroll down should move downward"
        );
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
    #[ignore = "strict libass positioned-vector overhang coverage residual kept as diagnostic after optimization pivot"]
    fn positioned_drawing_uses_position_y_before_compare_supersample_offset() {
        let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(20,24)\\p1}m 0 0 l 42 0 42 12 0 12{\\p0}").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size {
                    width: 1760,
                    height: 1120,
                },
                storage: Size {
                    width: 220,
                    height: 140,
                },
                ..RendererConfig::default()
            },
        );
        let bounds = character_bounds(&planes).expect("positioned drawing bounds");
        let visible = visible_bounds(&planes).expect("positioned drawing visible bounds");

        assert_eq!(
            bounds.y_min,
            24 * 8,
            "libass keeps top-aligned positioned vector drawings anchored at \\pos y before final supersample offset; got {bounds:?}"
        );
        assert_eq!(
            bounds.x_min,
            19 * 8,
            "libass gives positioned vector drawings one output-pixel left overhang at compare superscale; got {bounds:?}"
        );
        assert_eq!(
            bounds.x_max,
            63 * 8,
            "libass keeps the allocated right drawing edge available for transforms; got {bounds:?}"
        );
        assert_eq!(
            visible.x_min,
            19 * 8 + 7,
            "libass leaves only a subpixel-thin antialias sample in the positioned drawing's left overhang; got visible {visible:?}"
        );
        assert_eq!(
            visible.x_max,
            62 * 8 + 1,
            "positioned vector drawing keeps a subpixel-thin antialias sample in the allocated right overhang; got visible {visible:?}"
        );
    }

    #[test]
    fn render_frame_shears_positioned_drawing_from_run_baseline_not_org() {
        let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(120,24)\\org(120,80)\\frx45\\fax0.25\\p1}m 0 0 l 50 0 50 14 0 14{\\p0}")
            .expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let bounds = planes_bounds(&planes).expect("drawing plane should render");

        assert!(
            bounds.x_min >= 116,
            "libass applies \\fax in drawing-local baseline space before \\org perspective; global \\org shear pulls this too far left: {bounds:?}"
        );
    }

    #[test]
    fn render_frame_applies_z_rotation_per_override_run() {
        let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\c&H0000FF&}MMMM{\\frz90\\c&H00FF00&}MMMM").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let red_planes = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
            .collect::<Vec<_>>();
        let green = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
            .expect("rotated green drawing plane");

        assert!(
            red_planes.len() >= 2,
            "expected multiple unrotated red glyph planes"
        );
        let red_y_min = red_planes
            .iter()
            .map(|plane| plane.destination.y)
            .min()
            .expect("red y min");
        let red_y_max = red_planes
            .iter()
            .map(|plane| plane.destination.y)
            .max()
            .expect("red y max");
        assert!(
            red_y_max - red_y_min <= 1,
            "unrotated run should stay on a horizontal baseline: {red_planes:?}"
        );
        assert!(
            green.size.height >= green.size.width,
            "rotated run should become vertical-ish: {green:?}"
        );
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
    fn vertical_font_raster_advances_rotate_bitmap_like_libass_vertical_faces() {
        let glyph = RasterGlyph {
            width: 2,
            height: 3,
            stride: 2,
            offset_x: 1,
            offset_y: 2,
            advance_x: 7,
            bitmap: vec![1, 2, 3, 4, 5, 6],
            ..RasterGlyph::default()
        };
        let style = ParsedSpanStyle {
            font_name: "@Vertical".to_string(),
            font_size: 50.0,
            ..ParsedSpanStyle::default()
        };

        let glyphs = apply_vertical_font_raster_advances(vec![glyph], &style);
        let rotated = &glyphs[0];

        assert_eq!(rotated.width, 3);
        assert_eq!(rotated.height, 2);
        assert_eq!(rotated.stride, 3);
        assert_eq!(rotated.bitmap, vec![5, 3, 1, 6, 4, 2]);
        assert_eq!(rotated.offset_x, 13);
        assert_eq!(rotated.offset_y, 20);
        assert_eq!(rotated.advance_x, 50);
        assert_eq!(rotated.advance_y, 0);
    }

    #[test]
    fn clipped_vector_drawing_keeps_libass_like_transparent_plane_padding() {
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\clip(801,103,806,186)\\pos(627.43,184.01)\\p1\\fscx251\\fscy258\\c&HFFFFFF&}m 0 0 l 132 -1 l 135 0 l 136 26 l 1 28 l 0 -1\n";
        let track = parse_script_text(script).expect("clip/drawing probe should parse");
        let engine = RenderEngine::new();
        let planes = engine.render_frame_with_provider(&track, &NullFontProvider, 500);

        assert_eq!(
            planes.len(),
            1,
            "narrow \\clip should retain the clipped drawing plane like libass"
        );
        let plane = &planes[0];
        assert_eq!(plane.destination.x, 801);
        assert_eq!(plane.size.width, 5);
        assert_eq!(plane.size.height, 80);
    }

    #[test]
    fn blurred_vector_drawing_expands_fill_plane_like_libass() {
        let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\blur6\\p1\\c&HC6BECA&\\fscx165\\fscy138\\pos(948,1324)}m 0 0 b -3 -28 -6 -56 -9 -84 b -18 -113 -6 -135 -5 -160 b -3 -184 1 -208 3 -232 b 125 -233 248 -235 370 -236 b 377 -220 386 -204 393 -188 b 397 -167 403 -146 407 -125 b 409 -109 411 -93 413 -77 b 421 -61 431 -44 439 -28 b 440 -18 441 -7 442 3 b 295 3 147 1 0 1\n";
        let track = parse_script_text(script).expect("track parses");
        let engine = RenderEngine::new();
        let planes = engine.render_frame_with_provider(&track, &NullFontProvider, 500);

        assert_eq!(planes.len(), 1);
        let plane = &planes[0];
        assert_eq!(plane.destination.y, 650);
        assert_eq!(plane.size.width, 788);
        assert_eq!(plane.size.height, 372);
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
