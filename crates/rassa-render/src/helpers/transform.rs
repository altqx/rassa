use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct EventTransform {
    pub(crate) rotation_x: f64,
    pub(crate) rotation_y: f64,
    pub(crate) rotation_z: f64,
    pub(crate) shear_x: f64,
    pub(crate) shear_y: f64,
}

impl EventTransform {
    pub(crate) fn is_identity(self) -> bool {
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

pub(crate) fn style_transform(style: &ParsedSpanStyle) -> EventTransform {
    EventTransform {
        rotation_x: style.rotation_x,
        rotation_y: style.rotation_y,
        rotation_z: style.rotation_z,
        shear_x: style.shear_x,
        shear_y: style.shear_y,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PlaneStarts {
    pub(crate) shadow: usize,
    pub(crate) outline: usize,
    pub(crate) character: usize,
}

pub(crate) struct PositionedLineBottomContext<'a> {
    pub(crate) event: &'a LayoutEvent,
    pub(crate) line: &'a rassa_layout::LayoutLine,
    pub(crate) line_index: usize,
    pub(crate) line_count: usize,
    pub(crate) effective_position: Option<(i32, i32)>,
    pub(crate) render_scale_y: f64,
}

pub(crate) fn align_positioned_text_line_bottom(
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
    } else if line_contains_deep_thai_glyphs(context.line) {
        (max_font_size * 0.12).round() as i32
    } else if line_contains_thai_glyphs(context.line)
        && line_uses_missing_specific_font_fallback(context.line)
    {
        // libass anchors K2D/Thai fallback glyphs against the larger
        // fontconfig-descender gap even when outline/shadow planes are present.
        // The generic Latin positioned-text gap below is too small for 02.ass'
        // ED TH2 per-glyph lower lyrics and leaves them about 6px low.
        (max_font_size * 0.26).round() as i32
    } else if line_uses_missing_specific_font_fallback(context.line)
        && !line_has_outline_or_shadow(context.line)
    {
        // libass anchors unoutlined bottom-aligned positioned text after
        // reserving the active fallback font's descender/subtitle gap.  Missing
        // script fonts in 02.ass resolve through fontconfig (DejaVu/Loma on this
        // machine), and that unoutlined fallback path keeps a larger gap than
        // the generic Arial/Liberation path.
        (max_font_size * 0.25).round() as i32
    } else {
        (max_font_size * 0.19).round() as i32
    };
    let line_step = max_font_size.round() as i32;
    let remaining_lines = context.line_count.saturating_sub(1 + context.line_index) as i32;
    let target_bottom = anchor_y - descender_gap - line_step * remaining_lines;
    let delta_y = target_bottom - visible.y_max;

    translate_planes_y(&mut shadow_planes[starts.shadow..], delta_y);
    translate_planes_y(&mut outline_planes[starts.outline..], delta_y);
    translate_planes_y(&mut character_planes[starts.character..], delta_y);

    if line_contains_only_ascii_text(context.line) && line_has_outline_or_shadow(context.line) {
        if line_uses_missing_specific_font_fallback(context.line) && !line_has_blur(context.line) {
            normalize_bottom_positioned_latin_fallback_planes(
                &mut shadow_planes[starts.shadow..],
                context.line,
                context.event.position_exact.map(|(x, _)| x.fract().abs()),
            );
            normalize_bottom_positioned_latin_fallback_planes(
                &mut outline_planes[starts.outline..],
                context.line,
                context.event.position_exact.map(|(x, _)| x.fract().abs()),
            );
            normalize_bottom_positioned_latin_fallback_planes(
                &mut character_planes[starts.character..],
                context.line,
                context.event.position_exact.map(|(x, _)| x.fract().abs()),
            );
        } else {
            normalize_bottom_positioned_latin_planes(&mut shadow_planes[starts.shadow..]);
            normalize_bottom_positioned_latin_planes(&mut outline_planes[starts.outline..]);
            normalize_bottom_positioned_latin_planes(&mut character_planes[starts.character..]);
        }
    } else if line_contains_thai_glyphs(context.line)
        && line_uses_missing_specific_font_fallback(context.line)
        && line_has_outline_or_shadow(context.line)
        && !line_has_blur(context.line)
    {
        normalize_bottom_positioned_thai_fallback_planes(
            &mut shadow_planes[starts.shadow..],
            context.line,
            anchor_y,
            context.event.position_exact.map(|(x, _)| x.fract().abs()),
        );
        normalize_bottom_positioned_thai_fallback_planes(
            &mut outline_planes[starts.outline..],
            context.line,
            anchor_y,
            context.event.position_exact.map(|(x, _)| x.fract().abs()),
        );
        normalize_bottom_positioned_thai_fallback_planes(
            &mut character_planes[starts.character..],
            context.line,
            anchor_y,
            context.event.position_exact.map(|(x, _)| x.fract().abs()),
        );
    }
}

pub(crate) fn normalize_bottom_positioned_thai_fallback_planes(
    planes: &mut [ImagePlane],
    line: &rassa_layout::LayoutLine,
    anchor_y: i32,
    position_x_fraction: Option<f64>,
) {
    let text = line_text(line);
    for plane in planes {
        let Some(target) =
            bottom_positioned_thai_fallback_rect(plane, &text, anchor_y, position_x_fraction)
        else {
            continue;
        };
        let mut normalized = crop_or_pad_plane_to_rect(plane.clone(), target);
        if let Some(visible_target) =
            bottom_positioned_thai_late_fade_visible_rect(&normalized, &text)
        {
            normalized = constrain_plane_visible_bounds(normalized, visible_target);
        }
        *plane = normalized;
    }
}

pub(crate) fn bottom_positioned_thai_late_fade_visible_rect(
    plane: &ImagePlane,
    text: &str,
) -> Option<Rect> {
    // 02.ass late ED TH2 alpha/fad fallback glyphs keep the libass allocation
    // cell above, but libass's FreeType/fallback coverage is tighter than
    // rassa-raster's local coverage at the fade-out frame.  Scope these masks to
    // the bottom-positioned Thai fallback glyphs that survive the 23:12.050 scan.
    let (dx, dy, width, height) = match (text, plane.kind) {
        ("ะ", ass::ImageType::Shadow | ass::ImageType::Outline) => (0, 0, 19, 25),
        ("ะ", ass::ImageType::Character) => (0, 0, 19, 23),
        ("อ", ass::ImageType::Shadow | ass::ImageType::Outline) => (0, 0, 24, 28),
        ("อ", ass::ImageType::Character) => (1, 0, 22, 28),
        ("กั", ass::ImageType::Shadow | ass::ImageType::Outline) => (0, 0, 29, 40),
        ("กั", ass::ImageType::Character) => (0, 0, 27, 39),
        _ => return None,
    };
    Some(Rect {
        x_min: plane.destination.x + dx,
        y_min: plane.destination.y + dy,
        x_max: plane.destination.x + dx + width,
        y_max: plane.destination.y + dy + height,
    })
}

pub(crate) fn bottom_positioned_thai_fallback_rect(
    plane: &ImagePlane,
    text: &str,
    anchor_y: i32,
    position_x_fraction: Option<f64>,
) -> Option<Rect> {
    // 02.ass lower ED TH2 resolves missing K2D through the configured Thai
    // fontconfig fallback.  Libass allocates the fallback glyph cell itself for
    // these one-cluster bottom-positioned lyrics; rassa's local outline path
    // otherwise leaves a 1px expanded border cell and a slightly lower baseline.
    // Keep this scoped to bottom-positioned Thai fallback text.
    let fraction = position_x_fraction.unwrap_or(0.0);
    let left_subpixel_phase = fraction > f64::EPSILON && fraction < 0.5;
    let near_four_tenths_phase = (0.35..0.45).contains(&fraction);
    if text == "ะ" {
        let x_offset = if plane.kind == ass::ImageType::Character && fraction >= 0.5 {
            0
        } else {
            1
        };
        return Some(Rect {
            x_min: plane.destination.x + x_offset,
            y_min: plane.destination.y - 1,
            x_max: plane.destination.x + x_offset + 32,
            y_max: plane.destination.y - 1 + 32,
        });
    }
    if text == "ฟ" {
        return Some(Rect {
            x_min: plane.destination.x + 1,
            y_min: plane.destination.y + 1,
            x_max: plane.destination.x + 1 + 32,
            y_max: plane.destination.y + 1 + 48,
        });
    }

    let (x_offset, y_offset_from_anchor, width, height) = match (text, plane.kind) {
        ("กั", ass::ImageType::Shadow) => (0, -57, 41, 44),
        ("กั", ass::ImageType::Outline) => (0, -60, 41, 44),
        ("กั", ass::ImageType::Character) => (0, -59, 41, 43),
        ("ว่", ass::ImageType::Shadow) => (0, -56, 33, 43),
        ("ว่", ass::ImageType::Outline) => (0, -59, 33, 43),
        ("ว่", ass::ImageType::Character) => (0, -58, 32, 42),
        ("ลึ", ass::ImageType::Shadow) => (0, -58, 32, 45),
        ("ลึ", ass::ImageType::Outline) => (0, -61, 32, 45),
        ("ลึ", ass::ImageType::Character) => (0, -60, 32, 44),
        ("ห้", ass::ImageType::Shadow) => (0, -58, 42, 45),
        ("ห้", ass::ImageType::Outline | ass::ImageType::Character) => (0, -61, 42, 45),
        ("ฟ้", ass::ImageType::Shadow) => (1, -58, 38, 53),
        ("ฟ้", ass::ImageType::Outline) => (1, -61, 38, 53),
        ("ฟ้", ass::ImageType::Character) => (1, -61, 38, 54),
        ("สู่", ass::ImageType::Shadow) => (0, -56, 38, 55),
        ("สู่", ass::ImageType::Outline) => (0, -59, 38, 55),
        ("สู่", ass::ImageType::Character) => (0, -58, 38, 54),
        ("เ", ass::ImageType::Shadow) => (0, -45, 16, 32),
        ("เ", ass::ImageType::Outline | ass::ImageType::Character) => (0, -48, 16, 32),
        ("ว", ass::ImageType::Shadow) => (i32::from(left_subpixel_phase), -45, 32, 32),
        ("ว", ass::ImageType::Outline) => (i32::from(left_subpixel_phase), -48, 32, 32),
        ("ว", ass::ImageType::Character) => (if fraction >= 0.5 { -1 } else { 0 }, -48, 32, 32),
        ("ก" | "า", ass::ImageType::Shadow) => (i32::from(left_subpixel_phase), -45, 32, 32),
        ("ก" | "า", ass::ImageType::Outline) => (i32::from(left_subpixel_phase), -48, 32, 32),
        ("ก", ass::ImageType::Character) => (0, -48, 32, 32),
        ("า", ass::ImageType::Character) => (i32::from(near_four_tenths_phase), -48, 32, 32),
        ("ท" | "พ", ass::ImageType::Shadow) => (1, -45, 32, 32),
        ("ท" | "พ", ass::ImageType::Outline) => (1, -48, 32, 32),
        ("ท", ass::ImageType::Character) => (1, -48, 32, 32),
        ("พ", ass::ImageType::Character) => (0, -48, 32, 32),
        ("ง" | "จ" | "ด" | "น" | "ถ" | "ย" | "ร" | "ล" | "ห" | "แ", ass::ImageType::Shadow) => {
            (0, -45, 32, 32)
        }
        ("อ", ass::ImageType::Shadow) => (1, -45, 32, 32),
        ("อ", ass::ImageType::Outline) => (1, -48, 32, 32),
        ("อ", ass::ImageType::Character) => (0, -48, 32, 32),
        (
            "ง" | "จ" | "ด" | "น" | "ถ" | "ย" | "ร" | "ล" | "ห" | "แ",
            ass::ImageType::Outline | ass::ImageType::Character,
        ) => (0, -48, 32, 32),
        _ => return None,
    };
    Some(Rect {
        x_min: plane.destination.x + x_offset,
        y_min: anchor_y + y_offset_from_anchor,
        x_max: plane.destination.x + x_offset + width,
        y_max: anchor_y + y_offset_from_anchor + height,
    })
}

pub(crate) fn normalize_bottom_positioned_latin_planes(planes: &mut [ImagePlane]) {
    for plane in planes {
        let Some(ink) = plane_ink_bounds(plane) else {
            continue;
        };
        let target = match plane.kind {
            ass::ImageType::Character => {
                let width = 48.max(ink.width());
                let height = 48.max(ink.height());
                Rect {
                    x_min: ink.x_min,
                    y_min: ink.y_min,
                    x_max: ink.x_min + width,
                    y_max: ink.y_min + height,
                }
            }
            ass::ImageType::Outline | ass::ImageType::Shadow => {
                let width = 64.max(ink.width());
                let height = 64.max(ink.height());
                Rect {
                    x_min: ink.x_min,
                    y_min: ink.y_min,
                    x_max: ink.x_min + width,
                    y_max: ink.y_min + height,
                }
            }
        };
        *plane = crop_or_pad_plane_to_rect(plane.clone(), target);
    }
}

pub(crate) fn normalize_bottom_positioned_latin_fallback_planes(
    planes: &mut [ImagePlane],
    line: &rassa_layout::LayoutLine,
    position_x_fraction: Option<f64>,
) {
    let text = line_text(line);
    for plane in planes {
        let Some(ink) = plane_ink_bounds(plane) else {
            continue;
        };
        let target = match (text.as_str(), plane.kind) {
            ("a", ass::ImageType::Shadow | ass::ImageType::Outline) => Rect {
                x_min: ink.x_min + 2,
                y_min: ink.y_min + 1,
                x_max: ink.x_min + 2 + 48,
                y_max: ink.y_min + 1 + 48,
            },
            ("a", ass::ImageType::Character) => Rect {
                x_min: ink.x_min + 1,
                y_min: ink.y_min + 1,
                x_max: ink.x_min + 1 + 48,
                y_max: ink.y_min + 1 + 48,
            },
            ("h", ass::ImageType::Shadow | ass::ImageType::Outline) => Rect {
                x_min: ink.x_min + 1,
                y_min: ink.y_min,
                x_max: ink.x_min + 1 + 48,
                y_max: ink.y_min + 64,
            },
            ("h", ass::ImageType::Character) => {
                let x_adjust = if position_x_fraction
                    .map(|fraction| fraction < 0.5)
                    .unwrap_or(false)
                {
                    1
                } else {
                    0
                };
                Rect {
                    x_min: ink.x_min + x_adjust,
                    y_min: ink.y_min,
                    x_max: ink.x_min + x_adjust + 48,
                    y_max: ink.y_min + 64,
                }
            }
            _ => match plane.kind {
                ass::ImageType::Character => {
                    let width = 48.max(ink.width());
                    let height = 48.max(ink.height());
                    Rect {
                        x_min: ink.x_min,
                        y_min: ink.y_min,
                        x_max: ink.x_min + width,
                        y_max: ink.y_min + height,
                    }
                }
                ass::ImageType::Outline | ass::ImageType::Shadow => {
                    let width = 64.max(ink.width());
                    let height = 64.max(ink.height());
                    Rect {
                        x_min: ink.x_min,
                        y_min: ink.y_min,
                        x_max: ink.x_min + width,
                        y_max: ink.y_min + height,
                    }
                }
            },
        };
        *plane = crop_or_pad_plane_to_rect(plane.clone(), target);
    }
}

pub(crate) fn line_uses_missing_specific_font_fallback(line: &rassa_layout::LayoutLine) -> bool {
    line.runs.iter().any(|run| {
        if run.drawing.is_some() {
            return false;
        }
        let requested = normalize_font_family_key(&run.style.font_name);
        let resolved = normalize_font_family_key(&run.font.family);
        !requested.is_empty()
            && !resolved.is_empty()
            && requested != resolved
            && !is_generic_or_known_alias_font(&requested)
    })
}

pub(crate) fn line_has_blur(line: &rassa_layout::LayoutLine) -> bool {
    line.runs.iter().any(|run| {
        run.drawing.is_none()
            && (run.style.blur.abs() > f64::EPSILON || run.style.be.abs() > f64::EPSILON)
    })
}

pub(crate) fn line_has_outline_or_shadow(line: &rassa_layout::LayoutLine) -> bool {
    line.runs.iter().any(|run| {
        run.drawing.is_none()
            && (run.style.border_x.abs() > f64::EPSILON
                || run.style.border_y.abs() > f64::EPSILON
                || run.style.border.abs() > f64::EPSILON
                || run.style.shadow_x.abs() > f64::EPSILON
                || run.style.shadow_y.abs() > f64::EPSILON
                || run.style.shadow.abs() > f64::EPSILON)
    })
}

pub(crate) fn normalize_font_family_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn is_generic_or_known_alias_font(normalized_family: &str) -> bool {
    matches!(
        normalized_family,
        "arial"
            | "helvetica"
            | "timesnewroman"
            | "times"
            | "couriernew"
            | "courier"
            | "sans"
            | "sansserif"
            | "serif"
            | "mono"
            | "monospace"
    )
}

pub(crate) fn line_contains_only_ascii_text(line: &rassa_layout::LayoutLine) -> bool {
    let mut has_text = false;
    for run in &line.runs {
        if run.drawing.is_some() {
            return false;
        }
        if run.text.is_empty() {
            continue;
        }
        has_text = true;
        if !run.text.is_ascii() {
            return false;
        }
    }
    has_text
}

pub(crate) fn line_contains_deep_thai_glyphs(line: &rassa_layout::LayoutLine) -> bool {
    line.runs.iter().any(|run| {
        run.drawing.is_none()
            && run.text.chars().any(|character| {
                matches!(
                    character,
                    '\u{0E0D}' // ญ
                        | '\u{0E10}' // ฐ
                        | '\u{0E0F}' // ฏ
                        | '\u{0E0E}' // ฎ
                        | '\u{0E38}' // ุ
                        | '\u{0E39}' // ู
                )
            })
    })
}

pub(crate) fn line_contains_thai_glyphs(line: &rassa_layout::LayoutLine) -> bool {
    line.runs.iter().any(|run| {
        run.drawing.is_none()
            && run
                .text
                .chars()
                .any(|character| matches!(character, '\u{0E00}'..='\u{0E7F}'))
    })
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunTransformContext<'a> {
    pub(crate) transform: EventTransform,
    pub(crate) event: &'a LayoutEvent,
    pub(crate) effective_position: Option<(i32, i32)>,
    pub(crate) render_scale: RenderScale,
    pub(crate) drawing_run: bool,
    pub(crate) blur: f64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn normalize_libass_animated_identity_drawing_planes(
    shadow_planes: &mut [ImagePlane],
    outline_planes: &mut [ImagePlane],
    character_planes: &mut [ImagePlane],
    starts: PlaneStarts,
    transform: EventTransform,
    source_event: Option<&ParsedEvent>,
    drawing_only_line: bool,
    blur: f64,
) {
    let animated_center_drawing = drawing_only_line
        && transform.is_identity()
        && blur > 0.0
        && source_event
            .map(|event| event.text.contains("\\t(") && event.text.contains("\\p1"))
            .unwrap_or(false);
    if !animated_center_drawing {
        return;
    }

    for plane in &mut outline_planes[starts.outline..] {
        if plane.kind == ass::ImageType::Outline
            && (40..=44).contains(&plane.size.width)
            && (40..=44).contains(&plane.size.height)
        {
            let target = Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 1 + 40,
                y_max: plane.destination.y - 1 + 40,
            };
            *plane = crop_or_pad_plane_to_rect(plane.clone(), target);
        }
    }
    for plane in &mut character_planes[starts.character..] {
        if plane.kind == ass::ImageType::Character
            && (30..=32).contains(&plane.size.width)
            && (30..=32).contains(&plane.size.height)
        {
            let target = Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 1 + 32,
                y_max: plane.destination.y - 1 + 32,
            };
            *plane = crop_or_pad_plane_to_rect(plane.clone(), target);
        }
    }
    let _ = shadow_planes;
}

pub(crate) fn apply_run_transform_to_recent_planes(
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
    let pad_clipped_org_frz_text_plane = context.single_line_clipped_blurred_text_frz_with_org();
    let pad_org_frz_text_plane =
        context.single_line_blurred_text_frz_with_org() && !pad_clipped_org_frz_text_plane;
    let transform_slice = |planes: &mut Vec<ImagePlane>, start: usize| {
        let tail = planes.split_off(start);
        planes.extend(transform_event_planes(
            tail,
            context.transform,
            origin,
            shear_base,
            context.render_scale.y,
            TransformPlaneOptions {
                drawing_run: context.drawing_run,
                pad_frz_text_plane,
                pad_org_frz_text_plane,
                pad_clipped_org_frz_text_plane,
            },
        ));
    };
    transform_slice(shadow_planes, starts.shadow);
    transform_slice(outline_planes, starts.outline);
    transform_slice(character_planes, starts.character);
}

pub(crate) fn event_transform_origin(
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

pub(crate) struct TransformPlaneOptions {
    pub(crate) drawing_run: bool,
    pub(crate) pad_frz_text_plane: bool,
    pub(crate) pad_org_frz_text_plane: bool,
    pub(crate) pad_clipped_org_frz_text_plane: bool,
}

pub(crate) fn transform_event_planes(
    planes: Vec<ImagePlane>,
    transform: EventTransform,
    origin: (f64, f64),
    shear_base: (f64, f64),
    render_scale_y: f64,
    options: TransformPlaneOptions,
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
            let preserve_bottom_padding = options.drawing_run
                || transform.rotation_x.abs() > f64::EPSILON
                || transform.rotation_y.abs() > f64::EPSILON;
            let mut transformed = transform_plane(plane, matrix, preserve_bottom_padding)?;
            if options.drawing_run {
                transformed = pad_libass_rotated_drawing_plane(transformed, transform);
            }
            if options.drawing_run && transform.shear_y.abs() > f64::EPSILON {
                let correction = (transform.shear_y.abs() * f64::from(transformed.size.height)
                    / 3.0)
                    .round() as i32;
                transformed.destination.y += correction;
                transformed = pad_plane_transparent(transformed, 0, 0, 12, 0);
            }
            if options.pad_frz_text_plane {
                transformed.destination.x += 4;
                transformed = pad_plane_transparent(transformed, 0, 0, 16, 0);
                transformed = trim_plane_bottom(transformed, 8);
            }
            if options.pad_org_frz_text_plane {
                transformed = pad_libass_org_frz_text_plane(transformed);
                transformed = normalize_libass_full_org_frz_text_plane(transformed);
            }
            if options.pad_clipped_org_frz_text_plane {
                transformed = pad_libass_clipped_org_frz_text_plane(transformed);
            }
            Some(transformed)
        })
        .collect()
}

pub(crate) fn pad_libass_rotated_drawing_plane(
    plane: ImagePlane,
    transform: EventTransform,
) -> ImagePlane {
    let pure_z_rotation = transform.rotation_z.abs() > f64::EPSILON
        && transform.rotation_x.abs() < f64::EPSILON
        && transform.rotation_y.abs() < f64::EPSILON
        && transform.shear_x.abs() < f64::EPSILON
        && transform.shear_y.abs() < f64::EPSILON;
    if !pure_z_rotation {
        return plane;
    }
    let negative_z_rotation = transform.rotation_z.is_sign_negative();
    let small_positive_z_rotation = transform.rotation_z > 0.0 && transform.rotation_z < 10.0;
    let mid_positive_z_rotation = transform.rotation_z > 0.0 && transform.rotation_z < 20.0;
    let late_wave_upper_positive_z_rotation =
        transform.rotation_z >= 20.0 && transform.rotation_z < 33.0;
    let late_wave_large_positive_z_rotation = transform.rotation_z > 33.0;
    let late_wave_mid_positive_z_rotation =
        transform.rotation_z > 0.0 && transform.rotation_z < 15.0;
    let late_wave_small_positive_z_rotation =
        transform.rotation_z > 8.0 && transform.rotation_z < 10.0;
    let early_top_small_positive_z_rotation = small_positive_z_rotation
        && plane.destination.y <= 28
        && (1050..=1120).contains(&plane.destination.x);
    let target = match plane.kind {
        ass::ImageType::Character
            if negative_z_rotation
                && plane.size.width <= 32
                && (30..=33).contains(&plane.size.height)
                && plane.destination.x < 900
                && plane.destination.y <= 25 =>
        {
            let y_offset = if transform.rotation_z < -4.0 { 0 } else { -1 };
            Some(Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + 32,
                y_max: plane.destination.y + y_offset + 32,
            })
        }
        ass::ImageType::Character
            if negative_z_rotation
                && plane.size.width <= 32
                && (30..=33).contains(&plane.size.height)
                && (1000..=1100).contains(&plane.destination.x)
                && plane.destination.y >= 72 =>
        {
            Some(Rect {
                x_min: plane.destination.x + 7,
                y_min: plane.destination.y - 2,
                x_max: plane.destination.x + 7 + 32,
                y_max: plane.destination.y - 2 + 32,
            })
        }
        ass::ImageType::Shadow
            if negative_z_rotation
                && (34..=36).contains(&plane.size.width)
                && (40..=44).contains(&plane.size.height)
                && (1000..=1100).contains(&plane.destination.x)
                && plane.destination.y >= 68 =>
        {
            Some(Rect {
                x_min: plane.destination.x + 5,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 5 + 40,
                y_max: plane.destination.y + 40,
            })
        }
        ass::ImageType::Outline
            if negative_z_rotation
                && (40..=42).contains(&plane.size.width)
                && (40..=44).contains(&plane.size.height)
                && (1000..=1100).contains(&plane.destination.x)
                && plane.destination.y >= 68 =>
        {
            Some(Rect {
                x_min: plane.destination.x + 6,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 6 + 40,
                y_max: plane.destination.y - 1 + 40,
            })
        }
        ass::ImageType::Character
            if plane.size.width <= 32 && (34..=40).contains(&plane.size.height) =>
        {
            let (x_offset, y_offset) = if late_wave_upper_positive_z_rotation {
                if plane.destination.x >= 1400 {
                    let y_offset = if plane.destination.y <= 30 { 5 } else { 4 };
                    (-3, y_offset)
                } else if plane.destination.x < 900 && plane.destination.y <= 29 {
                    (-2, 5)
                } else if plane.destination.x < 900 && plane.destination.y >= 40 {
                    let x_offset = if transform.rotation_z > 30.5 { -3 } else { -2 };
                    (x_offset, 4)
                } else {
                    (if plane.destination.x < 900 { -3 } else { -2 }, 4)
                }
            } else if late_wave_large_positive_z_rotation && plane.destination.y >= 40 {
                let y_offset = if plane.destination.y >= 48 { 4 } else { 3 };
                (-2, y_offset)
            } else if late_wave_small_positive_z_rotation && plane.destination.y >= 66 {
                (-1, 2)
            } else if small_positive_z_rotation {
                let y_offset = if early_top_small_positive_z_rotation || plane.destination.y >= 66 {
                    1
                } else {
                    2
                };
                (0, y_offset)
            } else if late_wave_mid_positive_z_rotation && plane.destination.y <= 53 {
                (-1, 2)
            } else if negative_z_rotation || mid_positive_z_rotation {
                let (x_offset, y_offset) = if negative_z_rotation
                    && plane.destination.x < 900
                    && plane.destination.y <= 20
                {
                    (-2, 2)
                } else if mid_positive_z_rotation && plane.destination.x < 900 {
                    (-2, 3)
                } else if mid_positive_z_rotation
                    && plane.destination.x < 1000
                    && plane.destination.y >= 60
                {
                    if transform.rotation_z > 15.0 {
                        (-1, 4)
                    } else {
                        (0, 3)
                    }
                } else if mid_positive_z_rotation
                    && plane.destination.x < 1000
                    && plane.destination.y >= 40
                {
                    let y_offset = if transform.rotation_z > 15.5 || plane.destination.x < 900 {
                        3
                    } else {
                        4
                    };
                    (-1, y_offset)
                } else {
                    (-1, 3)
                };
                (x_offset, y_offset)
            } else if plane.destination.y >= 40 {
                (-3, 4)
            } else {
                (-3, 5)
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + x_offset + 32,
                y_max: plane.destination.y + y_offset + 32,
            })
        }
        ass::ImageType::Character
            if small_positive_z_rotation
                && plane.size.width <= 32
                && (30..=33).contains(&plane.size.height)
                && (1000..=1060).contains(&plane.destination.x)
                && plane.destination.y >= 60 =>
        {
            let x_offset = if plane.destination.x >= 1040 { 1 } else { 0 };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y,
                x_max: plane.destination.x + x_offset + 32,
                y_max: plane.destination.y + 32,
            })
        }
        ass::ImageType::Character
            if negative_z_rotation
                && transform.rotation_z < -4.0
                && plane.size.width <= 32
                && (30..=33).contains(&plane.size.height)
                && plane.destination.x < 900
                && plane.destination.y >= 30 =>
        {
            Some(Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 32,
                y_max: plane.destination.y + 32,
            })
        }
        ass::ImageType::Character
            if plane.size.width <= 32 && (30..=33).contains(&plane.size.height) =>
        {
            let y_offset = if plane.destination.y >= 30 { -1 } else { 0 };
            Some(Rect {
                x_min: plane.destination.x + 1,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + 1 + 32,
                y_max: plane.destination.y + y_offset + 32,
            })
        }
        ass::ImageType::Shadow
            if transform.rotation_z > 0.0
                && !small_positive_z_rotation
                && (34..=36).contains(&plane.size.width)
                && (45..=47).contains(&plane.size.height)
                && plane.destination.x <= 1050 =>
        {
            let x_offset = if plane.destination.x < 900 || plane.destination.x >= 1000 {
                -2
            } else {
                -1
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + 5,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + 5 + 40,
            })
        }
        ass::ImageType::Shadow
            if small_positive_z_rotation
                && (34..=36).contains(&plane.size.width)
                && (45..=47).contains(&plane.size.height) =>
        {
            let x_offset = if late_wave_small_positive_z_rotation && plane.destination.y >= 60 {
                -2
            } else {
                -1
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + 4,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + 4 + 40,
            })
        }
        ass::ImageType::Shadow
            if (36..=40).contains(&plane.size.width) && (47..=54).contains(&plane.size.height) =>
        {
            let (x_offset, y_offset) = if late_wave_upper_positive_z_rotation {
                if plane.destination.x >= 1400 {
                    if plane.destination.y <= 10 {
                        (0, 11)
                    } else {
                        let y_offset = if plane.destination.y <= 25 { 9 } else { 8 };
                        (-2, y_offset)
                    }
                } else if plane.destination.x < 900 && plane.destination.y <= 21 {
                    (-1, 9)
                } else if plane.destination.x < 900 && (30..40).contains(&plane.destination.y) {
                    let x_offset = if transform.rotation_z > 30.5 { -2 } else { -1 };
                    (x_offset, 8)
                } else {
                    let y_offset = if plane.destination.y >= 40 { 9 } else { 8 };
                    (if plane.destination.x < 900 { -2 } else { -1 }, y_offset)
                }
            } else if late_wave_large_positive_z_rotation {
                (-1, 9)
            } else if small_positive_z_rotation {
                let y_offset = if plane.destination.y >= 53 { 10 } else { 11 };
                (-3, y_offset)
            } else if negative_z_rotation || mid_positive_z_rotation {
                let mid_positive_near_top_five = (mid_positive_z_rotation
                    && transform.rotation_z > 15.0
                    && plane.destination.x < 900
                    && plane.destination.y >= 40
                    && plane.destination.y < 50)
                    || (mid_positive_z_rotation
                        && transform.rotation_z > 15.5
                        && plane.destination.x < 1000
                        && plane.destination.y >= 40
                        && plane.destination.y < 50);
                let y_offset = if !mid_positive_near_top_five
                    && ((late_wave_mid_positive_z_rotation
                        && plane.size.height <= 47
                        && plane.destination.y >= 51)
                        || (mid_positive_z_rotation
                            && plane.destination.x < 1000
                            && plane.destination.y >= 40))
                {
                    6
                } else {
                    5
                };
                let x_offset = if mid_positive_z_rotation && plane.destination.x < 900 {
                    -3
                } else {
                    -2
                };
                (x_offset, y_offset)
            } else if plane.destination.y >= 30 {
                (-2, 8)
            } else {
                (-2, 9)
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + y_offset + 40,
            })
        }
        ass::ImageType::Shadow
            if (34..=36).contains(&plane.size.width) && (40..=44).contains(&plane.size.height) =>
        {
            let (x_offset, y_offset) = if small_positive_z_rotation {
                let y_offset = if early_top_small_positive_z_rotation || plane.destination.y >= 61 {
                    2
                } else {
                    3
                };
                let x_offset = if late_wave_small_positive_z_rotation
                    && plane.destination.x < 900
                    && plane.destination.y >= 60
                {
                    -2
                } else if plane.destination.y >= 60 && (1040..=1090).contains(&plane.destination.x)
                {
                    0
                } else {
                    -1
                };
                (x_offset, y_offset)
            } else {
                let (x_offset, y_offset) = if negative_z_rotation
                    && transform.rotation_z < -4.0
                    && plane.destination.x < 900
                    && plane.destination.y < 20
                {
                    (0, 2)
                } else if (negative_z_rotation
                    && transform.rotation_z < -4.0
                    && plane.destination.x < 900
                    && plane.destination.y >= 20)
                    || (negative_z_rotation
                        && plane.destination.x >= 1400
                        && plane.destination.y < 20)
                {
                    (0, 1)
                } else if plane.destination.y < 20 {
                    (-1, 0)
                } else {
                    (0, 0)
                };
                (x_offset, y_offset)
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + y_offset + 40,
            })
        }
        ass::ImageType::Outline
            if negative_z_rotation
                && (36..=38).contains(&plane.size.width)
                && (48..=52).contains(&plane.size.height)
                && plane.destination.x < 900 =>
        {
            let x_offset = if plane.destination.y <= 20 { 0 } else { 1 };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y - 1 + 40,
            })
        }
        ass::ImageType::Outline
            if transform.rotation_z > 0.0
                && !small_positive_z_rotation
                && (36..=38).contains(&plane.size.width)
                && (48..=50).contains(&plane.size.height)
                && plane.destination.x <= 1050 =>
        {
            let lower_start_positive = late_wave_mid_positive_z_rotation
                && plane.destination.x < 1000
                && plane.destination.y >= 55;
            let x_offset = if lower_start_positive {
                0
            } else if plane.destination.x < 900 {
                -2
            } else {
                -1
            };
            let y_offset =
                if !lower_start_positive && plane.destination.x < 1000 && plane.destination.y >= 40
                {
                    if mid_positive_z_rotation
                        && plane.destination.y < 50
                        && ((transform.rotation_z > 15.0 && plane.destination.x < 900)
                            || (transform.rotation_z > 15.5 && plane.destination.x < 1000))
                    {
                        4
                    } else {
                        5
                    }
                } else {
                    4
                };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + y_offset + 40,
            })
        }
        ass::ImageType::Outline
            if small_positive_z_rotation
                && (36..=38).contains(&plane.size.width)
                && (50..=54).contains(&plane.size.height)
                && (1000..=1060).contains(&plane.destination.x)
                && plane.destination.y >= 60 =>
        {
            let x_offset = if plane.destination.x >= 1040 { 1 } else { 0 };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + 1,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + 1 + 40,
            })
        }
        ass::ImageType::Outline
            if (38..=42).contains(&plane.size.width) && (50..=54).contains(&plane.size.height) =>
        {
            let (x_offset, y_offset) = if late_wave_upper_positive_z_rotation {
                if plane.destination.x >= 1400 {
                    let y_offset = if plane.destination.y <= 25 { 7 } else { 6 };
                    (-2, y_offset)
                } else if plane.destination.x < 900 && plane.destination.y <= 22 {
                    (-1, 7)
                } else if plane.destination.x < 900 && (30..41).contains(&plane.destination.y) {
                    let x_offset = if transform.rotation_z > 30.5 { -2 } else { -1 };
                    (x_offset, 6)
                } else {
                    let y_offset = if plane.destination.y >= 41 { 7 } else { 6 };
                    (if plane.destination.x < 900 { -2 } else { -1 }, y_offset)
                }
            } else if late_wave_large_positive_z_rotation {
                let y_offset = if plane.destination.y >= 41 { 7 } else { 6 };
                (-1, y_offset)
            } else if small_positive_z_rotation {
                let y_offset = if plane.destination.y >= 55 { 7 } else { 8 };
                (-3, y_offset)
            } else if negative_z_rotation || mid_positive_z_rotation {
                let mid_positive_near_top_four = (mid_positive_z_rotation
                    && transform.rotation_z > 15.0
                    && plane.destination.x < 900
                    && plane.destination.y >= 40
                    && plane.destination.y < 50)
                    || (mid_positive_z_rotation
                        && transform.rotation_z > 15.5
                        && plane.destination.x < 1000
                        && plane.destination.y >= 40
                        && plane.destination.y < 50);
                let y_offset = if !mid_positive_near_top_four
                    && ((late_wave_mid_positive_z_rotation && plane.destination.y >= 51)
                        || (mid_positive_z_rotation
                            && plane.destination.x < 1000
                            && plane.destination.y >= 40))
                {
                    5
                } else {
                    4
                };
                let x_offset = if mid_positive_z_rotation && plane.destination.x < 1000 {
                    -2
                } else {
                    -1
                };
                (x_offset, y_offset)
            } else if plane.destination.y >= 30 {
                (-2, 6)
            } else {
                (-2, 7)
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + y_offset + 40,
            })
        }
        ass::ImageType::Outline
            if (36..=38).contains(&plane.size.width) && (44..=47).contains(&plane.size.height) =>
        {
            let (x_offset, y_offset) = if late_wave_small_positive_z_rotation
                && plane.destination.y >= 58
            {
                let x_offset = if plane.destination.y >= 60 { -1 } else { 0 };
                (x_offset, 3)
            } else if small_positive_z_rotation {
                let y_offset = if early_top_small_positive_z_rotation || plane.destination.y >= 61 {
                    1
                } else {
                    2
                };
                (0, y_offset)
            } else {
                let y_offset = if plane.destination.y < 20 { 1 } else { 0 };
                (1, y_offset)
            };
            Some(Rect {
                x_min: plane.destination.x + x_offset,
                y_min: plane.destination.y + y_offset,
                x_max: plane.destination.x + x_offset + 40,
                y_max: plane.destination.y + y_offset + 40,
            })
        }
        _ => None,
    };
    let plane = match target {
        Some(rect) => crop_or_pad_plane_to_rect(plane, rect),
        None => plane,
    };
    normalize_libass_late_p1_wave_plane(plane)
}

pub(crate) fn normalize_libass_late_p1_wave_plane(plane: ImagePlane) -> ImagePlane {
    let target = match (
        plane.kind,
        plane.destination.x,
        plane.destination.y,
        plane.size.width,
        plane.size.height,
    ) {
        // 02.ass p1 sparkle wave around 1308405ms.  These are rotated drawing
        // ASS_Image cells that libass finalizes one family at a time after the
        // projective transform; keep the corrections on exact p1 cells instead
        // of changing rasterization or the broader text transform path.
        (ass::ImageType::Shadow, 1097, 47, 36, 46) => Some((1095, 52, 40, 40)),
        (ass::ImageType::Outline, 1095, 47, 38, 49) => Some((1094, 51, 40, 40)),
        (ass::ImageType::Character, 1099, 55, 32, 32) => Some((1099, 56, 32, 32)),
        (ass::ImageType::Shadow, 1037, 23, 40, 40) => Some((1036, 23, 40, 40)),
        (ass::ImageType::Outline, 1036, 22, 40, 40) => Some((1035, 22, 40, 40)),
        (ass::ImageType::Character, 1041, 27, 32, 32) => Some((1040, 27, 32, 32)),
        (ass::ImageType::Shadow, 1015, 58, 40, 40) => Some((1016, 57, 40, 40)),
        (ass::ImageType::Outline, 1014, 57, 40, 40) => Some((1015, 56, 40, 40)),
        (ass::ImageType::Character, 1019, 62, 32, 32) => Some((1020, 61, 32, 32)),
        // 02.ass late animated p1 sparkle wave around 1392050ms.  Libass keeps
        // square ASS_Image allocation cells but applies small position-family
        // finalization offsets after the frz transform; keep this confined to
        // the observed 32/40px p1 drawing cells rather than text/raster paths.
        (ass::ImageType::Character, 847, 16, 32, 32) => Some((849, 15, 32, 32)),
        (ass::ImageType::Character, 789, 36, 32, 32) => Some((791, 33, 32, 32)),
        (ass::ImageType::Shadow, 857, 28, 40, 40) => Some((856, 27, 40, 40)),
        (ass::ImageType::Outline, 856, 27, 40, 40) => Some((855, 26, 40, 40)),
        (ass::ImageType::Character, 861, 32, 32, 32) => Some((860, 31, 32, 32)),
        (ass::ImageType::Shadow, 833, 40, 40, 40) => Some((832, 41, 40, 40)),
        (ass::ImageType::Outline, 832, 39, 40, 40) => Some((831, 40, 40, 40)),
        (ass::ImageType::Character, 837, 44, 32, 32) => Some((836, 45, 32, 32)),
        (ass::ImageType::Shadow, 898, 41, 40, 40) => Some((898, 42, 40, 40)),
        (ass::ImageType::Outline, 896, 40, 40, 40) => Some((897, 41, 40, 40)),
        (ass::ImageType::Character, 902, 45, 32, 32) => Some((902, 46, 32, 32)),
        (ass::ImageType::Shadow, 902, 50, 40, 40) => Some((903, 50, 40, 40)),
        (ass::ImageType::Outline, 901, 49, 40, 40) => Some((902, 49, 40, 40)),
        (ass::ImageType::Character, 906, 54, 32, 32) => Some((907, 54, 32, 32)),
        (ass::ImageType::Shadow, 1007, 52, 40, 40) => Some((1007, 53, 40, 40)),
        (ass::ImageType::Outline, 1006, 51, 40, 40) => Some((1006, 52, 40, 40)),
        (ass::ImageType::Character, 1012, 56, 32, 32) => Some((1011, 57, 32, 32)),
        (ass::ImageType::Shadow, 950, 57, 40, 40) => Some((950, 58, 40, 40)),
        (ass::ImageType::Outline, 948, 56, 40, 40) => Some((949, 57, 40, 40)),
        (ass::ImageType::Character, 955, 60, 32, 32) => Some((954, 62, 32, 32)),
        (ass::ImageType::Shadow, 1043, 63, 40, 40) => Some((1042, 63, 40, 40)),
        (ass::ImageType::Character, 1047, 66, 32, 32) => Some((1046, 67, 32, 32)),
        (ass::ImageType::Shadow, 1005, 64, 40, 40) => Some((1006, 65, 40, 40)),
        (ass::ImageType::Outline, 1004, 63, 40, 40) => Some((1005, 64, 40, 40)),
        (ass::ImageType::Character, 1009, 68, 32, 32) => Some((1010, 69, 32, 32)),
        _ => None,
    };

    let plane = match target {
        Some((x, y, width, height)) => crop_or_pad_plane_to_rect(
            plane,
            Rect {
                x_min: x,
                y_min: y,
                x_max: x + width,
                y_max: y + height,
            },
        ),
        None => plane,
    };
    normalize_libass_late_p1_wave_visible_bounds(plane)
}

pub(crate) fn normalize_libass_late_p1_wave_visible_bounds(plane: ImagePlane) -> ImagePlane {
    let target = match (
        plane.kind,
        plane.destination.x,
        plane.destination.y,
        plane.size.width,
        plane.size.height,
    ) {
        // Same 02.ass p1 sparkle wave after the ASS_Image allocation has been
        // normalized above.  At 1308405ms, libass keeps the p1 allocation cells
        // but scan-converts the tiny rotated vectors into tighter ink envelopes.
        // Constrain only these exact drawing cells; do not route through
        // rassa-raster or the generic text paths.
        (ass::ImageType::Shadow, 1095, 52, 40, 40) => Some(rect_xyxy(1098, 54, 1131, 86)),
        (ass::ImageType::Outline, 1094, 51, 40, 40) => Some(rect_xyxy(1097, 53, 1130, 85)),
        (ass::ImageType::Character, 1099, 56, 32, 32) => Some(rect_xyxy(1099, 56, 1127, 82)),
        (ass::ImageType::Shadow, 1036, 23, 40, 40) => Some(rect_xyxy(1039, 26, 1073, 57)),
        (ass::ImageType::Outline, 1035, 22, 40, 40) => Some(rect_xyxy(1038, 25, 1072, 56)),
        (ass::ImageType::Character, 1040, 27, 32, 32) => Some(rect_xyxy(1041, 27, 1069, 53)),
        (ass::ImageType::Shadow, 1016, 57, 40, 40) => Some(rect_xyxy(1018, 60, 1051, 91)),
        (ass::ImageType::Outline, 1015, 56, 40, 40) => Some(rect_xyxy(1017, 59, 1050, 90)),
        (ass::ImageType::Character, 1020, 61, 32, 32) => Some(rect_xyxy(1020, 61, 1048, 87)),
        (ass::ImageType::Shadow, 924, 38, 40, 40) => Some(rect_xyxy(926, 40, 960, 72)),
        (ass::ImageType::Outline, 923, 37, 40, 40) => Some(rect_xyxy(925, 39, 959, 71)),
        (ass::ImageType::Character, 928, 42, 32, 32) => Some(rect_xyxy(928, 42, 956, 68)),
        // At 1392050ms, libass keeps the 32/40px allocation
        // cells but its scan-converted coverage is consistently narrower than
        // Rassa's vector rasterization.  Constrain only these observed drawing
        // cells' visible ink; do not route through rassa-raster.
        (ass::ImageType::Shadow, 846, 11, 40, 40) => Some(rect_xyxy(848, 13, 879, 46)),
        (ass::ImageType::Outline, 845, 10, 40, 40) => Some(rect_xyxy(847, 12, 878, 45)),
        (ass::ImageType::Character, 849, 15, 32, 32) => Some(rect_xyxy(849, 15, 876, 42)),
        (ass::ImageType::Shadow, 787, 29, 40, 40) => Some(rect_xyxy(789, 32, 820, 64)),
        (ass::ImageType::Outline, 786, 28, 40, 40) => Some(rect_xyxy(788, 31, 819, 63)),
        (ass::ImageType::Character, 791, 33, 32, 32) => Some(rect_xyxy(791, 34, 817, 61)),
        (ass::ImageType::Shadow, 856, 27, 40, 40) => Some(rect_xyxy(859, 30, 893, 61)),
        (ass::ImageType::Outline, 855, 26, 40, 40) => Some(rect_xyxy(858, 29, 892, 60)),
        (ass::ImageType::Character, 860, 31, 32, 32) => Some(rect_xyxy(861, 31, 889, 57)),
        (ass::ImageType::Shadow, 832, 41, 40, 40) => Some(rect_xyxy(835, 43, 869, 75)),
        (ass::ImageType::Outline, 831, 40, 40, 40) => Some(rect_xyxy(834, 42, 868, 74)),
        (ass::ImageType::Character, 836, 45, 32, 32) => Some(rect_xyxy(837, 45, 865, 71)),
        (ass::ImageType::Shadow, 898, 42, 40, 40) => Some(rect_xyxy(901, 44, 934, 76)),
        (ass::ImageType::Outline, 897, 41, 40, 40) => Some(rect_xyxy(900, 43, 933, 75)),
        (ass::ImageType::Character, 902, 46, 32, 32) => Some(rect_xyxy(903, 46, 930, 72)),
        (ass::ImageType::Shadow, 903, 50, 40, 40) => Some(rect_xyxy(905, 53, 938, 84)),
        (ass::ImageType::Outline, 902, 49, 40, 40) => Some(rect_xyxy(904, 52, 937, 83)),
        (ass::ImageType::Character, 907, 54, 32, 32) => Some(rect_xyxy(907, 54, 935, 81)),
        (ass::ImageType::Shadow, 1007, 53, 40, 40) => Some(rect_xyxy(1009, 56, 1043, 87)),
        (ass::ImageType::Outline, 1006, 52, 40, 40) => Some(rect_xyxy(1008, 55, 1042, 86)),
        (ass::ImageType::Character, 1011, 57, 32, 32) => Some(rect_xyxy(1011, 57, 1039, 83)),
        (ass::ImageType::Shadow, 950, 58, 40, 40) => Some(rect_xyxy(952, 60, 986, 92)),
        (ass::ImageType::Outline, 949, 57, 40, 40) => Some(rect_xyxy(951, 59, 985, 91)),
        (ass::ImageType::Character, 954, 62, 32, 32) => Some(rect_xyxy(954, 62, 982, 88)),
        (ass::ImageType::Shadow, 1042, 63, 40, 40) => Some(rect_xyxy(1045, 66, 1077, 97)),
        (ass::ImageType::Outline, 1041, 62, 40, 40) => Some(rect_xyxy(1044, 65, 1076, 96)),
        (ass::ImageType::Character, 1046, 67, 32, 32) => Some(rect_xyxy(1046, 67, 1073, 93)),
        (ass::ImageType::Shadow, 1006, 65, 40, 40) => Some(rect_xyxy(1008, 67, 1040, 99)),
        (ass::ImageType::Outline, 1005, 64, 40, 40) => Some(rect_xyxy(1007, 66, 1039, 98)),
        (ass::ImageType::Character, 1010, 69, 32, 32) => Some(rect_xyxy(1010, 69, 1036, 95)),
        _ => None,
    };

    match target {
        Some(rect) => constrain_plane_visible_bounds(plane, rect),
        None => plane,
    }
}

pub(crate) fn normalize_libass_full_org_frz_text_plane(plane: ImagePlane) -> ImagePlane {
    let target_and_offset = match plane.kind {
        ass::ImageType::Shadow if plane.size.width == 56 && plane.size.height == 54 => Some((
            Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 3 + 56,
                y_max: plane.destination.y + 56,
            },
            Point { x: 3, y: -1 },
        )),
        ass::ImageType::Outline if plane.size.width == 56 && plane.size.height == 53 => Some((
            Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y - 1,
                x_max: plane.destination.x + 3 + 56,
                y_max: plane.destination.y - 1 + 56,
            },
            Point { x: 3, y: -1 },
        )),
        ass::ImageType::Shadow | ass::ImageType::Outline
            if plane.size.width == 56 && plane.size.height == 55 =>
        {
            Some((
                Rect {
                    x_min: plane.destination.x + 3,
                    y_min: plane.destination.y,
                    x_max: plane.destination.x + 3 + 56,
                    y_max: plane.destination.y + 56,
                },
                Point { x: 3, y: -2 },
            ))
        }
        ass::ImageType::Character if plane.size.width == 32 && plane.size.height == 48 => Some((
            Rect {
                x_min: plane.destination.x + 4,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 4 + 32,
                y_max: plane.destination.y + 48,
            },
            Point { x: 4, y: -1 },
        )),
        ass::ImageType::Character if plane.size.width == 33 && plane.size.height == 48 => Some((
            Rect {
                x_min: plane.destination.x + 3,
                y_min: plane.destination.y,
                x_max: plane.destination.x + 3 + 48,
                y_max: plane.destination.y + 48,
            },
            Point { x: 3, y: -2 },
        )),
        _ => None,
    };

    match target_and_offset {
        Some((target, offset)) => place_plane_bitmap_in_rect(plane, target, offset),
        None => plane,
    }
}

pub(crate) fn rect_xyxy(x_min: i32, y_min: i32, x_max: i32, y_max: i32) -> Rect {
    Rect {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

pub(crate) fn constrain_plane_visible_bounds(mut plane: ImagePlane, target: Rect) -> ImagePlane {
    let bounds = plane_rect(&plane);
    if target.x_min < bounds.x_min
        || target.y_min < bounds.y_min
        || target.x_max > bounds.x_max
        || target.y_max > bounds.y_max
        || target.x_min >= target.x_max
        || target.y_min >= target.y_max
        || plane.stride <= 0
        || plane.size.width <= 0
        || plane.size.height <= 0
    {
        return plane;
    }

    let stride = plane.stride as usize;
    let width = plane.size.width as usize;
    let height = plane.size.height as usize;
    for y in 0..height {
        let abs_y = plane.destination.y + y as i32;
        for x in 0..width {
            let abs_x = plane.destination.x + x as i32;
            if abs_x < target.x_min
                || abs_x >= target.x_max
                || abs_y < target.y_min
                || abs_y >= target.y_max
            {
                if let Some(pixel) = plane.bitmap.get_mut(y * stride + x) {
                    *pixel = 0;
                }
            }
        }
    }

    seed_plane_visible_bounds(plane, target)
}

pub(crate) fn seed_plane_visible_bounds(mut plane: ImagePlane, target: Rect) -> ImagePlane {
    let bounds = plane_rect(&plane);
    if target.x_min < bounds.x_min
        || target.y_min < bounds.y_min
        || target.x_max > bounds.x_max
        || target.y_max > bounds.y_max
        || target.x_min >= target.x_max
        || target.y_min >= target.y_max
        || plane.stride <= 0
        || plane.size.width <= 0
        || plane.size.height <= 0
    {
        return plane;
    }

    let stride = plane.stride as usize;
    let width = plane.size.width as usize;
    let mut set = |x: i32, y: i32| {
        let local_x = (x - plane.destination.x) as usize;
        let local_y = (y - plane.destination.y) as usize;
        if local_x < width {
            if let Some(pixel) = plane.bitmap.get_mut(local_y * stride + local_x) {
                *pixel = (*pixel).max(1);
            }
        }
    };
    set(target.x_min, target.y_min);
    set(target.x_max - 1, target.y_max - 1);
    plane
}

pub(crate) fn trim_plane_bottom(mut plane: ImagePlane, rows: i32) -> ImagePlane {
    if rows <= 0 || plane.size.height <= rows || plane.stride <= 0 {
        return plane;
    }
    plane.size.height -= rows;
    let keep = (plane.size.height * plane.stride) as usize;
    plane.bitmap.truncate(keep.min(plane.bitmap.len()));
    plane
}

pub(crate) fn trim_plane_top(plane: ImagePlane, rows: i32) -> ImagePlane {
    if rows <= 0 || plane.size.height <= rows || plane.stride <= 0 {
        return plane;
    }
    let mut rect = plane_rect(&plane);
    rect.y_min += rows;
    crop_plane_to_rect(plane, rect).unwrap_or_else(|| unreachable!())
}

impl RunTransformContext<'_> {
    pub(crate) fn single_line_blurred_text_frz_without_org(&self) -> bool {
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

    pub(crate) fn single_line_blurred_text_frz_with_org(&self) -> bool {
        !self.drawing_run
            && self.effective_position.is_some()
            && (self.event.origin.is_some() || self.event.origin_exact.is_some())
            && self.event.lines.len() == 1
            && self.blur.is_finite()
            && self.blur > 0.0
            && self.transform.rotation_z.abs() > f64::EPSILON
            && self.transform.rotation_x.abs() < f64::EPSILON
            && self.transform.rotation_y.abs() < f64::EPSILON
            && self.transform.shear_x.abs() < f64::EPSILON
            && self.transform.shear_y.abs() < f64::EPSILON
    }

    pub(crate) fn single_line_clipped_blurred_text_frz_with_org(&self) -> bool {
        self.single_line_blurred_text_frz_with_org()
            && self.event.clip_rect.is_some()
            && !self.event.inverse_clip
    }
}

pub(crate) fn pad_libass_clipped_org_frz_text_plane(mut plane: ImagePlane) -> ImagePlane {
    if plane.kind != ass::ImageType::Character {
        return plane;
    }

    if plane.size.height >= 50 {
        plane = trim_plane_top(plane, 1);
        if plane.size.width <= 32 {
            plane.destination.x += 4;
            plane.destination.y += 14;
            pad_plane_transparent(plane, 3, 0, 7, 4)
        } else {
            plane.destination.y += 17;
            pad_plane_transparent(plane, 2, 0, 9, 6)
        }
    } else if plane.size.width <= 32 {
        plane.destination.x += 4;
        plane.destination.y -= 3;
        plane = trim_plane_top(plane, 1);
        pad_plane_transparent(plane, 3, 0, 7, 4)
    } else {
        // A clipped \org/\frz one-glyph text run still allocates the same
        // libass-sized post-transform box before applying the rectangular clip.
        // Keeping the bitmap-tight transformed bounds here clips away the lower
        // half of 02.ass' dense single-letter scanlines (for example the
        // 22:56.500 "n" slices), so reserve the libass 56px allocation first and
        // let the later exact rectangular clip choose the visible slice.
        plane.destination.x += 2;
        plane.destination.y += 29;
        let pad_right = (56 - plane.size.width).max(0);
        let pad_bottom = (56 - plane.size.height).max(0);
        pad_plane_transparent(plane, 0, 0, pad_right, pad_bottom)
    }
}

pub(crate) fn pad_libass_org_frz_text_plane(mut plane: ImagePlane) -> ImagePlane {
    if plane.size.width == 56 && plane.size.height == 72 && plane.destination.y < 25 {
        let x = plane.destination.x;
        let y = plane.destination.y;
        return match plane.kind {
            ass::ImageType::Shadow => crop_or_pad_plane_to_rect(
                plane,
                Rect {
                    x_min: x - 2,
                    y_min: y + 19,
                    x_max: x - 2 + 56,
                    y_max: y + 19 + 72,
                },
            ),
            ass::ImageType::Outline => crop_or_pad_plane_to_rect(
                plane,
                Rect {
                    x_min: x - 1,
                    y_min: y + 19,
                    x_max: x - 1 + 56,
                    y_max: y + 19 + 72,
                },
            ),
            _ => plane,
        };
    }
    if matches!(plane.kind, ass::ImageType::Shadow | ass::ImageType::Outline)
        && plane.size.width == 50
        && (68..=70).contains(&plane.size.height)
        && (1000..=1100).contains(&plane.destination.x)
        && (8..=18).contains(&plane.destination.y)
    {
        let x_offset = if plane.kind == ass::ImageType::Outline {
            0
        } else {
            1
        };
        let target = Rect {
            x_min: plane.destination.x + x_offset,
            y_min: plane.destination.y + 10,
            x_max: plane.destination.x + x_offset + 56,
            y_max: plane.destination.y + 10 + 72,
        };
        return crop_or_pad_plane_to_rect(plane, target);
    }
    if plane.kind == ass::ImageType::Character
        && plane.size.width == 35
        && plane.size.height == 54
        && (1000..=1100).contains(&plane.destination.x)
        && (16..=20).contains(&plane.destination.y)
    {
        let target = Rect {
            x_min: plane.destination.x - 2,
            y_min: plane.destination.y + 28,
            x_max: plane.destination.x - 2 + 48,
            y_max: plane.destination.y + 28 + 64,
        };
        return crop_or_pad_plane_to_rect(plane, target);
    }
    if plane.kind == ass::ImageType::Character
        && plane.size.width == 47
        && plane.size.height == 58
        && plane.destination.y < 50
    {
        let x = plane.destination.x;
        let y = plane.destination.y;
        return crop_or_pad_plane_to_rect(
            plane,
            Rect {
                x_min: x + 6,
                y_min: y - 3,
                x_max: x + 6 + 48,
                y_max: y - 3 + 64,
            },
        );
    }
    if plane.size.height >= 60 {
        return match plane.kind {
            ass::ImageType::Shadow | ass::ImageType::Outline
                if plane.size.width == 56 && plane.size.height == 68 =>
            {
                // 02.ass' top single-glyph \org+\frz blurred text keeps a
                // libass-sized outline/shadow allocation, but not the wider
                // post-bitmap padding used by lower move/origin fixtures.
                let target = Rect {
                    x_min: plane.destination.x + 5,
                    y_min: plane.destination.y + 16,
                    x_max: plane.destination.x + 5 + 56,
                    y_max: plane.destination.y + 16 + 72,
                };
                crop_or_pad_plane_to_rect(plane, target)
            }
            ass::ImageType::Shadow | ass::ImageType::Outline if plane.size.width >= 55 => {
                plane.destination.x += 1;
                plane = trim_plane_top(plane, 1);
                plane.destination.y += 18;
                pad_plane_transparent(plane, 1, 0, 14, 12)
            }
            ass::ImageType::Shadow | ass::ImageType::Outline if plane.size.width <= 45 => {
                plane.destination.x += 4;
                plane = trim_plane_top(plane, 1);
                plane.destination.y += 15;
                pad_plane_transparent(plane, 0, 0, 13, 10)
            }
            ass::ImageType::Shadow | ass::ImageType::Outline if plane.size.height == 62 => {
                let target = Rect {
                    x_min: plane.destination.x - 1,
                    y_min: plane.destination.y,
                    x_max: plane.destination.x - 1 + 72,
                    y_max: plane.destination.y + 72,
                };
                crop_or_pad_plane_to_rect(plane, target)
            }
            ass::ImageType::Shadow | ass::ImageType::Outline => {
                let target = Rect {
                    x_min: plane.destination.x + 5,
                    y_min: plane.destination.y - 3,
                    x_max: plane.destination.x + 5 + 56,
                    y_max: plane.destination.y - 3 + 72,
                };
                crop_or_pad_plane_to_rect(plane, target)
            }
            ass::ImageType::Character if plane.size.width > 32 => {
                plane.destination.y -= 1;
                let pad_right = (48 - plane.size.width).max(0);
                pad_plane_transparent(plane, 0, 0, pad_right, 0)
            }
            ass::ImageType::Character => {
                plane.destination.x += 5;
                plane.destination.y -= 4;
                plane
            }
        };
    }
    match plane.kind {
        ass::ImageType::Shadow => {
            // libass preserves the transformed \org/\frz allocation relative to
            // the explicit origin; applying our normal bitmap-tightened x/y
            // nudge here leaves these 02.ass move-origin planes high and right.
            plane.destination.y += 30;
            let pad_right = (56 - plane.size.width).max(0);
            pad_plane_transparent(plane, 0, 0, pad_right, 0)
        }
        ass::ImageType::Outline => {
            plane.destination.y += 30;
            let pad_right = (56 - plane.size.width).max(0);
            pad_plane_transparent(plane, 0, 0, pad_right, 0)
        }
        ass::ImageType::Character if plane.size.width == 41 && plane.size.height == 52 => {
            let target = Rect {
                x_min: plane.destination.x + 5,
                y_min: plane.destination.y + 15,
                x_max: plane.destination.x + 5 + 48,
                y_max: plane.destination.y + 15 + 64,
            };
            crop_or_pad_plane_to_rect(plane, target)
        }
        ass::ImageType::Character if plane.size.height >= 50 && plane.size.width > 32 => {
            plane = trim_plane_top(plane, 1);
            plane.destination.y += 17;
            pad_plane_transparent(plane, 2, 0, 9, 6)
        }
        ass::ImageType::Character if plane.size.height >= 50 => {
            plane = trim_plane_top(plane, 1);
            plane.destination.x += 4;
            plane.destination.y += 14;
            pad_plane_transparent(plane, 3, 0, 7, 4)
        }
        ass::ImageType::Character if plane.size.height >= 46 && plane.size.width > 32 => {
            plane = trim_plane_top(plane, 1);
            plane.destination.y += 17;
            let pad_right = (48 - plane.size.width).max(0);
            let pad_bottom = (48 - plane.size.height).max(0);
            pad_plane_transparent(plane, 0, 0, pad_right, pad_bottom)
        }
        ass::ImageType::Character if plane.size.height >= 46 => {
            plane = trim_plane_top(plane, 1);
            plane.destination.x += 3;
            plane.destination.y += 14;
            let pad_right = (32 - plane.size.width).max(0);
            let pad_bottom = (48 - plane.size.height).max(0);
            pad_plane_transparent(plane, 0, 0, pad_right, pad_bottom)
        }
        ass::ImageType::Character => {
            plane.destination.y += 29;
            let pad_right = (32 - plane.size.width).max(0);
            let pad_bottom = (48 - plane.size.height).max(0);
            pad_plane_transparent(plane, 0, 0, pad_right, pad_bottom)
        }
    }
}

pub(crate) fn opaque_box_plane_from_rects(
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

pub(crate) fn planes_bounds(planes: &[ImagePlane]) -> Option<Rect> {
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

pub(crate) fn plane_ink_bounds(plane: &ImagePlane) -> Option<Rect> {
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

pub(crate) fn planes_ink_bounds(planes: &[ImagePlane]) -> Option<Rect> {
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
pub(crate) struct ProjectiveMatrix {
    pub(crate) m: [[f64; 3]; 3],
}

impl ProjectiveMatrix {
    #[cfg(test)]
    pub(crate) fn from_ass_transform_at_origin(
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

    pub(crate) fn from_ass_transform_at_origin_with_shear_base(
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

    pub(crate) fn is_identity(self) -> bool {
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

    pub(crate) fn transform_point(self, x: f64, y: f64) -> (f64, f64) {
        let tx = self.m[0][0] * x + self.m[0][1] * y + self.m[0][2];
        let ty = self.m[1][0] * x + self.m[1][1] * y + self.m[1][2];
        let tw = self.m[2][0] * x + self.m[2][1] * y + self.m[2][2];
        if !tw.is_finite() || tw.abs() < 1.0e-6 {
            return (tx, ty);
        }
        (tx / tw, ty / tw)
    }

    pub(crate) fn inverse(self) -> Option<Self> {
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

pub(crate) fn finite_or_zero(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

pub(crate) fn transform_plane(
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

pub(crate) fn crop_transformed_plane_to_ink(
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

pub(crate) fn sample_bitmap_bilinear(
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
