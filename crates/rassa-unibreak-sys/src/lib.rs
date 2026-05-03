#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int};

pub type utf8_t = u8;
pub type utf16_t = u16;
pub type utf32_t = u32;

pub const LIBUNIBREAK_LINKED: bool = cfg!(libunibreak_available);

pub const LINEBREAK_MUSTBREAK: c_char = 0;
pub const LINEBREAK_ALLOWBREAK: c_char = 1;
pub const LINEBREAK_NOBREAK: c_char = 2;
pub const LINEBREAK_INSIDEACHAR: c_char = 3;
pub const LINEBREAK_INDETERMINATE: c_char = 4;

pub const WORDBREAK_BREAK: c_char = 0;
pub const WORDBREAK_NOBREAK: c_char = 1;
pub const WORDBREAK_INSIDEACHAR: c_char = 2;

#[cfg(libunibreak_available)]
unsafe extern "C" {
    pub fn init_linebreak();
    pub fn set_linebreaks_utf32(
        s: *const utf32_t,
        len: usize,
        lang: *const c_char,
        brks: *mut c_char,
    );
    pub fn is_line_breakable(char1: utf32_t, char2: utf32_t, lang: *const c_char) -> c_int;

    pub fn init_wordbreak();
    pub fn set_wordbreaks_utf32(
        s: *const utf32_t,
        len: usize,
        lang: *const c_char,
        brks: *mut c_char,
    );
}

/// Analyze Unicode line-break opportunities using libunibreak when available.
///
/// # Safety
///
/// `s` must point to at least `len` valid UTF-32 code units, `brks` must point
/// to writable storage for at least `len` break markers, and `lang` must either
/// be null or point to a valid NUL-terminated C string for the duration of the
/// call. The pointers must not alias in a way that violates Rust's aliasing
/// rules.
pub unsafe fn analyze_linebreaks_utf32(
    s: *const utf32_t,
    len: usize,
    lang: *const c_char,
    brks: *mut c_char,
) -> bool {
    #[cfg(libunibreak_available)]
    {
        unsafe {
            init_linebreak();
            set_linebreaks_utf32(s, len, lang, brks);
        }
        true
    }

    #[cfg(not(libunibreak_available))]
    {
        let _ = (s, len, lang, brks);
        false
    }
}

/// Analyze Unicode word-break opportunities using libunibreak when available.
///
/// # Safety
///
/// `s` must point to at least `len` valid UTF-32 code units, `brks` must point
/// to writable storage for at least `len` break markers, and `lang` must either
/// be null or point to a valid NUL-terminated C string for the duration of the
/// call. The pointers must not alias in a way that violates Rust's aliasing
/// rules.
pub unsafe fn analyze_wordbreaks_utf32(
    s: *const utf32_t,
    len: usize,
    lang: *const c_char,
    brks: *mut c_char,
) -> bool {
    #[cfg(libunibreak_available)]
    {
        unsafe {
            init_wordbreak();
            set_wordbreaks_utf32(s, len, lang, brks);
        }
        true
    }

    #[cfg(not(libunibreak_available))]
    {
        let _ = (s, len, lang, brks);
        false
    }
}
