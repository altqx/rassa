use std::{
    env,
    ffi::{CString, c_char},
    hint::black_box,
    ptr, slice,
    time::{Duration, Instant},
};

use rassa::{AttachedFontProvider, FontAttachment, ImagePlane, Renderer, Script, Size, ass};

const WORKLOAD_NAME: &str = "broad-karaoke";
const SCRIPT: &str = include_str!("../../fixtures/libass/compare/broad/broad_karaoke.ass");
const FONT_1: &[u8] = include_bytes!("../../fixtures/libass/compare/broad/font1.ttf");
const FONT_2: &[u8] = include_bytes!("../../fixtures/libass/compare/broad/font2.otf");
const EQUIVALENCE_TIMES_MS: &[i64] = &[0, 125, 499, 500, 873, 1_200, 1_800, 2_500, 3_999];
const DYNAMIC_DURATION_MS: u64 = 3_999;
const CACHED_TIME_MS: i64 = 1_500;

#[derive(Clone, Copy, Debug)]
struct Args {
    iterations: u64,
    samples: usize,
    warmup: u64,
    verify_only: bool,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            iterations: 200,
            samples: 5,
            warmup: 20,
            verify_only: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlaneSignature {
    kind: i32,
    color: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    stride: i32,
    bitmap: Vec<u8>,
}

impl PlaneSignature {
    fn summary(&self) -> String {
        format!(
            "kind={} color={:08x} pos=({}, {}) size={}x{} stride={} bytes={}",
            self.kind,
            self.color,
            self.x,
            self.y,
            self.width,
            self.height,
            self.stride,
            self.bitmap.len()
        )
    }
}

struct RustHarness {
    script: Script,
    renderer: Renderer,
    provider: AttachedFontProvider,
}

impl RustHarness {
    fn new() -> Result<Self, String> {
        Ok(Self {
            script: Script::parse(SCRIPT).map_err(|error| error.to_string())?,
            renderer: Renderer::new(),
            provider: AttachedFontProvider::from_attachments(&font_attachments()),
        })
    }

    fn render(&self, now_ms: i64) -> Result<Vec<ImagePlane>, String> {
        self.renderer
            .render_frame_with_provider(&self.script, &self.provider, now_ms)
            .map(|frame| frame.planes)
            .map_err(|error| error.to_string())
    }

    fn timed_render(&self, now_ms: i64) -> Result<u64, String> {
        let planes = self.render(now_ms)?;
        let checksum = cheap_rust_checksum(&planes);
        black_box(&planes);
        Ok(checksum)
    }
}

struct CapiHarness {
    library: *mut rassa_capi::ASS_Library,
    renderer: *mut rassa_capi::ASS_Renderer,
    track: *mut rassa_capi::ASS_Track,
}

impl CapiHarness {
    fn new(frame: Size) -> Result<Self, String> {
        unsafe {
            let library = rassa_capi::ass_library_init();
            if library.is_null() {
                return Err("ass_library_init returned null".to_string());
            }

            for attachment in font_attachments() {
                let name = CString::new(attachment.name)
                    .map_err(|_| "font attachment name contains a NUL".to_string())?;
                let data_size = i32::try_from(attachment.data.len())
                    .map_err(|_| "font attachment is too large for the C API".to_string())?;
                rassa_capi::ass_add_font(
                    library,
                    name.as_ptr(),
                    attachment.data.as_ptr().cast::<c_char>(),
                    data_size,
                );
            }

            let renderer = rassa_capi::ass_renderer_init(library);
            if renderer.is_null() {
                rassa_capi::ass_library_done(library);
                return Err("ass_renderer_init returned null".to_string());
            }
            rassa_capi::ass_set_frame_size(renderer, frame.width, frame.height);
            rassa_capi::ass_set_fonts(
                renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as i32,
                ptr::null(),
                1,
            );

            let track = rassa_capi::ass_read_memory(
                library,
                SCRIPT.as_ptr().cast::<c_char>().cast_mut(),
                SCRIPT.len(),
                ptr::null(),
            );
            if track.is_null() {
                rassa_capi::ass_renderer_done(renderer);
                rassa_capi::ass_library_done(library);
                return Err("ass_read_memory returned null".to_string());
            }

            Ok(Self {
                library,
                renderer,
                track,
            })
        }
    }

    fn render(&mut self, now_ms: i64) -> Result<*mut rassa_capi::ASS_Image, String> {
        let images = unsafe {
            rassa_capi::ass_render_frame(self.renderer, self.track, now_ms, ptr::null_mut())
        };
        if images.is_null() {
            Err(format!("ass_render_frame returned null at {now_ms} ms"))
        } else {
            Ok(images)
        }
    }

    fn timed_render(&mut self, now_ms: i64) -> Result<u64, String> {
        let images = self.render(now_ms)?;
        let checksum = unsafe { cheap_capi_checksum(images) };
        black_box(images);
        Ok(checksum)
    }
}

impl Drop for CapiHarness {
    fn drop(&mut self) {
        unsafe {
            rassa_capi::ass_free_track(self.track);
            rassa_capi::ass_renderer_done(self.renderer);
            rassa_capi::ass_library_done(self.library);
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum BenchMode {
    Dynamic,
    Cached,
}

impl BenchMode {
    fn label(self) -> &'static str {
        match self {
            Self::Dynamic => "dynamic",
            Self::Cached => "cached",
        }
    }

    fn timestamp(self, iteration: u64) -> i64 {
        match self {
            Self::Dynamic => ((iteration.wrapping_mul(37)) % DYNAMIC_DURATION_MS) as i64,
            Self::Cached => CACHED_TIME_MS,
        }
    }
}

#[derive(Debug)]
struct BenchStats {
    median: Duration,
    min: Duration,
    max: Duration,
    checksum: u64,
}

fn font_attachments() -> Vec<FontAttachment> {
    vec![
        FontAttachment {
            name: "font1.ttf".to_string(),
            data: FONT_1.to_vec(),
        },
        FontAttachment {
            name: "font2.otf".to_string(),
            data: FONT_2.to_vec(),
        },
    ]
}

fn rust_signatures(planes: &[ImagePlane]) -> Vec<PlaneSignature> {
    planes
        .iter()
        .map(|plane| PlaneSignature {
            kind: plane.kind as i32,
            color: plane.color.0,
            x: plane.destination.x,
            y: plane.destination.y,
            width: plane.size.width,
            height: plane.size.height,
            stride: plane.stride,
            bitmap: plane.bitmap.clone(),
        })
        .collect()
}

unsafe fn capi_signatures(
    mut image: *mut rassa_capi::ASS_Image,
) -> Result<Vec<PlaneSignature>, String> {
    let mut signatures = Vec::new();
    while !image.is_null() {
        let node = unsafe { &*image };
        if node.width_or_height_is_invalid() {
            return Err(format!(
                "C API returned invalid image geometry: {}x{} stride={}",
                node.w, node.h, node.stride
            ));
        }
        let bitmap_len = (node.stride as usize)
            .checked_mul(node.h as usize)
            .ok_or_else(|| "C API image bitmap length overflowed".to_string())?;
        let bitmap = if bitmap_len == 0 {
            Vec::new()
        } else if node.bitmap.is_null() {
            return Err("C API returned a null bitmap for a non-empty image".to_string());
        } else {
            unsafe { slice::from_raw_parts(node.bitmap, bitmap_len) }.to_vec()
        };
        signatures.push(PlaneSignature {
            kind: node.type_,
            color: node.color,
            x: node.dst_x,
            y: node.dst_y,
            width: node.w,
            height: node.h,
            stride: node.stride,
            bitmap,
        });
        image = node.next;
    }
    Ok(signatures)
}

trait AssImageGeometry {
    fn width_or_height_is_invalid(&self) -> bool;
}

impl AssImageGeometry for rassa_capi::ASS_Image {
    fn width_or_height_is_invalid(&self) -> bool {
        self.w < 0 || self.h < 0 || self.stride < self.w || self.stride < 0
    }
}

fn verify_equivalence(
    rust: &RustHarness,
    capi: &mut CapiHarness,
    times_ms: &[i64],
) -> Result<u64, String> {
    let mut combined_digest = FNV_OFFSET;
    for &now_ms in times_ms {
        let rust_signature = rust_signatures(&rust.render(now_ms)?);
        let capi_signature = unsafe { capi_signatures(capi.render(now_ms)?)? };
        if rust_signature != capi_signature {
            return Err(signature_difference(
                now_ms,
                &rust_signature,
                &capi_signature,
            ));
        }

        let rust_cached_signature = rust_signatures(&rust.render(now_ms)?);
        if rust_cached_signature != rust_signature {
            return Err(format!(
                "Rust cached output differs from its first render at {now_ms} ms"
            ));
        }
        let capi_cached_signature = unsafe { capi_signatures(capi.render(now_ms)?)? };
        if capi_cached_signature != capi_signature {
            return Err(format!(
                "C API cached output differs from its first render at {now_ms} ms"
            ));
        }

        combined_digest = fnv_bytes(combined_digest, &now_ms.to_le_bytes());
        combined_digest = digest_signatures(combined_digest, &rust_signature);
    }
    Ok(combined_digest)
}

fn signature_difference(now_ms: i64, rust: &[PlaneSignature], capi: &[PlaneSignature]) -> String {
    if rust.len() != capi.len() {
        return format!(
            "Rust/C output differs at {now_ms} ms: plane count {} != {}",
            rust.len(),
            capi.len()
        );
    }
    for (index, (rust_plane, capi_plane)) in rust.iter().zip(capi).enumerate() {
        if rust_plane != capi_plane {
            let first_bitmap_difference = rust_plane
                .bitmap
                .iter()
                .zip(&capi_plane.bitmap)
                .position(|(left, right)| left != right);
            return format!(
                "Rust/C output differs at {now_ms} ms, plane {index}: rust=[{}] capi=[{}] first_bitmap_difference={first_bitmap_difference:?}",
                rust_plane.summary(),
                capi_plane.summary()
            );
        }
    }
    format!("Rust/C output differs at {now_ms} ms")
}

fn benchmark(
    args: Args,
    mode: BenchMode,
    mut render: impl FnMut(i64) -> Result<u64, String>,
) -> Result<BenchStats, String> {
    let mut checksum = 0_u64;
    for iteration in 0..args.warmup {
        checksum = checksum.rotate_left(7) ^ render(mode.timestamp(iteration))?;
    }

    let mut durations = Vec::with_capacity(args.samples);
    for sample in 0..args.samples {
        let offset = (sample as u64).wrapping_mul(args.iterations);
        let start = Instant::now();
        for iteration in 0..args.iterations {
            checksum = checksum.rotate_left(7) ^ render(mode.timestamp(offset + iteration))?;
        }
        durations.push(start.elapsed());
    }
    durations.sort_unstable();
    Ok(BenchStats {
        median: durations[durations.len() / 2],
        min: durations[0],
        max: durations[durations.len() - 1],
        checksum,
    })
}

fn cheap_rust_checksum(planes: &[ImagePlane]) -> u64 {
    planes.first().map_or(0, |plane| {
        (plane.size.width as u32 as u64) << 32
            ^ plane.size.height as u32 as u64
            ^ (u64::from(plane.bitmap.first().copied().unwrap_or_default()) << 56)
    })
}

unsafe fn cheap_capi_checksum(images: *mut rassa_capi::ASS_Image) -> u64 {
    let Some(node) = (unsafe { images.as_ref() }) else {
        return 0;
    };
    let first_pixel = if node.bitmap.is_null() {
        0
    } else {
        unsafe { *node.bitmap }
    };
    ((node.w as u32 as u64) << 32) ^ node.h as u32 as u64 ^ (u64::from(first_pixel) << 56)
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn digest_signatures(mut hash: u64, signatures: &[PlaneSignature]) -> u64 {
    hash = fnv_bytes(hash, &(signatures.len() as u64).to_le_bytes());
    for plane in signatures {
        for value in [
            plane.kind,
            plane.x,
            plane.y,
            plane.width,
            plane.height,
            plane.stride,
        ] {
            hash = fnv_bytes(hash, &value.to_le_bytes());
        }
        hash = fnv_bytes(hash, &plane.color.to_le_bytes());
        hash = fnv_bytes(hash, &(plane.bitmap.len() as u64).to_le_bytes());
        hash = fnv_bytes(hash, &plane.bitmap);
    }
    hash
}

fn print_benchmark(api: &str, mode: BenchMode, args: Args, stats: &BenchStats) {
    let frames = args.iterations as f64;
    let median_ns_per_frame = stats.median.as_nanos() as f64 / frames;
    let min_ns_per_frame = stats.min.as_nanos() as f64 / frames;
    let max_ns_per_frame = stats.max.as_nanos() as f64 / frames;
    let fps = 1_000_000_000.0 / median_ns_per_frame;
    println!(
        "benchmark workload={WORKLOAD_NAME} api={api} mode={} iterations={} samples={} median_ns_per_frame={median_ns_per_frame:.1} min_ns_per_frame={min_ns_per_frame:.1} max_ns_per_frame={max_ns_per_frame:.1} fps={fps:.2} checksum={:016x}",
        mode.label(),
        args.iterations,
        args.samples,
        stats.checksum,
    );
}

fn parse_args() -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--iterations" => {
                parsed.iterations = parse_positive(&mut args, "--iterations")?;
            }
            "--samples" => {
                parsed.samples = usize::try_from(parse_positive(&mut args, "--samples")?)
                    .map_err(|_| "--samples is too large".to_string())?;
            }
            "--warmup" => {
                parsed.warmup = parse_nonnegative(&mut args, "--warmup")?;
            }
            "--verify-only" => parsed.verify_only = true,
            "--help" | "-h" => {
                println!(
                    "usage: rassa-perf [--iterations N] [--samples N] [--warmup N] [--verify-only]\n\nRuns exact Rust/C-API output equivalence checks, then release-mode render benchmarks."
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument: {argument}")),
        }
    }
    Ok(parsed)
}

fn parse_positive(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u64, String> {
    let value = parse_nonnegative(args, flag)?;
    if value == 0 {
        Err(format!("{flag} must be greater than zero"))
    } else {
        Ok(value)
    }
}

fn parse_nonnegative(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<u64, String> {
    args.next()
        .ok_or_else(|| format!("missing value after {flag}"))?
        .parse()
        .map_err(|_| format!("invalid integer for {flag}"))
}

fn run(args: Args) -> Result<(), String> {
    if cfg!(debug_assertions) && !args.verify_only {
        eprintln!("warning: benchmark results are meaningful only with --release");
    }

    let rust = RustHarness::new()?;
    let frame = rust.script.play_res();
    let mut capi = CapiHarness::new(frame)?;
    let digest = verify_equivalence(&rust, &mut capi, EQUIVALENCE_TIMES_MS)?;
    println!(
        "equivalence workload={WORKLOAD_NAME} frames={} result=exact hash={digest:016x}",
        EQUIVALENCE_TIMES_MS.len()
    );
    if args.verify_only {
        return Ok(());
    }

    for mode in [BenchMode::Dynamic, BenchMode::Cached] {
        let rust_stats = benchmark(args, mode, |now_ms| rust.timed_render(now_ms))?;
        print_benchmark("rust", mode, args, &rust_stats);
        let capi_stats = benchmark(args, mode, |now_ms| capi.timed_render(now_ms))?;
        print_benchmark("capi", mode, args, &capi_stats);
    }
    Ok(())
}

fn main() {
    let result = parse_args().and_then(run);
    if let Err(error) = result {
        eprintln!("rassa-perf: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_workload_is_exact_across_rust_and_capi() {
        let rust = RustHarness::new().expect("Rust harness should initialize");
        let mut capi =
            CapiHarness::new(rust.script.play_res()).expect("C API harness should initialize");
        verify_equivalence(&rust, &mut capi, EQUIVALENCE_TIMES_MS)
            .expect("Rust and C API output should be byte-for-byte identical");
    }
}
