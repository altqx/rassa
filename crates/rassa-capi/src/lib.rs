#![allow(
    dead_code,
    clippy::missing_safety_doc,
    clippy::vec_box,
    non_camel_case_types,
    non_snake_case,
    unsafe_op_in_unsafe_fn
)]

use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    ffi::{CStr, CString, c_char, c_double, c_int, c_void},
    fs,
    hash::{Hash, Hasher},
    mem, ptr, slice,
    sync::{Mutex, OnceLock},
};

#[cfg(target_arch = "wasm32")]
use std::alloc::{Layout, alloc, dealloc};

#[cfg(not(target_arch = "wasm32"))]
use libc::{free, malloc};
use rassa_core::{ImagePlane, Margins, RendererConfig, Size, ass};
#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
use rassa_fonts::validate_fontconfig_config;
use rassa_fonts::{
    AttachedFontProvider, CrossfontProvider, DefaultFontFileProvider, DirectoryFontProvider,
    FontAttachment as ProviderFontAttachment, FontProvider, MergedFontProvider, NullFontProvider,
};
use rassa_parse::{
    ParsedAttachment, ParsedEvent, ParsedStyle, ParsedTrack, parse_dialogue_text,
    parse_script_bytes, parse_script_bytes_with_codepage, parse_script_text,
    parse_style_section_bytes_with_codepage,
};
use rassa_raster::{RasterCacheLimits, RasterCacheScope, Rasterizer};
use rassa_render::RenderEngine;

const MESSAGE_LEVEL_ERROR: c_int = 1;
const MESSAGE_LEVEL_WARNING: c_int = 2;

unsafe extern "C" {
    fn rassa_emit_message(
        callback: *mut c_void,
        level: c_int,
        message: *const c_char,
        data: *mut c_void,
    );
}

#[cfg(all(target_arch = "wasm32", rassa_wasm_message_callback_test))]
unsafe extern "C" {
    #[link_name = "rassa_formatted_sink_callback_pointer"]
    fn wasm_formatted_sink_callback_pointer() -> *mut c_void;
}

#[cfg(all(target_arch = "wasm32", rassa_wasm_message_callback_test))]
#[repr(C)]
struct WasmFormattedMessageBridge {
    sink: unsafe extern "C" fn(c_int, *const c_char, *mut c_void),
    data: *mut c_void,
}

#[cfg(all(target_arch = "wasm32", rassa_wasm_message_callback_test))]
#[derive(Default)]
struct WasmMessageCallbackTestState {
    calls: u32,
    level: c_int,
    message_matches: bool,
}

#[cfg(all(target_arch = "wasm32", rassa_wasm_message_callback_test))]
unsafe extern "C" fn capture_wasm_formatted_message(
    level: c_int,
    message: *const c_char,
    data: *mut c_void,
) {
    if message.is_null() || data.is_null() {
        return;
    }
    let state = &mut *data.cast::<WasmMessageCallbackTestState>();
    state.calls = state.calls.saturating_add(1);
    state.level = level;
    state.message_matches = CStr::from_ptr(message).to_bytes() == b"wasm callback lifecycle";
}

unsafe fn emit_library_message(library: *mut ASS_Library, level: c_int, message: impl AsRef<str>) {
    let Some(library) = library.as_ref() else {
        return;
    };
    let callback = library.message_cb;
    let callback_data = library.message_data;
    let message = message.as_ref().replace('\0', " ");
    if callback.is_null() {
        if level < 5 {
            eprintln!("[ass] {message}");
        }
        return;
    }

    if let Ok(message) = CString::new(message) {
        rassa_emit_message(callback, level, message.as_ptr(), callback_data);
    }
}

/// Test-only wasm C-callback probe; compiled only with the explicit test cfg.
#[cfg(all(target_arch = "wasm32", rassa_wasm_message_callback_test))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rassa_wasm_message_callback_lifecycle_test() -> c_int {
    let library = ass_library_init();
    if library.is_null() {
        return 0;
    }

    let mut state = WasmMessageCallbackTestState::default();
    let mut bridge = WasmFormattedMessageBridge {
        sink: capture_wasm_formatted_message,
        data: (&mut state as *mut WasmMessageCallbackTestState).cast(),
    };
    ass_set_message_cb(
        library,
        wasm_formatted_sink_callback_pointer(),
        (&mut bridge as *mut WasmFormattedMessageBridge).cast(),
    );
    emit_library_message(library, MESSAGE_LEVEL_WARNING, "wasm callback lifecycle");
    ass_library_done(library);

    c_int::from(state.calls == 1 && state.level == MESSAGE_LEVEL_WARNING && state.message_matches)
}

pub struct ASS_Library {
    fonts_dir: Option<String>,
    extract_fonts: bool,
    style_overrides: Vec<String>,
    message_cb: *mut c_void,
    message_data: *mut c_void,
    fonts: Vec<FontAttachment>,
}

pub struct ASS_Renderer {
    library: *mut ASS_Library,
    render_engine: RenderEngine,
    raster_cache_namespace: u64,
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
    fontselect_initialized: bool,
    selective_override_bits: c_int,
    selective_override_style: Option<OwnedStyleOverride>,
    cache_limits: RasterCacheLimits,
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
            FontSize: 0.0,
            PrimaryColour: 0,
            SecondaryColour: 0,
            OutlineColour: 0,
            BackColour: 0,
            Bold: 0,
            Italic: 0,
            Underline: 0,
            StrikeOut: 0,
            ScaleX: 0.0,
            ScaleY: 0.0,
            Spacing: 0.0,
            Angle: 0.0,
            BorderStyle: 0,
            Outline: 0.0,
            Shadow: 0.0,
            Alignment: 0,
            MarginL: 0,
            MarginR: 0,
            MarginV: 0,
            Encoding: 0,
            treat_fontname_as_pattern: 0,
            Blur: 0.0,
            Justify: 0,
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
    read_order_seen: Option<HashSet<c_int>>,
    prune_delay: Option<i64>,
    parser_state: CapiParserState,
    cache_generation: u64,
    parsed_cache_signature: Option<ParsedTrackCacheSignature>,
    parsed_cache: Option<ParsedTrack>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum CapiParserState {
    #[default]
    Unknown,
    Info,
    Styles,
    Events,
    Fonts,
}

#[derive(Clone, Copy, Debug, Default)]
struct CapiScriptInfoPresence {
    play_res_x: bool,
    play_res_y: bool,
    play_res_x_value: c_int,
    play_res_y_value: c_int,
    timer: bool,
    kerning: bool,
    language: bool,
}

impl CapiScriptInfoPresence {
    fn any(self) -> bool {
        self.play_res_x || self.play_res_y || self.timer || self.kerning || self.language
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CapiFormatPresence {
    explicit_style: bool,
    fallback_style: bool,
    explicit_event: bool,
    fallback_event: bool,
}

impl CapiFormatPresence {
    fn any(self) -> bool {
        self.explicit_style || self.fallback_style || self.explicit_event || self.fallback_event
    }

    fn should_keep_style(self, existing_format: bool) -> bool {
        self.explicit_style || self.fallback_style && !existing_format
    }

    fn should_keep_event(self, existing_format: bool) -> bool {
        self.explicit_event || self.fallback_event && !existing_format
    }
}

const ASS_STYLES_ALLOC: usize = 20;
const LIBASS_MAX_READ_ORDER_ID: c_int = 10 * 1024 * 1024 * 8;

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
    content_fingerprint: u64,
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
    library_fonts_dir: Option<String>,
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
    frame_time_key: i64,
}

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

fn owned_image_lists_detect_change(
    previous: Option<&OwnedImageList>,
    current: &OwnedImageList,
) -> c_int {
    let Some(previous_list) = previous else {
        return if current.nodes.is_empty() { 0 } else { 2 };
    };

    if previous_list.nodes.len() != current.nodes.len() {
        return 2;
    }

    let mut diff = 0;
    for (index, (previous_node, current_node)) in previous_list
        .nodes
        .iter()
        .zip(current.nodes.iter())
        .enumerate()
    {
        if previous_node.w != current_node.w
            || previous_node.h != current_node.h
            || previous_node.stride != current_node.stride
            || previous_node.color != current_node.color
            || previous_node.type_ != current_node.type_
            || previous_list.bitmaps.get(index) != current.bitmaps.get(index)
        {
            return 2;
        }

        if previous_node.dst_x != current_node.dst_x || previous_node.dst_y != current_node.dst_y {
            diff = 1;
        }
    }

    diff
}

fn render_frame_planes(
    parsed: &ParsedTrack,
    renderer: &mut ASS_Renderer,
    library: *mut ASS_Library,
    now: i64,
    renderer_config: &RendererConfig,
) -> Vec<ImagePlane> {
    let _cache_scope =
        RasterCacheScope::enter(renderer.raster_cache_namespace, renderer.cache_limits);
    let provider = cached_font_provider(renderer, library);
    let provider: &dyn FontProvider = unsafe { &*provider };
    renderer
        .render_engine
        .render_frame_with_provider_and_config(parsed, &provider, now, renderer_config)
}

fn ass_color_to_rgba(color: u32) -> u32 {
    let alpha = (color >> 24) & 0xff;
    let blue = (color >> 16) & 0xff;
    let green = (color >> 8) & 0xff;
    let red = color & 0xff;
    (red << 24) | (green << 16) | (blue << 8) | alpha
}

fn rgba_to_ass_color(color: u32) -> u32 {
    ass_color_to_rgba(color)
}

impl OwnedStyleOverride {
    unsafe fn from_ffi(style: *mut ASS_Style) -> Option<Self> {
        let style = style.as_ref()?;
        Some(Self {
            style: parsed_style_from_ffi(style),
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
            library: ptr::null_mut(),
            render_engine: RenderEngine::new(),
            raster_cache_namespace: next_raster_cache_namespace(),
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
            fontselect_initialized: false,
            selective_override_bits: ass::override_bits::SELECTIVE_FONT_SCALE,
            selective_override_style: None,
            cache_limits: RasterCacheLimits::default(),
            font_provider_cache: None,
            frame_cache_signature: None,
            last_timestamp: None,
            last_active_count: 0,
            rendered_images: None,
        }
    }
}

impl Drop for ASS_Renderer {
    fn drop(&mut self) {
        Rasterizer::clear_cache_namespace(self.raster_cache_namespace);
    }
}

fn next_raster_cache_namespace() -> u64 {
    static NEXT_NAMESPACE: OnceLock<Mutex<u64>> = OnceLock::new();
    let mut next = NEXT_NAMESPACE
        .get_or_init(|| Mutex::new(1))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let namespace = *next;
    *next = next.wrapping_add(1).max(1);
    namespace
}

fn raster_cache_limits_from_c(glyph_max: c_int, bitmap_max_size: c_int) -> RasterCacheLimits {
    let defaults = RasterCacheLimits::default();
    RasterCacheLimits {
        // libass assigns a nonzero int to size_t; negatives mean unlimited.
        glyph_max: if glyph_max == 0 {
            defaults.glyph_max
        } else {
            glyph_max as usize
        },
        bitmap_max_bytes: if bitmap_max_size == 0 {
            defaults.bitmap_max_bytes
        } else {
            (bitmap_max_size as usize).saturating_mul(1024 * 1024)
        },
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
        let Some((key, value)) = override_entry.rsplit_once('=') else {
            continue;
        };
        if key.is_empty() {
            continue;
        }

        if apply_track_override(track_ref, key, value) {
            continue;
        }

        let (style_name, field_name) = match key.rsplit_once('.') {
            Some((style_name, field_name)) => (Some(style_name), field_name),
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
        if !msg_cb.is_null() {
            library.message_cb = msg_cb;
            library.message_data = data;
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_renderer_init(library: *mut ASS_Library) -> *mut ASS_Renderer {
    let mut renderer = ASS_Renderer::default();
    renderer.library = library;
    Box::into_raw(Box::new(renderer))
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
    ass_set_pixel_aspect(priv_, dar / sar);
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

#[cfg(target_os = "macos")]
const AVAILABLE_FONT_PROVIDERS: &[c_int] = &[
    ass::DefaultFontProvider::None as c_int,
    ass::DefaultFontProvider::Autodetect as c_int,
    ass::DefaultFontProvider::CoreText as c_int,
];

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
const AVAILABLE_FONT_PROVIDERS: &[c_int] = &[
    ass::DefaultFontProvider::None as c_int,
    ass::DefaultFontProvider::Autodetect as c_int,
    ass::DefaultFontProvider::Fontconfig as c_int,
];

#[cfg(windows)]
const AVAILABLE_FONT_PROVIDERS: &[c_int] = &[
    ass::DefaultFontProvider::None as c_int,
    ass::DefaultFontProvider::Autodetect as c_int,
    ass::DefaultFontProvider::DirectWrite as c_int,
];

#[cfg(all(
    not(target_os = "macos"),
    not(windows),
    any(not(unix), target_arch = "wasm32")
))]
const AVAILABLE_FONT_PROVIDERS: &[c_int] = &[
    ass::DefaultFontProvider::None as c_int,
    ass::DefaultFontProvider::Autodetect as c_int,
];

fn system_font_provider_is_available(provider: c_int) -> bool {
    if cfg!(target_arch = "wasm32") {
        return false;
    }
    if provider == ass::DefaultFontProvider::Autodetect as c_int {
        return true;
    }

    #[cfg(target_os = "macos")]
    if provider == ass::DefaultFontProvider::CoreText as c_int {
        return true;
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    if provider == ass::DefaultFontProvider::Fontconfig as c_int {
        return true;
    }
    #[cfg(windows)]
    if provider == ass::DefaultFontProvider::DirectWrite as c_int {
        return true;
    }

    false
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

    let allocation_size = mem::size_of_val(AVAILABLE_FONT_PROVIDERS);
    let allocation = ass_malloc(allocation_size) as *mut c_int;
    if allocation.is_null() {
        *providers = ptr::null_mut();
        *size = usize::MAX;
        return;
    }

    ptr::copy_nonoverlapping(
        AVAILABLE_FONT_PROVIDERS.as_ptr(),
        allocation,
        AVAILABLE_FONT_PROVIDERS.len(),
    );
    *providers = allocation;
    *size = AVAILABLE_FONT_PROVIDERS.len();
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
        renderer.fontselect_initialized = true;
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
        renderer.cache_limits = raster_cache_limits_from_c(glyph_max, bitmap_max_size);
        Rasterizer::set_cache_limits(renderer.raster_cache_namespace, renderer.cache_limits);
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

    let Some(pre_init_track_ref) = track.as_ref() else {
        let empty_images = OwnedImageList::default();
        if let Some(detect_change) = detect_change.as_mut() {
            *detect_change =
                owned_image_lists_detect_change(renderer.rendered_images.as_ref(), &empty_images);
        }
        renderer.rendered_images = None;
        renderer.frame_cache_signature = None;
        return ptr::null_mut();
    };

    if renderer.frame_width == 0 && renderer.frame_height == 0 {
        if let Some(detect_change) = detect_change.as_mut() {
            *detect_change = 2;
        }
        return ptr::null_mut();
    }
    if !renderer.fontselect_initialized {
        if let Some(detect_change) = detect_change.as_mut() {
            *detect_change = 2;
        }
        return ptr::null_mut();
    }
    if renderer.library != pre_init_track_ref.library {
        if let Some(detect_change) = detect_change.as_mut() {
            *detect_change = 2;
        }
        return ptr::null_mut();
    }
    if pre_init_track_ref.n_events == 0 {
        if let Some(detect_change) = detect_change.as_mut() {
            *detect_change = 2;
        }
        return ptr::null_mut();
    }

    ass_lazy_track_init(track);
    let active_event_indices = active_event_indices(track, now);
    let Some(track_ref) = track.as_ref() else {
        return ptr::null_mut();
    };

    let parsed_track_signature = parsed_track_cache_signature(track_ref, &active_event_indices);
    let cached = cached_parsed_track_from_ffi(track, track_ref, parsed_track_signature);
    let override_active = selective_style_overrides_active(renderer);
    let parsed_with_overrides;
    let parsed = if override_active {
        let mut parsed = cached.clone();
        apply_selective_style_overrides(&mut parsed, renderer, &active_event_indices);
        parsed_with_overrides = parsed;
        &parsed_with_overrides
    } else {
        cached
    };
    let track_features = track_state_ref(track)
        .map(|state| state.features)
        .unwrap_or_default();
    let renderer_config = renderer_config(
        renderer,
        parsed,
        track_features[1], // ASS_FEATURE_BIDI_BRACKETS
        track_features[2], // ASS_FEATURE_WHOLE_TEXT_LAYOUT
        track_features[3], // ASS_FEATURE_WRAP_UNICODE
    );
    let font_provider_signature = font_provider_cache_signature(renderer, track_ref.library);
    let track_generation = track_state_ref(track)
        .map(|state| state.cache_generation)
        .unwrap_or_default();
    let frame_time_key = frame_cache_time_key(parsed, &active_event_indices, now);
    let frame_cache_signature = frame_time_key.map(|frame_time_key| RenderedFrameCacheSignature {
        track: track as usize,
        track_generation,
        parsed_track: parsed_track_signature,
        renderer_config: renderer_config.clone(),
        font_provider: font_provider_signature,
        selective_override_bits: renderer.selective_override_bits,
        selective_override_style: renderer.selective_override_style.clone(),
        active_event_indices: active_event_indices.clone(),
        frame_time_key,
    });
    if frame_cache_signature.is_some()
        && renderer.frame_cache_signature == frame_cache_signature
        && renderer.rendered_images.is_some()
    {
        if let Some(detect_change) = detect_change.as_mut() {
            *detect_change = 0;
        }
        let head = renderer
            .rendered_images
            .as_mut()
            .map(OwnedImageList::head_ptr)
            .unwrap_or(ptr::null_mut());
        prune_configured_events_after_successful_render(track, now);
        return head;
    }

    let planes = render_frame_planes(parsed, renderer, track_ref.library, now, &renderer_config);
    let rendered_images = OwnedImageList::from_planes(planes);
    if let Some(detect_change) = detect_change.as_mut() {
        *detect_change =
            owned_image_lists_detect_change(renderer.rendered_images.as_ref(), &rendered_images);
    }
    renderer.rendered_images = Some(rendered_images);
    renderer.frame_cache_signature = frame_cache_signature;
    let head = renderer
        .rendered_images
        .as_mut()
        .map(OwnedImageList::head_ptr)
        .unwrap_or(ptr::null_mut());
    prune_configured_events_after_successful_render(track, now);
    head
}

unsafe fn prune_configured_events_after_successful_render(track: *mut ASS_Track, now: i64) {
    if let Some(delay) = track_state_mut(track).and_then(|state| state.prune_delay) {
        ass_prune_events(track, now - delay);
    }
}

unsafe fn ass_lazy_track_init(track: *mut ASS_Track) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    if track_ref.PlayResX > 0 && track_ref.PlayResY > 0 {
        return;
    }

    let old_play_res = (track_ref.PlayResX, track_ref.PlayResY);
    if track_ref.PlayResX <= 0 && track_ref.PlayResY <= 0 {
        track_ref.PlayResX = 384;
        track_ref.PlayResY = 288;
    } else if track_ref.PlayResY <= 0 {
        if track_ref.PlayResX == 1280 {
            track_ref.PlayResY = 1024;
        } else {
            let play_res_x = track_ref.PlayResX as u32;
            track_ref.PlayResY = ((play_res_x - 1) - (play_res_x - 1) / 4).max(1) as c_int;
        }
    } else if track_ref.PlayResX <= 0 {
        if track_ref.PlayResY == 1024 {
            track_ref.PlayResX = 1280;
        } else {
            track_ref.PlayResX = (i64::from(track_ref.PlayResY) + i64::from(track_ref.PlayResY) / 3)
                .min(i64::from(c_int::MAX)) as c_int;
        }
    }

    if old_play_res != (track_ref.PlayResX, track_ref.PlayResY) {
        invalidate_parsed_track_cache_for_track(track_ref);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_new_track(library: *mut ASS_Library) -> *mut ASS_Track {
    let state = Box::new(TrackState {
        check_readorder: true,
        ..TrackState::default()
    });
    let parser_priv = Box::into_raw(state) as *mut ASS_ParserPriv;
    let mut styles = Vec::with_capacity(ASS_STYLES_ALLOC);
    styles.push(make_builtin_default_style());
    let track = ASS_Track {
        n_styles: styles.len() as c_int,
        max_styles: styles.capacity() as c_int,
        n_events: 0,
        max_events: 0,
        styles: styles.as_mut_ptr(),
        events: ptr::null_mut(),
        style_format: ptr::null_mut(),
        event_format: ptr::null_mut(),
        track_type: ass::TrackType::Unknown as c_int,
        PlayResX: 0,
        PlayResY: 0,
        Timer: 0.0,
        WrapStyle: 0,
        ScaledBorderAndShadow: 0,
        Kerning: 0,
        Language: ptr::null_mut(),
        YCbCrMatrix: ass::YCbCrMatrix::Default as c_int,
        default_style: 0,
        name: ptr::null_mut(),
        library,
        parser_priv,
        LayoutResX: 0,
        LayoutResY: 0,
    };

    mem::forget(styles);
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
    let enabled = enable != 0;
    match feature {
        0 => {
            state.features[1] = enabled;
            state.features[2] = enabled;
            state.features[3] = enabled;
            0
        }
        1..=3 => {
            state.features[feature as usize] = enabled;
            0
        }
        _ => -1,
    }
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
    let id = push_style_libass(&mut styles, ASS_Style::default());
    store_styles(track_ref, styles);
    id
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_alloc_event(track: *mut ASS_Track) -> c_int {
    let Some(track_ref) = track.as_mut() else {
        return -1;
    };
    let mut events = take_events(track_ref);
    let id = push_event_libass(&mut events, ASS_Event::default());
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
    let mut parsed = match parse_script_bytes(bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            emit_library_message(
                (*track).library,
                MESSAGE_LEVEL_WARNING,
                format!("Unable to parse subtitle data: {error:?}"),
            );
            return;
        }
    };
    let parser_state = track_state_ref(track)
        .map(|state| state.parser_state)
        .unwrap_or_default();
    let script_info_presence = capi_script_info_presence_from_bytes(bytes);
    let format_presence = capi_format_presence_from_bytes(bytes, parser_state);
    apply_capi_script_info_presence_to_parsed(&mut parsed, script_info_presence);
    zero_process_data_read_orders(&mut parsed);
    if parser_state == CapiParserState::Fonts {
        maybe_extract_fonts_from_font_state_bytes(track, bytes);
    }
    maybe_extract_parsed_fonts(track, &parsed);
    if parsed.styles.len() <= 1
        && parsed.events.is_empty()
        && parser_state == CapiParserState::Info
        && merge_info_from_sectionless_bytes(track, bytes)
    {
    } else if track.as_ref().is_some_and(track_is_pristine) {
        replace_track_from_parsed(track, parsed, script_info_presence, format_presence);
    } else if parsed.events.is_empty() && parser_state == CapiParserState::Events {
        if let Some(mut parsed_events) = parse_event_section_from_existing_track(track, bytes) {
            zero_process_data_read_orders(&mut parsed_events);
            merge_event_format_from_parsed(track, &parsed_events, format_presence);
            append_events_from_parsed(track, &parsed_events.events);
        } else {
            merge_script_info_from_bytes(track, bytes);
            merge_parsed_into_track(track, parsed, format_presence);
        }
    } else if parsed.styles.len() <= 1
        && parsed.events.is_empty()
        && parser_state == CapiParserState::Styles
    {
        if let Some(parsed_styles) = parse_style_section_from_existing_track(track, bytes) {
            merge_styles_from_parsed(track, parsed_styles, format_presence);
        } else {
            merge_script_info_from_bytes(track, bytes);
            merge_parsed_into_track(track, parsed, format_presence);
        }
    } else {
        merge_script_info_from_bytes(track, bytes);
        merge_parsed_into_track(track, parsed, format_presence);
    }
    update_parser_state_from_bytes(track, bytes);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_process_codec_private(
    track: *mut ASS_Track,
    data: *const c_char,
    size: c_int,
) {
    ass_process_data(track, data, size);
    ensure_event_format_fallback(track);
    ass_process_force_style(track);
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
    let has_event_format =
        string_option_from_ptr(track_ref.event_format).is_some_and(|format| !format.is_empty());
    let Some(event) = parse_chunk_event(track, track_ref, &text, timecode, duration) else {
        if has_event_format {
            reserve_failed_chunk_event_slot(track_ref);
        }
        return;
    };

    let mut events = take_events(track_ref);
    let event_format = string_option_from_ptr(track_ref.event_format).unwrap_or_default();
    push_event_libass(&mut events, make_event(&event, &event_format));
    store_events(track_ref, events);
}

unsafe fn reserve_failed_chunk_event_slot(track: &mut ASS_Track) {
    let mut events = take_events(track);
    let index = push_event_libass(&mut events, ASS_Event::default());
    if let Some(event) = events.get_mut(index as usize) {
        free_event(event);
    }
    events.pop();
    store_events(track, events);
}

unsafe fn parse_chunk_event(
    track_ptr: *mut ASS_Track,
    track: &ASS_Track,
    text: &str,
    timecode: i64,
    duration: i64,
) -> Option<ParsedEvent> {
    let event_format = string_option_from_ptr(track.event_format)?;
    if event_format.is_empty() {
        return None;
    }

    let mut payload = text;
    let read_order = parse_c_decimal_i32(next_chunk_token(&mut payload, false)?);
    if chunk_read_order_is_duplicate(track_ptr, track, read_order) {
        return None;
    }
    let layer = parse_libass_header_i32(next_chunk_token(&mut payload, false)?);

    let mut format = event_format.as_str();
    for _ in 0..3 {
        next_chunk_token(&mut format, true)?;
    }

    let mut event = ParsedEvent {
        start: timecode,
        duration,
        read_order,
        layer,
        ..ParsedEvent::default()
    };

    while let Some(field) = next_chunk_token(&mut format, true) {
        if field.eq_ignore_ascii_case("Text") {
            event.text = payload.trim_end_matches(['\r', '\t', ' ']).to_string();
            return Some(event);
        }

        let value = next_chunk_token(&mut payload, false)?;
        if field.eq_ignore_ascii_case("Style") {
            event.style = lookup_chunk_style(track, value);
        } else if field.eq_ignore_ascii_case("Name") || field.eq_ignore_ascii_case("Actor") {
            event.name = value.to_string();
        } else if field.eq_ignore_ascii_case("Effect") {
            event.effect = value.to_string();
        } else if field.eq_ignore_ascii_case("Layer") {
            event.layer = parse_libass_header_i32(value);
        } else if field.eq_ignore_ascii_case("MarginL") {
            event.margin_l = parse_libass_header_i32(value);
        } else if field.eq_ignore_ascii_case("MarginR") {
            event.margin_r = parse_libass_header_i32(value);
        } else if field.eq_ignore_ascii_case("MarginV") {
            event.margin_v = parse_libass_header_i32(value);
        }
    }

    None
}

unsafe fn chunk_read_order_is_duplicate(
    track_ptr: *mut ASS_Track,
    track: &ASS_Track,
    read_order: c_int,
) -> bool {
    let Some(state) = track_state_mut(track_ptr) else {
        return chunk_read_order_exists(track, read_order);
    };
    if !state.check_readorder {
        return false;
    }

    if state.read_order_seen.is_none() {
        state.read_order_seen = build_read_order_seen(track);
    }

    let Some(seen) = state.read_order_seen.as_mut() else {
        return chunk_read_order_exists(track, read_order);
    };

    if !libass_read_order_id_is_valid(read_order) {
        state.read_order_seen = None;
        return false;
    }

    if seen.contains(&read_order) {
        return true;
    }
    seen.insert(read_order);
    false
}

unsafe fn build_read_order_seen(track: &ASS_Track) -> Option<HashSet<c_int>> {
    let mut seen = HashSet::new();
    if !track.events.is_null() && track.n_events > 0 {
        for event in slice::from_raw_parts(track.events, track.n_events as usize) {
            if !libass_read_order_id_is_valid(event.ReadOrder) {
                return None;
            }
            seen.insert(event.ReadOrder);
        }
    }
    Some(seen)
}

fn libass_read_order_id_is_valid(read_order: c_int) -> bool {
    (0..LIBASS_MAX_READ_ORDER_ID).contains(&read_order)
}

unsafe fn chunk_read_order_exists(track: &ASS_Track, read_order: c_int) -> bool {
    if track.events.is_null() || track.n_events <= 0 {
        return false;
    }

    slice::from_raw_parts(track.events, track.n_events as usize)
        .iter()
        .any(|event| event.ReadOrder == read_order)
}

fn next_chunk_token<'a>(input: &mut &'a str, rtrim: bool) -> Option<&'a str> {
    *input = input.trim_start_matches([' ', '\t']);
    if input.is_empty() {
        return None;
    }

    let (token, rest) = input
        .split_once(',')
        .map_or((*input, ""), |(token, rest)| (token, rest));
    *input = rest;
    Some(if rtrim {
        token.trim_end_matches([' ', '\t'])
    } else {
        token
    })
}

fn parse_c_decimal_i32(value: &str) -> c_int {
    parse_signed_i32(value.trim_start_matches(is_force_c_space), 10, false)
}

fn parse_libass_header_i32(value: &str) -> c_int {
    let (value, base, allow_hex_prefix) = if let Some(rest) = value
        .strip_prefix("&H")
        .or_else(|| value.strip_prefix("&h"))
        .or_else(|| value.strip_prefix("0x"))
        .or_else(|| value.strip_prefix("0X"))
    {
        (rest, 16, false)
    } else {
        (value, 10, true)
    };
    parse_signed_i32(value, base, allow_hex_prefix)
}

fn parse_signed_i32(value: &str, base: u32, allow_hex_prefix: bool) -> c_int {
    let mut value = value.trim_start_matches([' ', '\t']);
    let mut negative = false;
    if let Some(rest) = value.strip_prefix('+') {
        value = rest;
    } else if let Some(rest) = value.strip_prefix('-') {
        value = rest;
        negative = true;
    }

    if allow_hex_prefix && base == 16 {
        if let Some(rest) = value
            .strip_prefix("0x")
            .or_else(|| value.strip_prefix("0X"))
        {
            value = rest;
        }
    }

    let mut parsed = 0_u32;
    let mut found_digit = false;
    for byte in value.bytes() {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a' + 10),
            b'A'..=b'F' => u32::from(byte - b'A' + 10),
            _ => break,
        };
        if digit >= base {
            break;
        }
        parsed = parsed.wrapping_mul(base).wrapping_add(digit);
        found_digit = true;
    }

    if !found_digit {
        return 0;
    }
    if negative {
        parsed = 0_u32.wrapping_sub(parsed);
    }
    parsed as c_int
}

unsafe fn lookup_chunk_style(track: &ASS_Track, value: &str) -> c_int {
    let mut name = value.trim_start_matches('*');
    if name.eq_ignore_ascii_case("Default") {
        name = "Default";
    }

    if !track.styles.is_null() && track.n_styles > 0 {
        let styles = slice::from_raw_parts(track.styles, track.n_styles as usize);
        for (index, style) in styles.iter().enumerate().rev() {
            if string_option_from_ptr(style.Name).as_deref() == Some(name) {
                return index as c_int;
            }
        }
    }

    track.default_style
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

    let clear_read_order = track_state_ref(track)
        .map(|state| state.check_readorder)
        .unwrap_or(true);
    let mut removed_read_orders = Vec::new();
    let mut events = take_events(track_ref);
    events.retain_mut(|event| {
        let keep = event.Start + event.Duration >= deadline;
        if !keep {
            if clear_read_order {
                removed_read_orders.push(event.ReadOrder);
            }
            free_event(event);
        }
        keep
    });
    store_events(track_ref, events);
    if clear_read_order {
        if let Some(state) = track_state_mut(track) {
            if let Some(seen) = state.read_order_seen.as_mut() {
                for read_order in removed_read_orders {
                    seen.remove(&read_order);
                }
            }
        }
    }
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
    events.clear();
    store_events(track_ref, events);
    if let Some(state) = track_state_mut(track) {
        state.read_order_seen = None;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_read_file(
    library: *mut ASS_Library,
    fname: *const c_char,
    codepage: *const c_char,
) -> *mut ASS_Track {
    let Some(path) = string_option_from_ptr(fname) else {
        emit_library_message(
            library,
            MESSAGE_LEVEL_WARNING,
            "ass_read_file: filename is NULL",
        );
        return ptr::null_mut();
    };
    let codepage = string_option_from_ptr(codepage);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            emit_library_message(
                library,
                MESSAGE_LEVEL_WARNING,
                format!("ass_read_file({path}): read failed: {error}"),
            );
            return ptr::null_mut();
        }
    };
    let mut parsed = match parse_script_bytes_with_codepage(&bytes, codepage.as_deref()) {
        Ok(parsed) => parsed,
        Err(error) => {
            emit_library_message(
                library,
                MESSAGE_LEVEL_ERROR,
                format!("Unable to parse subtitle file '{path}': {error:?}"),
            );
            return ptr::null_mut();
        }
    };
    if parsed.track_type == ass::TrackType::Unknown {
        emit_library_message(
            library,
            MESSAGE_LEVEL_WARNING,
            format!("No recognizable subtitle track in '{path}'"),
        );
        return ptr::null_mut();
    }
    let script_info_presence = capi_script_info_presence_from_bytes(&bytes);
    let format_presence = capi_format_presence_from_bytes(&bytes, CapiParserState::Unknown);
    apply_capi_script_info_presence_to_parsed(&mut parsed, script_info_presence);
    maybe_extract_fonts_to_library(library, &parsed.attachments);
    let track = track_from_parsed(library, parsed, script_info_presence, format_presence);
    if let Some(track_ref) = track.as_mut() {
        replace_string(&mut track_ref.name, &path);
    }
    update_parser_state_from_bytes(track, &bytes);
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
        emit_library_message(
            library,
            MESSAGE_LEVEL_WARNING,
            "ass_read_memory: buffer is NULL",
        );
        return ptr::null_mut();
    }

    let codepage = string_option_from_ptr(codepage);
    let bytes = slice::from_raw_parts(buf as *const u8, bufsize);
    let mut parsed = match parse_script_bytes_with_codepage(bytes, codepage.as_deref()) {
        Ok(parsed) => parsed,
        Err(error) => {
            emit_library_message(
                library,
                MESSAGE_LEVEL_ERROR,
                format!("Unable to parse subtitle data: {error:?}"),
            );
            return ptr::null_mut();
        }
    };
    if parsed.track_type == ass::TrackType::Unknown {
        emit_library_message(
            library,
            MESSAGE_LEVEL_WARNING,
            "No recognizable subtitle track in memory",
        );
        return ptr::null_mut();
    }
    let script_info_presence = capi_script_info_presence_from_bytes(bytes);
    let format_presence = capi_format_presence_from_bytes(bytes, CapiParserState::Unknown);
    apply_capi_script_info_presence_to_parsed(&mut parsed, script_info_presence);
    maybe_extract_fonts_to_library(library, &parsed.attachments);
    let track = track_from_parsed(library, parsed, script_info_presence, format_presence);
    update_parser_state_from_bytes(track, bytes);
    ass_process_force_style(track);
    track
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_read_styles(
    track: *mut ASS_Track,
    fname: *const c_char,
    codepage: *const c_char,
) -> c_int {
    let library = track
        .as_ref()
        .map(|track| track.library)
        .unwrap_or(ptr::null_mut());
    let Some(path) = string_option_from_ptr(fname) else {
        emit_library_message(
            library,
            MESSAGE_LEVEL_WARNING,
            "ass_read_styles: filename is NULL",
        );
        return 1;
    };
    let codepage = string_option_from_ptr(codepage);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            emit_library_message(
                library,
                MESSAGE_LEVEL_WARNING,
                format!("ass_read_styles({path}): read failed: {error}"),
            );
            return 1;
        }
    };
    if track.as_ref().is_none() {
        return 1;
    };

    let script_info_presence = capi_script_info_presence_from_bytes(&bytes);
    let format_presence = capi_format_presence_from_bytes(&bytes, CapiParserState::Styles);
    let has_section_header = capi_has_libass_section_header_from_bytes(&bytes);
    if let Ok(mut parsed) = parse_script_bytes_with_codepage(&bytes, codepage.as_deref()) {
        let has_full_script_content = parsed.track_type != ass::TrackType::Unknown
            || parsed.styles.len() > 1
            || !parsed.events.is_empty()
            || !parsed.attachments.is_empty()
            || script_info_presence.any()
            || format_presence.any() && has_section_header;
        if has_full_script_content {
            apply_capi_script_info_presence_to_parsed(&mut parsed, script_info_presence);
            zero_process_data_read_orders(&mut parsed);
            if let Some(track_ref) = track.as_ref() {
                maybe_extract_fonts_to_library(track_ref.library, &parsed.attachments);
            }
            merge_script_info_from_bytes(track, &bytes);
            merge_parsed_into_track(track, parsed, format_presence);
            return 0;
        }
    }

    let parsed = if codepage.as_deref().is_none() {
        parse_style_section_from_existing_track(track, &bytes)
    } else {
        let Some(track_ref) = track.as_ref() else {
            return 1;
        };
        let track_type = match track_ref.track_type {
            value if value == ass::TrackType::Ssa as c_int => ass::TrackType::Ssa,
            _ => ass::TrackType::Ass,
        };
        match parse_style_section_bytes_with_codepage(&bytes, codepage.as_deref(), track_type) {
            Ok(parsed) => Some(parsed),
            Err(error) => {
                emit_library_message(
                    library,
                    MESSAGE_LEVEL_ERROR,
                    format!("Unable to parse style file '{path}': {error:?}"),
                );
                return 1;
            }
        }
    };
    let Some(parsed) = parsed else {
        return 0;
    };
    merge_styles_from_parsed(track, parsed, format_presence);
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
    if name.is_null() || data.is_null() || data_size <= 0 {
        return;
    }

    library.fonts.push(FontAttachment {
        name: string_from_ptr(name),
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
        library_fonts_dir: library_ref.and_then(|library| library.fonts_dir.clone()),
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
    let has_system_provider = system_font_provider_is_available(renderer.default_provider);
    let system_provider: Box<dyn FontProvider> = match renderer.default_provider {
        _ if has_system_provider => build_crossfont_provider(renderer, library),
        _ => Box::new(NullFontProvider),
    };

    if renderer.default_provider != ass::DefaultFontProvider::None as c_int && !has_system_provider
    {
        unsafe {
            emit_library_message(
                library,
                MESSAGE_LEVEL_WARNING,
                format!(
                    "can't find selected font provider {}",
                    renderer.default_provider
                ),
            );
        }
    }

    let Some(library_ref) = (unsafe { library.as_ref() }) else {
        return wrap_default_font_path(system_provider, renderer);
    };

    let attachments = library_ref
        .fonts
        .iter()
        .map(|font| ProviderFontAttachment {
            name: font.name.clone(),
            data: font.data.clone(),
        })
        .collect::<Vec<_>>();
    let fonts_dir = library_ref.fonts_dir.clone();
    let attached = AttachedFontProvider::from_attachments(&attachments);
    let directory = if let Some(fonts_dir) = fonts_dir.as_deref().filter(|dir| !dir.is_empty()) {
        let (provider, issues) = DirectoryFontProvider::scan(fonts_dir);
        for issue in issues {
            unsafe {
                emit_library_message(
                    library,
                    MESSAGE_LEVEL_WARNING,
                    format!("font directory scan: {issue}"),
                );
            }
        }
        provider
    } else {
        DirectoryFontProvider::default()
    };
    let local = MergedFontProvider::new(attached, directory);

    let provider: Box<dyn FontProvider> = if has_system_provider {
        Box::new(MergedFontProvider::new(local, system_provider))
    } else {
        Box::new(local)
    };
    wrap_default_font_path(provider, renderer)
}

fn build_crossfont_provider(
    renderer: &ASS_Renderer,
    library: *mut ASS_Library,
) -> Box<dyn FontProvider> {
    #[cfg(not(all(unix, not(target_os = "macos"), not(target_arch = "wasm32"))))]
    let _ = library;

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    if matches!(
        renderer.default_provider,
        value if value == ass::DefaultFontProvider::Autodetect as c_int
            || value == ass::DefaultFontProvider::Fontconfig as c_int
    ) {
        if let Some(config) = renderer
            .fontconfig_config
            .as_deref()
            .filter(|config| !config.is_empty())
        {
            if let Err(error) = validate_fontconfig_config(config) {
                unsafe {
                    emit_library_message(
                        library,
                        MESSAGE_LEVEL_WARNING,
                        format!(
                            "No usable fontconfig configuration file '{config}' found ({error}); using fallback"
                        ),
                    );
                }
            }
            return if let Some(fallback_family) = renderer.default_family.as_deref() {
                Box::new(CrossfontProvider::with_config_and_fallback_family(
                    config,
                    fallback_family,
                ))
            } else {
                Box::new(CrossfontProvider::with_config(config))
            };
        }
    }

    if let Some(fallback_family) = renderer.default_family.as_deref() {
        Box::new(CrossfontProvider::with_fallback_family(fallback_family))
    } else {
        Box::new(CrossfontProvider::new())
    }
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

fn renderer_config(
    renderer: &ASS_Renderer,
    track: &ParsedTrack,
    bidi_brackets: bool,
    whole_text_layout: bool,
    wrap_unicode: bool,
) -> RendererConfig {
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
        selective_font_scale: renderer.selective_override_bits
            & ass::override_bits::SELECTIVE_FONT_SCALE
            != 0,
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
        wrap_unicode,
        bidi_brackets,
        whole_text_layout,
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
        if attachment.data.is_empty() {
            continue;
        }
        library.fonts.push(FontAttachment {
            name: attachment.name.clone(),
            data: attachment.data.clone(),
        });
    }
}

unsafe fn maybe_extract_fonts_from_font_state_bytes(track: *mut ASS_Track, bytes: &[u8]) {
    let Some(track_ref) = track.as_ref() else {
        return;
    };
    let Some(library) = track_ref.library.as_ref() else {
        return;
    };
    if !library.extract_fonts {
        return;
    }

    let text = String::from_utf8_lossy(bytes);
    let mut prefix = String::new();
    let mut reached_section_header = false;
    for_each_libass_process_line(&text, |line| {
        if reached_section_header {
            return;
        }
        if line_is_libass_section_header(line) {
            reached_section_header = true;
            return;
        }
        prefix.push_str(line);
        prefix.push('\n');
    });
    if prefix.is_empty() {
        return;
    }

    if let Ok(parsed) = parse_script_text(&format!("[Fonts]\n{prefix}")) {
        maybe_extract_fonts_to_library(track_ref.library, &parsed.attachments);
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
    } else if key.eq_ignore_ascii_case("YCbCr Matrix") {
        track.YCbCrMatrix = parse_force_ycbcr_matrix(value);
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
    } else if field_name.eq_ignore_ascii_case("AlphaLevel") {
        set_ffi_style_alpha(
            style,
            parse_override_i32(value, 0),
            parse_override_i32(value, 0),
        );
    } else if field_name.eq_ignore_ascii_case("FontSize") {
        style.FontSize = parse_override_f64(value, style.FontSize);
    } else if field_name.eq_ignore_ascii_case("Bold") {
        style.Bold = parse_override_i32(value, style.Bold);
    } else if field_name.eq_ignore_ascii_case("Italic") {
        style.Italic = parse_override_i32(value, style.Italic);
    } else if field_name.eq_ignore_ascii_case("Underline") {
        style.Underline = parse_override_i32(value, style.Underline);
    } else if field_name.eq_ignore_ascii_case("StrikeOut") {
        style.StrikeOut = parse_override_i32(value, style.StrikeOut);
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

fn parse_override_i32(value: &str, _default: i32) -> i32 {
    parse_libass_header_i32(value)
}

fn parse_override_f64(value: &str, _default: f64) -> f64 {
    parse_force_f64_prefix(value).unwrap_or(0.0)
}

fn parse_force_f64_prefix(value: &str) -> Option<f64> {
    let start = value
        .char_indices()
        .find_map(|(index, character)| (!is_force_c_space(character)).then_some(index))?;

    let mut index = start;
    if let Some(character) = value.get(index..)?.chars().next() {
        if character == '+' || character == '-' {
            index += character.len_utf8();
        }
    }

    let mut seen_digit = false;
    let mut seen_dot = false;
    while let Some(character) = value.get(index..)?.chars().next() {
        if character.is_ascii_digit() {
            seen_digit = true;
            index += character.len_utf8();
        } else if character == '.' && !seen_dot {
            seen_dot = true;
            index += character.len_utf8();
        } else {
            break;
        }
    }

    if !seen_digit {
        return None;
    }

    let mantissa_end = index;
    let mut parse_end = mantissa_end;
    if let Some(character) = value.get(index..)?.chars().next() {
        if character == 'e' || character == 'E' {
            index += character.len_utf8();
            if let Some(character) = value.get(index..)?.chars().next() {
                if character == '+' || character == '-' {
                    index += character.len_utf8();
                }
            }
            let exponent_start = index;
            while let Some(character) = value.get(index..)?.chars().next() {
                if !character.is_ascii_digit() {
                    break;
                }
                index += character.len_utf8();
            }
            if index > exponent_start {
                parse_end = index;
            }
        }
    }

    parse_ass_f64_prefix(&value[start..parse_end])
}

fn parse_ass_f64_prefix(value: &str) -> Option<f64> {
    value.parse::<f64>().ok().or_else(|| {
        let sign_len = value
            .starts_with('+')
            .then_some(1)
            .or_else(|| value.starts_with('-').then_some(1))
            .unwrap_or(0);
        value[sign_len..].starts_with('.').then(|| {
            let mut normalized = String::with_capacity(value.len() + 1);
            normalized.push_str(&value[..sign_len]);
            normalized.push('0');
            normalized.push_str(&value[sign_len..]);
            normalized.parse::<f64>().ok()
        })?
    })
}

fn is_force_c_space(character: char) -> bool {
    matches!(
        character,
        ' ' | '\t' | '\n' | '\r' | '\u{000b}' | '\u{000c}'
    )
}

fn parse_override_bool(value: &str, default: bool) -> bool {
    let _ = default;
    let trimmed = value.trim_start_matches([' ', '\t']);
    trimmed
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("yes"))
        || parse_force_decimal_bool(trimmed)
}

fn parse_force_decimal_bool(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    let negative = match bytes.get(index).copied() {
        Some(b'+') => {
            index += 1;
            false
        }
        Some(b'-') => {
            index += 1;
            true
        }
        _ => false,
    };
    if negative {
        return false;
    }

    while let Some(byte) = bytes.get(index).copied() {
        if !byte.is_ascii_digit() {
            break;
        }
        if byte != b'0' {
            return true;
        }
        index += 1;
    }
    false
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

fn parse_override_color(value: &str, _default: u32) -> u32 {
    ass_color_to_rgba(parse_libass_header_i32(value) as u32)
}

fn set_ffi_style_alpha(style: &mut ASS_Style, front_alpha: c_int, back_alpha: c_int) {
    let front_alpha = front_alpha.clamp(0, 0xff) as u32;
    let back_alpha = back_alpha.clamp(0, 0xff) as u32;
    style.PrimaryColour = (style.PrimaryColour & 0xFFFF_FF00) | front_alpha;
    style.SecondaryColour = (style.SecondaryColour & 0xFFFF_FF00) | front_alpha;
    style.OutlineColour = (style.OutlineColour & 0xFFFF_FF00) | front_alpha;
    style.BackColour = (style.BackColour & 0xFFFF_FF00) | back_alpha;
}

fn parse_force_ycbcr_matrix(value: &str) -> c_int {
    let trimmed = value.trim_matches([' ', '\t']);
    match trimmed.to_ascii_lowercase().as_str() {
        "" => ass::YCbCrMatrix::Default as c_int,
        "none" => ass::YCbCrMatrix::None as c_int,
        "tv.601" => ass::YCbCrMatrix::Bt601Tv as c_int,
        "pc.601" => ass::YCbCrMatrix::Bt601Pc as c_int,
        "tv.709" => ass::YCbCrMatrix::Bt709Tv as c_int,
        "pc.709" => ass::YCbCrMatrix::Bt709Pc as c_int,
        "tv.240m" => ass::YCbCrMatrix::Smpte240mTv as c_int,
        "pc.240m" => ass::YCbCrMatrix::Smpte240mPc as c_int,
        "tv.fcc" => ass::YCbCrMatrix::FccTv as c_int,
        "pc.fcc" => ass::YCbCrMatrix::FccPc as c_int,
        _ => ass::YCbCrMatrix::Unknown as c_int,
    }
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

#[cfg(target_arch = "wasm32")]
#[repr(C, align(16))]
struct WasmAllocationHeader {
    allocation_size: usize,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ass_malloc(size: usize) -> *mut c_void {
    #[cfg(not(target_arch = "wasm32"))]
    {
        malloc(size)
    }

    #[cfg(target_arch = "wasm32")]
    {
        let header_size = mem::size_of::<WasmAllocationHeader>();
        let Some(allocation_size) = header_size.checked_add(size.max(1)) else {
            return ptr::null_mut();
        };
        let Ok(layout) =
            Layout::from_size_align(allocation_size, mem::align_of::<WasmAllocationHeader>())
        else {
            return ptr::null_mut();
        };
        let allocation = alloc(layout);
        if allocation.is_null() {
            return ptr::null_mut();
        }
        allocation
            .cast::<WasmAllocationHeader>()
            .write(WasmAllocationHeader { allocation_size });
        allocation.add(header_size).cast()
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
        let header_size = mem::size_of::<WasmAllocationHeader>();
        let allocation = ptr.cast::<u8>().sub(header_size);
        let allocation_size = (*allocation.cast::<WasmAllocationHeader>()).allocation_size;
        let layout = Layout::from_size_align_unchecked(
            allocation_size,
            mem::align_of::<WasmAllocationHeader>(),
        );
        dealloc(allocation, layout);
    }
}

unsafe fn track_from_parsed(
    library: *mut ASS_Library,
    parsed: ParsedTrack,
    script_info_presence: CapiScriptInfoPresence,
    format_presence: CapiFormatPresence,
) -> *mut ASS_Track {
    let track = ass_new_track(library);
    replace_track_from_parsed(track, parsed, script_info_presence, format_presence);
    track
}

unsafe fn replace_track_from_parsed(
    track: *mut ASS_Track,
    parsed: ParsedTrack,
    script_info_presence: CapiScriptInfoPresence,
    format_presence: CapiFormatPresence,
) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };

    ass_process_force_style(track);
    let library = track_ref.library;
    let parser_priv = track_ref.parser_priv;
    free_track_contents(track_ref);
    *track_ref = build_track(parsed, library, parser_priv);
    apply_capi_format_presence_to_track(track_ref, format_presence, false, false);
    apply_missing_capi_script_info_fields(track_ref, script_info_presence);
}

fn track_is_pristine(track: &ASS_Track) -> bool {
    track.track_type == ass::TrackType::Unknown as c_int
        && track.n_styles == 1
        && track.n_events == 0
        && track.style_format.is_null()
        && track.event_format.is_null()
}

fn zero_process_data_read_orders(parsed: &mut ParsedTrack) {
    for event in &mut parsed.events {
        event.read_order = 0;
    }
}

fn apply_capi_script_info_presence_to_parsed(
    parsed: &mut ParsedTrack,
    presence: CapiScriptInfoPresence,
) {
    if presence.play_res_x {
        parsed.play_res_x = presence.play_res_x_value;
    } else {
        parsed.play_res_x = 0;
    }
    if presence.play_res_y {
        parsed.play_res_y = presence.play_res_y_value;
    } else {
        parsed.play_res_y = 0;
    }
    if !presence.timer {
        parsed.timer = 0.0;
    }
    if !presence.kerning {
        parsed.kerning = false;
    }
    if !presence.language {
        parsed.language.clear();
    }
}

unsafe fn apply_missing_capi_script_info_fields(
    track: &mut ASS_Track,
    presence: CapiScriptInfoPresence,
) {
    if !presence.language {
        free_c_string(&mut track.Language);
    }
}

unsafe fn merge_parsed_into_track(
    track: *mut ASS_Track,
    parsed: ParsedTrack,
    format_presence: CapiFormatPresence,
) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };

    invalidate_parsed_track_cache_for_track(track_ref);

    let had_style_format = !track_ref.style_format.is_null();
    let had_event_format = !track_ref.event_format.is_null();
    if parsed.track_type != ass::TrackType::Unknown {
        track_ref.track_type = parsed.track_type as c_int;
    }
    if format_presence.should_keep_style(had_style_format) {
        replace_string(&mut track_ref.style_format, &parsed.style_format);
    }
    if format_presence.should_keep_event(had_event_format) {
        replace_string(&mut track_ref.event_format, &parsed.event_format);
    }

    let mut style_index_map = Vec::with_capacity(parsed.styles.len());
    style_index_map.push(track_ref.default_style);

    append_styles_from_parsed(track_ref, &parsed.styles, Some(&mut style_index_map));

    let mut events = take_events(track_ref);
    for event in &parsed.events {
        let mut event = event.clone();
        event.style = usize::try_from(event.style)
            .ok()
            .and_then(|index| style_index_map.get(index).copied())
            .unwrap_or(track_ref.default_style);
        push_event_libass(&mut events, make_event(&event, &parsed.event_format));
    }
    store_events(track_ref, events);
}

unsafe fn apply_capi_format_presence_to_track(
    track: &mut ASS_Track,
    format_presence: CapiFormatPresence,
    had_style_format: bool,
    had_event_format: bool,
) {
    if !format_presence.should_keep_style(had_style_format) {
        free_c_string(&mut track.style_format);
    }
    if !format_presence.should_keep_event(had_event_format) {
        free_c_string(&mut track.event_format);
    }
}

unsafe fn merge_event_format_from_parsed(
    track: *mut ASS_Track,
    parsed: &ParsedTrack,
    format_presence: CapiFormatPresence,
) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    let had_event_format = !track_ref.event_format.is_null();
    if format_presence.should_keep_event(had_event_format) {
        replace_string(&mut track_ref.event_format, &parsed.event_format);
        invalidate_parsed_track_cache_for_track(track_ref);
    }
}

unsafe fn ensure_event_format_fallback(track: *mut ASS_Track) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    if track_ref.event_format.is_null() {
        let event_format = if track_ref.track_type == ass::TrackType::Ssa as c_int {
            "Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
        } else {
            "Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text"
        };
        replace_string(&mut track_ref.event_format, event_format);
        invalidate_parsed_track_cache_for_track(track_ref);
    }
}

unsafe fn append_styles_from_parsed(
    track: &mut ASS_Track,
    parsed_styles: &[ParsedStyle],
    mut style_index_map: Option<&mut Vec<c_int>>,
) {
    let mut styles = take_styles(track);
    for style in parsed_styles.iter().skip(1) {
        let new_index = styles.len() as c_int;
        if style.name == "Default" {
            track.default_style = new_index;
        }
        if let Some(style_index_map) = style_index_map.as_deref_mut() {
            style_index_map.push(new_index);
        }
        push_style_libass(&mut styles, make_style(style));
    }
    store_styles(track, styles);
}

unsafe fn merge_styles_from_parsed(
    track: *mut ASS_Track,
    parsed: ParsedTrack,
    format_presence: CapiFormatPresence,
) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    if parsed.track_type != ass::TrackType::Unknown {
        track_ref.track_type = parsed.track_type as c_int;
    }
    let had_style_format = !track_ref.style_format.is_null();
    if format_presence.should_keep_style(had_style_format) {
        replace_string(&mut track_ref.style_format, &parsed.style_format);
    }
    append_styles_from_parsed(track_ref, &parsed.styles, None);
}

unsafe fn parse_event_section_from_existing_track(
    track: *mut ASS_Track,
    bytes: &[u8],
) -> Option<ParsedTrack> {
    let track_ref = track.as_ref()?;
    let event_format = string_option_from_ptr(track_ref.event_format);

    let incoming = String::from_utf8_lossy(bytes);
    if !incoming.lines().any(|line| {
        let line = line.trim_start_matches([' ', '\t']);
        line.starts_with("Format:") || line.starts_with("Dialogue:") || line.starts_with("Comment:")
    }) {
        return None;
    }

    let mut script = String::new();
    if track_ref.track_type == ass::TrackType::Ssa as c_int {
        script.push_str("[V4 Styles]\n");
    } else {
        script.push_str("[V4+ Styles]\n");
    }
    script.push_str("Format: Name\n");
    if !track_ref.styles.is_null() && track_ref.n_styles > 1 {
        for style in slice::from_raw_parts(track_ref.styles, track_ref.n_styles as usize)
            .iter()
            .skip(1)
        {
            script.push_str("Style: ");
            script.push_str(&string_option_from_ptr(style.Name).unwrap_or_default());
            script.push('\n');
        }
    }
    script.push_str("\n[Events]\n");
    if let Some(event_format) = event_format {
        script.push_str("Format: ");
        script.push_str(&event_format);
        script.push('\n');
    }
    script.push_str(&incoming);

    parse_script_text(&script).ok()
}

unsafe fn parse_style_section_from_existing_track(
    track: *mut ASS_Track,
    bytes: &[u8],
) -> Option<ParsedTrack> {
    let track_ref = track.as_ref()?;
    if !String::from_utf8_lossy(bytes).lines().any(|line| {
        let line = line.trim_start_matches([' ', '\t']);
        line.starts_with("Format:") || line.starts_with("Style:")
    }) {
        return None;
    }

    let track_type = if track_ref.track_type == ass::TrackType::Ssa as c_int {
        ass::TrackType::Ssa
    } else {
        ass::TrackType::Ass
    };
    let style_format = string_option_from_ptr(track_ref.style_format);
    let incoming = String::from_utf8_lossy(bytes);
    let mut script = String::new();
    if track_type == ass::TrackType::Ssa {
        script.push_str("[V4 Styles]\n");
    } else {
        script.push_str("[V4+ Styles]\n");
    }
    if let Some(style_format) = style_format {
        script.push_str("Format: ");
        script.push_str(&style_format);
        script.push('\n');
    }
    script.push_str(&incoming);
    parse_script_text(&script).ok()
}

unsafe fn merge_info_from_sectionless_bytes(track: *mut ASS_Track, bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    if capi_has_libass_section_header(&text) {
        return false;
    }

    let Some(track_ref) = track.as_mut() else {
        return false;
    };
    let mut applied = false;
    for_each_libass_process_line(&text, |line| {
        let line = line.trim_start_matches([' ', '\t']);
        applied |= apply_info_line(track_ref, line);
    });
    if applied {
        invalidate_parsed_track_cache_for_track(track_ref);
    }
    applied
}

fn capi_has_libass_section_header_from_bytes(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    capi_has_libass_section_header(&text)
}

fn capi_has_libass_section_header(text: &str) -> bool {
    let mut has_section_header = false;
    for_each_libass_process_line(text, |line| {
        if line_is_libass_section_header(line) {
            has_section_header = true;
        }
    });
    has_section_header
}

unsafe fn merge_script_info_from_bytes(track: *mut ASS_Track, bytes: &[u8]) -> bool {
    let Some(track_ref) = track.as_mut() else {
        return false;
    };
    let text = String::from_utf8_lossy(bytes);
    let mut state = CapiParserState::Unknown;
    let mut applied = false;
    for_each_libass_process_line(&text, |line| {
        let line = line.trim_start_matches([' ', '\t']);
        if update_parser_state_from_line(&mut state, line) {
            return;
        }
        if state == CapiParserState::Info {
            applied |= apply_info_line(track_ref, line);
        }
    });
    if applied {
        invalidate_parsed_track_cache_for_track(track_ref);
    }
    applied
}

fn capi_script_info_presence_from_bytes(bytes: &[u8]) -> CapiScriptInfoPresence {
    let text = String::from_utf8_lossy(bytes);
    let mut state = CapiParserState::Unknown;
    let mut presence = CapiScriptInfoPresence::default();
    for_each_libass_process_line(&text, |line| {
        let line = line.trim_start_matches([' ', '\t']);
        if update_parser_state_from_line(&mut state, line) {
            return;
        }
        if state != CapiParserState::Info {
            return;
        }
        if let Some(value) = line.strip_prefix("PlayResX:") {
            presence.play_res_x = true;
            presence.play_res_x_value = parse_libass_header_i32(value);
        } else if let Some(value) = line.strip_prefix("PlayResY:") {
            presence.play_res_y = true;
            presence.play_res_y_value = parse_libass_header_i32(value);
        } else if line.starts_with("Timer:") {
            presence.timer = true;
        } else if line.starts_with("Kerning:") {
            presence.kerning = true;
        } else if line.starts_with("Language:") {
            presence.language = true;
        }
    });
    presence
}

fn capi_format_presence_from_bytes(
    bytes: &[u8],
    initial_state: CapiParserState,
) -> CapiFormatPresence {
    let text = String::from_utf8_lossy(bytes);
    let mut state = initial_state;
    let mut presence = CapiFormatPresence::default();
    let mut style_format_seen = false;
    let mut event_format_seen = false;
    for_each_libass_process_line(&text, |line| {
        let line = line.trim_start_matches([' ', '\t']);
        if update_parser_state_from_line(&mut state, line) {
            return;
        }
        match state {
            CapiParserState::Styles if line.starts_with("Format:") => {
                presence.explicit_style = true;
                style_format_seen = true;
            }
            CapiParserState::Styles if line.starts_with("Style:") && !style_format_seen => {
                presence.fallback_style = true;
                style_format_seen = true;
            }
            CapiParserState::Events if line.starts_with("Format:") => {
                presence.explicit_event = true;
                event_format_seen = true;
            }
            CapiParserState::Events if line.starts_with("Dialogue:") && !event_format_seen => {
                presence.fallback_event = true;
                event_format_seen = true;
            }
            _ => {}
        }
    });
    presence
}

unsafe fn apply_info_line(track: &mut ASS_Track, line: &str) -> bool {
    if let Some(value) = line.strip_prefix("PlayResX:") {
        track.PlayResX = parse_libass_header_i32(value);
    } else if let Some(value) = line.strip_prefix("PlayResY:") {
        track.PlayResY = parse_libass_header_i32(value);
    } else if let Some(value) = line.strip_prefix("LayoutResX:") {
        track.LayoutResX = parse_libass_header_i32(value);
    } else if let Some(value) = line.strip_prefix("LayoutResY:") {
        track.LayoutResY = parse_libass_header_i32(value);
    } else if let Some(value) = line.strip_prefix("Timer:") {
        track.Timer = parse_override_f64(value, track.Timer);
    } else if let Some(value) = line.strip_prefix("WrapStyle:") {
        track.WrapStyle = parse_libass_header_i32(value);
    } else if let Some(value) = line.strip_prefix("ScaledBorderAndShadow:") {
        track.ScaledBorderAndShadow =
            parse_override_bool(value, track.ScaledBorderAndShadow != 0) as c_int;
    } else if let Some(value) = line.strip_prefix("Kerning:") {
        track.Kerning = parse_override_bool(value, track.Kerning != 0) as c_int;
    } else if let Some(value) = line.strip_prefix("YCbCr Matrix:") {
        track.YCbCrMatrix = parse_force_ycbcr_matrix(value);
    } else if let Some(value) = line.strip_prefix("Language:") {
        replace_string(&mut track.Language, &parse_info_language(value));
    } else if let Some(value) = line.strip_prefix("ScriptType:") {
        if let Some(track_type) = parse_info_script_type(value) {
            track.track_type = track_type;
        }
    } else {
        return false;
    }
    true
}

fn parse_info_language(value: &str) -> String {
    let trimmed = value.trim_start_matches(is_force_c_space);
    let mut end = trimmed.len().min(2);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_string()
}

fn parse_info_script_type(value: &str) -> Option<c_int> {
    let value = value.trim_end_matches([' ', '\t']);
    if value.len() < 4 {
        return None;
    }

    let (version, track_type) = if let Some(version) = value.strip_suffix('+') {
        (version, ass::TrackType::Ass as c_int)
    } else {
        (value, ass::TrackType::Ssa as c_int)
    };
    version.ends_with("4.00").then_some(track_type)
}

unsafe fn update_parser_state_from_bytes(track: *mut ASS_Track, bytes: &[u8]) {
    let Some(state) = track_state_mut(track) else {
        return;
    };
    let text = String::from_utf8_lossy(bytes);
    for_each_libass_process_line(&text, |line| {
        let line = line.trim_start_matches([' ', '\t']);
        update_parser_state_from_line(&mut state.parser_state, line);
    });
}

fn update_parser_state_from_line(state: &mut CapiParserState, line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    if line.starts_with("[script info]") {
        *state = CapiParserState::Info;
    } else if line.starts_with("[v4 styles]") || line.starts_with("[v4+ styles]") {
        *state = CapiParserState::Styles;
    } else if line.starts_with("[events]") {
        *state = CapiParserState::Events;
    } else if line.starts_with("[fonts]") {
        *state = CapiParserState::Fonts;
    } else {
        return false;
    }
    true
}

fn line_is_libass_section_header(line: &str) -> bool {
    let line = line.trim_start_matches([' ', '\t']);
    let line = line.to_ascii_lowercase();
    line.starts_with("[script info]")
        || line.starts_with("[v4 styles]")
        || line.starts_with("[v4+ styles]")
        || line.starts_with("[events]")
        || line.starts_with("[fonts]")
}

fn for_each_libass_process_line(text: &str, mut process: impl FnMut(&str)) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        loop {
            if matches!(bytes.get(index), Some(b'\r' | b'\n')) {
                index += 1;
            } else if bytes
                .get(index..)
                .is_some_and(|rest| rest.starts_with(&[0xef, 0xbb, 0xbf]))
            {
                index += 3;
            } else {
                break;
            }
        }

        let start = index;
        while !matches!(bytes.get(index), None | Some(b'\r' | b'\n')) {
            index += 1;
        }
        if index == start {
            break;
        }

        process(&text[start..index]);
        if index == bytes.len() {
            break;
        }
        index += 1;
    }
}

unsafe fn append_events_from_parsed(track: *mut ASS_Track, parsed_events: &[ParsedEvent]) {
    let Some(track_ref) = track.as_mut() else {
        return;
    };
    let mut events = take_events(track_ref);
    let event_format = string_option_from_ptr(track_ref.event_format).unwrap_or_default();
    for event in parsed_events {
        push_event_libass(&mut events, make_event(event, &event_format));
    }
    store_events(track_ref, events);
}

unsafe fn build_track(
    parsed: ParsedTrack,
    library: *mut ASS_Library,
    parser_priv: *mut ASS_ParserPriv,
) -> ASS_Track {
    let mut styles = Vec::with_capacity(libass_style_capacity_for_len(parsed.styles.len()));
    for (index, style) in parsed.styles.iter().enumerate() {
        styles.push(if index == 0 {
            make_builtin_default_style()
        } else {
            make_style(style)
        });
    }
    let mut events = Vec::with_capacity(libass_event_capacity_for_len(parsed.events.len()));
    for event in &parsed.events {
        events.push(make_event(event, &parsed.event_format));
    }

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

fn libass_style_capacity_for_len(len: usize) -> usize {
    if len == 0 {
        0
    } else {
        len.div_ceil(ASS_STYLES_ALLOC) * ASS_STYLES_ALLOC
    }
}

fn libass_event_capacity_for_len(len: usize) -> usize {
    let mut capacity = 0_usize;
    while capacity < len {
        capacity = capacity.saturating_mul(2).saturating_add(1);
    }
    capacity
}

fn grow_styles_like_libass(styles: &mut Vec<ASS_Style>) {
    let new_capacity = styles.capacity().saturating_add(ASS_STYLES_ALLOC);
    let mut grown = Vec::with_capacity(new_capacity);
    grown.append(styles);
    *styles = grown;
}

fn grow_events_like_libass(events: &mut Vec<ASS_Event>) {
    let new_capacity = events.capacity().saturating_mul(2).saturating_add(1);
    let mut grown = Vec::with_capacity(new_capacity);
    grown.append(events);
    *events = grown;
}

fn push_style_libass(styles: &mut Vec<ASS_Style>, style: ASS_Style) -> c_int {
    if styles.len() == styles.capacity() {
        grow_styles_like_libass(styles);
    }
    let index = styles.len() as c_int;
    styles.push(style);
    index
}

fn push_event_libass(events: &mut Vec<ASS_Event>, event: ASS_Event) -> c_int {
    if events.len() == events.capacity() {
        grow_events_like_libass(events);
    }
    let index = events.len() as c_int;
    events.push(event);
    index
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
    if !(*value).is_null() {
        ass_free((*value).cast());
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
        PrimaryColour: ass_color_to_rgba(style.primary_colour),
        SecondaryColour: ass_color_to_rgba(style.secondary_colour),
        OutlineColour: ass_color_to_rgba(style.outline_colour),
        BackColour: ass_color_to_rgba(style.back_colour),
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

fn make_builtin_default_style() -> ASS_Style {
    let mut style = make_style(&ParsedStyle::default());
    style.Bold = 200;
    style
}

fn make_event(event: &ParsedEvent, event_format: &str) -> ASS_Event {
    let has_name = event_format_includes_field(event_format, &["Name", "Actor"]);
    let has_effect = event_format_includes_field(event_format, &["Effect"]);
    ASS_Event {
        Start: event.start,
        Duration: event.duration,
        ReadOrder: event.read_order,
        Layer: event.layer,
        Style: event.style,
        Name: optional_event_string_to_c_ptr(&event.name, has_name),
        MarginL: event.margin_l,
        MarginR: event.margin_r,
        MarginV: event.margin_v,
        Effect: optional_event_string_to_c_ptr(&event.effect, has_effect),
        Text: string_to_c_ptr(&event.text),
        render_priv: ptr::null_mut(),
    }
}

fn event_format_includes_field(event_format: &str, aliases: &[&str]) -> bool {
    event_format.split(',').any(|field| {
        let field = field.trim_matches([' ', '\t']);
        aliases
            .iter()
            .any(|alias| field.eq_ignore_ascii_case(alias))
    })
}

fn optional_event_string_to_c_ptr(value: &str, field_present: bool) -> *mut c_char {
    if field_present {
        string_to_c_ptr(value)
    } else {
        ptr::null_mut()
    }
}

fn string_to_c_ptr(value: &str) -> *mut c_char {
    let sanitized = value.replace('\0', " ");
    let bytes = sanitized.as_bytes();
    let Some(allocation_size) = bytes.len().checked_add(1) else {
        return ptr::null_mut();
    };
    let allocation = unsafe { ass_malloc(allocation_size).cast::<c_char>() };
    if allocation.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), allocation, bytes.len());
        allocation.add(bytes.len()).write(0);
    }
    allocation
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

unsafe fn parsed_track_cache_signature(
    track: &ASS_Track,
    active_event_indices: &[usize],
) -> ParsedTrackCacheSignature {
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
        content_fingerprint: public_track_content_fingerprint(track, active_event_indices),
    }
}

unsafe fn hash_c_string_for_cache(hasher: &mut DefaultHasher, value: *const c_char) {
    if value.is_null() {
        0_u8.hash(hasher);
    } else {
        1_u8.hash(hasher);
        CStr::from_ptr(value).to_bytes().hash(hasher);
    }
}

unsafe fn hash_style_for_cache(hasher: &mut DefaultHasher, style: &ASS_Style) {
    hash_c_string_for_cache(hasher, style.Name);
    hash_c_string_for_cache(hasher, style.FontName);
    style.FontSize.to_bits().hash(hasher);
    style.PrimaryColour.hash(hasher);
    style.SecondaryColour.hash(hasher);
    style.OutlineColour.hash(hasher);
    style.BackColour.hash(hasher);
    style.Bold.hash(hasher);
    style.Italic.hash(hasher);
    style.Underline.hash(hasher);
    style.StrikeOut.hash(hasher);
    style.ScaleX.to_bits().hash(hasher);
    style.ScaleY.to_bits().hash(hasher);
    style.Spacing.to_bits().hash(hasher);
    style.Angle.to_bits().hash(hasher);
    style.BorderStyle.hash(hasher);
    style.Outline.to_bits().hash(hasher);
    style.Shadow.to_bits().hash(hasher);
    style.Alignment.hash(hasher);
    style.MarginL.hash(hasher);
    style.MarginR.hash(hasher);
    style.MarginV.hash(hasher);
    style.Encoding.hash(hasher);
    style.treat_fontname_as_pattern.hash(hasher);
    style.Blur.to_bits().hash(hasher);
    style.Justify.hash(hasher);
}

unsafe fn hash_event_for_cache(hasher: &mut DefaultHasher, event: &ASS_Event) {
    event.Start.hash(hasher);
    event.Duration.hash(hasher);
    event.ReadOrder.hash(hasher);
    event.Layer.hash(hasher);
    event.Style.hash(hasher);
    hash_c_string_for_cache(hasher, event.Name);
    event.MarginL.hash(hasher);
    event.MarginR.hash(hasher);
    event.MarginV.hash(hasher);
    hash_c_string_for_cache(hasher, event.Effect);
    hash_c_string_for_cache(hasher, event.Text);
}

unsafe fn public_track_content_fingerprint(
    track: &ASS_Track,
    active_event_indices: &[usize],
) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_c_string_for_cache(&mut hasher, track.style_format);
    hash_c_string_for_cache(&mut hasher, track.event_format);
    hash_c_string_for_cache(&mut hasher, track.Language);

    if !track.styles.is_null() && track.n_styles > 0 {
        for style in slice::from_raw_parts(track.styles, track.n_styles as usize) {
            hash_style_for_cache(&mut hasher, style);
        }
    }

    active_event_indices.hash(&mut hasher);
    if !track.events.is_null() && track.n_events > 0 {
        let events = slice::from_raw_parts(track.events, track.n_events as usize);
        for index in active_event_indices {
            if let Some(event) = events.get(*index) {
                hash_event_for_cache(&mut hasher, event);
            }
        }
    }
    hasher.finish()
}

unsafe fn cached_parsed_track_from_ffi<'a>(
    track: *mut ASS_Track,
    track_ref: &ASS_Track,
    signature: ParsedTrackCacheSignature,
) -> &'a ParsedTrack {
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

fn frame_cache_time_key(
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

    Some(now)
}

fn active_events_are_static(track: &ParsedTrack, active_event_indices: &[usize]) -> bool {
    active_event_indices.iter().all(|index| {
        track.events.get(*index).is_some_and(|event| {
            event_text_is_static(track, event) && event.effect.trim().is_empty()
        })
    })
}

fn event_text_is_static(track: &ParsedTrack, event: &ParsedEvent) -> bool {
    let Some(style) = track
        .styles
        .get(event.style.max(0) as usize)
        .or_else(|| track.styles.first())
    else {
        return false;
    };
    let parsed = parse_dialogue_text(&event.text, style, &track.styles);
    parsed.movement.is_none()
        && parsed.movement_exact.is_none()
        && parsed.fade.is_none()
        && parsed.lines.iter().all(|line| {
            line.spans
                .iter()
                .all(|span| span.transforms.is_empty() && span.karaoke.is_none())
        })
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
        primary_colour: rgba_to_ass_color(style.PrimaryColour),
        secondary_colour: rgba_to_ass_color(style.SecondaryColour),
        outline_colour: rgba_to_ass_color(style.OutlineColour),
        back_colour: rgba_to_ass_color(style.BackColour),
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

fn apply_selective_style_overrides(
    track: &mut ParsedTrack,
    renderer: &ASS_Renderer,
    active_event_indices: &[usize],
) {
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

    let base_styles = track.styles.clone();
    if base_styles.is_empty() {
        return;
    }

    let scale = f64::from(track.play_res_y) / 288.0;
    for event_index in active_event_indices {
        let Some(event) = track.events.get(*event_index).cloned() else {
            continue;
        };
        let style_index =
            parsed_event_style_index_from_len(base_styles.len(), track.default_style, event.style);
        if event_is_explicit_for_selective_overrides(&base_styles, &event, style_index) {
            continue;
        }

        let mut style = base_styles[style_index].clone();
        apply_selective_style_override_fields(&mut style, user_style, requested, scale);
        let Ok(cloned_style_index) = i32::try_from(track.styles.len()) else {
            continue;
        };
        track.styles.push(style);
        if let Some(event) = track.events.get_mut(*event_index) {
            event.style = cloned_style_index;
        }
    }
}

fn apply_selective_style_override_fields(
    style: &mut ParsedStyle,
    user_style: &ParsedStyle,
    requested: c_int,
    scale: f64,
) {
    if requested & ass::override_bits::FULL_STYLE != 0 {
        *style = user_style.clone();
    }

    if requested & ass::override_bits::FONT_NAME != 0 {
        style.font_name = user_style.font_name.clone();
        style.treat_fontname_as_pattern = user_style.treat_fontname_as_pattern;
    }
    if requested & ass::override_bits::FONT_SIZE_FIELDS != 0 {
        style.font_size = user_style.font_size * scale;
        style.spacing = user_style.spacing * scale;
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
        style.outline = user_style.outline * scale;
        style.shadow = user_style.shadow * scale;
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
        style.blur = user_style.blur * scale;
    }
}

fn parsed_event_style_index_from_len(
    styles_len: usize,
    default_style: i32,
    event_style: i32,
) -> usize {
    if styles_len == 0 {
        return 0;
    }

    let candidate = usize::try_from(event_style).unwrap_or(0);
    if candidate < styles_len {
        candidate
    } else {
        usize::try_from(default_style)
            .ok()
            .filter(|index| *index < styles_len)
            .unwrap_or(0)
    }
}

fn event_is_explicit_for_selective_overrides(
    styles: &[ParsedStyle],
    event: &ParsedEvent,
    style_index: usize,
) -> bool {
    let Some(style) = styles.get(style_index).or_else(|| styles.first()) else {
        return true;
    };
    parse_dialogue_text(&event.text, style, styles).hard_override
        || event_has_libass_transition_effect(&event.effect)
}

fn event_has_libass_transition_effect(effect: &str) -> bool {
    if effect.starts_with("Banner;") {
        return effect.split(';').skip(1).take(1).count() == 1;
    }
    if effect.starts_with("Scroll up;") || effect.starts_with("Scroll down;") {
        return effect.split(';').skip(1).take(3).count() == 3;
    }
    false
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::CString, fs, path::PathBuf, ptr};

    fn read_be_u16(data: &[u8], offset: usize) -> u16 {
        u16::from_be_bytes([data[offset], data[offset + 1]])
    }

    fn read_be_u32(data: &[u8], offset: usize) -> usize {
        u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize
    }

    fn font_with_distinct_typographic_and_legacy_families() -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test/font2.otf");
        let mut data = fs::read(path).expect("Aileron fixture should be readable");
        let table_count = read_be_u16(&data, 4) as usize;
        let name_offset = (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|offset| &data[*offset..*offset + 4] == b"name")
            .map(|offset| read_be_u32(&data, offset + 8))
            .expect("fixture should contain an SFNT name table");
        let name_count = read_be_u16(&data, name_offset + 2) as usize;
        let mut changed = false;
        for index in 0..name_count {
            let record = name_offset + 6 + index * 12;
            let platform = read_be_u16(&data, record);
            let name_id = read_be_u16(&data, record + 6);
            if platform == 3 && name_id == 6 {
                data[record + 6..record + 8].copy_from_slice(&16_u16.to_be_bytes());
                changed = true;
            }
        }
        assert!(changed, "fixture should contain a Windows PostScript name");
        data
    }

    fn font_with_windows_family(path: &str, family: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test")
            .join(path);
        let mut data = fs::read(path).expect("font fixture should be readable");
        let table_count = read_be_u16(&data, 4) as usize;
        let name_offset = (0..table_count)
            .map(|index| 12 + index * 16)
            .find(|offset| &data[*offset..*offset + 4] == b"name")
            .map(|offset| read_be_u32(&data, offset + 8))
            .expect("fixture should contain an SFNT name table");
        let name_count = read_be_u16(&data, name_offset + 2) as usize;
        let storage_offset = read_be_u16(&data, name_offset + 4) as usize;
        let encoded = family
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect::<Vec<_>>();
        let mut changed = false;
        for index in 0..name_count {
            let record = name_offset + 6 + index * 12;
            let platform = read_be_u16(&data, record);
            let name_id = read_be_u16(&data, record + 6);
            if platform != 3 || name_id != 1 {
                continue;
            }
            let old_length = read_be_u16(&data, record + 8) as usize;
            assert!(encoded.len() <= old_length, "replacement family must fit");
            let string_offset = read_be_u16(&data, record + 10) as usize;
            let start = name_offset + storage_offset + string_offset;
            data[record + 8..record + 10].copy_from_slice(&(encoded.len() as u16).to_be_bytes());
            data[start..start + encoded.len()].copy_from_slice(&encoded);
            changed = true;
        }
        assert!(changed, "fixture should contain a Windows family name");
        data
    }

    fn collection_from_faces(first: &[u8], second: &[u8]) -> Vec<u8> {
        let first_offset = 20_usize.next_multiple_of(4);
        let second_offset = (first_offset + first.len()).next_multiple_of(4);
        let mut collection = vec![0_u8; second_offset + second.len()];
        collection[0..4].copy_from_slice(b"ttcf");
        collection[4..8].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
        collection[8..12].copy_from_slice(&2_u32.to_be_bytes());
        collection[12..16].copy_from_slice(&(first_offset as u32).to_be_bytes());
        collection[16..20].copy_from_slice(&(second_offset as u32).to_be_bytes());
        collection[first_offset..first_offset + first.len()].copy_from_slice(first);
        collection[second_offset..second_offset + second.len()].copy_from_slice(second);
        for (font, base_offset) in [(first, first_offset), (second, second_offset)] {
            let table_count = read_be_u16(font, 4) as usize;
            for index in 0..table_count {
                let table = base_offset + 12 + index * 16;
                let original_offset = read_be_u32(&collection, table + 8);
                collection[table + 8..table + 12]
                    .copy_from_slice(&((original_offset + base_offset) as u32).to_be_bytes());
            }
        }
        collection
    }

    fn two_face_font_collection() -> Vec<u8> {
        let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rassa-test/fixtures/libass/compare/test");
        let first = fs::read(fixture_root.join("font1.ttf")).expect("TTF fixture should read");
        let second = fs::read(fixture_root.join("font2.otf")).expect("OTF fixture should read");
        collection_from_faces(&first, &second)
    }

    fn unique_test_directory(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("rassa-{label}-{}-{nonce}", std::process::id()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[repr(C)]
    struct FormattedMessageBridge {
        sink: unsafe extern "C" fn(c_int, *const c_char, *mut c_void),
        data: *mut c_void,
    }

    #[cfg(not(target_arch = "wasm32"))]
    unsafe extern "C" {
        fn rassa_formatted_sink_callback_pointer() -> *mut c_void;
    }

    #[cfg(not(target_arch = "wasm32"))]
    unsafe extern "C" fn capture_formatted_message(
        level: c_int,
        message: *const c_char,
        data: *mut c_void,
    ) {
        let messages = &mut *data.cast::<Vec<(c_int, String)>>();
        messages.push((
            level,
            CStr::from_ptr(message).to_string_lossy().into_owned(),
        ));
    }

    #[test]
    fn renderer_init_defaults_match_libass() {
        unsafe {
            let library = ass_library_init();
            assert!(!library.is_null());
            let renderer = ass_renderer_init(library);
            assert!(!renderer.is_null());

            assert_eq!((*renderer).hinting, ass::Hinting::None as c_int);
            assert_eq!(
                (*renderer).selective_override_bits,
                ass::override_bits::SELECTIVE_FONT_SCALE
            );

            ass_renderer_done(renderer);
            ass_library_done(library);
        }
    }

    #[test]
    fn cache_limit_arguments_use_libass_defaults_and_megabytes() {
        let defaults = RasterCacheLimits::default();
        assert_eq!(raster_cache_limits_from_c(0, 0), defaults);
        assert_eq!(
            raster_cache_limits_from_c(7, 3),
            RasterCacheLimits {
                glyph_max: 7,
                bitmap_max_bytes: 3 * 1024 * 1024,
            }
        );
        assert_eq!(
            raster_cache_limits_from_c(-1, -1),
            RasterCacheLimits {
                glyph_max: usize::MAX,
                bitmap_max_bytes: usize::MAX,
            }
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn c_cache_limits_bound_growth_preserve_output_and_isolate_renderers() {
        unsafe {
            let library = ass_library_init();
            assert!(!library.is_null());
            let renderer = ass_renderer_init(library);
            let other_renderer = ass_renderer_init(library);
            assert!(!renderer.is_null());
            assert!(!other_renderer.is_null());

            let font_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../rassa-test/fixtures/libass/compare/test/font2.otf");
            let font_path =
                CString::new(font_path.to_string_lossy().as_bytes()).expect("font path cstring");
            let family = CString::new("Aileron").expect("font family cstring");
            for target in [renderer, other_renderer] {
                ass_set_frame_size(target, 640, 360);
                ass_set_fonts(
                    target,
                    font_path.as_ptr(),
                    family.as_ptr(),
                    ass::DefaultFontProvider::None as c_int,
                    ptr::null(),
                    0,
                );
            }

            let script = b"[Script Info]\n\
                ScriptType: v4.00+\n\
                PlayResX: 640\n\
                PlayResY: 360\n\
                [V4+ Styles]\n\
                Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
                Style: Default,Aileron,72,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,1,0,5,10,10,10,1\n\
                [Events]\n\
                Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                Dialogue: 0,0:00:00.00,0:00:04.00,Default,,0,0,0,,ABCDEFGHIJ\n";
            let track = ass_read_memory(
                library,
                script.as_ptr().cast::<c_char>().cast_mut(),
                script.len(),
                ptr::null(),
            );
            assert!(!track.is_null());

            ass_set_cache_limits(renderer, 2, 1);
            let first = ass_render_frame(renderer, track, 1_000, ptr::null_mut());
            assert!(!first.is_null());
            let namespace = (*renderer).raster_cache_namespace;
            let small_stats = Rasterizer::cache_stats_for_namespace(namespace);
            assert!((1..=2).contains(&small_stats.glyph_entries));
            assert!(small_stats.bitmap_bytes <= 1024 * 1024);
            let first_bitmaps = (*renderer)
                .rendered_images
                .as_ref()
                .expect("first frame retained")
                .bitmaps
                .clone();
            assert!(first_bitmaps.iter().any(|bitmap| !bitmap.is_empty()));

            ass_set_cache_limits(renderer, 64, 1);
            (*renderer).frame_cache_signature = None;
            let second = ass_render_frame(renderer, track, 1_000, ptr::null_mut());
            assert!(!second.is_null());
            let expanded_stats = Rasterizer::cache_stats_for_namespace(namespace);
            assert!(expanded_stats.glyph_entries > small_stats.glyph_entries);
            assert!(expanded_stats.glyph_entries <= 64);
            assert!(expanded_stats.bitmap_bytes <= 1024 * 1024);
            assert_eq!(
                (*renderer)
                    .rendered_images
                    .as_ref()
                    .expect("rerendered frame retained")
                    .bitmaps,
                first_bitmaps,
                "eviction and rerasterization must not return stale glyph data"
            );

            ass_set_cache_limits(renderer, 1, 1);
            assert!(Rasterizer::cache_stats_for_namespace(namespace).glyph_entries <= 1);

            ass_set_cache_limits(other_renderer, 64, 1);
            assert!(!ass_render_frame(other_renderer, track, 1_000, ptr::null_mut()).is_null());
            let other_namespace = (*other_renderer).raster_cache_namespace;
            let other_stats = Rasterizer::cache_stats_for_namespace(other_namespace);
            assert!(other_stats.glyph_entries > 1);
            ass_set_cache_limits(renderer, 1, 1);
            assert_eq!(
                Rasterizer::cache_stats_for_namespace(other_namespace),
                other_stats,
                "one renderer's setter must not evict another renderer's glyphs"
            );

            ass_free_track(track);
            ass_renderer_done(renderer);
            assert_eq!(
                Rasterizer::cache_stats_for_namespace(namespace),
                rassa_raster::RasterCacheStats::default()
            );
            assert_eq!(
                Rasterizer::cache_stats_for_namespace(other_namespace),
                other_stats
            );
            ass_renderer_done(other_renderer);
            ass_library_done(library);
        }
    }

    #[test]
    fn all_current_libass_track_features_are_accepted_and_meta_toggled() {
        unsafe {
            let library = ass_library_init();
            let track = ass_new_track(library);
            assert!(!track.is_null());

            for feature in 1..=3 {
                assert_eq!(ass_track_set_feature(track, feature, 1), 0);
                assert!(track_state_ref(track).unwrap().features[feature as usize]);
                assert_eq!(ass_track_set_feature(track, feature, 0), 0);
                assert!(!track_state_ref(track).unwrap().features[feature as usize]);
            }
            assert_eq!(ass_track_set_feature(track, 0, 1), 0);
            assert_eq!(
                &track_state_ref(track).unwrap().features[1..],
                &[true, true, true]
            );
            assert_eq!(ass_track_set_feature(track, 0, 0), 0);
            assert_eq!(
                &track_state_ref(track).unwrap().features[1..],
                &[false, false, false]
            );
            assert_eq!(ass_track_set_feature(track, -1, 1), -1);
            assert_eq!(ass_track_set_feature(track, 4, 1), -1);

            ass_free_track(track);
            ass_library_done(library);
        }
    }

    #[test]
    fn allocator_and_public_c_string_lifecycles_are_compatible() {
        unsafe {
            ass_free(ptr::null_mut());

            for size in [1_usize, 16, 257] {
                let allocation = ass_malloc(size).cast::<u8>();
                assert!(!allocation.is_null());
                for offset in 0..size {
                    allocation.add(offset).write((offset & 0xff) as u8);
                }
                ass_free(allocation.cast());
            }

            let internal = string_to_c_ptr("embedded\0nul");
            assert!(!internal.is_null());
            assert_eq!(CStr::from_ptr(internal).to_bytes(), b"embedded nul");
            ass_free(internal.cast());

            let bytes = b"caller-owned\0";
            let mut caller_owned = ass_malloc(bytes.len()).cast::<c_char>();
            assert!(!caller_owned.is_null());
            ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), caller_owned, bytes.len());
            free_c_string(&mut caller_owned);
            assert!(caller_owned.is_null());
        }
    }

    #[test]
    fn available_font_providers_match_the_target_backend() {
        unsafe {
            let mut providers = ptr::null_mut();
            let mut size = 0;
            ass_get_available_font_providers(ptr::null_mut(), &mut providers, &mut size);

            assert!(!providers.is_null());
            assert_eq!(size, AVAILABLE_FONT_PROVIDERS.len());
            assert_eq!(
                slice::from_raw_parts(providers, size),
                AVAILABLE_FONT_PROVIDERS
            );
            ass_free(providers.cast());
        }
    }

    #[test]
    fn forced_system_provider_routes_are_target_scoped() {
        assert_eq!(
            system_font_provider_is_available(ass::DefaultFontProvider::Autodetect as c_int),
            !cfg!(target_arch = "wasm32")
        );
        assert_eq!(
            system_font_provider_is_available(ass::DefaultFontProvider::CoreText as c_int),
            cfg!(target_os = "macos")
        );
        assert_eq!(
            system_font_provider_is_available(ass::DefaultFontProvider::Fontconfig as c_int),
            cfg!(all(
                unix,
                not(target_os = "macos"),
                not(target_arch = "wasm32")
            ))
        );
        assert_eq!(
            system_font_provider_is_available(ass::DefaultFontProvider::DirectWrite as c_int),
            cfg!(windows)
        );
        assert!(!system_font_provider_is_available(99));
    }

    #[test]
    fn add_font_ignores_invalid_inputs_like_libass() {
        unsafe {
            let library = ass_library_init();
            assert!(!library.is_null());
            let name = CString::new("font.ttf").expect("font name cstring");
            let data = [1_u8, 2, 3, 4];

            ass_add_font(
                library,
                ptr::null(),
                data.as_ptr() as *const c_char,
                data.len() as c_int,
            );
            ass_add_font(library, name.as_ptr(), ptr::null(), data.len() as c_int);
            ass_add_font(library, name.as_ptr(), data.as_ptr() as *const c_char, 0);
            ass_add_font(library, name.as_ptr(), data.as_ptr() as *const c_char, -1);
            assert_eq!((*library).fonts.len(), 0);

            ass_add_font(
                library,
                name.as_ptr(),
                data.as_ptr() as *const c_char,
                data.len() as c_int,
            );
            assert_eq!((*library).fonts.len(), 1);
            let fonts = &(*library).fonts;
            assert_eq!(fonts[0].name, "font.ttf");
            assert_eq!(fonts[0].data, data);

            ass_clear_fonts(library);
            assert_eq!((*library).fonts.len(), 0);
            ass_library_done(library);
        }
    }

    #[test]
    fn set_message_cb_ignores_null_callback_like_libass() {
        unsafe {
            let library = ass_library_init();
            assert!(!library.is_null());
            let mut callback_slot = ();
            let mut data_slot = ();
            let mut replacement_data_slot = ();
            let callback = (&mut callback_slot as *mut ()).cast::<c_void>();
            let data = (&mut data_slot as *mut ()).cast::<c_void>();
            let replacement_data = (&mut replacement_data_slot as *mut ()).cast::<c_void>();

            ass_set_message_cb(library, callback, data);
            assert_eq!((*library).message_cb, callback);
            assert_eq!((*library).message_data, data);

            ass_set_message_cb(library, ptr::null_mut(), replacement_data);
            assert_eq!((*library).message_cb, callback);
            assert_eq!((*library).message_data, data);

            ass_library_done(library);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn message_callback_receives_formatted_parser_file_font_and_provider_warnings() {
        unsafe {
            let library = ass_library_init();
            let mut messages = Vec::<(c_int, String)>::new();
            let mut bridge = FormattedMessageBridge {
                sink: capture_formatted_message,
                data: (&mut messages as *mut Vec<(c_int, String)>).cast(),
            };
            ass_set_message_cb(
                library,
                rassa_formatted_sink_callback_pointer(),
                (&mut bridge as *mut FormattedMessageBridge).cast(),
            );

            let missing_file = unique_test_directory("missing-subtitle");
            let missing_file = CString::new(missing_file.to_string_lossy().as_bytes())
                .expect("missing path cstring");
            assert!(ass_read_file(library, missing_file.as_ptr(), ptr::null()).is_null());

            let invalid_script = b"this is not an ASS subtitle";
            let track = ass_read_memory(
                library,
                invalid_script.as_ptr() as *mut c_char,
                invalid_script.len(),
                ptr::null(),
            );
            assert!(track.is_null());

            let missing_fonts = unique_test_directory("missing-fonts");
            let missing_fonts = CString::new(missing_fonts.to_string_lossy().as_bytes())
                .expect("missing font path cstring");
            ass_set_fonts_dir(library, missing_fonts.as_ptr());
            let renderer = ass_renderer_init(library);
            (*renderer).default_provider = 99;
            drop(build_font_provider(&*renderer, library));

            assert!(
                messages
                    .iter()
                    .any(|(_, message)| message.contains("read failed"))
            );
            assert!(
                messages
                    .iter()
                    .any(|(_, message)| message.contains("No recognizable subtitle track"))
            );
            assert!(
                messages
                    .iter()
                    .any(|(_, message)| message.contains("font directory scan"))
            );
            assert!(
                messages
                    .iter()
                    .any(|(_, message)| message.contains("can't find selected font provider 99"))
            );
            assert!(
                messages
                    .iter()
                    .all(|(level, _)| *level <= MESSAGE_LEVEL_WARNING)
            );

            ass_renderer_done(renderer);
            ass_library_done(library);
        }
    }

    #[test]
    fn fonts_dir_provider_resolves_usable_font_without_system_provider() {
        unsafe {
            let system = CrossfontProvider::new().resolve_family("sans");
            let source = system.path.expect("system font path should exist");
            let directory = unique_test_directory("capi-font-directory");
            fs::create_dir_all(&directory).expect("font directory should be creatable");
            let copied = directory.join("font.data");
            fs::copy(&source, &copied).expect("font fixture should copy");

            let library = ass_library_init();
            let directory_c = CString::new(directory.to_string_lossy().as_bytes())
                .expect("font directory cstring");
            ass_set_fonts_dir(library, directory_c.as_ptr());
            let renderer = ass_renderer_init(library);
            let family = CString::new(system.family.clone()).expect("font family cstring");
            ass_set_fonts(
                renderer,
                ptr::null(),
                family.as_ptr(),
                ass::DefaultFontProvider::None as c_int,
                ptr::null(),
                0,
            );

            let provider = build_font_provider(&*renderer, library);
            let resolved = provider.resolve_family(&system.family);
            assert_eq!(resolved.provider, rassa_fonts::FontProviderKind::Attached);
            assert_eq!(resolved.path, Some(copied));
            drop(provider);

            ass_renderer_done(renderer);
            ass_library_done(library);
            fs::remove_dir_all(&directory).expect("font fixture should clean up");
        }
    }

    #[test]
    fn fonts_dir_provider_resolves_legacy_family_alias_through_capi_state() {
        unsafe {
            let directory = unique_test_directory("capi-font-family-alias");
            fs::create_dir_all(&directory).expect("font directory should be creatable");
            let copied = directory.join("unrelated-name.data");
            fs::write(
                &copied,
                font_with_distinct_typographic_and_legacy_families(),
            )
            .expect("mutated font fixture should write");

            let library = ass_library_init();
            let directory_c = CString::new(directory.to_string_lossy().as_bytes())
                .expect("font directory cstring");
            ass_set_fonts_dir(library, directory_c.as_ptr());
            let renderer = ass_renderer_init(library);
            ass_set_fonts(
                renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as c_int,
                ptr::null(),
                0,
            );

            let provider = build_font_provider(&*renderer, library);
            let resolved = provider.resolve_family("Aileron");
            assert_eq!(resolved.provider, rassa_fonts::FontProviderKind::Attached);
            assert_eq!(resolved.family, "Aileron");
            assert_eq!(resolved.path, Some(copied));
            drop(provider);

            ass_renderer_done(renderer);
            ass_library_done(library);
            fs::remove_dir_all(&directory).expect("font fixture should clean up");
        }
    }

    #[test]
    fn added_font_resolves_legacy_family_alias_through_capi_state() {
        unsafe {
            let data = font_with_distinct_typographic_and_legacy_families();
            let library = ass_library_init();
            let name = CString::new("unrelated-name.data").expect("font attachment name");
            ass_add_font(
                library,
                name.as_ptr(),
                data.as_ptr().cast(),
                data.len() as c_int,
            );
            let renderer = ass_renderer_init(library);
            ass_set_fonts(
                renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as c_int,
                ptr::null(),
                0,
            );

            let provider = build_font_provider(&*renderer, library);
            let resolved = provider.resolve_family("Aileron");
            assert_eq!(resolved.provider, rassa_fonts::FontProviderKind::Attached);
            assert_eq!(resolved.family, "Aileron");
            assert!(resolved.path.as_ref().is_some_and(|path| path.is_file()));
            drop(provider);

            ass_renderer_done(renderer);
            ass_library_done(library);
        }
    }

    #[test]
    fn added_font_resolves_nonzero_collection_face_through_capi_state() {
        unsafe {
            let data = two_face_font_collection();
            let library = ass_library_init();
            let name = CString::new("fixture.ttc").expect("font attachment name");
            ass_add_font(
                library,
                name.as_ptr(),
                data.as_ptr().cast(),
                data.len() as c_int,
            );
            let renderer = ass_renderer_init(library);
            ass_set_fonts(
                renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as c_int,
                ptr::null(),
                0,
            );

            let provider = build_font_provider(&*renderer, library);
            let first = provider.resolve_family("Pixel Operator Mono");
            let second = provider.resolve_family("Aileron");
            assert_eq!(first.provider, rassa_fonts::FontProviderKind::Attached);
            assert_eq!(first.face_index, None);
            assert_eq!(second.provider, rassa_fonts::FontProviderKind::Attached);
            assert_eq!(second.face_index, Some(1));
            assert_eq!(second.path, first.path);
            drop(provider);

            ass_renderer_done(renderer);
            ass_library_done(library);
        }
    }

    #[test]
    fn capi_render_preserves_coverage_selected_collection_face() {
        unsafe fn render_with_font(data: &[u8], name: &str) -> Vec<(c_int, c_int, Vec<u8>)> {
            let library = unsafe { ass_library_init() };
            let name = CString::new(name).expect("font attachment name");
            unsafe {
                ass_add_font(
                    library,
                    name.as_ptr(),
                    data.as_ptr().cast(),
                    data.len() as c_int,
                );
            }
            let renderer = unsafe { ass_renderer_init(library) };
            unsafe {
                ass_set_frame_size(renderer, 640, 360);
                ass_set_fonts(
                    renderer,
                    ptr::null(),
                    ptr::null(),
                    ass::DefaultFontProvider::None as c_int,
                    ptr::null(),
                    0,
                );
            }
            let script = "[Script Info]\n\
                ScriptType: v4.00+\n\
                PlayResX: 640\n\
                PlayResY: 360\n\
                [V4+ Styles]\n\
                Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
                Style: Default,Shared,64,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,1,0,5,10,10,10,1\n\
                [Events]\n\
                Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n\
                Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,∂\n";
            let track = unsafe {
                ass_read_memory(
                    library,
                    script.as_ptr().cast::<c_char>().cast_mut(),
                    script.len(),
                    ptr::null(),
                )
            };
            assert!(!track.is_null());
            assert!(
                !unsafe { ass_render_frame(renderer, track, 1_000, ptr::null_mut()) }.is_null()
            );
            let images = unsafe {
                (*renderer)
                    .rendered_images
                    .as_ref()
                    .expect("rendered frame should be retained")
            };
            let snapshot = images
                .nodes
                .iter()
                .zip(&images.bitmaps)
                .map(|(node, bitmap)| (node.w, node.h, bitmap.clone()))
                .collect();
            unsafe {
                ass_free_track(track);
                ass_renderer_done(renderer);
                ass_library_done(library);
            }
            snapshot
        }

        let first = font_with_windows_family("font1.ttf", "Shared");
        let second = font_with_windows_family("font2.otf", "Shared");
        let collection = collection_from_faces(&first, &second);

        unsafe {
            let library = ass_library_init();
            let name = CString::new("shared.ttc").expect("font attachment name");
            ass_add_font(
                library,
                name.as_ptr(),
                collection.as_ptr().cast(),
                collection.len() as c_int,
            );
            let renderer = ass_renderer_init(library);
            ass_set_fonts(
                renderer,
                ptr::null(),
                ptr::null(),
                ass::DefaultFontProvider::None as c_int,
                ptr::null(),
                0,
            );
            let provider = build_font_provider(&*renderer, library);
            let query = rassa_fonts::FontQuery {
                family: "Shared".to_owned(),
                style: Some("Bold".to_owned()),
                weight: Some(700),
            };
            assert_eq!(provider.resolve(&query).face_index, None);
            assert_eq!(provider.resolve_for_text(&query, "∂").face_index, Some(1));
            drop(provider);
            ass_renderer_done(renderer);
            ass_library_done(library);
        }

        let collection_render = unsafe { render_with_font(&collection, "shared.ttc") };
        let standalone_render = unsafe { render_with_font(&second, "shared.otf") };
        assert_eq!(
            collection_render, standalone_render,
            "the selected collection face must render identically to the exact standalone face"
        );
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
    #[test]
    fn set_fonts_honors_custom_fontconfig_configuration() {
        unsafe {
            let system = CrossfontProvider::new().resolve_family("sans");
            let source = system.path.expect("system font path should exist");
            let directory = unique_test_directory("capi-fontconfig");
            let font_directory = directory.join("fonts");
            fs::create_dir_all(&font_directory).expect("font directory should be creatable");
            let copied = font_directory.join("configured-font.ttf");
            fs::copy(&source, &copied).expect("font fixture should copy");
            let config = directory.join("fonts.conf");
            fs::write(
                &config,
                format!(
                    "<?xml version=\"1.0\"?><!DOCTYPE fontconfig SYSTEM \"urn:fontconfig:fonts.dtd\"><fontconfig><dir>{}</dir></fontconfig>",
                    font_directory.display()
                ),
            )
            .expect("fontconfig fixture should write");

            let library = ass_library_init();
            let renderer = ass_renderer_init(library);
            let family = CString::new(system.family.clone()).expect("font family cstring");
            let config_c =
                CString::new(config.to_string_lossy().as_bytes()).expect("config cstring");
            ass_set_fonts(
                renderer,
                ptr::null(),
                family.as_ptr(),
                ass::DefaultFontProvider::Fontconfig as c_int,
                config_c.as_ptr(),
                0,
            );

            let provider = build_font_provider(&*renderer, library);
            let resolved = provider.resolve_family(&system.family);
            assert_eq!(
                resolved
                    .path
                    .as_deref()
                    .and_then(|path| path.canonicalize().ok()),
                copied.canonicalize().ok()
            );
            drop(provider);

            ass_renderer_done(renderer);
            ass_library_done(library);
            fs::remove_dir_all(&directory).expect("fontconfig fixture should clean up");
        }
    }

    #[test]
    fn process_data_extracts_sectionless_fonts_in_fonts_state_like_libass() {
        unsafe {
            let library = ass_library_init();
            ass_set_extract_fonts(library, 1);
            let track = ass_new_track(library);
            let header = b"[Fonts]\n";
            ass_process_data(
                track,
                header.as_ptr() as *const c_char,
                header.len() as c_int,
            );

            let encoded = encode_font_bytes(b"ABC");
            let font = format!("fontname: Stream.ttf\n{encoded}\n");
            ass_process_data(track, font.as_ptr() as *const c_char, font.len() as c_int);
            assert_eq!((*library).fonts.len(), 1);
            let fonts = &(*library).fonts;
            assert_eq!(fonts[0].name, "Stream.ttf");
            assert_eq!(fonts[0].data, b"ABC");

            let mixed = format!(
                "fontname: Prefix.otf\n{encoded}\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize\nStyle: Later,Arial,20\n"
            );
            ass_process_data(track, mixed.as_ptr() as *const c_char, mixed.len() as c_int);
            assert_eq!((*library).fonts.len(), 2);
            let fonts = &(*library).fonts;
            assert_eq!(fonts[1].name, "Prefix.otf");
            assert_eq!(fonts[1].data, b"ABC");
            assert_eq!((*track).n_styles, 2);

            ass_free_track(track);
            ass_library_done(library);
        }
    }

    #[test]
    fn process_data_does_not_carry_font_payload_across_calls_like_libass() {
        unsafe {
            let library = ass_library_init();
            ass_set_extract_fonts(library, 1);
            let track = ass_new_track(library);
            let header = b"[Fonts]\n";
            ass_process_data(
                track,
                header.as_ptr() as *const c_char,
                header.len() as c_int,
            );

            let name_only = b"fontname: Empty.ttf\n";
            ass_process_data(
                track,
                name_only.as_ptr() as *const c_char,
                name_only.len() as c_int,
            );
            assert_eq!((*library).fonts.len(), 0);

            let encoded = encode_font_bytes(b"ABC");
            ass_process_data(
                track,
                encoded.as_ptr() as *const c_char,
                encoded.len() as c_int,
            );
            assert_eq!((*library).fonts.len(), 0);

            ass_free_track(track);
            ass_library_done(library);
        }
    }

    fn encode_font_bytes(bytes: &[u8]) -> String {
        let mut encoded = String::new();
        for chunk in bytes.chunks(3) {
            let value = match chunk.len() {
                1 => u32::from(chunk[0]) << 16,
                2 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8),
                _ => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]),
            };
            for index in 0..(chunk.len() + 1) {
                let shift = 6 * (3 - index);
                encoded.push(char::from(((value >> shift) as u8 & 0x3f) + 33));
            }
        }
        encoded
    }
}
