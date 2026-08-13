use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct BitmapBlur {
    /// libass's logarithmically quantized Gaussian radius, not pixels.
    pub(crate) qblur_x: u32,
    pub(crate) qblur_y: u32,
    pub(crate) be: u32,
}

impl BitmapBlur {
    pub(crate) fn from_scaled_blur(blur_x: f64, blur_y: f64, be: u32) -> Self {
        Self {
            qblur_x: libass_quantize_blur(blur_x),
            qblur_y: libass_quantize_blur(blur_y),
            be,
        }
    }

    pub(crate) fn is_zero(self) -> bool {
        self.qblur_x == 0 && self.qblur_y == 0 && self.be == 0
    }
}

pub(crate) fn blur_image_plane_xy(plane: ImagePlane, blur: BitmapBlur) -> ImagePlane {
    if blur.is_zero() || plane.size.width <= 0 || plane.size.height <= 0 || plane.bitmap.is_empty()
    {
        return plane;
    }
    let (bitmap, width, height, pad_x, pad_y) = blur_bitmap_xy(
        plane.bitmap,
        plane.size.width as usize,
        plane.size.height as usize,
        blur,
    );
    ImagePlane {
        size: Size {
            width: width as i32,
            height: height as i32,
        },
        stride: width as i32,
        destination: Point {
            x: plane.destination.x - pad_x as i32,
            y: plane.destination.y - pad_y as i32,
        },
        bitmap,
        ..plane
    }
}

pub(crate) fn blur_bitmap_xy(
    mut source: Vec<u8>,
    mut width: usize,
    mut height: usize,
    blur: BitmapBlur,
) -> (Vec<u8>, usize, usize, usize, usize) {
    if blur.is_zero() || width == 0 || height == 0 || source.is_empty() {
        return (source, width, height, 0, 0);
    }

    // libass reserves a small zero border before applying \be.  The box
    // filter itself keeps the bitmap dimensions unchanged; this padding is
    // enough for every supported number of passes (MAX_BE = 127).
    let be = blur.be.min(127);
    let be_pad = ass_be_padding(be);
    if be_pad > 0 {
        let Some(padded_width) = width.checked_add(2 * be_pad) else {
            return (source, width, height, 0, 0);
        };
        let Some(padded_height) = height.checked_add(2 * be_pad) else {
            return (source, width, height, 0, 0);
        };
        let Some(padded_len) = padded_width.checked_mul(padded_height) else {
            return (source, width, height, 0, 0);
        };
        let Some(mut padded) = zeroed_blur_bitmap(padded_len) else {
            return (source, width, height, 0, 0);
        };
        for row in 0..height {
            let src_start = row * width;
            let dst_start = (row + be_pad) * padded_width + be_pad;
            padded[dst_start..dst_start + width]
                .copy_from_slice(&source[src_start..src_start + width]);
        }
        source = padded;
        width = padded_width;
        height = padded_height;
    }

    let mut pad_x = be_pad;
    let mut pad_y = be_pad;
    if blur.qblur_x > 0 || blur.qblur_y > 0 {
        let r2x = libass_blur_r2_from_qblur(blur.qblur_x);
        let r2y = libass_blur_r2_from_qblur(blur.qblur_y);
        let (bitmap, blurred_width, blurred_height, gaussian_pad_x, gaussian_pad_y) =
            libass_gaussian_blur(&source, width, height, r2x, r2y);
        source = bitmap;
        width = blurred_width;
        height = blurred_height;
        pad_x = pad_x.saturating_add(gaussian_pad_x);
        pad_y = pad_y.saturating_add(gaussian_pad_y);
    }

    if be > 0 && width > 1 && height > 1 {
        apply_ass_be_blur(&mut source, width, height, be);
    }
    (source, width, height, pad_x, pad_y)
}

const MAX_BLUR_BITMAP_BYTES: usize = 128 * 1024 * 1024;

fn zeroed_blur_bitmap(len: usize) -> Option<Vec<u8>> {
    if len > MAX_BLUR_BITMAP_BYTES {
        return None;
    }
    let mut bitmap = Vec::new();
    bitmap.try_reserve_exact(len).ok()?;
    bitmap.resize(len, 0);
    Some(bitmap)
}

fn ass_be_padding(be: u32) -> usize {
    match be {
        0 => 0,
        1..=3 => be as usize,
        4..=7 => 4,
        _ => 5,
    }
}

fn apply_ass_be_blur(bitmap: &mut [u8], width: usize, height: usize, be: u32) {
    let passes = be.min(127);
    if passes > 1 {
        for value in bitmap.iter_mut() {
            *value = ((*value >> 1) + 1) >> 1;
        }
        for _ in 1..passes {
            ass_be_blur_pass(bitmap, width, height);
        }
        for value in bitmap.iter_mut() {
            let expanded = (u16::from(*value) << 2) - u16::from(*value > 32);
            *value = expanded.min(u16::from(u8::MAX)) as u8;
        }
    }
    ass_be_blur_pass(bitmap, width, height);
}

fn ass_be_blur_pass(bitmap: &mut [u8], width: usize, height: usize) {
    let source = bitmap.to_vec();
    const WEIGHTS: [u32; 3] = [1, 2, 1];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0_u32;
            for (kernel_y, weight_y) in WEIGHTS.iter().copied().enumerate() {
                let sample_y = y as isize + kernel_y as isize - 1;
                if !(0..height as isize).contains(&sample_y) {
                    continue;
                }
                for (kernel_x, weight_x) in WEIGHTS.iter().copied().enumerate() {
                    let sample_x = x as isize + kernel_x as isize - 1;
                    if (0..width as isize).contains(&sample_x) {
                        sum += u32::from(source[sample_y as usize * width + sample_x as usize])
                            * weight_x
                            * weight_y;
                    }
                }
            }
            bitmap[y * width + x] = (sum >> 4) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_blur_uses_libass_logarithmic_buckets() {
        let qblur_2 = libass_quantize_blur(2.0);
        assert_eq!(qblur_2, 13);
        assert_eq!(libass_quantize_blur(2.01), qblur_2);
        assert_eq!(libass_quantize_blur(2.04), qblur_2);
        assert_eq!(libass_quantize_blur(2.05), 14);

        let r2 = libass_blur_r2_from_qblur(qblur_2);
        assert!((r2 - 2.778_779_413_952_41).abs() < 1e-12, "r2={r2}");
    }

    #[test]
    fn nearby_blur_values_in_one_bucket_produce_identical_bitmaps() {
        let mut source = vec![0_u8; 9 * 9];
        source[4 * 9 + 4] = 255;
        let blur_2 = BitmapBlur::from_scaled_blur(2.0, 2.0, 0);
        let blur_201 = BitmapBlur::from_scaled_blur(2.01, 2.01, 0);
        assert_eq!(blur_2, blur_201);
        assert_eq!(
            blur_bitmap_xy(source.clone(), 9, 9, blur_2),
            blur_bitmap_xy(source, 9, 9, blur_201)
        );
    }

    #[test]
    fn hostile_blur_quantization_cannot_overflow_filter_geometry() {
        let source = vec![255_u8];
        let result = blur_bitmap_xy(
            source.clone(),
            1,
            1,
            BitmapBlur {
                qblur_x: u32::MAX,
                qblur_y: u32::MAX,
                be: 0,
            },
        );
        assert_eq!(result, (source, 1, 1, 0, 0));
        assert_eq!(
            libass_gaussian_blur(&[255], 1, 1, f64::MAX, f64::MAX),
            (vec![255], 1, 1, 0, 0)
        );
    }

    #[test]
    fn anisotropic_gaussian_blur_expands_axes_independently() {
        let source = vec![255_u8; 9];
        let (horizontal, horizontal_width, horizontal_height, pad_x, pad_y) = blur_bitmap_xy(
            source.clone(),
            3,
            3,
            BitmapBlur {
                qblur_x: libass_quantize_blur(8.0),
                qblur_y: libass_quantize_blur(1.0),
                be: 0,
            },
        );
        assert_eq!(horizontal.len(), horizontal_width * horizontal_height);
        assert!(pad_x > pad_y);
        assert!(horizontal_width - 3 > horizontal_height - 3);

        let (vertical, vertical_width, vertical_height, vertical_pad_x, vertical_pad_y) =
            blur_bitmap_xy(
                source,
                3,
                3,
                BitmapBlur {
                    qblur_x: libass_quantize_blur(1.0),
                    qblur_y: libass_quantize_blur(8.0),
                    be: 0,
                },
            );
        assert_eq!(vertical.len(), vertical_width * vertical_height);
        assert!(vertical_pad_y > vertical_pad_x);
        assert!(vertical_height - 3 > vertical_width - 3);
    }

    #[test]
    fn edge_blur_uses_unscaled_be_pass_count_and_padding() {
        let mut source = vec![0_u8; 25];
        source[12] = 255;
        let (bitmap, width, height, pad_x, pad_y) = blur_bitmap_xy(
            source,
            5,
            5,
            BitmapBlur {
                qblur_x: 0,
                qblur_y: 0,
                be: 1,
            },
        );
        assert_eq!((width, height, pad_x, pad_y), (7, 7, 1, 1));
        assert!(bitmap[3 * width + 2] > 0);
        assert!(bitmap[3 * width + 3] > bitmap[3 * width + 2]);

        let mut solid = vec![255_u8; 9 * 9];
        apply_ass_be_blur(&mut solid, 9, 9, 2);
        assert_eq!(
            solid[4 * 9 + 4],
            255,
            r"\be pre/post scaling keeps solid alpha"
        );
    }
}

#[derive(Clone)]
pub(crate) struct LibassBlurMethod {
    pub(crate) level: usize,
    pub(crate) radius: usize,
    pub(crate) coeff: [i16; 8],
}

/// Quantize an authored `\blur` value after applying libass's per-axis
/// renderer scale.  libass stores this logarithmic index in the bitmap-cache
/// key, so nearby animated values deliberately share one filter kernel.
pub(crate) fn libass_quantize_blur(blur: f64) -> u32 {
    const POSITION_PRECISION: f64 = 8.0;
    const BLUR_PRECISION: f64 = 1.0 / 256.0;
    if !(blur.is_finite() && blur > 0.0) {
        return 0;
    }
    let blur_radius_scale = 2.0 / 256.0_f64.ln().sqrt();
    let scale = 64.0 * BLUR_PRECISION / POSITION_PRECISION;
    let qblur = (blur * blur_radius_scale * scale).ln_1p() / BLUR_PRECISION;
    if !qblur.is_finite() {
        return u32::MAX;
    }
    qblur.round_ties_even().clamp(0.0, f64::from(u32::MAX)) as u32
}

/// Restore libass's squared Gaussian sigma from its cached quantization
/// index.  Keeping the index intact avoids the old quarter-pixel ceiling,
/// which made `\blur2.01` jump to the kernel for `\blur2.25`.
pub(crate) fn libass_blur_r2_from_qblur(qblur: u32) -> f64 {
    const POSITION_PRECISION: f64 = 8.0;
    const BLUR_PRECISION: f64 = 1.0 / 256.0;
    const SCALE: f64 = 64.0 * BLUR_PRECISION / POSITION_PRECISION;
    let sigma = (BLUR_PRECISION * f64::from(qblur)).exp_m1() / SCALE;
    sigma * sigma
}

pub(crate) fn libass_gaussian_blur(
    source: &[u8],
    width: usize,
    height: usize,
    r2x: f64,
    r2y: f64,
) -> (Vec<u8>, usize, usize, usize, usize) {
    let max_sigma = MAX_BLUR_BITMAP_BYTES as f64;
    if !r2x.is_finite()
        || !r2y.is_finite()
        || r2x < 0.0
        || r2y < 0.0
        || r2x.sqrt() > max_sigma
        || r2y.sqrt() > max_sigma
    {
        return (source.to_vec(), width, height, 0, 0);
    }
    let blur_x = find_libass_blur_method(r2x);
    let blur_y = if (r2y - r2x).abs() < f64::EPSILON {
        blur_x.clone()
    } else {
        find_libass_blur_method(r2y)
    };

    let Some(factor_x) = u32::try_from(blur_x.level)
        .ok()
        .and_then(|level| 1_usize.checked_shl(level))
    else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let Some(factor_y) = u32::try_from(blur_y.level)
        .ok()
        .and_then(|level| 1_usize.checked_shl(level))
    else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let Some(offset_x) = (2 * blur_x.radius + 9)
        .checked_mul(factor_x)
        .and_then(|value| value.checked_sub(5))
    else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let Some(offset_y) = (2 * blur_y.radius + 9)
        .checked_mul(factor_y)
        .and_then(|value| value.checked_sub(5))
    else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let mask_x = factor_x - 1;
    let mask_y = factor_y - 1;
    let Some(padded_width) = width.checked_add(offset_x) else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let Some(padded_height) = height.checked_add(offset_y) else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let end_width = (padded_width & !mask_x).saturating_sub(4);
    let end_height = (padded_height & !mask_y).saturating_sub(4);
    let Some(pad_x) = (blur_x.radius + 4)
        .checked_mul(factor_x)
        .and_then(|value| value.checked_sub(4))
    else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let Some(pad_y) = (blur_y.radius + 4)
        .checked_mul(factor_y)
        .and_then(|value| value.checked_sub(4))
    else {
        return (source.to_vec(), width, height, 0, 0);
    };
    let safe_bitmap = |w: usize, h: usize| {
        w.checked_mul(h)
            .and_then(|len| len.checked_mul(std::mem::size_of::<i16>()))
            .is_some_and(|bytes| bytes <= MAX_BLUR_BITMAP_BYTES)
    };
    if !safe_bitmap(width, height) || !safe_bitmap(end_width, end_height) {
        return (source.to_vec(), width, height, 0, 0);
    }

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
