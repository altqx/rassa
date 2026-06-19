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

#[derive(Clone, Copy, Debug)]
pub(crate) struct RunTransformContext<'a> {
    pub(crate) transform: EventTransform,
    pub(crate) event: &'a LayoutEvent,
    pub(crate) effective_position: Option<(i32, i32)>,
    pub(crate) render_scale: RenderScale,
    pub(crate) mapping: &'a EventMapping,
    /// Screen-space y of the run's ascender line (baseline - ascender).
    /// libass calc_transform_matrix anchors the \fax/\fay shear at the
    /// glyph cell top (outline y = -asc), not the rendered ink bbox top.
    pub(crate) shear_pivot_y: Option<f64>,
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
        context.mapping,
    );
    let bounds_base = planes_bounds(&recent_planes)
        .map(|bounds| (f64::from(bounds.x_min), f64::from(bounds.y_min)))
        .unwrap_or(origin);
    // libass shears the outline about the glyph cell top (outline y = -asc),
    // i.e. the run's ascender line in screen space; only the ink bbox top is
    // used as a fallback when the ascender line is unavailable.
    let shear_base = (
        bounds_base.0,
        context.shear_pivot_y.unwrap_or(bounds_base.1),
    );
    let transform_slice = |planes: &mut Vec<ImagePlane>, start: usize| {
        let tail = planes.split_off(start);
        planes.extend(transform_event_planes(
            tail,
            context.transform,
            origin,
            shear_base,
            context.render_scale.y,
        ));
    };
    transform_slice(shadow_planes, starts.shadow);
    transform_slice(outline_planes, starts.outline);
    transform_slice(character_planes, starts.character);
}

/// Rotation/shear origin per libass calculate_rotation_params: \org if
/// given, otherwise the event position; with neither, the text bbox center.
pub(crate) fn event_transform_origin(
    event: &LayoutEvent,
    planes: &[ImagePlane],
    effective_position: Option<(i32, i32)>,
    mapping: &EventMapping,
) -> (f64, f64) {
    if let Some((x, y)) = event.origin_exact {
        return (mapping.map_x_pos(x).round(), mapping.map_y_pos(y).round());
    }
    if let Some((x, y)) = event.origin {
        return (
            mapping.map_x_pos(f64::from(x)).round(),
            mapping.map_y_pos(f64::from(y)).round(),
        );
    }
    if let Some((x, y)) = effective_position {
        return (f64::from(x), f64::from(y));
    }
    planes_bounds(planes)
        .map(|bounds| {
            // libass calculate_rotation_params + get_base_point (ass_render.c):
            // with neither \org nor an explicit position, rotation/shear pivots
            // about the alignment base point of the bounding box, not its
            // geometric center (only \an4/5/6 + \an2/5/8 land on the center).
            let x = match event.alignment & 0x3 {
                ass::HALIGN_LEFT => f64::from(bounds.x_min),
                ass::HALIGN_RIGHT => f64::from(bounds.x_max),
                _ => f64::from(bounds.x_min + bounds.x_max) / 2.0,
            };
            let y = match event.alignment & (ass::VALIGN_TOP | ass::VALIGN_CENTER) {
                ass::VALIGN_TOP => f64::from(bounds.y_min),
                ass::VALIGN_CENTER => f64::from(bounds.y_min + bounds.y_max) / 2.0,
                _ => f64::from(bounds.y_max),
            };
            (x, y)
        })
        .unwrap_or((0.0, 0.0))
}

pub(crate) fn transform_event_planes(
    planes: Vec<ImagePlane>,
    transform: EventTransform,
    origin: (f64, f64),
    shear_base: (f64, f64),
    render_scale_y: f64,
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
        .filter_map(|plane| transform_plane(plane, matrix))
        .collect()
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
    Some(ImagePlane {
        size: Size { width, height },
        stride: width,
        color: rgba_color_from_ass(color),
        destination: Point {
            x: bounds.x_min + offset.x,
            y: bounds.y_min + offset.y,
        },
        kind,
        bitmap: vec![255; (width * height) as usize],
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

        // libass calc_transform_matrix: dist = 20000 * blur_scale_y in 26.6
        // outline units; blur_scale_y is frame_height/PlayResY, i.e. our
        // render_scale_y, so in output pixels dist = 20000/64 * render_scale_y.
        let dist = 20_000.0 / 64.0 * render_scale_y.max(f64::EPSILON);

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

pub(crate) fn transform_plane(plane: ImagePlane, matrix: ProjectiveMatrix) -> Option<ImagePlane> {
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

    crop_transformed_plane_to_ink(ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        destination: Point { x: min_x, y: min_y },
        bitmap,
        ..plane
    })
}

pub(crate) fn crop_transformed_plane_to_ink(mut plane: ImagePlane) -> Option<ImagePlane> {
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
    if min_x == 0 && min_y == 0 && max_x == width && max_y == height {
        return Some(plane);
    }
    let new_width = max_x - min_x;
    let new_height = max_y - min_y;
    let mut cropped = vec![0_u8; new_width * new_height];
    for y in 0..new_height {
        let src_start = (min_y + y) * stride + min_x;
        let dst_start = y * new_width;
        cropped[dst_start..dst_start + new_width]
            .copy_from_slice(&plane.bitmap[src_start..src_start + new_width]);
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
