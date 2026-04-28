use rassa_core::{ass, ImagePlane, Point, Rect, RendererConfig, RgbaColor, Size};
use rassa_fonts::{FontProvider, FontconfigProvider};
use rassa_layout::{LayoutEngine, LayoutEvent, LayoutGlyphRun};
use rassa_parse::{ParsedDrawing, ParsedEvent, ParsedFade, ParsedKaraokeMode, ParsedMovement, ParsedSpanStyle, ParsedTrack, ParsedVectorClip};
use rassa_raster::{RasterGlyph, RasterOptions, Rasterizer};
use rassa_shape::ShapingMode;

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

fn layout_line_height(config: &RendererConfig) -> i32 {
    let extra_spacing = if config.line_spacing.is_finite() {
        config.line_spacing.round() as i32
    } else {
        0
    };
    (LINE_HEIGHT + extra_spacing).max(1)
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

        RenderSelection { active_event_indices }
    }

    pub fn prepare_frame<P: FontProvider>(&self, track: &ParsedTrack, provider: &P, now_ms: i64) -> PreparedFrame {
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
            .filter_map(|index| self.layout.layout_track_event_with_mode(track, index, provider, shaping_mode).ok())
            .collect();

        PreparedFrame { now_ms, active_events }
    }

    pub fn render_frame_with_provider<P: FontProvider>(&self, track: &ParsedTrack, provider: &P, now_ms: i64) -> Vec<ImagePlane> {
        self.render_frame_with_provider_and_config(track, provider, now_ms, &default_renderer_config(track))
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
        let mut occupied_bounds = Vec::new();

        for event in &prepared.active_events {
            let Some(_style) = track.styles.get(event.style_index) else {
                continue;
            };
            let mut shadow_planes = Vec::new();
            let mut outline_planes = Vec::new();
            let mut character_planes = Vec::new();
            let effective_position = resolve_event_position(track, event, now_ms);
            let vertical_layout = resolve_vertical_layout(track, event, effective_position, &occupied_bounds, config);
            let occupied_bound = effective_position.is_none().then(|| event_bounds(track, event, &vertical_layout, effective_position, config));
            for (line, line_top) in event.lines.iter().zip(vertical_layout.iter().copied()) {
                let origin_x = compute_horizontal_origin(track, event, scaled_line_width(line.width, config), effective_position);
                let mut line_pen_x = 0;
                for run in &line.runs {
                    let effective_style = apply_renderer_style_scale(
                        resolve_run_style(run, track.events.get(event.event_index), now_ms),
                        track,
                        config,
                    );
                    if let Some(drawing) = &run.drawing {
                        if let Some(plane) = image_plane_from_drawing(
                            drawing,
                            origin_x + line_pen_x,
                            line_top,
                            resolve_run_fill_color(run, &effective_style, track.events.get(event.event_index), now_ms),
                        ) {
                            if effective_style.border > 0.0 {
                                let mut outline_glyph = plane_to_raster_glyph(&plane);
                                let rasterizer = Rasterizer::with_options(RasterOptions {
                                    pixel_height: 1,
                                    hinting: config.hinting,
                                });
                                let mut outline_glyphs = rasterizer.outline_glyphs(
                                    &[outline_glyph.clone()],
                                    effective_style.border.round().max(1.0) as i32,
                                );
                                if effective_style.blur > 0.0 {
                                    outline_glyphs = rasterizer.blur_glyphs(&outline_glyphs, effective_style.blur.round().max(1.0) as u32);
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
                                    pixel_height: 1,
                                    hinting: config.hinting,
                                });
                                let mut shadow_glyph = plane_to_raster_glyph(character_planes.last().expect("drawing plane"));
                                if effective_style.blur > 0.0 {
                                    shadow_glyph = rasterizer.blur_glyphs(&[shadow_glyph], effective_style.blur.round().max(1.0) as u32).into_iter().next().expect("shadow glyph");
                                }
                                shadow_planes.extend(image_planes_from_absolute_glyphs(
                                    &[RasterGlyph {
                                        left: shadow_glyph.left + effective_style.shadow.round() as i32,
                                        top: shadow_glyph.top - effective_style.shadow.round() as i32,
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
                        pixel_height: effective_style.font_size.max(1.0).round() as u32,
                        hinting: config.hinting,
                    });
                    let Ok(raster_glyphs) = rasterizer.rasterize_glyphs(&run.font, &run.glyphs) else {
                        line_pen_x += run.width.round() as i32;
                        continue;
                    };
                    if effective_style.border > 0.0 && !karaoke_hides_outline(run, track.events.get(event.event_index), now_ms) {
                        let mut outline_glyphs = rasterizer.outline_glyphs(&raster_glyphs, effective_style.border.round().max(1.0) as i32);
                        if effective_style.blur > 0.0 {
                            outline_glyphs = rasterizer.blur_glyphs(&outline_glyphs, effective_style.blur.round().max(1.0) as u32);
                        }
                        outline_planes.extend(image_planes_from_glyphs_with_kind(
                            &outline_glyphs,
                            origin_x + line_pen_x,
                            line_top,
                            effective_style.outline_colour,
                            ass::ImageType::Outline,
                        ));
                    }
                    character_planes.extend(apply_karaoke_to_character_planes(
                        image_planes_from_glyphs(
                        &raster_glyphs,
                        origin_x + line_pen_x,
                        line_top,
                        resolve_run_fill_color(run, &effective_style, track.events.get(event.event_index), now_ms),
                        ),
                        run,
                        &effective_style,
                        track.events.get(event.event_index),
                        now_ms,
                        origin_x + line_pen_x,
                        raster_glyphs.iter().map(|glyph| glyph.advance_x).sum::<i32>(),
                    ));
                    if effective_style.shadow > 0.0 {
                        let mut shadow_glyphs = raster_glyphs.clone();
                        if effective_style.blur > 0.0 {
                            shadow_glyphs = rasterizer.blur_glyphs(&shadow_glyphs, effective_style.blur.round().max(1.0) as u32);
                        }
                        shadow_planes.extend(image_planes_from_glyphs_with_kind(
                            &shadow_glyphs,
                            origin_x + line_pen_x + effective_style.shadow.round() as i32,
                            line_top + effective_style.shadow.round() as i32,
                            effective_style.back_colour,
                            ass::ImageType::Shadow,
                        ));
                    }
                    line_pen_x += raster_glyphs.iter().map(|glyph| glyph.advance_x).sum::<i32>();
                }
            }

            let mut event_planes = shadow_planes;
            event_planes.extend(outline_planes);
            event_planes.extend(character_planes);
            if let Some(clip_rect) = event.clip_rect {
                event_planes = apply_event_clip(event_planes, clip_rect, event.inverse_clip);
            } else if let Some(vector_clip) = &event.vector_clip {
                event_planes = apply_vector_clip(event_planes, vector_clip, event.inverse_clip);
            }
            if let Some(fade) = event.fade {
                event_planes = apply_fade_to_planes(event_planes, fade, track.events.get(event.event_index), now_ms);
            }
            event_planes = scale_output_planes(event_planes, track, config);
            event_planes = apply_event_clip(event_planes, frame_clip_rect(track, config, event, effective_position), false);
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

fn resolve_run_fill_color(run: &LayoutGlyphRun, style: &ParsedSpanStyle, source_event: Option<&ParsedEvent>, now_ms: i64) -> u32 {
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

fn karaoke_hides_outline(run: &LayoutGlyphRun, source_event: Option<&ParsedEvent>, now_ms: i64) -> bool {
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
                plane.color = RgbaColor(if relative >= karaoke.duration_ms { style.primary_colour } else { style.secondary_colour });
                plane
            })
            .collect(),
        ParsedKaraokeMode::Sweep => {
            if relative <= 0 {
                return planes
                    .into_iter()
                    .map(|mut plane| {
                        plane.color = RgbaColor(style.secondary_colour);
                        plane
                    })
                    .collect();
            }
            if relative >= karaoke.duration_ms {
                return planes
                    .into_iter()
                    .map(|mut plane| {
                        plane.color = RgbaColor(style.primary_colour);
                        plane
                    })
                    .collect();
            }

            let progress = f64::from(relative) / f64::from(karaoke.duration_ms.max(1));
            let split_x = run_origin_x + (f64::from(run_width.max(0)) * progress).round() as i32;
            let mut result = Vec::new();
            for plane in planes {
                if let Some(mut left) = clip_plane_horizontally(&plane, plane.destination.x, split_x) {
                    left.color = RgbaColor(style.primary_colour);
                    result.push(left);
                }
                if let Some(mut right) = clip_plane_horizontally(&plane, split_x, plane.destination.x + plane.size.width) {
                    right.color = RgbaColor(style.secondary_colour);
                    result.push(right);
                }
            }
            result
        }
    }
}

fn clip_plane_horizontally(plane: &ImagePlane, clip_left: i32, clip_right: i32) -> Option<ImagePlane> {
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

fn resolve_run_style(run: &LayoutGlyphRun, source_event: Option<&ParsedEvent>, now_ms: i64) -> ParsedSpanStyle {
    let Some(event) = source_event else {
        return run.style.clone();
    };

    let mut style = run.style.clone();
    let elapsed = (now_ms - event.start).clamp(0, event.duration.max(0)) as i32;
    for transform in &run.transforms {
        let start_ms = transform.start_ms.max(0);
        let end_ms = transform.end_ms.unwrap_or(event.duration.max(0) as i32).max(start_ms);
        let progress = if elapsed <= start_ms {
            0.0
        } else if elapsed >= end_ms {
            1.0
        } else {
            let linear = f64::from(elapsed - start_ms) / f64::from((end_ms - start_ms).max(1));
            linear.powf(if transform.accel > 0.0 { transform.accel } else { 1.0 })
        };

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

fn apply_renderer_style_scale(mut style: ParsedSpanStyle, track: &ParsedTrack, config: &RendererConfig) -> ParsedSpanStyle {
    let scale = renderer_font_scale(config);
    if (scale - 1.0).abs() >= f64::EPSILON {
        style.font_size *= scale;
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

fn scaled_line_width(width: f32, config: &RendererConfig) -> i32 {
    (f64::from(width) * renderer_font_scale(config)).round() as i32
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
        let value = f64::from(from_channel) + (f64::from(to_channel) - f64::from(from_channel)) * progress;
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
        ParsedFade::Simple { fade_in_ms, fade_out_ms } => {
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

fn interpolate_alpha(now: i64, t1: i32, t2: i32, t3: i32, t4: i32, a1: i32, a2: i32, a3: i32) -> i32 {
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
    let base_alpha = (color & 0xFF) as u8;
    let combined = mult_alpha(base_alpha, fade_alpha);
    (color & 0xFFFF_FF00) | u32::from(combined)
}

fn mult_alpha(a: u8, b: u8) -> u8 {
    let a = u32::from(a);
    let b = u32::from(b);
    (a - ((a * b + 0x7F) / 0xFF) + b) as u8
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
    if layout_resolution(track).is_some() || !(config.pixel_aspect.is_finite() && config.pixel_aspect > 0.0) {
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
    let frame_width = if config.frame.width > 0 { config.frame.width } else { track.play_res_x };
    let frame_height = if config.frame.height > 0 { config.frame.height } else { track.play_res_y };

    Size {
        width: (frame_width - config.margins.left - config.margins.right).max(0),
        height: (frame_height - config.margins.top - config.margins.bottom).max(0),
    }
}

fn output_mapping_size(track: &ParsedTrack, config: &RendererConfig) -> Size {
    if config.use_margins {
        Size {
            width: if config.frame.width > 0 { config.frame.width } else { track.play_res_x },
            height: if config.frame.height > 0 { config.frame.height } else { track.play_res_y },
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

fn scale_output_planes(planes: Vec<ImagePlane>, track: &ParsedTrack, config: &RendererConfig) -> Vec<ImagePlane> {
    let scale_x = output_scale_x(track, config);
    let scale_y = output_scale_y(track, config);
    let offset = output_offset(config);
    if (scale_x - 1.0).abs() < f64::EPSILON && (scale_y - 1.0).abs() < f64::EPSILON && offset == Point::default() {
        return planes;
    }

    planes
        .into_iter()
        .filter_map(|plane| scale_plane(plane, scale_x, scale_y, offset))
        .collect()
}

fn scale_plane(plane: ImagePlane, scale_x: f64, scale_y: f64, offset: Point) -> Option<ImagePlane> {
    let src_width = plane.size.width.max(0) as usize;
    let src_height = plane.size.height.max(0) as usize;
    if src_width == 0 || src_height == 0 || plane.bitmap.is_empty() {
        return None;
    }

    let left = (f64::from(plane.destination.x) * scale_x).round() as i32 + offset.x;
    let top = (f64::from(plane.destination.y) * scale_y).round() as i32 + offset.y;
    let right = (f64::from(plane.destination.x + plane.size.width) * scale_x).round() as i32 + offset.x;
    let bottom = (f64::from(plane.destination.y + plane.size.height) * scale_y).round() as i32 + offset.y;
    let dst_width = (right - left).max(1) as usize;
    let dst_height = (bottom - top).max(1) as usize;
    let src_stride = plane.stride.max(0) as usize;
    let mut bitmap = vec![0_u8; dst_width * dst_height];

    for row in 0..dst_height {
        let src_row = ((row * src_height) / dst_height).min(src_height - 1);
        for column in 0..dst_width {
            let src_column = ((column * src_width) / dst_width).min(src_width - 1);
            bitmap[row * dst_width + column] = plane.bitmap[src_row * src_stride + src_column];
        }
    }

    Some(ImagePlane {
        size: Size {
            width: dst_width as i32,
            height: dst_height as i32,
        },
        stride: dst_width as i32,
        destination: Point { x: left, y: top },
        bitmap,
        ..plane
    })
}

fn frame_clip_rect(track: &ParsedTrack, config: &RendererConfig, event: &LayoutEvent, effective_position: Option<(i32, i32)>) -> Rect {
    let frame_width = if config.frame.width > 0 { config.frame.width } else { track.play_res_x.max(0) };
    let frame_height = if config.frame.height > 0 { config.frame.height } else { track.play_res_y.max(0) };
    if config.use_margins && effective_position.is_none() && event.clip_rect.is_none() && event.vector_clip.is_none() {
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

fn compute_horizontal_origin(track: &ParsedTrack, event: &LayoutEvent, line_width: i32, effective_position: Option<(i32, i32)>) -> i32 {
    if let Some((x, _)) = effective_position {
        return x;
    }
    match event.alignment & 0x3 {
        ass::HALIGN_LEFT => event.margin_l,
        ass::HALIGN_RIGHT => (track.play_res_x - event.margin_r - line_width).max(0),
        _ => ((track.play_res_x - line_width) / 2).max(0),
    }
}

fn resolve_event_position(track: &ParsedTrack, event: &LayoutEvent, now_ms: i64) -> Option<(i32, i32)> {
    event.position.or_else(|| {
        event.movement.map(|movement| interpolate_move(movement, track.events.get(event.event_index), now_ms))
    })
}

fn interpolate_move(movement: ParsedMovement, source_event: Option<&ParsedEvent>, now_ms: i64) -> (i32, i32) {
    let event_duration = source_event.map(|event| event.duration).unwrap_or_default().max(0) as i32;
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
) -> Vec<i32> {
    if let Some((_, y)) = position {
        let line_height = layout_line_height(config);
        let mut positions = Vec::with_capacity(lines.len());
        let mut current_y = y;
        for _ in lines {
            positions.push(current_y);
            current_y += line_height;
        }
        return positions;
    }
    let line_heights = vec![layout_line_height(config); lines.len()];
    let total_height: i32 = line_heights.iter().sum();
    let default_start_y = match alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
        ass::VALIGN_TOP => margin_v,
        ass::VALIGN_CENTER => ((track.play_res_y - total_height) / 2).max(0),
        _ => (track.play_res_y - margin_v - total_height).max(0),
    };

    let line_position = config.line_position.clamp(0.0, 100.0);
    let start_y = if (alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER)) == ass::VALIGN_SUB && line_position > 0.0 {
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
) -> Vec<i32> {
    let mut vertical_layout = compute_vertical_layout(track, &event.lines, event.alignment, event.margin_v, effective_position, config);
    if effective_position.is_some() || occupied_bounds.is_empty() {
        return vertical_layout;
    }

    let line_height = layout_line_height(config);
    let shift = match event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
        ass::VALIGN_TOP => line_height,
        ass::VALIGN_CENTER => line_height,
        _ => -line_height,
    };

    let mut bounds = event_bounds(track, event, &vertical_layout, effective_position, config);
    let frame_height = track.play_res_y.max(0);
    while occupied_bounds.iter().any(|occupied| bounds.intersect(*occupied).is_some()) {
        for line_top in &mut vertical_layout {
            *line_top += shift;
        }
        bounds = event_bounds(track, event, &vertical_layout, effective_position, config);
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
) -> Rect {
    let mut x_min = i32::MAX;
    let mut y_min = i32::MAX;
    let mut x_max = i32::MIN;
    let mut y_max = i32::MIN;

    for (line, line_top) in event.lines.iter().zip(vertical_layout.iter().copied()) {
        let line_width = scaled_line_width(line.width, config);
        let origin_x = compute_horizontal_origin(track, event, line_width, effective_position);
        x_min = x_min.min(origin_x);
        y_min = y_min.min(line_top);
        x_max = x_max.max(origin_x + line_width);
        y_max = y_max.max(line_top + layout_line_height(config));
    }

    if x_min == i32::MAX {
        Rect::default()
    } else {
        Rect { x_min, y_min, x_max, y_max }
    }
}

fn image_planes_from_glyphs(glyphs: &[RasterGlyph], origin_x: i32, line_top: i32, color: u32) -> Vec<ImagePlane> {
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
            color: RgbaColor(color),
            destination: Point {
                x: origin_x + pen_x + glyph.left,
                y: baseline_y - glyph.top,
            },
            kind,
            bitmap: glyph.bitmap.clone(),
        });
        pen_x += glyph.advance_x;
    }

    planes
}

fn image_planes_from_absolute_glyphs(glyphs: &[RasterGlyph], color: u32, kind: ass::ImageType) -> Vec<ImagePlane> {
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
                color: RgbaColor(color),
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

fn image_plane_from_drawing(drawing: &ParsedDrawing, origin_x: i32, line_top: i32, color: u32) -> Option<ImagePlane> {
    let bounds = drawing.bounds()?;
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
            if drawing.polygons.iter().any(|polygon| point_in_polygon(x, y, polygon)) {
                bitmap[row * stride + column] = 255;
                any_visible = true;
            }
        }
    }

    any_visible.then_some(ImagePlane {
        size: Size { width, height },
        stride: width,
        color: RgbaColor(color),
        destination: Point {
            x: origin_x + bounds.x_min,
            y: line_top + bounds.y_min,
        },
        kind: ass::ImageType::Character,
        bitmap,
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

fn apply_vector_clip(planes: Vec<ImagePlane>, clip: &ParsedVectorClip, inverse: bool) -> Vec<ImagePlane> {
    planes
        .into_iter()
        .filter_map(|plane| mask_plane_with_vector_clip(plane, clip, inverse))
        .collect()
}

fn mask_plane_with_vector_clip(plane: ImagePlane, clip: &ParsedVectorClip, inverse: bool) -> Option<ImagePlane> {
    let mut bitmap = plane.bitmap.clone();
    let stride = plane.stride as usize;
    let mut any_visible = false;

    for row in 0..plane.size.height as usize {
        for column in 0..plane.size.width as usize {
            let global_x = plane.destination.x + column as i32;
            let global_y = plane.destination.y + row as i32;
            let inside = clip.polygons.iter().any(|polygon| point_in_polygon(global_x, global_y, polygon));
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
            let x_intersection = (previous_x - current_x) * (sample_y - current_y) / (previous_y - current_y) + current_x;
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
        Rect { x_min: plane_rect.x_min, y_min: plane_rect.y_min, x_max: plane_rect.x_max, y_max: intersection.y_min },
        Rect { x_min: plane_rect.x_min, y_min: intersection.y_max, x_max: plane_rect.x_max, y_max: plane_rect.y_max },
        Rect { x_min: plane_rect.x_min, y_min: intersection.y_min, x_max: intersection.x_min, y_max: intersection.y_max },
        Rect { x_min: intersection.x_max, y_min: intersection.y_min, x_max: plane_rect.x_max, y_max: intersection.y_max },
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

    fn config(frame_width: i32, frame_height: i32, margins: rassa_core::Margins, use_margins: bool) -> RendererConfig {
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
        planes.iter().map(|plane| plane.size.width * plane.size.height).sum()
    }

    fn vertical_span(planes: &[ImagePlane]) -> i32 {
        let min_y = planes.iter().map(|plane| plane.destination.y).min().expect("plane");
        let max_y = planes
            .iter()
            .map(|plane| plane.destination.y + plane.size.height)
            .max()
            .expect("plane");
        max_y - min_y
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

        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x0011_2233));
        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x0044_5566));
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
        assert_eq!(first_character.color.0, 0x0000_FF00);
    }

    #[test]
    fn render_frame_orders_shadow_outline_before_character_within_event() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00111111,&H0000FFFF,&H00222222,&H00333333,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let kinds = planes.iter().map(|plane| plane.kind).collect::<Vec<_>>();

        let first_shadow = kinds.iter().position(|kind| *kind == ass::ImageType::Shadow).expect("shadow plane");
        let first_outline = kinds.iter().position(|kind| *kind == ass::ImageType::Outline).expect("outline plane");
        let first_character = kinds.iter().position(|kind| *kind == ass::ImageType::Character).expect("character plane");

        assert!(first_shadow < first_outline);
        assert!(first_outline < first_character);
    }

    #[test]
    fn render_frame_emits_outline_planes_for_border_override() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord3\\3c&H0A0B0C&}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x000A_0B0C));
    }

    #[test]
    fn render_frame_blurs_outline_and_shadow_layers() {
        let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord2\\blur2\\3c&H0A0B0C&\\shad2}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Outline && plane.bitmap.iter().any(|value| *value > 0 && *value < 255)));
        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Shadow && plane.bitmap.iter().any(|value| *value > 0 && *value < 255)));
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
        assert!(planes.iter().all(|plane| plane.destination.x + plane.size.width <= 64));
        assert!(planes.iter().all(|plane| plane.destination.y + plane.size.height <= 64));
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
            size: Size { width: 6, height: 4 },
            stride: 6,
            color: RgbaColor(0x00FF_FFFF),
            destination: Point { x: 0, y: 0 },
            kind: ass::ImageType::Character,
            bitmap: vec![255; 24],
        };
        let parts = inverse_clip_plane(plane, Rect { x_min: 2, y_min: 1, x_max: 4, y_max: 3 });

        assert_eq!(parts.len(), 4);
        assert_eq!(parts.iter().map(|plane| plane.bitmap.len()).sum::<usize>(), 20);
    }

    #[test]
    fn render_frame_applies_vector_clip() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(m 0 0 l 32 0 32 32 0 32)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(!planes.is_empty());
        assert!(planes.iter().all(|plane| plane.bitmap.iter().any(|value| *value > 0)));
        assert!(planes.iter().all(|plane| plane.destination.x >= 0));
        assert!(planes.iter().all(|plane| plane.destination.y >= 0));
    }

    #[test]
    fn render_frame_clips_to_frame_bounds() {
        let plane = ImagePlane {
            size: Size { width: 20, height: 20 },
            stride: 20,
            color: RgbaColor(0x00FF_FFFF),
            destination: Point { x: 50, y: 50 },
            kind: ass::ImageType::Character,
            bitmap: vec![255; 400],
        };
        let clipped = apply_event_clip(vec![plane], Rect { x_min: 0, y_min: 0, x_max: 60, y_max: 60 }, false);

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
            &config(100, 100, rassa_core::Margins { top: 10, bottom: 10, left: 10, right: 10 }, true),
        );

        assert!(!planes.is_empty());
        assert!(planes.iter().all(|plane| plane.destination.x >= 10));
        assert!(planes.iter().all(|plane| plane.destination.y >= 10));
        assert!(planes.iter().all(|plane| plane.destination.x + plane.size.width <= 90));
        assert!(planes.iter().all(|plane| plane.destination.y + plane.size.height <= 90));
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
            &config(120, 120, rassa_core::Margins { top: 10, bottom: 10, left: 10, right: 10 }, false),
        );

        assert!(!planes.is_empty());
        assert!(planes.iter().all(|plane| plane.destination.x >= 10));
        assert!(planes.iter().all(|plane| plane.destination.y >= 10));
        assert!(planes.iter().all(|plane| plane.destination.x + plane.size.width <= 110));
        assert!(planes.iter().all(|plane| plane.destination.y + plane.size.height <= 110));
    }

    #[test]
    fn render_frame_keeps_border_closer_to_device_size_when_scaled_border_is_disabled() {
        let enabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
        let disabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: no\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let config = config(200, 200, rassa_core::Margins::default(), true);
        let enabled_planes = engine.render_frame_with_provider_and_config(&enabled, &provider, 500, &config);
        let disabled_planes = engine.render_frame_with_provider_and_config(&disabled, &provider, 500, &config);
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
                frame: Size { width: 200, height: 120 },
                font_scale: 2.0,
                ..RendererConfig::default()
            },
        );

        assert!(!baseline.is_empty());
        assert!(!scaled.is_empty());
        assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
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
                frame: Size { width: 400, height: 240 },
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
                frame: Size { width: 400, height: 120 },
                ..default_renderer_config(&track)
            },
        );
        let widened = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size { width: 400, height: 120 },
                pixel_aspect: 2.0,
                ..default_renderer_config(&track)
            },
        );

        assert!(total_plane_area(&baseline) > 0);
        assert!(total_plane_area(&widened) > total_plane_area(&baseline));
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
                frame: Size { width: 400, height: 240 },
                ..default_renderer_config(&track)
            },
        );
        let storage_adjusted = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size { width: 400, height: 240 },
                storage: Size { width: 400, height: 120 },
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
                frame: Size { width: 400, height: 240 },
                ..default_renderer_config(&track)
            },
        );
        let overridden_inputs = engine.render_frame_with_provider_and_config(
            &track,
            &provider,
            500,
            &RendererConfig {
                frame: Size { width: 400, height: 240 },
                storage: Size { width: 400, height: 120 },
                pixel_aspect: 2.0,
                ..default_renderer_config(&track)
            },
        );

        assert_eq!(total_plane_area(&overridden_inputs), total_plane_area(&baseline));
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
                frame: Size { width: 200, height: 120 },
                line_position: 50.0,
                ..RendererConfig::default()
            },
        );

        let baseline_y = baseline.iter().map(|plane| plane.destination.y).min().expect("baseline plane");
        let shifted_y = shifted.iter().map(|plane| plane.destination.y).min().expect("shifted plane");

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
                frame: Size { width: 200, height: 140 },
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
    fn render_frame_interpolates_move_position() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\move(0,0,100,0,0,1000)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

        let start_x = start_planes.iter().map(|plane| plane.destination.x).min().expect("start plane");
        let mid_x = mid_planes.iter().map(|plane| plane.destination.x).min().expect("mid plane");
        let end_x = end_planes.iter().map(|plane| plane.destination.x).min().expect("end plane");

        assert!(start_x <= mid_x);
        assert!(mid_x <= end_x);
        assert!(end_x - start_x >= 80);
    }

    #[test]
    fn render_frame_applies_fad_alpha() {
        let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fad(200,200)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

        let start_alpha = start_planes.iter().map(|plane| plane.color.0 & 0xFF).max().expect("start alpha");
        let mid_alpha = mid_planes.iter().map(|plane| plane.color.0 & 0xFF).max().expect("mid alpha");
        let end_alpha = end_planes.iter().map(|plane| plane.color.0 & 0xFF).max().expect("end alpha");

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

        let start_alpha = start_planes.iter().map(|plane| plane.color.0 & 0xFF).max().expect("start alpha");
        let middle_alpha = middle_planes.iter().map(|plane| plane.color.0 & 0xFF).max().expect("middle alpha");
        let late_alpha = late_planes.iter().map(|plane| plane.color.0 & 0xFF).max().expect("late alpha");

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

        assert!(early_planes.iter().any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x0044_5566));
        assert!(late_planes.iter().any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x0011_2233));
    }

    #[test]
    fn render_frame_sweeps_karaoke_fill_during_active_span() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\K100}Kara").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(mid_planes.iter().any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x0011_2233));
        assert!(mid_planes.iter().any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x0044_5566));
    }

    #[test]
    fn render_frame_hides_outline_for_ko_until_span_ends() {
        let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H000A0B0C,&H00000000,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\ko50}Ko").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
        let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

        assert!(!early_planes.iter().any(|plane| plane.kind == ass::ImageType::Outline));
        assert!(late_planes.iter().any(|plane| plane.kind == ass::ImageType::Outline));
    }

    #[test]
    fn render_frame_renders_drawing_plane() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x0011_2233));
        let plane = planes.iter().find(|plane| plane.kind == ass::ImageType::Character).expect("drawing plane");
        assert_eq!(plane.destination.x, 10);
        assert_eq!(plane.destination.y, 10);
        assert!(plane.bitmap.iter().any(|value| *value == 255));
    }

    #[test]
    fn render_frame_renders_bezier_drawing_plane() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 b 10 0 10 10 0 10").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let plane = planes.iter().find(|plane| plane.kind == ass::ImageType::Character).expect("drawing plane");
        assert!(plane.bitmap.iter().any(|value| *value == 255));
        assert!(plane.size.width >= 8);
        assert!(plane.size.height >= 8);
    }

    #[test]
    fn render_frame_emits_outline_and_shadow_for_drawings() {
        let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H000A0B0C,&H00445566,0,0,0,0,100,100,0,0,1,2,3,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x000A_0B0C));
        assert!(planes.iter().any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x0044_5566));
    }

    #[test]
    fn render_frame_renders_spline_drawing_plane() {
        let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 s 10 0 10 10 0 10 p -5 5 c").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let plane = planes.iter().find(|plane| plane.kind == ass::ImageType::Character).expect("drawing plane");
        assert!(plane.bitmap.iter().any(|value| *value == 255));
        assert!(plane.size.width >= 10);
        assert!(plane.size.height >= 10);
    }

    #[test]
    fn render_frame_renders_non_closing_move_subpaths() {
        let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8 n 20 20 l 28 20 28 28 20 28").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let planes = engine.render_frame_with_provider(&track, &provider, 500);

        let plane = planes.iter().find(|plane| plane.kind == ass::ImageType::Character).expect("drawing plane");
        assert!(plane.bitmap.iter().any(|value| *value == 255));
        assert!(plane.size.width >= 28);
        assert!(plane.size.height >= 28);
    }

    #[test]
    fn render_frame_applies_timed_transform_style() {
        let track = parse_script_text("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H000000FF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\t(0,1000,\\1c&H00112233&\\bord4)}Hi").expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
        let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
        let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

        assert!(!start_planes.iter().any(|plane| plane.kind == ass::ImageType::Outline));
        assert!(mid_planes.iter().any(|plane| plane.kind == ass::ImageType::Outline));
        assert!(end_planes.iter().any(|plane| plane.kind == ass::ImageType::Outline));

        let start_fill = start_planes.iter().find(|plane| plane.kind == ass::ImageType::Character).expect("start fill").color.0;
        let end_fill = end_planes.iter().find(|plane| plane.kind == ass::ImageType::Character).expect("end fill").color.0;
        assert_ne!(start_fill, end_fill);
    }
}
