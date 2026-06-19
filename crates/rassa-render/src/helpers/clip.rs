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
pub(crate) fn scale_vector_clip(clip: &ParsedVectorClip, mapping: &EventMapping) -> ParsedVectorClip {
    ParsedVectorClip {
        scale: clip.scale,
        polygons: clip
            .polygons
            .iter()
            .map(|poly| {
                poly.iter()
                    .map(|point| Point {
                        x: mapping.map_x_pos(f64::from(point.x)).round() as i32,
                        y: mapping.map_y_pos(f64::from(point.y)).round() as i32,
                    })
                    .collect()
            })
            .collect(),
    }
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
            let inside = point_in_drawing_polygons_at(
                f64::from(global_x) + 0.5,
                f64::from(global_y) + 0.5,
                &clip.polygons,
            );
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
    crop_plane_to_bitmap_bounds(masked, min_x, min_y, max_x, max_y, 0, 0, 0, 0)
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
    polygons
        .iter()
        .map(|polygon| polygon_winding_at(sample_x, sample_y, polygon))
        .sum::<i32>()
        != 0
}

pub(crate) fn polygon_winding_at(sample_x: f64, sample_y: f64, polygon: &[Point]) -> i32 {
    if polygon.len() < 3 {
        return 0;
    }

    let mut winding = 0;
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
        x_max: plane.destination.x + plane.size.width,
        y_max: plane.destination.y + plane.size.height,
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

pub(crate) fn crop_plane_to_rect(plane: ImagePlane, rect: Rect) -> Option<ImagePlane> {
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
pub(crate) fn is_event_active(event: &ParsedEvent, now_ms: i64) -> bool {
    now_ms >= event.start && now_ms < event.start + event.duration
}
