use super::*;

pub(crate) fn apply_event_clip(
    planes: Vec<ImagePlane>,
    clip_rect: Rect,
    inverse: bool,
) -> Vec<ImagePlane> {
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

/// Map a vector `\clip`/`\iclip` drawing from script (PlayRes) coordinates into
/// render space, mirroring how `scale_clip_rect` and position tags are scaled.
/// libass scales clip drawings by the same frame transform as glyph positions;
/// without this the polygon stays in the PlayRes corner and clips everything.
pub(crate) fn scale_vector_clip(
    clip: &ParsedVectorClip,
    mapping: &EventMapping,
) -> Option<ParsedVectorClip> {
    let mut polygons = Vec::new();
    polygons.try_reserve_exact(clip.polygons.len()).ok()?;
    for polygon in &clip.polygons {
        let mut scaled = Vec::new();
        scaled.try_reserve_exact(polygon.len()).ok()?;
        for point in polygon {
            let point = Point {
                x: libass_outline_coordinate_from_f64(mapping.map_x_pos(f64::from(point.x)))?,
                y: libass_outline_coordinate_from_f64(mapping.map_y_pos(f64::from(point.y)))?,
            };
            if !libass_outline_point_is_valid(point) {
                return None;
            }
            scaled.push(point);
        }
        polygons.push(scaled);
    }
    Some(ParsedVectorClip {
        scale: clip.scale,
        polygons,
    })
}

pub(crate) fn apply_vector_clip(
    planes: Vec<ImagePlane>,
    clip: &ParsedVectorClip,
    inverse: bool,
) -> Vec<ImagePlane> {
    planes
        .into_iter()
        .filter_map(|plane| mask_plane_with_vector_clip(plane, clip, inverse))
        .collect()
}

pub(crate) fn mask_plane_with_vector_clip(
    plane: ImagePlane,
    clip: &ParsedVectorClip,
    inverse: bool,
) -> Option<ImagePlane> {
    if clip
        .polygons
        .iter()
        .flatten()
        .copied()
        .any(|point| !libass_outline_point_is_valid(point))
    {
        // Programmatically constructed clips bypass the ASS parser.  Mirror
        // libass's invalid-outline behavior here too: do not apply either
        // regular or inverse clipping.
        return Some(plane);
    }
    if clip.polygons.is_empty() {
        return inverse.then_some(plane);
    }

    let stride = usize::try_from(plane.stride).ok()?;
    let width = usize::try_from(plane.size.width).ok()?;
    let height = usize::try_from(plane.size.height).ok()?;
    let required_len = stride.checked_mul(height)?;
    if stride < width || required_len > plane.bitmap.len() {
        return None;
    }
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(plane.bitmap.len()).ok()?;
    bitmap.extend_from_slice(&plane.bitmap);
    let mut clip_min_x = width;
    let mut clip_min_y = height;
    let mut clip_max_x = 0_usize;
    let mut clip_max_y = 0_usize;

    for row in 0..height {
        for column in 0..width {
            let global_x = i64::from(plane.destination.x) + i64::try_from(column).ok()?;
            let global_y = i64::from(plane.destination.y) + i64::try_from(row).ok()?;
            let inside = point_in_drawing_polygons_at(
                global_x as f64 + 0.5,
                global_y as f64 + 0.5,
                &clip.polygons,
            );
            if inside {
                clip_min_x = clip_min_x.min(column);
                clip_min_y = clip_min_y.min(row);
                clip_max_x = clip_max_x.max(column + 1);
                clip_max_y = clip_max_y.max(row + 1);
            }
            let keep = if inverse { !inside } else { inside };
            let index = row.checked_mul(stride)?.checked_add(column)?;
            if !keep {
                *bitmap.get_mut(index)? = 0;
            }
        }
    }

    let masked = ImagePlane { bitmap, ..plane };
    if inverse {
        return Some(masked);
    }
    if clip_min_x >= clip_max_x || clip_min_y >= clip_max_y {
        return Some(zero_size_plane(masked));
    }
    crop_plane_to_bitmap_bounds(
        masked, clip_min_x, clip_min_y, clip_max_x, clip_max_y, 0, 0, 0, 0,
    )
}

fn zero_size_plane(plane: ImagePlane) -> ImagePlane {
    ImagePlane {
        size: Size {
            width: 0,
            height: 0,
        },
        stride: 0,
        bitmap: Vec::new(),
        ..plane
    }
}

pub(crate) fn drawing_pixel_coverage(x: i32, y: i32, polygons: &[Vec<Point>]) -> u8 {
    const SAMPLES: [f64; 4] = [0.125, 0.375, 0.625, 0.875];
    let mut inside = 0_u32;
    for sample_y in SAMPLES {
        for sample_x in SAMPLES {
            if point_in_drawing_polygons_at(x as f64 + sample_x, y as f64 + sample_y, polygons) {
                inside += 1;
            }
        }
    }
    if inside == 0 {
        0
    } else {
        ((inside * 255 + 8) / 16) as u8
    }
}

/// libass rasterizes drawings (and vector clips) with its standard
/// nonzero-winding rasterizer (ass_rasterizer.c get_fill_flags: solid when
/// the winding count is non-zero); holes require opposite-direction
/// subpaths, not even-odd alternation.
pub(crate) fn point_in_drawing_polygons_at(
    sample_x: f64,
    sample_y: f64,
    polygons: &[Vec<Point>],
) -> bool {
    polygons.iter().fold(0_i64, |winding, polygon| {
        winding.saturating_add(polygon_winding_at(sample_x, sample_y, polygon))
    }) != 0
}

pub(crate) fn polygon_winding_at(sample_x: f64, sample_y: f64, polygon: &[Point]) -> i64 {
    if polygon.len() < 3 {
        return 0;
    }

    let mut winding = 0_i64;
    let mut previous = polygon[polygon.len() - 1];

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
                winding += if current_y > previous_y { 1 } else { -1 };
            }
        }
        previous = current;
    }

    winding
}

pub(crate) fn clip_plane(plane: ImagePlane, clip_rect: Rect) -> Option<ImagePlane> {
    if plane.size.width <= 0 || plane.size.height <= 0 || plane.stride <= 0 {
        return Some(plane);
    }

    let plane_rect = plane_rect(&plane);
    let intersection = plane_rect.intersect(clip_rect)?;
    if intersection == plane_rect {
        return Some(plane);
    }
    crop_plane_to_rect(plane, intersection)
}

pub(crate) fn inverse_clip_plane(plane: ImagePlane, clip_rect: Rect) -> Vec<ImagePlane> {
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

pub(crate) fn plane_rect(plane: &ImagePlane) -> Rect {
    Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x.saturating_add(plane.size.width),
        y_max: plane.destination.y.saturating_add(plane.size.height),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn crop_plane_to_bitmap_bounds(
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
    let plane_width = usize::try_from(plane.size.width).ok()?;
    let plane_height = usize::try_from(plane.size.height).ok()?;
    let x_min = plane
        .destination
        .x
        .saturating_add(i32::try_from(min_x.saturating_sub(pad_left).min(plane_width)).ok()?);
    let y_min = plane
        .destination
        .y
        .saturating_add(i32::try_from(min_y.saturating_sub(pad_top).min(plane_height)).ok()?);
    let x_max = plane
        .destination
        .x
        .saturating_add(i32::try_from(max_x.saturating_add(pad_right).min(plane_width)).ok()?);
    let y_max = plane
        .destination
        .y
        .saturating_add(i32::try_from(max_y.saturating_add(pad_bottom).min(plane_height)).ok()?);
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

pub(crate) fn crop_plane_to_rect(plane: ImagePlane, rect: Rect) -> Option<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let rect = plane_rect.intersect(rect)?;
    if rect == plane_rect {
        return Some(plane);
    }
    let offset_x = usize::try_from(rect.x_min.checked_sub(plane_rect.x_min)?).ok()?;
    let offset_y = usize::try_from(rect.y_min.checked_sub(plane_rect.y_min)?).ok()?;
    let width = usize::try_from(rect.width()).ok()?;
    let height = usize::try_from(rect.height()).ok()?;
    let src_stride = usize::try_from(plane.stride).ok()?;
    let source_height = usize::try_from(plane.size.height).ok()?;
    let source_width = usize::try_from(plane.size.width).ok()?;
    let source_len = src_stride.checked_mul(source_height)?;
    if src_stride < source_width || source_len > plane.bitmap.len() {
        return None;
    }
    let bitmap_len = width.checked_mul(height)?;
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(bitmap_len).ok()?;

    for row in 0..height {
        let start = offset_y
            .checked_add(row)?
            .checked_mul(src_stride)?
            .checked_add(offset_x)?;
        let end = start.checked_add(width)?;
        bitmap.extend_from_slice(plane.bitmap.get(start..end)?);
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
pub(crate) fn is_event_active(event: &ParsedEvent, now_ms: i64) -> bool {
    now_ms >= event.start && now_ms < event.start + event.duration
}
