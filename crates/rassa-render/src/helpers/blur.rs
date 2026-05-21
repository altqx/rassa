use super::*;

pub(crate) fn blur_image_plane(plane: ImagePlane, radius: u32) -> ImagePlane {
    if radius == 0 || plane.size.width <= 0 || plane.size.height <= 0 || plane.bitmap.is_empty() {
        return plane;
    }
    let (bitmap, width, height, pad) = blur_bitmap(
        plane.bitmap,
        plane.size.width as usize,
        plane.size.height as usize,
        radius,
    );
    ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        destination: Point {
            x: plane.destination.x - pad as i32,
            y: plane.destination.y - pad as i32,
        },
        bitmap,
        ..plane
    }
}

pub(crate) fn blur_bitmap(
    source: Vec<u8>,
    width: usize,
    height: usize,
    radius: u32,
) -> (Vec<u8>, usize, usize, usize) {
    if radius == 0 || width == 0 || height == 0 || source.is_empty() {
        return (source, width, height, 0);
    }
    let r2 = libass_blur_r2_from_radius(radius);
    let (bitmap, width, height, pad_x, pad_y) =
        libass_gaussian_blur(&source, width, height, r2, r2);
    debug_assert_eq!(pad_x, pad_y);
    (bitmap, width, height, pad_x)
}

#[derive(Clone)]
pub(crate) struct LibassBlurMethod {
    pub(crate) level: usize,
    pub(crate) radius: usize,
    pub(crate) coeff: [i16; 8],
}

pub(crate) fn libass_blur_r2_from_radius(radius: u32) -> f64 {
    const POSITION_PRECISION: f64 = 8.0;
    const BLUR_PRECISION: f64 = 1.0 / 256.0;
    let blur = f64::from(radius) / 4.0;
    let blur_radius_scale = 2.0 / 256.0_f64.ln().sqrt();
    let scale = 64.0 * BLUR_PRECISION / POSITION_PRECISION;
    let qblur = ((1.0 + blur * blur_radius_scale * scale).ln() / BLUR_PRECISION).round();
    let sigma = (BLUR_PRECISION * qblur).exp_m1() / scale;
    sigma * sigma
}

pub(crate) fn libass_gaussian_blur(
    source: &[u8],
    width: usize,
    height: usize,
    r2x: f64,
    r2y: f64,
) -> (Vec<u8>, usize, usize, usize, usize) {
    let blur_x = find_libass_blur_method(r2x);
    let blur_y = if (r2y - r2x).abs() < f64::EPSILON {
        blur_x.clone()
    } else {
        find_libass_blur_method(r2y)
    };

    let offset_x = ((2 * blur_x.radius + 9) << blur_x.level) - 5;
    let offset_y = ((2 * blur_y.radius + 9) << blur_y.level) - 5;
    let mask_x = (1_usize << blur_x.level) - 1;
    let mask_y = (1_usize << blur_y.level) - 1;
    let end_width = ((width + offset_x) & !mask_x).saturating_sub(4);
    let end_height = ((height + offset_y) & !mask_y).saturating_sub(4);
    let pad_x = ((blur_x.radius + 4) << blur_x.level) - 4;
    let pad_y = ((blur_y.radius + 4) << blur_y.level) - 4;

    let mut buffer = unpack_libass_blur(source);
    let mut w = width;
    let mut h = height;

    for _ in 0..blur_y.level {
        let next = shrink_vert_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }
    for _ in 0..blur_x.level {
        let next = shrink_horz_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }

    let next = blur_horz_libass(&buffer, w, h, &blur_x.coeff, blur_x.radius);
    buffer = next.0;
    w = next.1;
    h = next.2;
    let next = blur_vert_libass(&buffer, w, h, &blur_y.coeff, blur_y.radius);
    buffer = next.0;
    w = next.1;
    h = next.2;

    for _ in 0..blur_x.level {
        let next = expand_horz_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }
    for _ in 0..blur_y.level {
        let next = expand_vert_libass(&buffer, w, h);
        buffer = next.0;
        w = next.1;
        h = next.2;
    }

    debug_assert_eq!(w, end_width);
    debug_assert_eq!(h, end_height);
    (pack_libass_blur(&buffer, w, h), w, h, pad_x, pad_y)
}

pub(crate) fn find_libass_blur_method(r2: f64) -> LibassBlurMethod {
    let mut mu = [0.0_f64; 8];
    let (level, radius) = if r2 < 0.5 {
        mu[1] = 0.085 * r2 * r2 * r2;
        mu[0] = 0.5 * r2 - 4.0 * mu[1];
        (0_usize, 4_usize)
    } else {
        let (frac, level) = frexp((0.11569 * r2 + 0.20591047).sqrt());
        let mul = 0.25_f64.powi(level);
        let radius = (8_i32 - ((10.1525 + 0.8335 * mul) * (1.0 - frac)) as i32).max(4) as usize;
        calc_libass_coeff(&mut mu, radius, r2, mul);
        (level.max(0) as usize, radius)
    };
    let mut coeff = [0_i16; 8];
    for i in 0..radius {
        coeff[i] = (65536.0 * mu[i] + 0.5) as i16;
    }
    LibassBlurMethod {
        level,
        radius,
        coeff,
    }
}

pub(crate) fn calc_libass_coeff(mu: &mut [f64; 8], n: usize, r2: f64, mul: f64) {
    let w = 12096.0;
    let kernel = [
        (((3280.0 / w) * mul + 1092.0 / w) * mul + 2520.0 / w) * mul + 5204.0 / w,
        (((-2460.0 / w) * mul - 273.0 / w) * mul - 210.0 / w) * mul + 2943.0 / w,
        (((984.0 / w) * mul - 546.0 / w) * mul - 924.0 / w) * mul + 486.0 / w,
        (((-164.0 / w) * mul + 273.0 / w) * mul - 126.0 / w) * mul + 17.0 / w,
    ];
    let mut mat_freq = [0.0_f64; 17];
    mat_freq[..4].copy_from_slice(&kernel);
    coeff_filter_libass(&mut mat_freq, 7, &kernel);
    let mut vec_freq = [0.0_f64; 12];
    calc_gauss_libass(&mut vec_freq, n + 4, r2 * mul);
    coeff_filter_libass(&mut vec_freq, n + 1, &kernel);
    let mut mat = [[0.0_f64; 8]; 8];
    calc_matrix_libass(&mut mat, &mat_freq, n);
    let mut vec = [0.0_f64; 8];
    for i in 0..n {
        vec[i] = mat_freq[0] - mat_freq[i + 1] - vec_freq[0] + vec_freq[i + 1];
    }
    for i in 0..n {
        let mut res = 0.0;
        for (j, value) in vec.iter().enumerate().take(n) {
            res += mat[i][j] * value;
        }
        mu[i] = res.max(0.0);
    }
}

pub(crate) fn calc_gauss_libass(res: &mut [f64], n: usize, r2: f64) {
    let alpha = 0.5 / r2;
    let mut mul = (-alpha).exp();
    let mul2 = mul * mul;
    let mut cur = (alpha / std::f64::consts::PI).sqrt();
    res[0] = cur;
    cur *= mul;
    res[1] = cur;
    for value in res.iter_mut().take(n).skip(2) {
        mul *= mul2;
        cur *= mul;
        *value = cur;
    }
}

pub(crate) fn coeff_filter_libass(coeff: &mut [f64], n: usize, kernel: &[f64; 4]) {
    let mut prev1 = coeff[1];
    let mut prev2 = coeff[2];
    let mut prev3 = coeff[3];
    for i in 0..n {
        let res = coeff[i] * kernel[0]
            + (prev1 + coeff[i + 1]) * kernel[1]
            + (prev2 + coeff[i + 2]) * kernel[2]
            + (prev3 + coeff[i + 3]) * kernel[3];
        prev3 = prev2;
        prev2 = prev1;
        prev1 = coeff[i];
        coeff[i] = res;
    }
}

pub(crate) fn calc_matrix_libass(mat: &mut [[f64; 8]; 8], mat_freq: &[f64], n: usize) {
    for i in 0..n {
        mat[i][i] = mat_freq[2 * i + 2] + 3.0 * mat_freq[0] - 4.0 * mat_freq[i + 1];
        for j in i + 1..n {
            let v = mat_freq[i + j + 2]
                + mat_freq[j - i]
                + 2.0 * (mat_freq[0] - mat_freq[i + 1] - mat_freq[j + 1]);
            mat[i][j] = v;
            mat[j][i] = v;
        }
    }
    for k in 0..n {
        let z = 1.0 / mat[k][k];
        mat[k][k] = 1.0;
        let pivot_row = mat[k];
        for (i, row) in mat.iter_mut().enumerate().take(n) {
            if i == k {
                continue;
            }
            let mul = row[k] * z;
            row[k] = 0.0;
            for j in 0..n {
                row[j] -= pivot_row[j] * mul;
            }
        }
        for value in mat[k].iter_mut().take(n) {
            *value *= z;
        }
    }
}

pub(crate) fn frexp(value: f64) -> (f64, i32) {
    if value == 0.0 {
        return (0.0, 0);
    }
    let exponent = value.abs().log2().floor() as i32 + 1;
    (value / 2.0_f64.powi(exponent), exponent)
}

#[inline]
pub(crate) fn get_libass_sample(
    source: &[i16],
    width: usize,
    height: usize,
    x: isize,
    y: isize,
) -> i16 {
    if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
        0
    } else {
        source[y as usize * width + x as usize]
    }
}

pub(crate) fn unpack_libass_blur(source: &[u8]) -> Vec<i16> {
    source
        .iter()
        .map(|value| {
            let value = u16::from(*value);
            ((((value << 7) | (value >> 1)) + 1) >> 1) as i16
        })
        .collect()
}

const LIBASS_DITHER_LINE: [i16; 32] = [
    8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 8, 40, 56, 24, 56, 24, 56, 24, 56, 24, 56, 24,
    56, 24, 56, 24, 56, 24,
];

pub(crate) fn pack_libass_blur(source: &[i16], width: usize, height: usize) -> Vec<u8> {
    let mut bitmap = vec![0_u8; width * height];
    for y in 0..height {
        let dither = &LIBASS_DITHER_LINE[16 * (y & 1)..];
        for x in 0..width {
            let sample = i32::from(source[y * width + x]);
            let value = ((sample - (sample >> 8) + i32::from(dither[x & 15])) >> 6).clamp(0, 255);
            bitmap[y * width + x] = value as u8;
        }
    }
    bitmap
}

#[inline]
pub(crate) fn shrink_func_libass(
    p1p: i16,
    p1n: i16,
    z0p: i16,
    z0n: i16,
    n1p: i16,
    n1n: i16,
) -> i16 {
    let mut r = (i32::from(p1p) + i32::from(p1n) + i32::from(n1p) + i32::from(n1n)) >> 1;
    r = (r + i32::from(z0p) + i32::from(z0n)) >> 1;
    r = (r + i32::from(p1n) + i32::from(n1p)) >> 1;
    ((r + i32::from(z0p) + i32::from(z0n) + 2) >> 2) as i16
}

#[inline]
pub(crate) fn expand_func_libass(p1: i16, z0: i16, n1: i16) -> (i16, i16) {
    let r = ((((p1 as u16).wrapping_add(n1 as u16)) >> 1).wrapping_add(z0 as u16)) >> 1;
    let rp = (((r.wrapping_add(p1 as u16) >> 1)
        .wrapping_add(z0 as u16)
        .wrapping_add(1))
        >> 1) as i16;
    let rn = (((r.wrapping_add(n1 as u16) >> 1)
        .wrapping_add(z0 as u16)
        .wrapping_add(1))
        >> 1) as i16;
    (rp, rn)
}

pub(crate) fn shrink_horz_libass(
    source: &[i16],
    width: usize,
    height: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_width = (width + 5) >> 1;
    let mut dst = vec![0_i16; dst_width * height];
    for y in 0..height {
        for x in 0..dst_width {
            let sx = (2 * x) as isize;
            dst[y * dst_width + x] = shrink_func_libass(
                get_libass_sample(source, width, height, sx - 4, y as isize),
                get_libass_sample(source, width, height, sx - 3, y as isize),
                get_libass_sample(source, width, height, sx - 2, y as isize),
                get_libass_sample(source, width, height, sx - 1, y as isize),
                get_libass_sample(source, width, height, sx, y as isize),
                get_libass_sample(source, width, height, sx + 1, y as isize),
            );
        }
    }
    (dst, dst_width, height)
}

pub(crate) fn shrink_vert_libass(
    source: &[i16],
    width: usize,
    height: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_height = (height + 5) >> 1;
    let mut dst = vec![0_i16; width * dst_height];
    for y in 0..dst_height {
        let sy = (2 * y) as isize;
        for x in 0..width {
            dst[y * width + x] = shrink_func_libass(
                get_libass_sample(source, width, height, x as isize, sy - 4),
                get_libass_sample(source, width, height, x as isize, sy - 3),
                get_libass_sample(source, width, height, x as isize, sy - 2),
                get_libass_sample(source, width, height, x as isize, sy - 1),
                get_libass_sample(source, width, height, x as isize, sy),
                get_libass_sample(source, width, height, x as isize, sy + 1),
            );
        }
    }
    (dst, width, dst_height)
}

pub(crate) fn expand_horz_libass(
    source: &[i16],
    width: usize,
    height: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_width = 2 * width + 4;
    let mut dst = vec![0_i16; dst_width * height];
    for y in 0..height {
        for i in 0..(width + 2) {
            let sx = i as isize;
            let (rp, rn) = expand_func_libass(
                get_libass_sample(source, width, height, sx - 2, y as isize),
                get_libass_sample(source, width, height, sx - 1, y as isize),
                get_libass_sample(source, width, height, sx, y as isize),
            );
            let dx = 2 * i;
            dst[y * dst_width + dx] = rp;
            dst[y * dst_width + dx + 1] = rn;
        }
    }
    (dst, dst_width, height)
}

pub(crate) fn expand_vert_libass(
    source: &[i16],
    width: usize,
    height: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_height = 2 * height + 4;
    let mut dst = vec![0_i16; width * dst_height];
    for i in 0..(height + 2) {
        let sy = i as isize;
        for x in 0..width {
            let (rp, rn) = expand_func_libass(
                get_libass_sample(source, width, height, x as isize, sy - 2),
                get_libass_sample(source, width, height, x as isize, sy - 1),
                get_libass_sample(source, width, height, x as isize, sy),
            );
            let dy = 2 * i;
            dst[dy * width + x] = rp;
            dst[(dy + 1) * width + x] = rn;
        }
    }
    (dst, width, dst_height)
}

pub(crate) fn blur_horz_libass(
    source: &[i16],
    width: usize,
    height: usize,
    param: &[i16; 8],
    radius: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_width = width + 2 * radius;
    let mut dst = vec![0_i16; dst_width * height];
    for y in 0..height {
        for x in 0..dst_width {
            let center_x = x as isize - radius as isize;
            let center = i32::from(get_libass_sample(
                source, width, height, center_x, y as isize,
            ));
            let mut acc = 0x8000_i32;
            for i in (1..=radius).rev() {
                let coeff = i32::from(param[i - 1]);
                let left = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    center_x - i as isize,
                    y as isize,
                ));
                let right = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    center_x + i as isize,
                    y as isize,
                ));
                acc += ((left - center) as i16 as i32) * coeff;
                acc += ((right - center) as i16 as i32) * coeff;
            }
            dst[y * dst_width + x] = (center + (acc >> 16)) as i16;
        }
    }
    (dst, dst_width, height)
}

pub(crate) fn blur_vert_libass(
    source: &[i16],
    width: usize,
    height: usize,
    param: &[i16; 8],
    radius: usize,
) -> (Vec<i16>, usize, usize) {
    let dst_height = height + 2 * radius;
    let mut dst = vec![0_i16; width * dst_height];
    for y in 0..dst_height {
        let center_y = y as isize - radius as isize;
        for x in 0..width {
            let center = i32::from(get_libass_sample(
                source, width, height, x as isize, center_y,
            ));
            let mut acc = 0x8000_i32;
            for i in (1..=radius).rev() {
                let coeff = i32::from(param[i - 1]);
                let top = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    x as isize,
                    center_y - i as isize,
                ));
                let bottom = i32::from(get_libass_sample(
                    source,
                    width,
                    height,
                    x as isize,
                    center_y + i as isize,
                ));
                acc += ((top - center) as i16 as i32) * coeff;
                acc += ((bottom - center) as i16 as i32) * coeff;
            }
            dst[y * width + x] = (center + (acc >> 16)) as i16;
        }
    }
    (dst, width, dst_height)
}
