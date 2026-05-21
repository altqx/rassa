use super::*;

pub(crate) fn image_planes_from_absolute_glyphs(
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

pub(crate) fn drawing_baseline_ascender(style: &ParsedSpanStyle, _render_scale_y: f64) -> i32 {
    let scale_y = style_scale(style.scale_y);
    (style.font_size.max(1.0) * scale_y * 0.75).round() as i32
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
    pub(crate) pad_to_libass_geometry: bool,
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
            let coverage = drawing_pixel_coverage(x, y, &polygons);
            if coverage > 0 {
                bitmap[row * stride + column] = coverage;
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

pub(crate) fn pad_drawing_plane_to_libass_geometry(plane: ImagePlane) -> ImagePlane {
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

pub(crate) fn align_i32(value: i32, alignment: i32) -> i32 {
    if alignment <= 1 {
        return value;
    }
    ((value + alignment - 1) / alignment) * alignment
}

pub(crate) fn scaled_drawing_polygons(
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

pub(crate) fn drawing_bounds(polygons: &[Vec<Point>]) -> Option<Rect> {
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

pub(crate) fn plane_to_raster_glyph(plane: &ImagePlane) -> RasterGlyph {
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
