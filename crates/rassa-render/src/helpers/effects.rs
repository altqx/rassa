use super::*;

pub(crate) fn apply_fade_to_planes(
    planes: Vec<ImagePlane>,
    fade: ParsedFade,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
) -> Vec<ImagePlane> {
    let fade_alpha = compute_fad_alpha(fade, source_event, now_ms);
    // libass (ass_render.c:2513) only sets FILTER_FILL_IN_BORDER when the base
    // primary/secondary alpha are opaque AND `info->fade == 0`. While a fade is
    // active it leaves the flag clear, so ass_fix_outline (ass_bitmap.c:205)
    // carves the glyph fill out of the border bitmap. Without this carve the
    // border interior double-blends with the fill under the semi-transparent
    // fade alpha, leaving rassa more opaque than libass mid-fade. The base
    // translucency case (non-fade \alpha/\1a/\2a) is carved in the main path.
    let mut planes = planes;
    if fade_alpha != 0 || planes_have_translucent_fill(&planes) {
        carve_fill_out_of_outline(&mut planes);
    }
    planes
        .into_iter()
        .map(|mut plane| {
            plane.color = RgbaColor(with_fade_alpha(plane.color.0, fade_alpha));
            plane
        })
        .collect()
}

/// Mirror of libass `ass_fix_outline` (ass_bitmap.c:205): subtract the glyph
/// fill coverage `g` from the border coverage `o` so the border becomes a
/// hollow ring (`o = o > g ? o - g/2 : 0`). Operates per-pixel on the overlap
/// of each Outline plane with each Character plane, addressed in shared screen
/// space via plane `destination`.
/// A fill is translucent when any character (fill) plane carries a non-zero
/// colour alpha (libass `_a(c[0])`/`_a(c[1])`; 0 == opaque). Karaoke unswept
/// syllables surface their secondary colour the same way.
pub(crate) fn planes_have_translucent_fill(planes: &[ImagePlane]) -> bool {
    planes
        .iter()
        .any(|plane| plane.kind == ass::ImageType::Character && (plane.color.0 & 0xFF) != 0)
}

pub(crate) fn carve_fill_out_of_outline(planes: &mut [ImagePlane]) {
    let fills: Vec<(Point, Size, i32, Vec<u8>)> = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| {
            (
                plane.destination,
                plane.size,
                plane.stride,
                plane.bitmap.clone(),
            )
        })
        .collect();
    if fills.is_empty() {
        return;
    }
    for outline in planes
        .iter_mut()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
    {
        for (fill_dst, fill_size, fill_stride, fill_bitmap) in &fills {
            let left = outline.destination.x.max(fill_dst.x);
            let top = outline.destination.y.max(fill_dst.y);
            let right =
                (outline.destination.x + outline.size.width).min(fill_dst.x + fill_size.width);
            let bottom =
                (outline.destination.y + outline.size.height).min(fill_dst.y + fill_size.height);
            if right <= left || bottom <= top {
                continue;
            }
            for y in top..bottom {
                let o_row = ((y - outline.destination.y) * outline.stride) as usize;
                let g_row = ((y - fill_dst.y) * fill_stride) as usize;
                for x in left..right {
                    let o_idx = o_row + (x - outline.destination.x) as usize;
                    let g_idx = g_row + (x - fill_dst.x) as usize;
                    let g = fill_bitmap[g_idx];
                    let o = outline.bitmap[o_idx];
                    outline.bitmap[o_idx] = if o > g { o - g / 2 } else { 0 };
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_effect_to_planes(
    planes: Vec<ImagePlane>,
    source_event: Option<&ParsedEvent>,
    track: &ParsedTrack,
    config: &RendererConfig,
    now_ms: i64,
    mapping: &EventMapping,
    line_box: Option<Rect>,
) -> Vec<ImagePlane> {
    let Some(event) = source_event else {
        return planes;
    };
    if planes.is_empty() || event.effect.is_empty() {
        return planes;
    }
    // libass positions transition effects from the event's text box
    // (device position and bbox), not from rendered ink.
    let Some(bounds) = line_box
        .or_else(|| planes_ink_bounds(&planes))
        .or_else(|| planes_bounds(&planes))
    else {
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
        let delay = scaled_effect_delay(delay, effect_delay_scale.x);
        let shift = elapsed / delay;
        let left_to_right = values.get(1).copied().unwrap_or(0) != 0;
        // libass ass_render_event: SCROLL_RL puts the text box's left edge at
        // x2scr_pos(PlayResX - shift); SCROLL_LR puts its right edge at
        // x2scr_pos(shift).
        let target_left = if left_to_right {
            mapping.map_x_pos(shift).round() as i32 - (bounds.x_max - bounds.x_min)
        } else {
            mapping
                .map_x_pos(f64::from(track.play_res_x) - shift)
                .round() as i32
        };
        return translate_planes(
            planes,
            Point {
                x: target_left - bounds.x_min,
                y: 0,
            },
        );
    }

    let scroll_up = effect.starts_with("Scroll up;");
    let scroll_down = effect.starts_with("Scroll down;");
    if scroll_up || scroll_down {
        if values.len() < 3 {
            return planes;
        }
        let delay = scaled_effect_delay(values[2], effect_delay_scale.y);
        let shift = elapsed / delay;
        let y0 = values[0].min(values[1]);
        let y1 = values[0].max(values[1]);
        let clip_y0 = mapping.map_y_pos(f64::from(y0)).round() as i32;
        let clip_y1 = mapping.map_y_pos(f64::from(y1)).round() as i32;
        // libass: SCROLL_BT anchors the box top at y2scr(y1 - shift),
        // SCROLL_TB the box bottom at y2scr(y0 + shift), clipped to y0..y1.
        let target_offset = if scroll_up {
            let target_top = mapping.map_y_pos(f64::from(y1) - shift).round() as i32;
            target_top - bounds.y_min
        } else {
            let target_bottom = mapping.map_y_pos(f64::from(y0) + shift).round() as i32;
            target_bottom - bounds.y_max
        };
        let translated = translate_planes(
            planes,
            Point {
                x: 0,
                y: target_offset,
            },
        );
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

pub(crate) fn transition_effect_disables_collision(event: &ParsedEvent) -> bool {
    let effect = event.effect.as_str();
    if effect.starts_with("Banner;") {
        return !effect_values(effect).is_empty();
    }
    if effect.starts_with("Scroll up;") || effect.starts_with("Scroll down;") {
        return effect_values(effect).len() >= 3;
    }
    false
}

pub(crate) fn effect_values(effect: &str) -> Vec<i32> {
    effect.split(';').skip(1).take(4).map(atoi_prefix).collect()
}

pub(crate) fn atoi_prefix(value: &str) -> i32 {
    let trimmed = value.trim_start_matches([' ', '\t', '\n', '\r', '\u{000b}', '\u{000c}']);
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

pub(crate) fn scaled_effect_delay(delay: i32, scale: f64) -> f64 {
    let unscaled = (f64::from(delay) / scale).max(1.0).trunc();
    (unscaled * scale).max(f64::EPSILON)
}

pub(crate) fn effect_delay_scales(track: &ParsedTrack, config: &RendererConfig) -> RenderScale {
    let layout = layout_resolution(track).or_else(|| storage_resolution(config));
    let x = layout
        .map(|size| f64::from(size.width.max(1)) / f64::from(track.play_res_x.max(1)))
        .unwrap_or(1.0);
    let y = layout
        .map(|size| f64::from(size.height.max(1)) / f64::from(track.play_res_y.max(1)))
        .unwrap_or(1.0);
    RenderScale { x, y }
}

pub(crate) fn resolve_run_fill_color(
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
    let elapsed = karaoke_elapsed_ms(event, now_ms);
    // libass ass_parse.c process_karaoke_effects: for \k and \ko,
    // tm_end = tm_start, so the fill switches to primary at syllable START.
    if elapsed >= i64::from(karaoke.start_ms) {
        style.primary_colour
    } else {
        style.secondary_colour
    }
}

pub(crate) fn karaoke_hides_outline(
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
    let elapsed = karaoke_elapsed_ms(event, now_ms);
    // libass render_text skips the outline only while effect_timing <= 0,
    // i.e. before the syllable starts (ass_render.c).
    elapsed < i64::from(karaoke.start_ms)
}

fn karaoke_elapsed_ms(event: &ParsedEvent, now_ms: i64) -> i64 {
    (now_ms - event.start).clamp(0, event.duration.max(0))
}

pub(crate) fn apply_karaoke_to_character_planes(
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
    let elapsed = karaoke_elapsed_ms(event, now_ms);
    let relative = elapsed - i64::from(karaoke.start_ms);
    match karaoke.mode {
        // \k and \ko: libass sets tm_end = tm_start, so the whole syllable
        // is primary from its start time onward.
        ParsedKaraokeMode::FillSwap | ParsedKaraokeMode::OutlineToggle => planes
            .into_iter()
            .map(|mut plane| {
                plane.color = rgba_color_from_ass(if relative >= 0 {
                    style.primary_colour
                } else {
                    style.secondary_colour
                });
                plane
            })
            .collect(),
        ParsedKaraokeMode::Sweep => {
            // libass process_karaoke_effects: when fmod(frz, 360) lies in
            // (90, 270) the fill sweeps right-to-left with swapped colors.
            // C fmod keeps the sign of frz, so negative angles never reverse.
            let frz = style.rotation_z % 360.0;
            let reversed = frz > 90.0 && frz < 270.0;
            let (filled_colour, pending_colour) = if reversed {
                (style.secondary_colour, style.primary_colour)
            } else {
                (style.primary_colour, style.secondary_colour)
            };
            if relative < 0 {
                return planes
                    .into_iter()
                    .map(|mut plane| {
                        plane.color = rgba_color_from_ass(pending_colour);
                        plane
                    })
                    .collect();
            }
            if relative >= i64::from(karaoke.duration_ms) {
                return planes
                    .into_iter()
                    .map(|mut plane| {
                        plane.color = rgba_color_from_ass(filled_colour);
                        plane
                    })
                    .collect();
            }

            let mut progress = (relative as f64) / f64::from(karaoke.duration_ms.max(1));
            if reversed {
                progress = 1.0 - progress;
            }
            // libass anchors the sweep to the run's leftmost transformed
            // outline, not its logical pen origin.  Its rounded outline edge
            // includes the first antialiased bitmap column at progress zero;
            // Rassa's raster bitmaps are already trimmed to nonzero coverage,
            // so advance one column from the visible left edge to preserve
            // that primary-colour sliver.
            let sweep_start_x = planes
                .iter()
                .filter_map(plane_ink_bounds)
                .map(|bounds| bounds.x_min)
                .min()
                .map(|x| x.saturating_add(1))
                .unwrap_or(run_origin_x);
            let split_x = sweep_start_x
                .saturating_add((f64::from(run_width.max(0)) * progress).round() as i32);
            let mut result = Vec::new();
            for plane in planes {
                if let Some(mut left) =
                    clip_plane_horizontally(&plane, plane.destination.x, split_x)
                {
                    left.color = rgba_color_from_ass(filled_colour);
                    result.push(left);
                }
                if let Some(mut right) =
                    clip_plane_horizontally(&plane, split_x, plane.destination.x + plane.size.width)
                {
                    right.color = rgba_color_from_ass(pending_colour);
                    result.push(right);
                }
            }
            result
        }
    }
}

pub(crate) fn clip_plane_horizontally(
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

pub(crate) fn apply_quarter_turn_karaoke_sweep_after_transform(
    planes: Vec<ImagePlane>,
    run: &LayoutGlyphRun,
    style: &ParsedSpanStyle,
    source_event: Option<&ParsedEvent>,
    now_ms: i64,
    run_width: i32,
) -> Vec<ImagePlane> {
    let Some(karaoke) = run
        .karaoke
        .filter(|karaoke| karaoke.mode == ParsedKaraokeMode::Sweep)
    else {
        return planes;
    };
    let Some(event) = source_event else {
        return planes;
    };
    let relative = karaoke_elapsed_ms(event, now_ms) - i64::from(karaoke.start_ms);
    if relative < 0 || relative >= i64::from(karaoke.duration_ms) {
        return planes;
    }
    let progress = relative as f64 / f64::from(karaoke.duration_ms.max(1));
    // libass keeps the \kf colour boundary screen-horizontal even after a
    // quarter-turn transform.  The boundary starts at the leftmost
    // transformed outline and advances by the syllable's untransformed
    // logical width; it does not turn into a vertical wipe.  Rassa's fill
    // bitmaps are trimmed to visible coverage, so retain the same one-column
    // primary sliver used by the upright sweep path at progress zero.
    let sweep_start_x = planes
        .iter()
        .filter_map(plane_ink_bounds)
        .map(|bounds| bounds.x_min)
        .min()
        .map(|x| x.saturating_add(1))
        .unwrap_or_else(|| {
            planes
                .iter()
                .map(|plane| plane.destination.x)
                .min()
                .unwrap_or(0)
        });
    let split_x =
        sweep_start_x.saturating_add((f64::from(run_width.max(0)) * progress).round() as i32);
    let mut result = Vec::new();
    for plane in planes {
        if let Some(mut left) = clip_plane_horizontally(&plane, plane.destination.x, split_x) {
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
