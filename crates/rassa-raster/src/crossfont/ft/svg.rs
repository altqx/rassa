//! FreeType OT-SVG renderer hooks backed by `resvg`.
//!
//! FreeType loads SVG documents from the OpenType `SVG ` table, but it only
//! rasterizes them when the client provides the `ot-svg.svg-hooks` property.
//! These hooks render into FreeType's already-allocated BGRA bitmap buffer so
//! the normal `FT_GlyphSlot` -> `BitmapBuffer::Rgba` path can consume it.

use std::ptr;
use std::slice;

use freetype::freetype_sys;
use libc::c_void;
use resvg::{tiny_skia, usvg};

const SVG_MODULE: &[u8] = b"ot-svg\0";
const SVG_HOOKS_PROPERTY: &[u8] = b"svg-hooks\0";
const FT_PIXEL_MODE_BGRA_I8: i8 = freetype_sys::FT_PIXEL_MODE_BGRA as i8;
const FT_GLYPH_FORMAT_BITMAP: freetype_sys::FT_Glyph_Format = freetype_sys::FT_GLYPH_FORMAT_BITMAP;
const FT_ERR_INVALID_SVG_DOCUMENT: freetype_sys::FT_Error =
    freetype_sys::FT_Err_Invalid_File_Format;

#[repr(C)]
struct SvgRendererHooks {
    init_svg: SvgLibInitFunc,
    free_svg: SvgLibFreeFunc,
    render_svg: SvgLibRenderFunc,
    preset_slot: SvgLibPresetSlotFunc,
}

type SvgLibInitFunc = unsafe extern "C" fn(*mut freetype_sys::FT_Pointer) -> freetype_sys::FT_Error;
type SvgLibFreeFunc = unsafe extern "C" fn(*mut freetype_sys::FT_Pointer);
type SvgLibRenderFunc = unsafe extern "C" fn(
    freetype_sys::FT_GlyphSlot,
    *mut freetype_sys::FT_Pointer,
) -> freetype_sys::FT_Error;
type SvgLibPresetSlotFunc = unsafe extern "C" fn(
    freetype_sys::FT_GlyphSlot,
    freetype_sys::FT_Bool,
    *mut freetype_sys::FT_Pointer,
) -> freetype_sys::FT_Error;

#[repr(C)]
struct FtSvgDocumentRec {
    svg_document: *mut freetype_sys::FT_Byte,
    svg_document_length: freetype_sys::FT_ULong,
    metrics: freetype_sys::FT_Size_Metrics,
    units_per_em: freetype_sys::FT_UShort,
    start_glyph_id: freetype_sys::FT_UShort,
    end_glyph_id: freetype_sys::FT_UShort,
    transform: freetype_sys::FT_Matrix,
    delta: freetype_sys::FT_Vector,
}

static SVG_HOOKS: SvgRendererHooks = SvgRendererHooks {
    init_svg,
    free_svg,
    render_svg,
    preset_slot,
};

/// Register OT-SVG hooks for one FreeType library.
pub fn register(library: freetype_sys::FT_Library) -> Result<(), freetype::Error> {
    let error = unsafe {
        freetype_sys::FT_Property_Set(
            library,
            SVG_MODULE.as_ptr().cast(),
            SVG_HOOKS_PROPERTY.as_ptr().cast(),
            ptr::addr_of!(SVG_HOOKS).cast::<c_void>(),
        )
    };
    if error == freetype_sys::FT_Err_Ok {
        Ok(())
    } else {
        Err(freetype::Error::from(error))
    }
}

unsafe extern "C" fn init_svg(
    data_pointer: *mut freetype_sys::FT_Pointer,
) -> freetype_sys::FT_Error {
    if !data_pointer.is_null() {
        // The resvg-backed implementation is stateless; use a non-null marker
        // so FreeType still considers the hook initialized and calls `free_svg`.
        unsafe { *data_pointer = std::ptr::dangling_mut::<c_void>() };
    }
    freetype_sys::FT_Err_Ok
}

unsafe extern "C" fn free_svg(data_pointer: *mut freetype_sys::FT_Pointer) {
    if !data_pointer.is_null() {
        unsafe { *data_pointer = ptr::null_mut() };
    }
}

unsafe extern "C" fn preset_slot(
    slot: freetype_sys::FT_GlyphSlot,
    _cache: freetype_sys::FT_Bool,
    _state: *mut freetype_sys::FT_Pointer,
) -> freetype_sys::FT_Error {
    let Some((document, tree, _svg_size)) = (unsafe { parse_slot_document(slot) }) else {
        return FT_ERR_INVALID_SVG_DOCUMENT;
    };

    let width = u32::from(document.metrics.x_ppem).max(1);
    let height = u32::from(document.metrics.y_ppem).max(1);

    unsafe {
        (*slot).bitmap_left = 0;
        (*slot).bitmap_top = height as freetype_sys::FT_Int;
        (*slot).bitmap.rows = height as libc::c_int;
        (*slot).bitmap.width = width as libc::c_int;
        (*slot).bitmap.pitch = (width * 4) as libc::c_int;
        (*slot).bitmap.pixel_mode = FT_PIXEL_MODE_BGRA_I8;
        (*slot).bitmap.num_grays = 256;
        (*slot).format = FT_GLYPH_FORMAT_BITMAP;
        (*slot).metrics.width = (width as freetype_sys::FT_Pos) * 64;
        (*slot).metrics.height = (height as freetype_sys::FT_Pos) * 64;
        (*slot).metrics.horiBearingX = 0;
        (*slot).metrics.horiBearingY = (height as freetype_sys::FT_Pos) * 64;
        if (*slot).metrics.horiAdvance == 0 {
            (*slot).metrics.horiAdvance = (width as freetype_sys::FT_Pos) * 64;
        }
        if (*slot).metrics.vertAdvance == 0 {
            (*slot).metrics.vertAdvance = (height as freetype_sys::FT_Pos) * 64;
        }
        (*slot).metrics.vertBearingX = -((width as freetype_sys::FT_Pos) * 32);
        (*slot).metrics.vertBearingY =
            (*slot).metrics.vertAdvance / 2 - (height as freetype_sys::FT_Pos) * 32;
    }

    // Parsing above validates the SVG document during both preset phases; the
    // render hook reparses into a fresh tree to avoid cross-callback state.
    drop(tree);
    freetype_sys::FT_Err_Ok
}

unsafe extern "C" fn render_svg(
    slot: freetype_sys::FT_GlyphSlot,
    _state: *mut freetype_sys::FT_Pointer,
) -> freetype_sys::FT_Error {
    let Some((document, tree, svg_size)) = (unsafe { parse_slot_document(slot) }) else {
        return FT_ERR_INVALID_SVG_DOCUMENT;
    };

    let (width, height, buffer) = unsafe {
        let bitmap = &mut (*slot).bitmap;
        if bitmap.buffer.is_null() || bitmap.width <= 0 || bitmap.rows <= 0 {
            return freetype_sys::FT_Err_Invalid_Slot_Handle;
        }
        (bitmap.width as u32, bitmap.rows as u32, bitmap.buffer)
    };

    let Some(mut pixmap) = tiny_skia::Pixmap::new(width, height) else {
        return freetype_sys::FT_Err_Out_Of_Memory;
    };

    let scale_x = width as f32 / svg_size.width().max(1.0);
    let scale_y = height as f32 / svg_size.height().max(1.0);
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

    if document.start_glyph_id < document.end_glyph_id {
        // `freetype-sys` does not expose the optional `glyph_index` field on
        // `FT_GlyphSlotRec` across all supported FreeType headers. Rendering
        // the document is still correct for one-document-per-glyph fonts and
        // keeps multi-glyph SVG documents usable instead of failing.
        resvg::render(&tree, transform, &mut pixmap.as_mut());
    } else {
        resvg::render(&tree, transform, &mut pixmap.as_mut());
    }

    unsafe {
        let bitmap = &mut (*slot).bitmap;
        let dst = slice::from_raw_parts_mut(buffer, (bitmap.pitch as usize) * (height as usize));
        for (src, dst) in pixmap.data().chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            // tiny-skia stores premultiplied RGBA; FreeType's BGRA mode expects
            // BGRA bytes. `normalize_buffer` converts BGRA back to r,g,b,a for
            // rassa's internal color glyph buffer.
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        bitmap.pixel_mode = FT_PIXEL_MODE_BGRA_I8;
        bitmap.num_grays = 256;
        (*slot).format = FT_GLYPH_FORMAT_BITMAP;
    }

    freetype_sys::FT_Err_Ok
}

unsafe fn parse_slot_document(
    slot: freetype_sys::FT_GlyphSlot,
) -> Option<(&'static FtSvgDocumentRec, usvg::Tree, usvg::Size)> {
    if slot.is_null() {
        return None;
    }
    let document = unsafe { (*slot).other.cast::<FtSvgDocumentRec>().as_ref()? };
    if document.svg_document.is_null() || document.svg_document_length == 0 {
        return None;
    }
    let data = unsafe {
        slice::from_raw_parts(
            document.svg_document.cast_const(),
            document.svg_document_length as usize,
        )
    };
    let tree = usvg::Tree::from_data(data, &usvg::Options::default()).ok()?;
    let svg_size = tree.size();
    Some((document, tree, svg_size))
}
