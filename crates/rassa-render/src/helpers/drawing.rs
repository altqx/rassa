use super::*;

pub(crate) fn image_planes_from_absolute_glyphs(
    glyphs: &[RasterGlyph],
    color: u32,
    kind: ass::ImageType,
) -> Vec<ImagePlane> {
    glyphs
        .iter()
        .filter_map(|glyph| {
            let stride = usize::try_from(glyph.stride).ok()?;
            let width = usize::try_from(glyph.width).ok()?;
            let height = usize::try_from(glyph.height).ok()?;
            let required_len = stride.checked_mul(height)?;
            if width == 0 || height == 0 || stride < width || required_len > glyph.bitmap.len() {
                return None;
            }

            let mut bitmap = Vec::new();
            bitmap.try_reserve_exact(glyph.bitmap.len()).ok()?;
            bitmap.extend_from_slice(&glyph.bitmap);
            Some(ImagePlane {
                size: Size {
                    width: glyph.width,
                    height: glyph.height,
                },
                stride: glyph.stride,
                color: rgba_color_from_ass(color),
                destination: Point {
                    x: glyph.left,
                    y: glyph.top.saturating_sub(glyph.height),
                },
                kind,
                bitmap,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawingPlaneParams {
    pub(crate) origin_x: i32,
    pub(crate) line_top: i32,
    pub(crate) color: u32,
    pub(crate) render_scale_y: f64,
    pub(crate) baseline_offset: f64,
    /// Fractional drawing translation in 26.6; quantized identity drawings store the bitmap-cache remainder.
    pub(crate) anchor_phase_d6: Point,
}

pub(crate) fn image_plane_from_drawing(
    polygons: &[Vec<Point>],
    params: DrawingPlaneParams,
) -> Option<ImagePlane> {
    let bounds = drawing_pixel_bounds_with_phase_d6(polygons, params.anchor_phase_d6)?;
    let (width, height, bitmap_len) = checked_drawing_bitmap_dimensions(bounds)?;
    let sample_grid = drawing_sample_grid_d6(polygons);

    let stride = usize::try_from(width).ok()?;
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(bitmap_len).ok()?;
    bitmap.resize(bitmap_len, 0_u8);
    let mut any_visible = false;

    for row in 0..usize::try_from(height).ok()? {
        for column in 0..stride {
            let x = bounds.x_min.checked_add(i32::try_from(column).ok()?)?;
            let y = bounds.y_min.checked_add(i32::try_from(row).ok()?)?;
            let coverage =
                drawing_pixel_coverage_d6(x, y, polygons, sample_grid, params.anchor_phase_d6);
            if coverage > 0 {
                bitmap[row * stride + column] = coverage;
                any_visible = true;
            }
        }
    }

    // \pbo is a signed baseline offset; positive moves the drawing down (desc = pbo).
    let vertical_offset =
        libass_outline_coordinate_from_f64(params.baseline_offset * params.render_scale_y)?;

    if !any_visible {
        return None;
    }

    Some(ImagePlane {
        size: Size { width, height },
        stride: width,
        color: rgba_color_from_ass(params.color),
        destination: Point {
            x: params.origin_x.checked_add(bounds.x_min)?,
            y: params
                .line_top
                .checked_add(bounds.y_min)?
                .checked_add(vertical_offset)?,
        },
        kind: ass::ImageType::Character,
        bitmap,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QuantizedIdentityDrawing {
    pub(crate) polygons: Vec<Vec<Point>>,
    pub(crate) origin: Point,
    pub(crate) phase_d6: Point,
    pub(crate) restored_scale: (f64, f64),
}

/// Identity \an7 drawing: quantize/restore the raster matrix; layout metrics keep exact-scale polygons.
#[allow(clippy::too_many_arguments)]
pub(crate) fn quantized_identity_drawing(
    drawing: &ParsedDrawing,
    drawing_text: &str,
    scale_x: f64,
    scale_y: f64,
    render_scale_x: f64,
    render_scale_y: f64,
    anchor_d6: Point,
    drawing_pbo: f64,
) -> Option<QuantizedIdentityDrawing> {
    let scale_base = libass_drawing_scale_base(drawing.scale);
    if scale_base <= 0 {
        return None;
    }
    let source = parse_drawing_polygons_d6(drawing_text, drawing.scale)?;
    let layout_bbox = parse_drawing_bbox_d6(drawing_text, drawing.scale)?;
    let outline_cbox = parse_drawing_outline_cbox_d6(drawing_text, drawing.scale)?;
    if !scale_x.is_finite()
        || !scale_y.is_finite()
        || scale_x <= 0.0
        || scale_y <= 0.0
        || !render_scale_x.is_finite()
        || !render_scale_y.is_finite()
        || render_scale_x <= 0.0
        || render_scale_y <= 0.0
    {
        return None;
    }
    let coordinate_scale = 1.0 / f64::from(scale_base);
    let scale_x = style_scale(scale_x) * render_scale_x * coordinate_scale;
    let scale_y = style_scale(scale_y) * render_scale_y * coordinate_scale;
    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
        return None;
    }

    let cbox_center_x = (f64::from(outline_cbox.x_min) + f64::from(outline_cbox.x_max)) / 2.0;
    let cbox_center_y = (f64::from(outline_cbox.y_min) + f64::from(outline_cbox.y_max)) / 2.0;
    let cbox_radius_x =
        (f64::from(outline_cbox.x_max) - f64::from(outline_cbox.x_min)) / 2.0 + 64.0;
    let cbox_radius_y =
        (f64::from(outline_cbox.y_max) - f64::from(outline_cbox.y_min)) / 2.0 + 64.0;
    if cbox_radius_x <= 0.0 || cbox_radius_y <= 0.0 {
        return None;
    }

    // drawing_pbo is stored as int, so truncate toward zero before the 64× scale.
    let pbo = if drawing_pbo.is_finite() {
        drawing_pbo
            .trunc()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX))
    } else {
        0.0
    };
    let layout_height_d6 = f64::from(layout_bbox.y_max) - f64::from(layout_bbox.y_min);
    let asc_exact_d6 = (layout_height_d6 - 64.0 * pbo) * scale_y;
    let asc_d6 = checked_round_ties_even_i32(asc_exact_d6)?;
    // Negative drawing ascent (positive \pbo) contributes 0 to the line base; outline offset stays signed.
    let line_asc_d6 = asc_d6.max(0);
    let exact_center_x = f64::from(anchor_d6.x) + cbox_center_x * scale_x;
    let exact_center_y =
        f64::from(anchor_d6.y) + f64::from(line_asc_d6) - asc_exact_d6 + cbox_center_y * scale_y;
    let qr_x = checked_round_ties_even_i32(exact_center_x / 8.0)?;
    let qr_y = checked_round_ties_even_i32(exact_center_y / 8.0)?;

    let quantized_scale = |scale: f64, radius: f64| {
        let coefficient = checked_round_ties_even_i32(scale * radius / 8.0)?;
        Some(f64::from(coefficient) * 8.0 / radius)
    };
    let restored_scale_x = quantized_scale(scale_x, cbox_radius_x)?;
    let restored_scale_y = quantized_scale(scale_y, cbox_radius_y)?;
    let mut polygons = Vec::new();
    polygons.try_reserve_exact(source.len()).ok()?;
    for polygon in source {
        let mut transformed = Vec::new();
        transformed.try_reserve_exact(polygon.len()).ok()?;
        for point in polygon {
            let point = Point {
                x: checked_round_ties_even_i32(
                    restored_scale_x * (f64::from(point.x) - cbox_center_x),
                )?,
                y: checked_round_ties_even_i32(
                    restored_scale_y * (f64::from(point.y) - cbox_center_y),
                )?,
            };
            if !fixed_d6_point_is_valid(point) {
                return None;
            }
            transformed.push(point);
        }
        polygons.push(transformed);
    }

    Some(QuantizedIdentityDrawing {
        polygons,
        origin: Point {
            x: qr_x.div_euclid(8),
            y: qr_y.div_euclid(8),
        },
        phase_d6: Point {
            x: qr_x.rem_euclid(8) * 8,
            y: qr_y.rem_euclid(8) * 8,
        },
        restored_scale: (restored_scale_x, restored_scale_y),
    })
}

fn checked_round_ties_even_i32(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let rounded = value.round_ties_even();
    (rounded >= f64::from(i32::MIN) && rounded <= f64::from(i32::MAX)).then_some(rounded as i32)
}

pub(crate) fn scaled_drawing_polygons(
    drawing: &ParsedDrawing,
    drawing_text: &str,
    scale_x: f64,
    scale_y: f64,
    render_scale_x: f64,
    render_scale_y: f64,
) -> Option<Vec<Vec<Point>>> {
    let scale_base = libass_drawing_scale_base(drawing.scale);
    if scale_base <= 0 {
        return Some(Vec::new());
    }
    let fixed = parse_drawing_polygons_d6(drawing_text, drawing.scale);
    let (source, coordinate_scale) = match fixed.as_deref() {
        // Source is already 26.6; keep that precision through the style/frame transform.
        Some(polygons) => (polygons, 1.0 / f64::from(scale_base)),
        // Programmatic ParsedDrawing polygons are integer; promote them to 26.6 first.
        None => (drawing.polygons.as_slice(), 64.0),
    };
    let scale_x = style_scale(scale_x) * render_scale_x * coordinate_scale;
    let scale_y = style_scale(scale_y) * render_scale_y * coordinate_scale;
    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
        return None;
    }
    let mut polygons = Vec::new();
    polygons.try_reserve_exact(source.len()).ok()?;
    for polygon in source {
        let mut scaled = Vec::new();
        scaled.try_reserve_exact(polygon.len()).ok()?;
        for point in polygon {
            let point = Point {
                x: fixed_d6_coordinate_from_f64(f64::from(point.x) * scale_x)?,
                y: fixed_d6_coordinate_from_f64(f64::from(point.y) * scale_y)?,
            };
            if !fixed_d6_point_is_valid(point) {
                return None;
            }
            scaled.push(point);
        }
        polygons.push(scaled);
    }
    Some(polygons)
}

fn drawing_pixel_bounds_with_phase_d6(polygons: &[Vec<Point>], phase_d6: Point) -> Option<Rect> {
    let mut points = polygons.iter().flat_map(|polygon| polygon.iter().copied());
    let first = points.next()?;
    if !fixed_d6_point_is_valid(first) {
        return None;
    }
    let mut x_min = first.x.checked_add(phase_d6.x)?;
    let mut y_min = first.y.checked_add(phase_d6.y)?;
    let mut x_max = x_min;
    let mut y_max = y_min;
    for point in points {
        if !fixed_d6_point_is_valid(point) {
            return None;
        }
        let x = point.x.checked_add(phase_d6.x)?;
        let y = point.y.checked_add(phase_d6.y)?;
        x_min = x_min.min(x);
        y_min = y_min.min(y);
        x_max = x_max.max(x);
        y_max = y_max.max(y);
    }
    let floor_d6 = |value: i32| value.div_euclid(64);
    Some(Rect {
        x_min: floor_d6(x_min),
        y_min: floor_d6(y_min),
        x_max: floor_d6(x_max).checked_add(1)?,
        y_max: floor_d6(y_max).checked_add(1)?,
    })
}

pub(crate) fn drawing_height_from_d6(polygons: &[Vec<Point>]) -> Option<i32> {
    fixed_d6_coordinate_from_f64(drawing_height_exact_from_d6(polygons)?)
}

pub(crate) fn drawing_height_exact_from_d6(polygons: &[Vec<Point>]) -> Option<f64> {
    let mut ys = polygons.iter().flatten().map(|point| point.y);
    let first = ys.next()?;
    let (mut y_min, mut y_max) = (first, first);
    for y in ys {
        y_min = y_min.min(y);
        y_max = y_max.max(y);
    }
    Some(f64::from(y_max.checked_sub(y_min)?) / 64.0)
}

fn fixed_d6_coordinate_from_f64(value: f64) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let rounded = value.round();
    if rounded < -f64::from(LIBASS_OUTLINE_MAX_D6) || rounded > f64::from(LIBASS_OUTLINE_MAX_D6) {
        return None;
    }
    Some(rounded as i32)
}

fn fixed_d6_point_is_valid(point: Point) -> bool {
    (-LIBASS_OUTLINE_MAX_D6..=LIBASS_OUTLINE_MAX_D6).contains(&point.x)
        && (-LIBASS_OUTLINE_MAX_D6..=LIBASS_OUTLINE_MAX_D6).contains(&point.y)
}

fn drawing_sample_grid_d6(polygons: &[Vec<Point>]) -> u32 {
    let Some(bounds) = drawing_d6_bounds(polygons) else {
        return 4;
    };
    let bounds_area =
        i128::from(bounds.x_max - bounds.x_min) * i128::from(bounds.y_max - bounds.y_min);
    if bounds_area <= 0 {
        return 4;
    }
    let signed_double_area = polygons.iter().fold(0_i128, |total, polygon| {
        if polygon.len() < 3 {
            return total;
        }
        let contour = polygon
            .iter()
            .copied()
            .zip(polygon.iter().copied().cycle().skip(1))
            .take(polygon.len())
            .fold(0_i128, |area, (left, right)| {
                area + i128::from(left.x) * i128::from(right.y)
                    - i128::from(right.x) * i128::from(left.y)
            });
        total.saturating_add(contour)
    });
    // Thin compound paths (low net area vs outline box) use denser sampling; ordinary fills stay 4×4.
    let thin_area_ceiling = ((bounds_area as u128) * 2 - 1) / 5;
    if signed_double_area.unsigned_abs() <= thin_area_ceiling {
        16
    } else {
        4
    }
}

pub(crate) fn drawing_d6_bounds(polygons: &[Vec<Point>]) -> Option<Rect> {
    let mut points = polygons.iter().flatten().copied();
    let first = points.next()?;
    let mut bounds = Rect {
        x_min: first.x,
        y_min: first.y,
        x_max: first.x,
        y_max: first.y,
    };
    for point in points {
        bounds.x_min = bounds.x_min.min(point.x);
        bounds.y_min = bounds.y_min.min(point.y);
        bounds.x_max = bounds.x_max.max(point.x);
        bounds.y_max = bounds.y_max.max(point.y);
    }
    Some(bounds)
}

fn drawing_pixel_coverage_d6(
    x: i32,
    y: i32,
    polygons: &[Vec<Point>],
    sample_grid: u32,
    phase_d6: Point,
) -> u8 {
    // 4×4 cannot split adjacent 1/8-pixel phases; refine only outline-crossed pixels (thin paths already use 16×16).
    let sample_grid = if sample_grid < 8
        && phase_d6 != Point::default()
        && drawing_edge_intersects_pixel_d6(x, y, polygons, phase_d6)
    {
        8
    } else {
        sample_grid
    };
    let mut inside = 0_u32;
    for row in 0..sample_grid {
        let sample_y = (f64::from(y) + (f64::from(row) + 0.5) / f64::from(sample_grid)) * 64.0
            - f64::from(phase_d6.y);
        for column in 0..sample_grid {
            let sample_x = (f64::from(x) + (f64::from(column) + 0.5) / f64::from(sample_grid))
                * 64.0
                - f64::from(phase_d6.x);
            if point_in_drawing_polygons_at(sample_x, sample_y, polygons) {
                inside += 1;
            }
        }
    }
    if inside == 0 {
        0
    } else {
        ((inside * 255 + sample_grid * sample_grid / 2) / (sample_grid * sample_grid)) as u8
    }
}

fn drawing_edge_intersects_pixel_d6(
    x: i32,
    y: i32,
    polygons: &[Vec<Point>],
    phase_d6: Point,
) -> bool {
    let left = i64::from(x) * 64;
    let top = i64::from(y) * 64;
    let right = left + 64;
    let bottom = top + 64;
    polygons.iter().any(|polygon| {
        polygon
            .iter()
            .copied()
            .zip(polygon.iter().copied().cycle().skip(1))
            .take(polygon.len())
            .any(|(start, end)| {
                segment_intersects_rect_d6(
                    (
                        i64::from(start.x) + i64::from(phase_d6.x),
                        i64::from(start.y) + i64::from(phase_d6.y),
                    ),
                    (
                        i64::from(end.x) + i64::from(phase_d6.x),
                        i64::from(end.y) + i64::from(phase_d6.y),
                    ),
                    (left, top, right, bottom),
                )
            })
    })
}

fn segment_intersects_rect_d6(
    start: (i64, i64),
    end: (i64, i64),
    rect: (i64, i64, i64, i64),
) -> bool {
    let (left, top, right, bottom) = rect;
    let in_rect = |(x, y): (i64, i64)| (left..=right).contains(&x) && (top..=bottom).contains(&y);
    if in_rect(start) || in_rect(end) {
        return true;
    }
    let min_x = start.0.min(end.0);
    let max_x = start.0.max(end.0);
    let min_y = start.1.min(end.1);
    let max_y = start.1.max(end.1);
    if max_x < left || min_x > right || max_y < top || min_y > bottom {
        return false;
    }
    let corners = [(left, top), (right, top), (right, bottom), (left, bottom)];
    corners
        .iter()
        .copied()
        .zip(corners.iter().copied().cycle().skip(1))
        .take(corners.len())
        .any(|(rect_start, rect_end)| segments_intersect_d6(start, end, rect_start, rect_end))
}

fn segments_intersect_d6(
    left_start: (i64, i64),
    left_end: (i64, i64),
    right_start: (i64, i64),
    right_end: (i64, i64),
) -> bool {
    let orientation = |a: (i64, i64), b: (i64, i64), c: (i64, i64)| {
        i128::from(b.0 - a.0) * i128::from(c.1 - a.1)
            - i128::from(b.1 - a.1) * i128::from(c.0 - a.0)
    };
    let on_segment = |a: (i64, i64), b: (i64, i64), point: (i64, i64)| {
        (a.0.min(b.0)..=a.0.max(b.0)).contains(&point.0)
            && (a.1.min(b.1)..=a.1.max(b.1)).contains(&point.1)
    };
    let o1 = orientation(left_start, left_end, right_start);
    let o2 = orientation(left_start, left_end, right_end);
    let o3 = orientation(right_start, right_end, left_start);
    let o4 = orientation(right_start, right_end, left_end);
    ((o1 > 0) != (o2 > 0) && (o3 > 0) != (o4 > 0))
        || (o1 == 0 && on_segment(left_start, left_end, right_start))
        || (o2 == 0 && on_segment(left_start, left_end, right_end))
        || (o3 == 0 && on_segment(right_start, right_end, left_start))
        || (o4 == 0 && on_segment(right_start, right_end, left_end))
}

// Cap eager drawing bitmaps at libass's 128 MiB cache budget to avoid multi-gigabyte overcommit.
const MAX_DRAWING_BITMAP_BYTES: usize = 128 * 1024 * 1024;

fn checked_drawing_bitmap_dimensions(bounds: Rect) -> Option<(i32, i32, usize)> {
    let width = i64::from(bounds.x_max).checked_sub(i64::from(bounds.x_min))?;
    let height = i64::from(bounds.y_max).checked_sub(i64::from(bounds.y_min))?;
    if width <= 0 || height <= 0 {
        return None;
    }
    let width = i32::try_from(width).ok()?;
    let height = i32::try_from(height).ok()?;
    let len = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?;
    (len <= MAX_DRAWING_BITMAP_BYTES).then_some((width, height, len))
}

pub(crate) fn plane_to_raster_glyph(plane: &ImagePlane) -> Option<RasterGlyph> {
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(plane.bitmap.len()).ok()?;
    bitmap.extend_from_slice(&plane.bitmap);
    Some(RasterGlyph {
        width: plane.size.width,
        height: plane.size.height,
        stride: plane.stride,
        left: plane.destination.x,
        top: plane.destination.y.saturating_add(plane.size.height),
        bitmap,
        ..RasterGlyph::default()
    })
}
