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
}

pub(crate) fn image_plane_from_drawing(
    polygons: &[Vec<Point>],
    params: DrawingPlaneParams,
) -> Option<ImagePlane> {
    let bounds = drawing_pixel_bounds_from_d6(polygons)?;
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
            let coverage = drawing_pixel_coverage_d6(x, y, polygons, sample_grid);
            if coverage > 0 {
                bitmap[row * stride + column] = coverage;
                any_visible = true;
            }
        }
    }

    // \pbo is a signed baseline offset: positive moves the drawing down
    // (libass: desc = pbo, applied with the drawing's scale).
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
        // Source is already 26.6; retain that precision through the style and
        // frame transform instead of rounding it to output pixels here.
        Some(polygons) => (polygons, 1.0 / f64::from(scale_base)),
        // Programmatically-created ParsedDrawing values only expose integer
        // polygons, so promote their coordinates into 26.6 first.
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

pub(crate) fn drawing_pixel_bounds_from_d6(polygons: &[Vec<Point>]) -> Option<Rect> {
    let mut points = polygons.iter().flat_map(|polygon| polygon.iter().copied());
    let first = points.next()?;
    if !fixed_d6_point_is_valid(first) {
        return None;
    }
    let mut x_min = first.x;
    let mut y_min = first.y;
    let mut x_max = first.x;
    let mut y_max = first.y;
    for point in points {
        if !fixed_d6_point_is_valid(point) {
            return None;
        }
        x_min = x_min.min(point.x);
        y_min = y_min.min(point.y);
        x_max = x_max.max(point.x);
        y_max = y_max.max(point.y);
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
    let mut ys = polygons.iter().flatten().map(|point| point.y);
    let first = ys.next()?;
    let (mut y_min, mut y_max) = (first, first);
    for y in ys {
        y_min = y_min.min(y);
        y_max = y_max.max(y);
    }
    fixed_d6_coordinate_from_f64(f64::from(y_max.checked_sub(y_min)?) / 64.0)
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
    // A low net area relative to the outline box identifies thin compound
    // paths (typically an outer contour plus an opposing inner contour).  Use
    // denser sampling only there; ordinary filled drawings keep the 4x4 path.
    let thin_area_ceiling = ((bounds_area as u128) * 2 - 1) / 5;
    if signed_double_area.unsigned_abs() <= thin_area_ceiling {
        16
    } else {
        4
    }
}

fn drawing_d6_bounds(polygons: &[Vec<Point>]) -> Option<Rect> {
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

fn drawing_pixel_coverage_d6(x: i32, y: i32, polygons: &[Vec<Point>], sample_grid: u32) -> u8 {
    let mut inside = 0_u32;
    for row in 0..sample_grid {
        let sample_y = (f64::from(y) + (f64::from(row) + 0.5) / f64::from(sample_grid)) * 64.0;
        for column in 0..sample_grid {
            let sample_x =
                (f64::from(x) + (f64::from(column) + 0.5) / f64::from(sample_grid)) * 64.0;
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

// libass's default bitmap-cache budget is 128 MiB.  Rassa rasterizes vector
// drawings eagerly, so use the same value as a hard per-drawing ceiling to
// avoid a successful multi-gigabyte overcommit followed by process abort while
// zero-filling the allocation.
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
