#![allow(
    dead_code,
    clippy::missing_safety_doc,
    clippy::vec_box,
    non_camel_case_types,
    non_snake_case,
    unsafe_op_in_unsafe_fn
)]

use std::{
    ffi::{CStr, CString, c_char, c_double, c_int, c_void},
    fs, mem, ptr, slice,
};

#[cfg(not(target_arch = "wasm32"))]
use libc::{free, malloc};
use rassa_core::{ImagePlane, Margins, Point, RendererConfig, RgbaColor, Size, ass};
use rassa_fonts::{
    AttachedFontProvider, DefaultFontFileProvider, FontAttachment as ProviderFontAttachment,
    FontProvider, FontconfigProvider, MergedFontProvider, NullFontProvider,
};
use rassa_parse::{
    ParsedAttachment, ParsedEvent, ParsedStyle, ParsedTrack, parse_script_bytes,
    parse_script_bytes_with_codepage,
};
use rassa_render::RenderEngine;

pub struct ASS_Library {
    fonts_dir: Option<String>,
    extract_fonts: bool,
    style_overrides: Vec<String>,
    message_cb: *mut c_void,
    message_data: *mut c_void,
    fonts: Vec<FontAttachment>,
}

pub struct ASS_Renderer {
    frame_width: c_int,
    frame_height: c_int,
    storage_width: c_int,
    storage_height: c_int,
    margins: [c_int; 4],
    use_margins: bool,
    pixel_aspect: c_double,
    shaping: c_int,
    font_scale: c_double,
    hinting: c_int,
    line_spacing: c_double,
    line_position: c_double,
    default_font: Option<String>,
    default_family: Option<String>,
    default_provider: c_int,
    fontconfig_config: Option<String>,
    fontconfig_update: bool,
    selective_override_bits: c_int,
    selective_override_style: Option<OwnedStyleOverride>,
    cache_limits: (c_int, c_int),
    font_provider_cache: Option<CachedFontProvider>,
    frame_cache_signature: Option<RenderedFrameCacheSignature>,
    last_timestamp: Option<i64>,
    last_active_count: usize,
    rendered_images: Option<OwnedImageList>,
}

#[repr(C)]
pub struct ASS_RenderPriv {
    _private: [u8; 0],
}

#[repr(C)]
pub struct ASS_ParserPriv {
    _private: [u8; 0],
}

#[repr(C)]
pub struct ASS_Style {
    pub Name: *mut c_char,
    pub FontName: *mut c_char,
    pub FontSize: c_double,
    pub PrimaryColour: u32,
    pub SecondaryColour: u32,
    pub OutlineColour: u32,
    pub BackColour: u32,
    pub Bold: c_int,
    pub Italic: c_int,
    pub Underline: c_int,
    pub StrikeOut: c_int,
    pub ScaleX: c_double,
    pub ScaleY: c_double,
    pub Spacing: c_double,
    pub Angle: c_double,
    pub BorderStyle: c_int,
    pub Outline: c_double,
    pub Shadow: c_double,
    pub Alignment: c_int,
    pub MarginL: c_int,
    pub MarginR: c_int,
    pub MarginV: c_int,
    pub Encoding: c_int,
    pub treat_fontname_as_pattern: c_int,
    pub Blur: c_double,
    pub Justify: c_int,
}

#[repr(C)]
pub struct ASS_Event {
    pub Start: i64,
    pub Duration: i64,
    pub ReadOrder: c_int,
    pub Layer: c_int,
    pub Style: c_int,
    pub Name: *mut c_char,
    pub MarginL: c_int,
    pub MarginR: c_int,
    pub MarginV: c_int,
    pub Effect: *mut c_char,
    pub Text: *mut c_char,
    pub render_priv: *mut ASS_RenderPriv,
}

#[repr(C)]
pub struct ASS_Image {
    pub w: c_int,
    pub h: c_int,
    pub stride: c_int,
    pub bitmap: *mut u8,
    pub color: u32,
    pub dst_x: c_int,
    pub dst_y: c_int,
    pub next: *mut ASS_Image,
    pub type_: c_int,
}

#[repr(C)]
pub struct ASS_Track {
    pub n_styles: c_int,
    pub max_styles: c_int,
    pub n_events: c_int,
    pub max_events: c_int,
    pub styles: *mut ASS_Style,
    pub events: *mut ASS_Event,
    pub style_format: *mut c_char,
    pub event_format: *mut c_char,
    pub track_type: c_int,
    pub PlayResX: c_int,
    pub PlayResY: c_int,
    pub Timer: c_double,
    pub WrapStyle: c_int,
    pub ScaledBorderAndShadow: c_int,
    pub Kerning: c_int,
    pub Language: *mut c_char,
    pub YCbCrMatrix: c_int,
    pub default_style: c_int,
    pub name: *mut c_char,
    pub library: *mut ASS_Library,
    pub parser_priv: *mut ASS_ParserPriv,
    pub LayoutResX: c_int,
    pub LayoutResY: c_int,
}

impl Default for ASS_Style {
    fn default() -> Self {
        Self {
            Name: ptr::null_mut(),
            FontName: ptr::null_mut(),
            FontSize: 20.0,
            PrimaryColour: 0x0000_00ff,
            SecondaryColour: 0x0000_ffff,
            OutlineColour: 0,
            BackColour: 0,
            Bold: 0,
            Italic: 0,
            Underline: 0,
            StrikeOut: 0,
            ScaleX: 1.0,
            ScaleY: 1.0,
            Spacing: 0.0,
            Angle: 0.0,
            BorderStyle: 1,
            Outline: 2.0,
            Shadow: 2.0,
            Alignment: ass::VALIGN_SUB | ass::HALIGN_CENTER,
            MarginL: 10,
            MarginR: 10,
            MarginV: 10,
            Encoding: 1,
            treat_fontname_as_pattern: 0,
            Blur: 0.0,
            Justify: ass::ASS_JUSTIFY_AUTO,
        }
    }
}

impl Default for ASS_Event {
    fn default() -> Self {
        Self {
            Start: 0,
            Duration: 0,
            ReadOrder: 0,
            Layer: 0,
            Style: 0,
            Name: ptr::null_mut(),
            MarginL: 0,
            MarginR: 0,
            MarginV: 0,
            Effect: ptr::null_mut(),
            Text: ptr::null_mut(),
            render_priv: ptr::null_mut(),
        }
    }
}

#[derive(Default)]
struct TrackState {
    features: [bool; 4],
    check_readorder: bool,
    prune_delay: Option<i64>,
    rendered: bool,
    cache_generation: u64,
    parsed_cache_signature: Option<ParsedTrackCacheSignature>,
    parsed_cache: Option<ParsedTrack>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ParsedTrackCacheSignature {
    n_styles: c_int,
    styles: usize,
    n_events: c_int,
    events: usize,
    style_format: usize,
    event_format: usize,
    track_type: c_int,
    play_res_x: c_int,
    play_res_y: c_int,
    timer_bits: u64,
    wrap_style: c_int,
    scaled_border_and_shadow: c_int,
    kerning: c_int,
    language: usize,
    ycbcr_matrix: c_int,
    default_style: c_int,
    layout_res_x: c_int,
    layout_res_y: c_int,
}

#[derive(Clone, Debug, Default)]
struct FontAttachment {
    name: String,
    data: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct OwnedStyleOverride {
    style: ParsedStyle,
}

struct CachedFontProvider {
    signature: FontProviderCacheSignature,
    provider: Box<dyn FontProvider>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FontProviderCacheSignature {
    library: usize,
    library_fonts_len: usize,
    library_fonts_data: Vec<(usize, usize)>,
    default_font: Option<String>,
    default_family: Option<String>,
    default_provider: c_int,
    fontconfig_config: Option<String>,
    fontconfig_update: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct RenderedFrameCacheSignature {
    track: usize,
    track_generation: u64,
    parsed_track: ParsedTrackCacheSignature,
    renderer_config: RendererConfig,
    font_provider: FontProviderCacheSignature,
    selective_override_bits: c_int,
    selective_override_style: Option<OwnedStyleOverride>,
    active_event_indices: Vec<usize>,
    approximate_animation_bucket: i64,
}

// Approximate animated ASS tags by reusing the previous rendered image within a
// time bucket. This intentionally trades sub-frame pixel accuracy/smoothness for
// much higher FPS on transform/karaoke/clip-heavy scripts.
const APPROXIMATE_ANIMATION_FRAME_BUCKET_MS: i64 = 500;
const APPROXIMATE_HEAVY_ANIMATION_FRAME_BUCKET_MS: i64 = 1000;

// If a frame contains hundreds/thousands of ASS_Image nodes, downstream
// compositors can spend more time walking tiny planes than rassa spent caching
// the frame. Collapse same-color/same-kind planes into coarse union bitmaps once
// the list is large. This is an intentional approximation: it preserves rough
// shape/color coverage while giving up exact per-glyph layering/overlap order.
const APPROXIMATE_SQUASH_PLANE_THRESHOLD: usize = 96;
const APPROXIMATE_MULTILINE_FAST_PATH_THRESHOLD: usize = 4;
const APPROXIMATE_ADJACENT_LINE_CHANGE_WINDOW_MS: i64 = 150;

#[derive(Default)]
struct OwnedImageList {
    bitmaps: Vec<Vec<u8>>,
    nodes: Vec<Box<ASS_Image>>,
}

impl OwnedImageList {
    fn from_planes(planes: Vec<rassa_core::ImagePlane>) -> Self {
        let mut bitmaps = Vec::with_capacity(planes.len());
        let mut nodes = Vec::with_capacity(planes.len());

        for plane in planes {
            bitmaps.push(plane.bitmap);
            let bitmap = bitmaps.last_mut().expect("bitmap just pushed");
            nodes.push(Box::new(ASS_Image {
                w: plane.size.width,
                h: plane.size.height,
                stride: plane.stride,
                bitmap: if bitmap.is_empty() {
                    ptr::null_mut()
                } else {
                    bitmap.as_mut_ptr()
                },
                color: plane.color.0,
                dst_x: plane.destination.x,
                dst_y: plane.destination.y,
                next: ptr::null_mut(),
                type_: plane.kind as c_int,
            }));
        }

        for index in 0..nodes.len() {
            let next = nodes
                .get_mut(index + 1)
                .map(|node| &mut **node as *mut ASS_Image)
                .unwrap_or(ptr::null_mut());
            nodes[index].next = next;
        }

        Self { bitmaps, nodes }
    }

    fn head_ptr(&mut self) -> *mut ASS_Image {
        self.nodes
            .first_mut()
            .map(|node| &mut **node as *mut ASS_Image)
            .unwrap_or(ptr::null_mut())
    }
}

#[derive(Clone)]
struct SquashedPlaneGroup {
    color: RgbaColor,
    kind: ass::ImageType,
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
    planes: Vec<ImagePlane>,
}

fn squash_dense_planes_approximately(planes: Vec<ImagePlane>) -> Vec<ImagePlane> {
    if planes.len() < APPROXIMATE_SQUASH_PLANE_THRESHOLD {
        return planes;
    }

    let mut groups: Vec<SquashedPlaneGroup> = Vec::new();
    for plane in planes {
        if plane.size.width <= 0
            || plane.size.height <= 0
            || plane.stride <= 0
            || plane.bitmap.is_empty()
        {
            continue;
        }

        let min_x = plane.destination.x;
        let min_y = plane.destination.y;
        let max_x = min_x.saturating_add(plane.size.width);
        let max_y = min_y.saturating_add(plane.size.height);
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.color == plane.color && group.kind == plane.kind)
        {
            group.min_x = group.min_x.min(min_x);
            group.min_y = group.min_y.min(min_y);
            group.max_x = group.max_x.max(max_x);
            group.max_y = group.max_y.max(max_y);
            group.planes.push(plane);
        } else {
            groups.push(SquashedPlaneGroup {
                color: plane.color,
                kind: plane.kind,
                min_x,
                min_y,
                max_x,
                max_y,
                planes: vec![plane],
            });
        }
    }

    groups
        .into_iter()
        .filter_map(squash_plane_group_approximately)
        .collect()
}

fn squash_plane_group_approximately(group: SquashedPlaneGroup) -> Option<ImagePlane> {
    let width = group.max_x.checked_sub(group.min_x)?;
    let height = group.max_y.checked_sub(group.min_y)?;
    if width <= 0 || height <= 0 {
        return None;
    }

    let width_usize = usize::try_from(width).ok()?;
    let height_usize = usize::try_from(height).ok()?;
    let len = width_usize.checked_mul(height_usize)?;
    let mut bitmap = vec![0_u8; len];

    for plane in group.planes {
        let plane_width = usize::try_from(plane.size.width).ok()?;
        let plane_height = usize::try_from(plane.size.height).ok()?;
        let stride = usize::try_from(plane.stride).ok()?;
        let dx = usize::try_from(plane.destination.x.checked_sub(group.min_x)?).ok()?;
        let dy = usize::try_from(plane.destination.y.checked_sub(group.min_y)?).ok()?;

        for row in 0..plane_height {
            let src_row = row.checked_mul(stride)?;
            let dst_row = dy.checked_add(row)?.checked_mul(width_usize)?;
            for column in 0..plane_width {
                let src = *plane.bitmap.get(src_row.checked_add(column)?)?;
                let dst_index = dst_row.checked_add(dx)?.checked_add(column)?;
                let dst = bitmap.get_mut(dst_index)?;
                *dst = (*dst).max(src);
            }
        }
    }

    Some(ImagePlane {
        size: Size { width, height },
        stride: width,
        color: group.color,
        destination: Point {
            x: group.min_x,
            y: group.min_y,
        },
        kind: group.kind,
        bitmap,
    })
}

fn render_frame_planes(
    parsed: &ParsedTrack,
    renderer: &mut ASS_Renderer,
    library: *mut ASS_Library,
    now: i64,
    renderer_config: &RendererConfig,
) -> Vec<ImagePlane> {
    let provider = cached_font_provider(renderer, library);
    let provider: &dyn FontProvider = unsafe { &*provider };
    let planes = RenderEngine::new().render_frame_with_provider_and_config(
        parsed,
        &provider,
        now,
        renderer_config,
    );
    squash_dense_planes_approximately(planes)
}

fn should_use_approximate_multiline_fast_path(
    track: &ParsedTrack,
    active_event_indices: &[usize],
    now: i64,
) -> bool {
    active_event_indices.len() >= APPROXIMATE_MULTILINE_FAST_PATH_THRESHOLD
        || active_events_have_adjacent_line_change(track, active_event_indices, now)
}

fn active_events_have_adjacent_line_change(
    track: &ParsedTrack,
    active_event_indices: &[usize],
    now: i64,
) -> bool {
    active_event_indices.iter().any(|index| {
        let Some(event) = track.events.get(*index) else {
            return false;
        };
        let near_own_boundary = (now - event.start).abs()
            <= APPROXIMATE_ADJACENT_LINE_CHANGE_WINDOW_MS
            || (now - (event.start + event.duration)).abs()
                <= APPROXIMATE_ADJACENT_LINE_CHANGE_WINDOW_MS;
        if !near_own_boundary {
            return false;
        }
        let event_end = event.start + event.duration;
        track.events.iter().enumerate().any(|(other_index, other)| {
            other_index != *index
                && ((other.start - event_end).abs() <= APPROXIMATE_ADJACENT_LINE_CHANGE_WINDOW_MS
                    || ((other.start + other.duration) - event.start).abs()
                        <= APPROXIMATE_ADJACENT_LINE_CHANGE_WINDOW_MS)
        })
    })
}

fn approximate_multiline_text_planes(
    track: &ParsedTrack,
    active_event_indices: &[usize],
    renderer_config: &RendererConfig,
) -> Option<Vec<ImagePlane>> {
    let mut planes = Vec::with_capacity(active_event_indices.len());
    for (fallback_row, index) in active_event_indices.iter().enumerate() {
        let event = track.events.get(*index)?;
        if !event.effect.trim().is_empty() || event_text_contains_vector_or_drawing(&event.text) {
            return None;
        }
        let style = track
            .styles
            .get(event.style.max(0) as usize)
            .or_else(|| track.styles.first())?;
        let visible = strip_ass_override_tags(&event.text)
            .replace("\\N", "\n")
            .replace("\\n", "\n");
        let visible_lines: Vec<&str> = visible
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let row_count = visible_lines.len().max(1);
        let scale_x =
            renderer_config.frame.width as f64 / renderer_config.storage.width.max(1) as f64;
        let scale_y =
            renderer_config.frame.height as f64 / renderer_config.storage.height.max(1) as f64;
        let font_height = (style.font_size * scale_y).clamp(8.0, 160.0);
        let line_height = (font_height * 1.18).round().max(8.0) as i32;
        let longest_chars = visible_lines
            .iter()
            .map(|line| line.chars().filter(|ch| !ch.is_control()).count())
            .max()
            .unwrap_or_else(|| visible.chars().count())
            .max(1);
        let width = ((longest_chars as f64 * font_height * 0.56 * scale_x)
            .round()
            .max(font_height)
            .min(renderer_config.frame.width as f64)) as i32;
        let height = (row_count as i32 * line_height).max(line_height);
        let (anchor_x, anchor_y) =
            approximate_event_anchor(event, style, renderer_config, fallback_row);
        let (x, y) = align_approximate_text_box(anchor_x, anchor_y, width, height, style.alignment);
        planes.push(make_filled_plane(
            x.clamp(-width, renderer_config.frame.width),
            y.clamp(-height, renderer_config.frame.height),
            width,
            height,
            RgbaColor(ass_color_to_rgba(style.primary_colour)),
            ass::ImageType::Character,
            210,
        ));
    }
    (!planes.is_empty()).then_some(planes)
}

fn make_filled_plane(
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: RgbaColor,
    kind: ass::ImageType,
    alpha: u8,
) -> ImagePlane {
    let width = width.max(1);
    let height = height.max(1);
    let len = (width as usize).saturating_mul(height as usize);
    ImagePlane {
        size: Size { width, height },
        stride: width,
        color,
        destination: Point { x, y },
        kind,
        bitmap: vec![alpha; len],
    }
}

fn approximate_event_anchor(
    event: &ParsedEvent,
    style: &ParsedStyle,
    renderer_config: &RendererConfig,
    fallback_row: usize,
) -> (i32, i32) {
    if let Some((x, y)) = parse_pos_override(&event.text) {
        let scale_x =
            renderer_config.frame.width as f64 / renderer_config.storage.width.max(1) as f64;
        let scale_y =
            renderer_config.frame.height as f64 / renderer_config.storage.height.max(1) as f64;
        return ((x * scale_x).round() as i32, (y * scale_y).round() as i32);
    }

    let x = renderer_config.frame.width / 2;
    let margin_v = event.margin_v.max(style.margin_v).max(0);
    let y = renderer_config
        .frame
        .height
        .saturating_sub(margin_v)
        .saturating_sub((fallback_row as i32) * (style.font_size.round() as i32 + 8));
    (x, y)
}

fn align_approximate_text_box(
    anchor_x: i32,
    anchor_y: i32,
    width: i32,
    height: i32,
    alignment: i32,
) -> (i32, i32) {
    let halign = alignment & 0x03;
    let valign = alignment & 0x0c;
    let x = match halign {
        ass::HALIGN_LEFT => anchor_x,
        ass::HALIGN_RIGHT => anchor_x - width,
        _ => anchor_x - width / 2,
    };
    let y = match valign {
        ass::VALIGN_TOP => anchor_y,
        ass::VALIGN_CENTER => anchor_y - height / 2,
        _ => anchor_y - height,
    };
    (x, y)
}

fn strip_ass_override_tags(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '{' => in_tag = true,
            '}' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn parse_pos_override(text: &str) -> Option<(f64, f64)> {
    let lower = text.to_ascii_lowercase();
    let start = lower.find("\\pos(")? + 5;
    let end = lower[start..].find(')')? + start;
    parse_two_numbers(&text[start..end])
}

fn parse_two_numbers(value: &str) -> Option<(f64, f64)> {
    let mut parts = value.split(',').map(str::trim);
    let x = parts.next()?.parse().ok()?;
    let y = parts.next()?.parse().ok()?;
    Some((x, y))
}

fn event_text_contains_vector_or_drawing(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("\\clip(")
        || text.contains("\\iclip(")
        || (0..=9).any(|value| text.contains(&format!("\\p{value}")))
        || text.contains("\\p ")
}

fn ass_color_to_rgba(color: u32) -> u32 {
    let alpha = (color >> 24) & 0xff;
    let blue = (color >> 16) & 0xff;
    let green = (color >> 8) & 0xff;
    let red = color & 0xff;
    (red << 24) | (green << 16) | (blue << 8) | alpha
}

impl OwnedStyleOverride {
    unsafe fn from_ffi(style: *mut ASS_Style) -> Option<Self> {
        let style = style.as_ref()?;
        Some(Self {
            style: ParsedStyle {
                name: string_option_from_ptr(style.Name).unwrap_or_default(),
                font_name: string_option_from_ptr(style.FontName).unwrap_or_default(),
                font_size: style.FontSize,
                primary_colour: style.PrimaryColour,
                secondary_colour: style.SecondaryColour,
                outline_colour: style.OutlineColour,
                back_colour: style.BackColour,
                bold: ffi_bold_is_active(style.Bold),
                font_weight: ffi_bold_weight(style.Bold),
                italic: style.Italic != 0,
                underline: style.Underline != 0,
                strike_out: style.StrikeOut != 0,
                scale_x: style.ScaleX,
                scale_y: style.ScaleY,
                spacing: style.Spacing,
                angle: style.Angle,
                border_style: style.BorderStyle,
                outline: style.Outline,
                shadow: style.Shadow,
                alignment: style.Alignment,
                margin_l: style.MarginL,
                margin_r: style.MarginR,
                margin_v: style.MarginV,
                encoding: style.Encoding,
                treat_fontname_as_pattern: style.treat_fontname_as_pattern,
                blur: style.Blur,
                justify: style.Justify,
            },
        })
    }
}

impl Default for ASS_Library {
    fn default() -> Self {
        Self {
            fonts_dir: None,
            extract_fonts: false,
            style_overrides: Vec::new(),
            message_cb: ptr::null_mut(),
            message_data: ptr::null_mut(),
            fonts: Vec::new(),
        }
    }
}

impl Default for ASS_Renderer {
    fn default() -> Self {
        Self {
            frame_width: 0,
            frame_height: 0,
            storage_width: 0,
            storage_height: 0,
            margins: [0; 4],
            use_margins: false,
            pixel_aspect: 0.0,
            shaping: ass::ShapingLevel::Complex as c_int,
            font_scale: 1.0,
            hinting: ass::Hinting::None as c_int,
            line_spacing: 0.0,
            line_position: 0.0,
            default_font: None,
            default_family: None,
            default_provider: ass::DefaultFontProvider::Autodetect as c_int,
            fontconfig_config: None,
            fontconfig_update: true,
            selective_override_bits: 0,
            selective_override_style: None,
            cache_limits: (0, 0),
            font_provider_cache: None,
            frame_cache_signature: None,
            last_timestamp: None,
            last_active_count: 0,
            rendered_images: None,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_library_version() -> c_int {
    ass::LIBASS_VERSION
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_library_init() -> *mut ASS_Library {
    Box::into_raw(Box::new(ASS_Library::default()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_library_done(priv_: *mut ASS_Library) {
    if !priv_.is_null() {
        drop(Box::from_raw(priv_));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_fonts_dir(priv_: *mut ASS_Library, fonts_dir: *const c_char) {
    if let Some(library) = priv_.as_mut() {
        library.fonts_dir = string_option_from_ptr(fonts_dir);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_extract_fonts(priv_: *mut ASS_Library, extract: c_int) {
    if let Some(library) = priv_.as_mut() {
        library.extract_fonts = extract != 0;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_style_overrides(priv_: *mut ASS_Library, list: *mut *mut c_char) {
    let Some(library) = priv_.as_mut() else {
        return;
    };

    library.style_overrides.clear();
    if list.is_null() {
        return;
    }

    let mut index = 0;
    loop {
        let entry = *list.add(index);
        if entry.is_null() {
            break;
        }
        library.style_overrides.push(string_from_ptr(entry));
        index += 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_process_force_style(track: *mut ASS_Track) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    let Some(library) = track_ref.library.as_ref() else {
        return;
    };

    let overrides = library.style_overrides.clone();
    for override_entry in overrides {
        let Some((raw_key, raw_value)) = override_entry.rsplit_once('=') else {
            continue;
        };
        let key = raw_key.trim();
        let value = raw_value.trim();
        if key.is_empty() {
            continue;
        }

        if apply_track_override(track_ref, key, value) {
            continue;
        }

        let (style_name, field_name) = match key.rsplit_once('.') {
            Some((style_name, field_name)) if !style_name.trim().is_empty() => {
                (Some(style_name.trim()), field_name.trim())
            }
            _ => (None, key),
        };

        if field_name.is_empty() || track_ref.styles.is_null() || track_ref.n_styles <= 0 {
            continue;
        }

        for style in slice::from_raw_parts_mut(track_ref.styles, track_ref.n_styles as usize) {
            let matches_style = style_name.is_none_or(|target| {
                string_option_from_ptr(style.Name)
                    .is_some_and(|name| name.eq_ignore_ascii_case(target))
            });
            if matches_style {
                apply_style_override(style, field_name, value);
            }
        }
    }
    invalidate_parsed_track_cache(track);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_message_cb(
    priv_: *mut ASS_Library,
    msg_cb: *mut c_void,
    data: *mut c_void,
) {
    if let Some(library) = priv_.as_mut() {
        library.message_cb = msg_cb;
        library.message_data = data;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_renderer_init(_library: *mut ASS_Library) -> *mut ASS_Renderer {
    Box::into_raw(Box::new(ASS_Renderer::default()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_renderer_done(priv_: *mut ASS_Renderer) {
    if !priv_.is_null() {
        drop(Box::from_raw(priv_));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_frame_size(priv_: *mut ASS_Renderer, w: c_int, h: c_int) {
    if let Some(renderer) = priv_.as_mut() {
        let (w, h) = sanitize_size_pair(w, h);
        renderer.frame_width = w;
        renderer.frame_height = h;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_storage_size(priv_: *mut ASS_Renderer, w: c_int, h: c_int) {
    if let Some(renderer) = priv_.as_mut() {
        let (w, h) = sanitize_size_pair(w, h);
        renderer.storage_width = w;
        renderer.storage_height = h;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_shaper(priv_: *mut ASS_Renderer, level: c_int) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.shaping = if level == ass::ShapingLevel::Simple as c_int
            || level == ass::ShapingLevel::Complex as c_int
        {
            level
        } else {
            ass::ShapingLevel::Complex as c_int
        };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_margins(
    priv_: *mut ASS_Renderer,
    t: c_int,
    b: c_int,
    l: c_int,
    r: c_int,
) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.margins = [t, b, l, r];
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_use_margins(priv_: *mut ASS_Renderer, use_margins: c_int) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.use_margins = use_margins != 0;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_pixel_aspect(priv_: *mut ASS_Renderer, par: c_double) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.pixel_aspect = if par < 0.0 { 0.0 } else { par };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_aspect_ratio(
    priv_: *mut ASS_Renderer,
    dar: c_double,
    sar: c_double,
) {
    if sar == 0.0 {
        ass_set_pixel_aspect(priv_, 0.0);
    } else {
        ass_set_pixel_aspect(priv_, dar / sar);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_font_scale(priv_: *mut ASS_Renderer, font_scale: c_double) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.font_scale = font_scale;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_hinting(priv_: *mut ASS_Renderer, hinting: c_int) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.hinting = hinting;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_line_spacing(priv_: *mut ASS_Renderer, line_spacing: c_double) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.line_spacing = line_spacing;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_line_position(priv_: *mut ASS_Renderer, line_position: c_double) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.line_position = line_position;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_get_available_font_providers(
    _priv_: *mut ASS_Library,
    providers: *mut *mut c_int,
    size: *mut usize,
) {
    if providers.is_null() || size.is_null() {
        return;
    }

    let values = [
        ass::DefaultFontProvider::None as c_int,
        ass::DefaultFontProvider::Autodetect as c_int,
        ass::DefaultFontProvider::Fontconfig as c_int,
    ];
    let allocation_size = mem::size_of_val(&values);
    let allocation = ass_malloc(allocation_size) as *mut c_int;
    if allocation.is_null() {
        *providers = ptr::null_mut();
        *size = usize::MAX;
        return;
    }

    ptr::copy_nonoverlapping(values.as_ptr(), allocation, values.len());
    *providers = allocation;
    *size = values.len();
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_fonts(
    priv_: *mut ASS_Renderer,
    default_font: *const c_char,
    default_family: *const c_char,
    dfp: c_int,
    config: *const c_char,
    update: c_int,
) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.default_font = string_option_from_ptr(default_font);
        renderer.default_family = string_option_from_ptr(default_family);
        renderer.default_provider = dfp;
        renderer.fontconfig_config = string_option_from_ptr(config);
        renderer.fontconfig_update = update != 0;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_selective_style_override_enabled(
    priv_: *mut ASS_Renderer,
    bits: c_int,
) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.selective_override_bits = bits;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_selective_style_override(
    priv_: *mut ASS_Renderer,
    style: *mut ASS_Style,
) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.selective_override_style = OwnedStyleOverride::from_ffi(style);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_fonts_update(_priv_: *mut ASS_Renderer) -> c_int {
    1
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_cache_limits(
    priv_: *mut ASS_Renderer,
    glyph_max: c_int,
    bitmap_max_size: c_int,
) {
    if let Some(renderer) = priv_.as_mut() {
        renderer.cache_limits = (glyph_max, bitmap_max_size);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_render_frame(
    priv_: *mut ASS_Renderer,
    track: *mut ASS_Track,
    now: i64,
    detect_change: *mut c_int,
) -> *mut ASS_Image {
    let Some(renderer) = priv_.as_mut() else {
        return ptr::null_mut();
    };

    if let Some(state) = track_state_mut(track) {
        state.rendered = true;
    }

    if let Some(delay) = track_state_mut(track).and_then(|state| state.prune_delay) {
        ass_prune_events(track, now - delay);
    }

    let active_event_indices = active_event_indices(track, now);
    let active_count = active_event_indices.len();
    if let Some(detect_change) = detect_change.as_mut() {
        *detect_change =
            if renderer.last_timestamp == Some(now) && renderer.last_active_count == active_count {
                0
            } else if renderer.last_active_count == active_count {
                1
            } else {
                2
            };
    }

    renderer.last_timestamp = Some(now);
    renderer.last_active_count = active_count;

    let Some(track_ref) = track.as_ref() else {
        renderer.rendered_images = None;
        renderer.frame_cache_signature = None;
        return ptr::null_mut();
    };

    let cached = cached_parsed_track_from_ffi(track, track_ref);
    let override_active = selective_style_overrides_active(renderer);
    let parsed_with_overrides;
    let parsed = if override_active {
        let mut parsed = cached.clone();
        apply_selective_style_overrides(&mut parsed, renderer);
        parsed_with_overrides = parsed;
        &parsed_with_overrides
    } else {
        cached
    };
    let renderer_config = renderer_config(renderer, parsed);
    let font_provider_signature = font_provider_cache_signature(renderer, track_ref.library);
    let track_generation = track_state_ref(track)
        .map(|state| state.cache_generation)
        .unwrap_or_default();
    let approximate_animation_bucket = frame_cache_time_bucket(parsed, &active_event_indices, now);
    let frame_cache_signature = approximate_animation_bucket.map(|approximate_animation_bucket| {
        RenderedFrameCacheSignature {
            track: track as usize,
            track_generation,
            parsed_track: parsed_track_cache_signature(track_ref),
            renderer_config: renderer_config.clone(),
            font_provider: font_provider_signature,
            selective_override_bits: renderer.selective_override_bits,
            selective_override_style: renderer.selective_override_style.clone(),
            active_event_indices: active_event_indices.clone(),
            approximate_animation_bucket,
        }
    });
    if frame_cache_signature.is_some()
        && renderer.frame_cache_signature == frame_cache_signature
        && renderer.rendered_images.is_some()
    {
        return renderer
            .rendered_images
            .as_mut()
            .map(OwnedImageList::head_ptr)
            .unwrap_or(ptr::null_mut());
    }

    let planes = if should_use_approximate_multiline_fast_path(parsed, &active_event_indices, now) {
        approximate_multiline_text_planes(parsed, &active_event_indices, &renderer_config)
            .unwrap_or_else(|| {
                render_frame_planes(parsed, renderer, track_ref.library, now, &renderer_config)
            })
    } else {
        render_frame_planes(parsed, renderer, track_ref.library, now, &renderer_config)
    };
    renderer.rendered_images = Some(OwnedImageList::from_planes(planes));
    renderer.frame_cache_signature = frame_cache_signature;
    renderer
        .rendered_images
        .as_mut()
        .map(OwnedImageList::head_ptr)
        .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_new_track(library: *mut ASS_Library) -> *mut ASS_Track {
    let state = Box::new(TrackState {
        check_readorder: true,
        ..TrackState::default()
    });
    let parser_priv = Box::into_raw(state) as *mut ASS_ParserPriv;
    let track = ASS_Track {
        n_styles: 0,
        max_styles: 0,
        n_events: 0,
        max_events: 0,
        styles: ptr::null_mut(),
        events: ptr::null_mut(),
        style_format: ptr::null_mut(),
        event_format: ptr::null_mut(),
        track_type: ass::TrackType::Unknown as c_int,
        PlayResX: 384,
        PlayResY: 288,
        Timer: 100.0,
        WrapStyle: 0,
        ScaledBorderAndShadow: 1,
        Kerning: 1,
        Language: ptr::null_mut(),
        YCbCrMatrix: ass::YCbCrMatrix::Default as c_int,
        default_style: 0,
        name: ptr::null_mut(),
        library,
        parser_priv,
        LayoutResX: 0,
        LayoutResY: 0,
    };

    Box::into_raw(Box::new(track))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_track_set_feature(
    track: *mut ASS_Track,
    feature: c_int,
    enable: c_int,
) -> c_int {
    let Some(state) = track_state_mut(track) else {
        return -1;
    };
    if state.rendered {
        return -1;
    }
    let Some(slot) = state.features.get_mut(feature as usize) else {
        return -1;
    };
    *slot = enable != 0;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_free_track(track: *mut ASS_Track) {
    if track.is_null() {
        return;
    }

    let mut boxed = Box::from_raw(track);
    free_track_contents(&mut boxed);
    if !boxed.parser_priv.is_null() {
        drop(Box::from_raw(boxed.parser_priv as *mut TrackState));
        boxed.parser_priv = ptr::null_mut();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_alloc_style(track: *mut ASS_Track) -> c_int {
    let Some(track_ref) = track.as_mut() else {
        return -1;
    };
    let mut styles = take_styles(track_ref);
    styles.push(ASS_Style::default());
    let id = (styles.len() - 1) as c_int;
    store_styles(track_ref, styles);
    id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_alloc_event(track: *mut ASS_Track) -> c_int {
    let Some(track_ref) = track.as_mut() else {
        return -1;
    };
    let mut events = take_events(track_ref);
    let event = ASS_Event {
        ReadOrder: events.len() as c_int,
        ..ASS_Event::default()
    };
    events.push(event);
    let id = (events.len() - 1) as c_int;
    store_events(track_ref, events);
    id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_free_style(track: *mut ASS_Track, sid: c_int) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    let mut styles = take_styles(track_ref);
    if let Some(style) = styles.get_mut(sid as usize) {
        free_style(style);
        *style = ASS_Style::default();
    }
    store_styles(track_ref, styles);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_free_event(track: *mut ASS_Track, eid: c_int) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    let mut events = take_events(track_ref);
    if let Some(event) = events.get_mut(eid as usize) {
        free_event(event);
        *event = ASS_Event::default();
    }
    store_events(track_ref, events);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_process_data(track: *mut ASS_Track, data: *const c_char, size: c_int) {
    if track.is_null() || data.is_null() || size < 0 {
        return;
    }

    let bytes = slice::from_raw_parts(data as *const u8, size as usize);
    if let Ok(parsed) = parse_script_bytes(bytes) {
        maybe_extract_parsed_fonts(track, &parsed);
        replace_track_from_parsed(track, parsed);
        ass_process_force_style(track);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_process_codec_private(
    track: *mut ASS_Track,
    data: *const c_char,
    size: c_int,
) {
    ass_process_data(track, data, size);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_process_chunk(
    track: *mut ASS_Track,
    data: *const c_char,
    size: c_int,
    timecode: i64,
    duration: i64,
) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    if data.is_null() || size < 0 {
        return;
    }

    let bytes = slice::from_raw_parts(data as *const u8, size as usize);
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut events = take_events(track_ref);
    events.push(make_event(&ParsedEvent {
        start: timecode,
        duration,
        read_order: if track_state_mut(track)
            .map(|state| state.check_readorder)
            .unwrap_or(true)
        {
            events.len() as c_int
        } else {
            0
        },
        layer: 0,
        style: 0,
        name: String::new(),
        margin_l: 0,
        margin_r: 0,
        margin_v: 0,
        effect: String::new(),
        text,
    }));
    store_events(track_ref, events);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_set_check_readorder(track: *mut ASS_Track, check_readorder: c_int) {
    if let Some(state) = track_state_mut(track) {
        state.check_readorder = check_readorder == 1;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_prune_events(track: *mut ASS_Track, deadline: i64) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };

    let mut events = take_events(track_ref);
    events.retain_mut(|event| {
        let keep = event.Start + event.Duration >= deadline;
        if !keep {
            free_event(event);
        }
        keep
    });
    for (index, event) in events.iter_mut().enumerate() {
        event.ReadOrder = index as c_int;
    }
    store_events(track_ref, events);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_configure_prune(track: *mut ASS_Track, delay: i64) {
    if let Some(state) = track_state_mut(track) {
        state.prune_delay = (delay >= 0).then_some(delay);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_flush_events(track: *mut ASS_Track) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };

    let mut events = take_events(track_ref);
    for event in &mut events {
        free_event(event);
    }
    store_events(track_ref, Vec::new());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_read_file(
    library: *mut ASS_Library,
    fname: *const c_char,
    codepage: *const c_char,
) -> *mut ASS_Track {
    let Some(path) = string_option_from_ptr(fname) else {
        return ptr::null_mut();
    };
    let codepage = string_option_from_ptr(codepage);
    let Ok(bytes) = fs::read(path) else {
        return ptr::null_mut();
    };
    let Ok(parsed) = parse_script_bytes_with_codepage(&bytes, codepage.as_deref()) else {
        return ptr::null_mut();
    };
    maybe_extract_fonts_to_library(library, &parsed.attachments);
    let track = track_from_parsed(library, parsed);
    ass_process_force_style(track);
    track
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_read_memory(
    library: *mut ASS_Library,
    buf: *mut c_char,
    bufsize: usize,
    codepage: *const c_char,
) -> *mut ASS_Track {
    if buf.is_null() {
        return ptr::null_mut();
    }

    let codepage = string_option_from_ptr(codepage);
    let bytes = slice::from_raw_parts(buf as *const u8, bufsize);
    let Ok(parsed) = parse_script_bytes_with_codepage(bytes, codepage.as_deref()) else {
        return ptr::null_mut();
    };
    maybe_extract_fonts_to_library(library, &parsed.attachments);
    let track = track_from_parsed(library, parsed);
    ass_process_force_style(track);
    track
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_read_styles(
    track: *mut ASS_Track,
    fname: *const c_char,
    codepage: *const c_char,
) -> c_int {
    let Some(path) = string_option_from_ptr(fname) else {
        return 1;
    };
    let codepage = string_option_from_ptr(codepage);
    let Ok(bytes) = fs::read(path) else {
        return 1;
    };
    let Ok(parsed) = parse_script_bytes_with_codepage(&bytes, codepage.as_deref()) else {
        return 1;
    };
    let Some(track_ref) = track.as_mut() else {
        return 1;
    };

    if track_styles_match_parsed(track_ref, &parsed) {
        return 0;
    }

    let mut styles = take_styles(track_ref);
    for mut style in styles.drain(..) {
        free_style(&mut style);
    }
    let new_styles = parsed.styles.iter().map(make_style).collect();
    store_styles(track_ref, new_styles);
    replace_string(&mut track_ref.style_format, &parsed.style_format);
    track_ref.track_type = parsed.track_type as c_int;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_add_font(
    library: *mut ASS_Library,
    name: *const c_char,
    data: *const c_char,
    data_size: c_int,
) {
    let Some(library) = library.as_mut() else {
        return;
    };
    if data.is_null() || data_size < 0 {
        return;
    }

    library.fonts.push(FontAttachment {
        name: string_option_from_ptr(name).unwrap_or_default(),
        data: slice::from_raw_parts(data as *const u8, data_size as usize).to_vec(),
    });
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_clear_fonts(library: *mut ASS_Library) {
    if let Some(library) = library.as_mut() {
        library.fonts.clear();
    }
}

fn font_provider_cache_signature(
    renderer: &ASS_Renderer,
    library: *mut ASS_Library,
) -> FontProviderCacheSignature {
    let library_ref = unsafe { library.as_ref() };
    let library_fonts_data = library_ref
        .map(|library| {
            library
                .fonts
                .iter()
                .map(|font| (font.data.as_ptr() as usize, font.data.len()))
                .collect()
        })
        .unwrap_or_default();
    FontProviderCacheSignature {
        library: library as usize,
        library_fonts_len: library_ref.map(|library| library.fonts.len()).unwrap_or(0),
        library_fonts_data,
        default_font: renderer.default_font.clone(),
        default_family: renderer.default_family.clone(),
        default_provider: renderer.default_provider,
        fontconfig_config: renderer.fontconfig_config.clone(),
        fontconfig_update: renderer.fontconfig_update,
    }
}

fn cached_font_provider(
    renderer: &mut ASS_Renderer,
    library: *mut ASS_Library,
) -> *const dyn FontProvider {
    let signature = font_provider_cache_signature(renderer, library);
    if renderer
        .font_provider_cache
        .as_ref()
        .is_none_or(|cache| cache.signature != signature)
    {
        let provider = build_font_provider(renderer, library);
        renderer.font_provider_cache = Some(CachedFontProvider {
            signature: signature.clone(),
            provider,
        });
    }
    &*renderer
        .font_provider_cache
        .as_ref()
        .expect("font provider cached")
        .provider
}

fn build_font_provider(
    renderer: &ASS_Renderer,
    library: *mut ASS_Library,
) -> Box<dyn FontProvider> {
    let has_system_provider = matches!(
        renderer.default_provider,
        value if value == ass::DefaultFontProvider::Autodetect as c_int
            || value == ass::DefaultFontProvider::Fontconfig as c_int
    );
    let system_provider: Box<dyn FontProvider> = match renderer.default_provider {
        _ if has_system_provider => {
            if let Some(fallback_family) = renderer.default_family.as_deref() {
                Box::new(FontconfigProvider::with_fallback_family(fallback_family))
            } else {
                Box::new(FontconfigProvider::new())
            }
        }
        _ => Box::new(NullFontProvider),
    };

    let Some(library) = (unsafe { library.as_ref() }) else {
        return wrap_default_font_path(system_provider, renderer);
    };
    if library.fonts.is_empty() {
        return wrap_default_font_path(system_provider, renderer);
    }

    let attachments = library
        .fonts
        .iter()
        .map(|font| ProviderFontAttachment {
            name: font.name.clone(),
            data: font.data.clone(),
        })
        .collect::<Vec<_>>();
    let attached = if let Some(fonts_dir) = library.fonts_dir.as_deref() {
        AttachedFontProvider::from_attachments_in_dir(&attachments, Some(fonts_dir))
    } else {
        AttachedFontProvider::from_attachments(&attachments)
    };

    let provider: Box<dyn FontProvider> = if has_system_provider {
        Box::new(MergedFontProvider::new(attached, system_provider))
    } else {
        Box::new(attached)
    };
    wrap_default_font_path(provider, renderer)
}

fn wrap_default_font_path(
    provider: Box<dyn FontProvider>,
    renderer: &ASS_Renderer,
) -> Box<dyn FontProvider> {
    let Some(default_font) = renderer.default_font.as_deref() else {
        return provider;
    };

    let fallback = DefaultFontFileProvider::new(provider, default_font);
    if let Some(default_family) = renderer.default_family.as_deref() {
        Box::new(fallback.with_family(default_family))
    } else {
        Box::new(fallback)
    }
}

fn renderer_config(renderer: &ASS_Renderer, track: &ParsedTrack) -> RendererConfig {
    RendererConfig {
        frame: Size {
            width: if renderer.frame_width > 0 {
                renderer.frame_width
            } else {
                track.play_res_x
            },
            height: if renderer.frame_height > 0 {
                renderer.frame_height
            } else {
                track.play_res_y
            },
        },
        storage: Size {
            width: renderer.storage_width,
            height: renderer.storage_height,
        },
        margins: Margins {
            top: renderer.margins[0],
            bottom: renderer.margins[1],
            left: renderer.margins[2],
            right: renderer.margins[3],
        },
        use_margins: renderer.use_margins,
        pixel_aspect: renderer.pixel_aspect,
        font_scale: renderer.font_scale,
        line_spacing: renderer.line_spacing,
        line_position: renderer.line_position,
        hinting: match renderer.hinting {
            value if value == ass::Hinting::Native as c_int => ass::Hinting::Native,
            value if value == ass::Hinting::Light as c_int => ass::Hinting::Light,
            value if value == ass::Hinting::Normal as c_int => ass::Hinting::Normal,
            _ => ass::Hinting::None,
        },
        shaping: match renderer.shaping {
            value if value == ass::ShapingLevel::Simple as c_int => ass::ShapingLevel::Simple,
            value if value == ass::ShapingLevel::Complex as c_int => ass::ShapingLevel::Complex,
            _ => ass::ShapingLevel::Complex,
        },
    }
}

fn maybe_extract_parsed_fonts(track: *mut ASS_Track, parsed: &ParsedTrack) {
    let Some(track_ref) = (unsafe { track.as_ref() }) else {
        return;
    };
    maybe_extract_fonts_to_library(track_ref.library, &parsed.attachments);
}

fn maybe_extract_fonts_to_library(library: *mut ASS_Library, attachments: &[ParsedAttachment]) {
    let Some(library) = (unsafe { library.as_mut() }) else {
        return;
    };
    if !library.extract_fonts || attachments.is_empty() {
        return;
    }

    for attachment in attachments {
        library.fonts.push(FontAttachment {
            name: attachment.name.clone(),
            data: attachment.data.clone(),
        });
    }
}

fn apply_track_override(track: &mut ASS_Track, key: &str, value: &str) -> bool {
    if key.eq_ignore_ascii_case("PlayResX") {
        track.PlayResX = parse_override_i32(value, track.PlayResX);
    } else if key.eq_ignore_ascii_case("PlayResY") {
        track.PlayResY = parse_override_i32(value, track.PlayResY);
    } else if key.eq_ignore_ascii_case("LayoutResX") {
        track.LayoutResX = parse_override_i32(value, track.LayoutResX);
    } else if key.eq_ignore_ascii_case("LayoutResY") {
        track.LayoutResY = parse_override_i32(value, track.LayoutResY);
    } else if key.eq_ignore_ascii_case("Timer") {
        track.Timer = parse_override_f64(value, track.Timer);
    } else if key.eq_ignore_ascii_case("WrapStyle") {
        track.WrapStyle = parse_override_i32(value, track.WrapStyle);
    } else if key.eq_ignore_ascii_case("ScaledBorderAndShadow") {
        track.ScaledBorderAndShadow =
            parse_override_bool(value, track.ScaledBorderAndShadow != 0) as c_int;
    } else if key.eq_ignore_ascii_case("Kerning") {
        track.Kerning = parse_override_bool(value, track.Kerning != 0) as c_int;
    } else {
        return false;
    }

    true
}

unsafe fn apply_style_override(style: &mut ASS_Style, field_name: &str, value: &str) {
    if field_name.eq_ignore_ascii_case("FontName") {
        replace_string(&mut style.FontName, value);
    } else if field_name.eq_ignore_ascii_case("PrimaryColour") {
        style.PrimaryColour = parse_override_color(value, style.PrimaryColour);
    } else if field_name.eq_ignore_ascii_case("SecondaryColour") {
        style.SecondaryColour = parse_override_color(value, style.SecondaryColour);
    } else if field_name.eq_ignore_ascii_case("OutlineColour") {
        style.OutlineColour = parse_override_color(value, style.OutlineColour);
    } else if field_name.eq_ignore_ascii_case("BackColour") {
        style.BackColour = parse_override_color(value, style.BackColour);
    } else if field_name.eq_ignore_ascii_case("FontSize") {
        style.FontSize = parse_override_f64(value, style.FontSize);
    } else if field_name.eq_ignore_ascii_case("Bold") {
        style.Bold = parse_override_bold(value, ffi_bold_is_active(style.Bold)) as c_int;
    } else if field_name.eq_ignore_ascii_case("Italic") {
        style.Italic = parse_override_bool(value, style.Italic != 0) as c_int;
    } else if field_name.eq_ignore_ascii_case("Underline") {
        style.Underline = parse_override_bool(value, style.Underline != 0) as c_int;
    } else if field_name.eq_ignore_ascii_case("StrikeOut") {
        style.StrikeOut = parse_override_bool(value, style.StrikeOut != 0) as c_int;
    } else if field_name.eq_ignore_ascii_case("Spacing") {
        style.Spacing = parse_override_f64(value, style.Spacing);
    } else if field_name.eq_ignore_ascii_case("Angle") {
        style.Angle = parse_override_f64(value, style.Angle);
    } else if field_name.eq_ignore_ascii_case("BorderStyle") {
        style.BorderStyle = parse_override_i32(value, style.BorderStyle);
    } else if field_name.eq_ignore_ascii_case("Alignment") {
        style.Alignment = parse_override_i32(value, style.Alignment);
    } else if field_name.eq_ignore_ascii_case("Justify") {
        style.Justify = parse_override_i32(value, style.Justify);
    } else if field_name.eq_ignore_ascii_case("MarginL") {
        style.MarginL = parse_override_i32(value, style.MarginL);
    } else if field_name.eq_ignore_ascii_case("MarginR") {
        style.MarginR = parse_override_i32(value, style.MarginR);
    } else if field_name.eq_ignore_ascii_case("MarginV") {
        style.MarginV = parse_override_i32(value, style.MarginV);
    } else if field_name.eq_ignore_ascii_case("Encoding") {
        style.Encoding = parse_override_i32(value, style.Encoding);
    } else if field_name.eq_ignore_ascii_case("ScaleX") {
        style.ScaleX = parse_override_f64(value, style.ScaleX);
    } else if field_name.eq_ignore_ascii_case("ScaleY") {
        style.ScaleY = parse_override_f64(value, style.ScaleY);
    } else if field_name.eq_ignore_ascii_case("Outline") {
        style.Outline = parse_override_f64(value, style.Outline);
    } else if field_name.eq_ignore_ascii_case("Shadow") {
        style.Shadow = parse_override_f64(value, style.Shadow);
    } else if field_name.eq_ignore_ascii_case("Blur") {
        style.Blur = parse_override_f64(value, style.Blur);
    }
}

fn sanitize_size_pair(w: c_int, h: c_int) -> (c_int, c_int) {
    if w <= 0 || h <= 0 || i64::from(w) > i64::from(c_int::MAX) / i64::from(h) {
        (0, 0)
    } else {
        (w, h)
    }
}

fn parse_override_i32(value: &str, default: i32) -> i32 {
    value.trim().parse::<i32>().unwrap_or(default)
}

fn parse_override_f64(value: &str, default: f64) -> f64 {
    value.trim().parse::<f64>().unwrap_or(default)
}

fn parse_override_bool(value: &str, default: bool) -> bool {
    if value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true") {
        true
    } else if value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("false") {
        false
    } else {
        value
            .trim()
            .parse::<i32>()
            .map(|parsed| parsed != 0)
            .unwrap_or(default)
    }
}

fn ffi_bold_is_active(value: c_int) -> bool {
    value == 1 || !(0..700).contains(&value)
}

fn ffi_bold_weight(value: c_int) -> i32 {
    match value {
        0 => 400,
        1 => 700,
        other => other,
    }
}

fn parse_override_bold(value: &str, default: bool) -> bool {
    if value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("true") {
        true
    } else if value.eq_ignore_ascii_case("no") || value.eq_ignore_ascii_case("false") {
        false
    } else {
        value
            .trim()
            .parse::<c_int>()
            .map(ffi_bold_is_active)
            .unwrap_or(default)
    }
}

fn parse_override_color(value: &str, default: u32) -> u32 {
    let trimmed = value.trim();
    let normalized = trimmed
        .strip_prefix("&H")
        .or_else(|| trimmed.strip_prefix("&h"))
        .unwrap_or(trimmed)
        .trim_end_matches('&');

    u32::from_str_radix(normalized, 16)
        .or_else(|_| trimmed.parse::<u32>())
        .unwrap_or(default)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_step_sub(track: *mut ASS_Track, now: i64, movement: c_int) -> i64 {
    let Some(track_ref) = track.as_ref() else {
        return 0;
    };
    if track_ref.events.is_null() || track_ref.n_events <= 0 {
        return 0;
    }

    let events = slice::from_raw_parts(track_ref.events, track_ref.n_events as usize);
    let direction = movement.signum();
    let mut remaining = movement;
    let mut target = now;
    let mut best_start = None;

    loop {
        let mut closest = None;
        let mut closest_time = now;
        for event in events {
            if direction < 0 {
                let end = event.Start.saturating_add(event.Duration);
                if end < target && closest.is_none_or(|_| end > closest_time) {
                    closest = Some(event.Start);
                    closest_time = end;
                }
            } else if direction > 0 {
                let start = event.Start;
                if start > target && closest.is_none_or(|_| start < closest_time) {
                    closest = Some(start);
                    closest_time = start;
                }
            } else {
                let start = event.Start;
                if start < target && closest.is_none_or(|_| start >= closest_time) {
                    closest = Some(start);
                    closest_time = start;
                }
            }
        }

        target = closest_time + i64::from(direction);
        remaining -= direction;
        if let Some(start) = closest {
            best_start = Some(start);
        }
        if remaining == 0 {
            break;
        }
    }

    best_start.map_or(0, |start| start - now)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_malloc(size: usize) -> *mut c_void {
    #[cfg(not(target_arch = "wasm32"))]
    {
        malloc(size)
    }

    #[cfg(target_arch = "wasm32")]
    {
        let mut bytes = Vec::<u8>::with_capacity(size);
        let ptr = bytes.as_mut_ptr();
        std::mem::forget(bytes);
        ptr.cast()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        free(ptr);
    }

    #[cfg(target_arch = "wasm32")]
    {
        let _ = ptr;
    }
}

unsafe fn track_from_parsed(library: *mut ASS_Library, parsed: ParsedTrack) -> *mut ASS_Track {
    let track = ass_new_track(library);
    replace_track_from_parsed(track, parsed);
    track
}

unsafe fn replace_track_from_parsed(track: *mut ASS_Track, parsed: ParsedTrack) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };

    ass_process_force_style(track);
    let library = track_ref.library;
    let parser_priv = track_ref.parser_priv;
    free_track_contents(track_ref);
    *track_ref = build_track(parsed, library, parser_priv);
}

unsafe fn build_track(
    parsed: ParsedTrack,
    library: *mut ASS_Library,
    parser_priv: *mut ASS_ParserPriv,
) -> ASS_Track {
    let mut styles = parsed.styles.iter().map(make_style).collect::<Vec<_>>();
    let mut events = parsed.events.iter().map(make_event).collect::<Vec<_>>();

    let track = ASS_Track {
        n_styles: styles.len() as c_int,
        max_styles: styles.capacity() as c_int,
        n_events: events.len() as c_int,
        max_events: events.capacity() as c_int,
        styles: styles.as_mut_ptr(),
        events: events.as_mut_ptr(),
        style_format: string_to_c_ptr(&parsed.style_format),
        event_format: string_to_c_ptr(&parsed.event_format),
        track_type: parsed.track_type as c_int,
        PlayResX: parsed.play_res_x,
        PlayResY: parsed.play_res_y,
        Timer: parsed.timer,
        WrapStyle: parsed.wrap_style,
        ScaledBorderAndShadow: parsed.scaled_border_and_shadow as c_int,
        Kerning: parsed.kerning as c_int,
        Language: string_to_c_ptr(&parsed.language),
        YCbCrMatrix: parsed.ycbcr_matrix as c_int,
        default_style: parsed.default_style,
        name: ptr::null_mut(),
        library,
        parser_priv,
        LayoutResX: parsed.layout_res_x,
        LayoutResY: parsed.layout_res_y,
    };

    mem::forget(styles);
    mem::forget(events);
    track
}

unsafe fn free_track_contents(track: &mut ASS_Track) {
    for mut style in take_styles(track) {
        free_style(&mut style);
    }
    for mut event in take_events(track) {
        free_event(&mut event);
    }
    free_c_string(&mut track.style_format);
    free_c_string(&mut track.event_format);
    free_c_string(&mut track.Language);
    free_c_string(&mut track.name);
    track.track_type = ass::TrackType::Unknown as c_int;
    track.PlayResX = 384;
    track.PlayResY = 288;
    track.Timer = 100.0;
    track.WrapStyle = 0;
    track.ScaledBorderAndShadow = 1;
    track.Kerning = 1;
    track.YCbCrMatrix = ass::YCbCrMatrix::Default as c_int;
    track.default_style = 0;
    track.LayoutResX = 0;
    track.LayoutResY = 0;
}

unsafe fn track_styles_match_parsed(track: &ASS_Track, parsed: &ParsedTrack) -> bool {
    let current_styles = if track.styles.is_null() || track.n_styles <= 0 {
        Vec::new()
    } else {
        slice::from_raw_parts(track.styles, track.n_styles as usize)
            .iter()
            .map(|style| parsed_style_from_ffi(style))
            .collect::<Vec<_>>()
    };

    current_styles == parsed.styles
        && string_option_from_ptr(track.style_format).unwrap_or_default() == parsed.style_format
        && track.track_type == parsed.track_type as c_int
}

unsafe fn take_styles(track: &mut ASS_Track) -> Vec<ASS_Style> {
    if track.styles.is_null() || track.max_styles <= 0 {
        track.styles = ptr::null_mut();
        track.n_styles = 0;
        track.max_styles = 0;
        Vec::new()
    } else {
        let vec = Vec::from_raw_parts(
            track.styles,
            track.n_styles as usize,
            track.max_styles as usize,
        );
        track.styles = ptr::null_mut();
        track.n_styles = 0;
        track.max_styles = 0;
        vec
    }
}

unsafe fn store_styles(track: &mut ASS_Track, mut styles: Vec<ASS_Style>) {
    invalidate_parsed_track_cache_for_track(track);
    track.n_styles = styles.len() as c_int;
    track.max_styles = styles.capacity() as c_int;
    track.styles = if styles.capacity() == 0 {
        ptr::null_mut()
    } else {
        styles.as_mut_ptr()
    };
    mem::forget(styles);
}

unsafe fn take_events(track: &mut ASS_Track) -> Vec<ASS_Event> {
    if track.events.is_null() || track.max_events <= 0 {
        track.events = ptr::null_mut();
        track.n_events = 0;
        track.max_events = 0;
        Vec::new()
    } else {
        let vec = Vec::from_raw_parts(
            track.events,
            track.n_events as usize,
            track.max_events as usize,
        );
        track.events = ptr::null_mut();
        track.n_events = 0;
        track.max_events = 0;
        vec
    }
}

unsafe fn store_events(track: &mut ASS_Track, mut events: Vec<ASS_Event>) {
    invalidate_parsed_track_cache_for_track(track);
    track.n_events = events.len() as c_int;
    track.max_events = events.capacity() as c_int;
    track.events = if events.capacity() == 0 {
        ptr::null_mut()
    } else {
        events.as_mut_ptr()
    };
    mem::forget(events);
}

unsafe fn free_style(style: &mut ASS_Style) {
    free_c_string(&mut style.Name);
    free_c_string(&mut style.FontName);
}

unsafe fn free_event(event: &mut ASS_Event) {
    free_c_string(&mut event.Name);
    free_c_string(&mut event.Effect);
    free_c_string(&mut event.Text);
}

unsafe fn free_c_string(value: &mut *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(*value));
        *value = ptr::null_mut();
    }
}

unsafe fn replace_string(target: &mut *mut c_char, value: &str) {
    free_c_string(target);
    *target = string_to_c_ptr(value);
}

fn make_style(style: &ParsedStyle) -> ASS_Style {
    ASS_Style {
        Name: string_to_c_ptr(&style.name),
        FontName: string_to_c_ptr(&style.font_name),
        FontSize: style.font_size,
        PrimaryColour: style.primary_colour,
        SecondaryColour: style.secondary_colour,
        OutlineColour: style.outline_colour,
        BackColour: style.back_colour,
        Bold: style.bold as c_int,
        Italic: style.italic as c_int,
        Underline: style.underline as c_int,
        StrikeOut: style.strike_out as c_int,
        ScaleX: style.scale_x,
        ScaleY: style.scale_y,
        Spacing: style.spacing,
        Angle: style.angle,
        BorderStyle: style.border_style,
        Outline: style.outline,
        Shadow: style.shadow,
        Alignment: style.alignment,
        MarginL: style.margin_l,
        MarginR: style.margin_r,
        MarginV: style.margin_v,
        Encoding: style.encoding,
        treat_fontname_as_pattern: style.treat_fontname_as_pattern,
        Blur: style.blur,
        Justify: style.justify,
    }
}

fn make_event(event: &ParsedEvent) -> ASS_Event {
    ASS_Event {
        Start: event.start,
        Duration: event.duration,
        ReadOrder: event.read_order,
        Layer: event.layer,
        Style: event.style,
        Name: string_to_c_ptr(&event.name),
        MarginL: event.margin_l,
        MarginR: event.margin_r,
        MarginV: event.margin_v,
        Effect: string_to_c_ptr(&event.effect),
        Text: string_to_c_ptr(&event.text),
        render_priv: ptr::null_mut(),
    }
}

fn string_to_c_ptr(value: &str) -> *mut c_char {
    let sanitized = value.replace('\0', " ");
    CString::new(sanitized)
        .map(CString::into_raw)
        .unwrap_or(ptr::null_mut())
}

unsafe fn string_option_from_ptr(value: *const c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(string_from_ptr(value))
    }
}

unsafe fn string_from_ptr(value: *const c_char) -> String {
    CStr::from_ptr(value).to_string_lossy().into_owned()
}

unsafe fn track_state_ref(track: *mut ASS_Track) -> Option<&'static TrackState> {
    track.as_ref().and_then(|track| {
        (!track.parser_priv.is_null()).then(|| &*(track.parser_priv as *const TrackState))
    })
}

unsafe fn track_state_mut(track: *mut ASS_Track) -> Option<&'static mut TrackState> {
    let track = track.as_mut()?;
    (!track.parser_priv.is_null()).then_some(&mut *(track.parser_priv as *mut TrackState))
}

unsafe fn invalidate_parsed_track_cache(track: *mut ASS_Track) {
    if let Some(state) = track_state_mut(track) {
        state.parsed_cache_signature = None;
        state.parsed_cache = None;
        state.cache_generation = state.cache_generation.wrapping_add(1);
    }
}

unsafe fn invalidate_parsed_track_cache_for_track(track: &mut ASS_Track) {
    if !track.parser_priv.is_null() {
        let state = &mut *(track.parser_priv as *mut TrackState);
        state.parsed_cache_signature = None;
        state.parsed_cache = None;
        state.cache_generation = state.cache_generation.wrapping_add(1);
    }
}

fn parsed_track_cache_signature(track: &ASS_Track) -> ParsedTrackCacheSignature {
    ParsedTrackCacheSignature {
        n_styles: track.n_styles,
        styles: track.styles as usize,
        n_events: track.n_events,
        events: track.events as usize,
        style_format: track.style_format as usize,
        event_format: track.event_format as usize,
        track_type: track.track_type,
        play_res_x: track.PlayResX,
        play_res_y: track.PlayResY,
        timer_bits: track.Timer.to_bits(),
        wrap_style: track.WrapStyle,
        scaled_border_and_shadow: track.ScaledBorderAndShadow,
        kerning: track.Kerning,
        language: track.Language as usize,
        ycbcr_matrix: track.YCbCrMatrix,
        default_style: track.default_style,
        layout_res_x: track.LayoutResX,
        layout_res_y: track.LayoutResY,
    }
}

unsafe fn cached_parsed_track_from_ffi<'a>(
    track: *mut ASS_Track,
    track_ref: &ASS_Track,
) -> &'a ParsedTrack {
    let signature = parsed_track_cache_signature(track_ref);
    let Some(state) = track_state_mut(track) else {
        panic!("ASS_Track missing parser state");
    };
    if state.parsed_cache_signature != Some(signature) || state.parsed_cache.is_none() {
        state.parsed_cache = Some(parsed_track_from_ffi(track_ref));
        state.parsed_cache_signature = Some(signature);
    }
    state.parsed_cache.as_ref().expect("parsed track cached")
}

unsafe fn active_event_indices(track: *mut ASS_Track, now: i64) -> Vec<usize> {
    let Some(track) = track.as_ref() else {
        return Vec::new();
    };
    if track.events.is_null() || track.n_events <= 0 {
        return Vec::new();
    }

    slice::from_raw_parts(track.events, track.n_events as usize)
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            (now >= event.Start && now < event.Start + event.Duration).then_some(index)
        })
        .collect()
}

fn frame_cache_time_bucket(
    track: &ParsedTrack,
    active_event_indices: &[usize],
    now: i64,
) -> Option<i64> {
    if active_event_indices
        .iter()
        .any(|index| track.events.get(*index).is_none())
    {
        return None;
    }

    if active_events_are_static(track, active_event_indices) {
        return Some(0);
    }

    let bucket_ms = if active_events_have_heavy_animation(track, active_event_indices) {
        APPROXIMATE_HEAVY_ANIMATION_FRAME_BUCKET_MS
    } else {
        APPROXIMATE_ANIMATION_FRAME_BUCKET_MS
    };
    Some(now.div_euclid(bucket_ms))
}

fn active_events_have_heavy_animation(track: &ParsedTrack, active_event_indices: &[usize]) -> bool {
    active_event_indices.iter().any(|index| {
        track
            .events
            .get(*index)
            .is_some_and(|event| event_text_has_heavy_animation(&event.text))
    })
}

fn active_events_are_static(track: &ParsedTrack, active_event_indices: &[usize]) -> bool {
    active_event_indices.iter().all(|index| {
        track.events.get(*index).is_some_and(|event| {
            event_text_is_static(&event.text) && event.effect.trim().is_empty()
        })
    })
}

fn event_text_is_static(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    !(text.contains("\\move")
        || text.contains("\\fad")
        || text.contains("\\fade")
        || text.contains("\\t(")
        || text.contains("\\k")
        || text.contains("\\ko"))
}

fn event_text_has_heavy_animation(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("\\t(")
        || text.contains("\\k")
        || text.contains("\\ko")
        || text.contains("\\clip")
        || text.contains("\\iclip")
}

unsafe fn parsed_track_from_ffi(track: &ASS_Track) -> ParsedTrack {
    let styles = if track.styles.is_null() || track.n_styles <= 0 {
        Vec::new()
    } else {
        slice::from_raw_parts(track.styles, track.n_styles as usize)
            .iter()
            .map(|style| unsafe { parsed_style_from_ffi(style) })
            .collect()
    };

    let events = if track.events.is_null() || track.n_events <= 0 {
        Vec::new()
    } else {
        slice::from_raw_parts(track.events, track.n_events as usize)
            .iter()
            .map(|event| unsafe { parsed_event_from_ffi(event) })
            .collect()
    };

    ParsedTrack {
        styles,
        events,
        attachments: Vec::new(),
        style_format: string_option_from_ptr(track.style_format).unwrap_or_default(),
        event_format: string_option_from_ptr(track.event_format).unwrap_or_default(),
        track_type: match track.track_type {
            value if value == ass::TrackType::Ass as c_int => ass::TrackType::Ass,
            value if value == ass::TrackType::Ssa as c_int => ass::TrackType::Ssa,
            _ => ass::TrackType::Unknown,
        },
        play_res_x: track.PlayResX,
        play_res_y: track.PlayResY,
        timer: track.Timer,
        wrap_style: track.WrapStyle,
        scaled_border_and_shadow: track.ScaledBorderAndShadow != 0,
        kerning: track.Kerning != 0,
        language: string_option_from_ptr(track.Language).unwrap_or_default(),
        ycbcr_matrix: match track.YCbCrMatrix {
            value if value == ass::YCbCrMatrix::None as c_int => ass::YCbCrMatrix::None,
            value if value == ass::YCbCrMatrix::Bt601Tv as c_int => ass::YCbCrMatrix::Bt601Tv,
            value if value == ass::YCbCrMatrix::Bt601Pc as c_int => ass::YCbCrMatrix::Bt601Pc,
            value if value == ass::YCbCrMatrix::Bt709Tv as c_int => ass::YCbCrMatrix::Bt709Tv,
            value if value == ass::YCbCrMatrix::Bt709Pc as c_int => ass::YCbCrMatrix::Bt709Pc,
            value if value == ass::YCbCrMatrix::Smpte240mTv as c_int => {
                ass::YCbCrMatrix::Smpte240mTv
            }
            value if value == ass::YCbCrMatrix::Smpte240mPc as c_int => {
                ass::YCbCrMatrix::Smpte240mPc
            }
            value if value == ass::YCbCrMatrix::FccTv as c_int => ass::YCbCrMatrix::FccTv,
            value if value == ass::YCbCrMatrix::FccPc as c_int => ass::YCbCrMatrix::FccPc,
            value if value == ass::YCbCrMatrix::Unknown as c_int => ass::YCbCrMatrix::Unknown,
            _ => ass::YCbCrMatrix::Default,
        },
        default_style: track.default_style,
        layout_res_x: track.LayoutResX,
        layout_res_y: track.LayoutResY,
    }
}

unsafe fn parsed_style_from_ffi(style: &ASS_Style) -> ParsedStyle {
    ParsedStyle {
        name: string_option_from_ptr(style.Name).unwrap_or_default(),
        font_name: string_option_from_ptr(style.FontName).unwrap_or_default(),
        font_size: style.FontSize,
        primary_colour: style.PrimaryColour,
        secondary_colour: style.SecondaryColour,
        outline_colour: style.OutlineColour,
        back_colour: style.BackColour,
        bold: ffi_bold_is_active(style.Bold),
        font_weight: ffi_bold_weight(style.Bold),
        italic: style.Italic != 0,
        underline: style.Underline != 0,
        strike_out: style.StrikeOut != 0,
        scale_x: style.ScaleX,
        scale_y: style.ScaleY,
        spacing: style.Spacing,
        angle: style.Angle,
        border_style: style.BorderStyle,
        outline: style.Outline,
        shadow: style.Shadow,
        alignment: style.Alignment,
        margin_l: style.MarginL,
        margin_r: style.MarginR,
        margin_v: style.MarginV,
        encoding: style.Encoding,
        treat_fontname_as_pattern: style.treat_fontname_as_pattern,
        blur: style.Blur,
        justify: style.Justify,
    }
}

fn selective_style_overrides_active(renderer: &ASS_Renderer) -> bool {
    renderer.selective_override_style.is_some()
        && renderer.selective_override_bits != ass::override_bits::DEFAULT
}

fn apply_selective_style_overrides(track: &mut ParsedTrack, renderer: &ASS_Renderer) {
    let Some(user_style) = renderer
        .selective_override_style
        .as_ref()
        .map(|style| &style.style)
    else {
        return;
    };

    let mut requested = renderer.selective_override_bits;
    if requested == ass::override_bits::DEFAULT {
        return;
    }

    if requested & ass::override_bits::STYLE != 0 {
        requested |= ass::override_bits::FONT_NAME
            | ass::override_bits::FONT_SIZE_FIELDS
            | ass::override_bits::COLORS
            | ass::override_bits::BORDER
            | ass::override_bits::ATTRIBUTES;
    }

    for style in &mut track.styles {
        if requested & ass::override_bits::FULL_STYLE != 0 {
            *style = user_style.clone();
            continue;
        }

        if requested & ass::override_bits::FONT_NAME != 0 {
            style.font_name = user_style.font_name.clone();
            style.treat_fontname_as_pattern = user_style.treat_fontname_as_pattern;
        }
        if requested & ass::override_bits::FONT_SIZE_FIELDS != 0 {
            style.font_size = user_style.font_size;
            style.spacing = user_style.spacing;
            style.scale_x = user_style.scale_x;
            style.scale_y = user_style.scale_y;
        }
        if requested & ass::override_bits::COLORS != 0 {
            style.primary_colour = user_style.primary_colour;
            style.secondary_colour = user_style.secondary_colour;
            style.outline_colour = user_style.outline_colour;
            style.back_colour = user_style.back_colour;
        }
        if requested & ass::override_bits::ATTRIBUTES != 0 {
            style.bold = user_style.bold;
            style.italic = user_style.italic;
            style.underline = user_style.underline;
            style.strike_out = user_style.strike_out;
        }
        if requested & ass::override_bits::BORDER != 0 {
            style.border_style = user_style.border_style;
            style.outline = user_style.outline;
            style.shadow = user_style.shadow;
        }
        if requested & ass::override_bits::ALIGNMENT != 0 {
            style.alignment = user_style.alignment;
        }
        if requested & ass::override_bits::MARGINS != 0 {
            style.margin_l = user_style.margin_l;
            style.margin_r = user_style.margin_r;
            style.margin_v = user_style.margin_v;
        }
        if requested & ass::override_bits::JUSTIFY != 0 {
            style.justify = user_style.justify;
        }
        if requested & ass::override_bits::BLUR != 0 {
            style.blur = user_style.blur;
        }
    }
}

unsafe fn parsed_event_from_ffi(event: &ASS_Event) -> ParsedEvent {
    ParsedEvent {
        start: event.Start,
        duration: event.Duration,
        read_order: event.ReadOrder,
        layer: event.Layer,
        style: event.Style,
        name: string_option_from_ptr(event.Name).unwrap_or_default(),
        margin_l: event.MarginL,
        margin_r: event.MarginR,
        margin_v: event.MarginV,
        effect: string_option_from_ptr(event.Effect).unwrap_or_default(),
        text: string_option_from_ptr(event.Text).unwrap_or_default(),
    }
}
