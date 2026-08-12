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
    pub(crate) scale_x: f64,
    pub(crate) scale_y: f64,
    pub(crate) render_scale: RenderScale,
    pub(crate) baseline_offset: f64,
}

pub(crate) fn image_plane_from_drawing(
    drawing: &ParsedDrawing,
    params: DrawingPlaneParams,
) -> Option<ImagePlane> {
    let polygons = scaled_drawing_polygons(
        drawing,
        params.scale_x,
        params.scale_y,
        params.render_scale.x,
        params.render_scale.y,
    )?;
    let bounds = drawing_bounds(&polygons)?;
    let (width, height, bitmap_len) = checked_drawing_bitmap_dimensions(bounds)?;

    let stride = usize::try_from(width).ok()?;
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(bitmap_len).ok()?;
    bitmap.resize(bitmap_len, 0_u8);
    let mut any_visible = false;

    for row in 0..usize::try_from(height).ok()? {
        for column in 0..stride {
            let x = bounds.x_min.checked_add(i32::try_from(column).ok()?)?;
            let y = bounds.y_min.checked_add(i32::try_from(row).ok()?)?;
            let coverage = drawing_pixel_coverage(x, y, &polygons);
            if coverage > 0 {
                bitmap[row * stride + column] = coverage;
                any_visible = true;
            }
        }
    }

    // \pbo is a signed baseline offset: positive moves the drawing down
    // (libass: desc = pbo, applied with the drawing's scale).
    let vertical_offset =
        libass_outline_coordinate_from_f64(params.baseline_offset * params.render_scale.y)?;

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
    scale_x: f64,
    scale_y: f64,
    render_scale_x: f64,
    render_scale_y: f64,
) -> Option<Vec<Vec<Point>>> {
    let scale_x = style_scale(scale_x) * render_scale_x;
    let scale_y = style_scale(scale_y) * render_scale_y;
    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x <= 0.0 || scale_y <= 0.0 {
        return None;
    }
    let mut polygons = Vec::new();
    polygons.try_reserve_exact(drawing.polygons.len()).ok()?;
    for polygon in &drawing.polygons {
        let mut scaled = Vec::new();
        scaled.try_reserve_exact(polygon.len()).ok()?;
        for point in polygon {
            let point = Point {
                x: libass_outline_coordinate_from_f64(f64::from(point.x) * scale_x)?,
                y: libass_outline_coordinate_from_f64(f64::from(point.y) * scale_y)?,
            };
            if !libass_outline_point_is_valid(point) {
                return None;
            }
            scaled.push(point);
        }
        polygons.push(scaled);
    }
    Some(polygons)
}

pub(crate) fn drawing_bounds(polygons: &[Vec<Point>]) -> Option<Rect> {
    let mut points = polygons.iter().flat_map(|polygon| polygon.iter().copied());
    let first = points.next()?;
    if !libass_outline_point_is_valid(first) {
        return None;
    }
    let mut x_min = first.x;
    let mut y_min = first.y;
    let mut x_max = first.x;
    let mut y_max = first.y;
    for point in points {
        if !libass_outline_point_is_valid(point) {
            return None;
        }
        x_min = x_min.min(point.x);
        y_min = y_min.min(point.y);
        x_max = x_max.max(point.x);
        y_max = y_max.max(point.y);
    }
    Some(Rect {
        x_min,
        y_min,
        x_max: x_max.checked_add(1)?,
        y_max: y_max.checked_add(1)?,
    })
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
