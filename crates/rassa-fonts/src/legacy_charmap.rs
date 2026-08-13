//! Map Unicode to Microsoft legacy cmaps as in libass `ass_charmap_magic`.

use encoding_rs::{BIG5, EUC_KR, EncoderResult, Encoding, GBK, SHIFT_JIS};
use ttf_parser::{Face, GlyphId, PlatformId, Tag, cmap::Subtable};

use crate::legacy_arabic_charmap;

const WINDOWS_SYMBOL: u16 = 0;
const WINDOWS_UNICODE_BMP: u16 = 1;
const WINDOWS_SHIFT_JIS: u16 = 2;
const WINDOWS_GB2312: u16 = 3;
const WINDOWS_BIG5: u16 = 4;
const WINDOWS_WANSUNG: u16 = 5;
const WINDOWS_JOHAB: u16 = 6;
const WINDOWS_UNICODE_FULL: u16 = 10;

const ARABIC_CHARSET_SIMPLIFIED: u8 = 178;
const ARABIC_CHARSET_TRADITIONAL: u8 = 179;

/// Cmap chosen by libass's Microsoft-preference policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontCharmap {
    Unicode,
    Symbol { charset: Option<u8> },
    ShiftJis,
    Gb2312,
    Big5,
    Wansung,
    Johab,
    OtherMicrosoft(u16),
    Other,
}

impl FontCharmap {
    /// True when Unicode must be remapped before querying this cmap.
    pub const fn is_legacy(self) -> bool {
        matches!(
            self,
            Self::Symbol { .. }
                | Self::ShiftJis
                | Self::Gb2312
                | Self::Big5
                | Self::Wansung
                | Self::Johab
        )
    }
}

pub fn font_face_charmap(face: &Face<'_>) -> FontCharmap {
    selected_microsoft_charmap(face)
        .map(|(_, kind)| kind)
        .unwrap_or_else(|| {
            if face
                .tables()
                .cmap
                .is_some_and(|cmap| cmap.subtables.into_iter().any(|table| table.is_unicode()))
            {
                FontCharmap::Unicode
            } else {
                FontCharmap::Other
            }
        })
}

/// True when shaping must use the legacy-compatible glyph path.
pub fn font_face_uses_legacy_charmap(face: &Face<'_>) -> bool {
    font_face_charmap(face).is_legacy()
}

/// Glyph ID from the selected cmap; do not pass the remapped codepoint to the rasterizer.
pub fn font_face_glyph_index(face: &Face<'_>, character: char) -> Option<GlyphId> {
    let Some((subtable, kind)) = selected_microsoft_charmap(face) else {
        return face.glyph_index(character);
    };
    let codepoint = map_codepoint(kind, u32::from(character))?;
    subtable.glyph_index(codepoint)
}

pub fn font_data_glyph_index(data: &[u8], face_index: u32, character: char) -> Option<GlyphId> {
    let face = Face::parse(data, face_index).ok()?;
    font_face_glyph_index(&face, character)
}

fn selected_microsoft_charmap<'a>(face: &Face<'a>) -> Option<(Subtable<'a>, FontCharmap)> {
    let cmap = face.tables().cmap?;
    let mut first_microsoft = None;
    let mut microsoft_bmp = None;

    for subtable in cmap.subtables {
        if subtable.platform_id != PlatformId::Windows {
            continue;
        }
        if subtable.encoding_id == WINDOWS_UNICODE_FULL {
            return Some((subtable, FontCharmap::Unicode));
        }
        if subtable.encoding_id == WINDOWS_UNICODE_BMP {
            microsoft_bmp.get_or_insert(subtable);
        } else {
            first_microsoft.get_or_insert(subtable);
        }
    }

    let subtable = microsoft_bmp.or(first_microsoft)?;
    Some((
        subtable,
        classify_microsoft_charmap(face, subtable.encoding_id),
    ))
}

fn classify_microsoft_charmap(face: &Face<'_>, encoding_id: u16) -> FontCharmap {
    match encoding_id {
        WINDOWS_SYMBOL => FontCharmap::Symbol {
            charset: os2_charset(face),
        },
        WINDOWS_UNICODE_BMP | WINDOWS_UNICODE_FULL => FontCharmap::Unicode,
        WINDOWS_SHIFT_JIS => FontCharmap::ShiftJis,
        WINDOWS_GB2312 => FontCharmap::Gb2312,
        WINDOWS_BIG5 => FontCharmap::Big5,
        WINDOWS_WANSUNG => FontCharmap::Wansung,
        WINDOWS_JOHAB => FontCharmap::Johab,
        other => FontCharmap::OtherMicrosoft(other),
    }
}

fn os2_charset(face: &Face<'_>) -> Option<u8> {
    // OS/2.fsSelection (BE u16 at 62) stores Arabic charset 178/179 in the reserved high byte.
    let os2 = face.raw_face().table(Tag::from_bytes(b"OS/2"))?;
    let selection = u16::from_be_bytes([*os2.get(62)?, *os2.get(63)?]);
    Some((selection >> 8) as u8)
}

fn map_codepoint(charmap: FontCharmap, symbol: u32) -> Option<u32> {
    match charmap {
        FontCharmap::Unicode | FontCharmap::Other | FontCharmap::OtherMicrosoft(_) => Some(symbol),
        FontCharmap::Symbol {
            charset: Some(ARABIC_CHARSET_SIMPLIFIED),
        } => Some(legacy_arabic_charmap::simplified(symbol)),
        FontCharmap::Symbol {
            charset: Some(ARABIC_CHARSET_TRADITIONAL),
        } => Some(legacy_arabic_charmap::traditional(symbol)),
        FontCharmap::Symbol { .. } => Some(0xF000 | symbol),
        FontCharmap::ShiftJis => encode_multibyte(SHIFT_JIS, symbol),
        FontCharmap::Gb2312 => encode_multibyte(GBK, symbol),
        FontCharmap::Big5 => encode_multibyte(BIG5, symbol),
        FontCharmap::Wansung => encode_multibyte(EUC_KR, symbol),
        FontCharmap::Johab => encode_johab(symbol),
    }
}

fn encode_multibyte(encoding: &'static Encoding, symbol: u32) -> Option<u32> {
    let character = char::from_u32(symbol)?;
    let mut input = [0; 4];
    let input = character.encode_utf8(&mut input);
    // encoding_rs may want a 4-byte buffer; reject encodings that are not 1-2 bytes.
    let mut output = [0; 4];
    let (result, read, written) =
        encoding
            .new_encoder()
            .encode_from_utf8_without_replacement(input, &mut output, true);
    if result != EncoderResult::InputEmpty || read != input.len() || !(1..=2).contains(&written) {
        return None;
    }
    Some(
        output[..written]
            .iter()
            .fold(0_u32, |packed, byte| (packed << 8) | u32::from(*byte)),
    )
}

fn encode_johab(symbol: u32) -> Option<u32> {
    const CHOSEONG: [u32; 19] = [
        0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x10,
        0x11, 0x12, 0x13, 0x14,
    ];
    const JUNGSEONG: [u32; 21] = [
        0x03, 0x04, 0x05, 0x06, 0x07, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F, 0x12, 0x13, 0x14, 0x15,
        0x16, 0x17, 0x1A, 0x1B, 0x1C, 0x1D,
    ];
    const JONGSEONG: [u32; 28] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D,
    ];
    const JAMO: [u32; 51] = [
        0x8841, 0x8C41, 0x8444, 0x9041, 0x8446, 0x8447, 0x9441, 0x9841, 0x9C41, 0x844A, 0x844B,
        0x844C, 0x844D, 0x844E, 0x844F, 0x8450, 0xA041, 0xA441, 0xA841, 0x8454, 0xAC41, 0xB041,
        0xB441, 0xB841, 0xBC41, 0xC041, 0xC441, 0xC841, 0xCC41, 0xD041, 0x8461, 0x8481, 0x84A1,
        0x84C1, 0x84E1, 0x8541, 0x8561, 0x8581, 0x85A1, 0x85C1, 0x85E1, 0x8641, 0x8661, 0x8681,
        0x86A1, 0x86C1, 0x86E1, 0x8741, 0x8761, 0x8781, 0x87A1,
    ];

    if symbol < 0x80 {
        return Some(symbol);
    }
    if (0xAC00..=0xD7A3).contains(&symbol) {
        let syllable = (symbol - 0xAC00) as usize;
        return Some(
            0x8000
                | (CHOSEONG[syllable / 588] << 10)
                | (JUNGSEONG[(syllable / 28) % 21] << 5)
                | JONGSEONG[syllable % 28],
        );
    }
    if (0x3131..=0x3163).contains(&symbol) {
        return Some(JAMO[(symbol - 0x3131) as usize]);
    }

    // Non-Hangul Johab remaps KS X 1001; strip the high bit from Windows-949 bytes.
    let cp949 = encode_multibyte(EUC_KR, symbol)?;
    if cp949 <= 0x7F {
        return Some(cp949);
    }
    let c1 = ((cp949 >> 8) as u8) & 0x7F;
    let c2 = (cp949 as u8) & 0x7F;
    if !((0x21..=0x2C).contains(&c1) || (0x4A..=0x7D).contains(&c1)) || !(0x21..=0x7E).contains(&c2)
    {
        return None;
    }
    let t1 = if c1 < 0x4A {
        u16::from(c1 - 0x21) + 0x1B2
    } else {
        u16::from(c1 - 0x21) + 0x197
    };
    let t2 = (if t1 & 1 == 1 { 0x5E } else { 0 }) + u16::from(c2 - 0x21);
    let second = if t2 < 0x4E { t2 + 0x31 } else { t2 + 0x43 };
    Some((u32::from(t1 >> 1) << 8) | u32::from(second))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use super::*;
    use crate::{FontMatch, FontProviderKind, font_match_supports_text};

    fn fixture_path(name: &str) -> Option<PathBuf> {
        let path = Path::new("/tmp/rassa-libass-tests/regression/font_nonunicode").join(name);
        path.is_file().then_some(path)
    }

    fn fixture(name: &str) -> Option<Vec<u8>> {
        fs::read(fixture_path(name)?).ok()
    }

    fn fixture_dialogue_text(name: &str) -> Option<String> {
        let contents = fs::read_to_string(fixture_path(name)?).ok()?;
        contents
            .lines()
            .find(|line| line.starts_with("Dialogue:"))?
            .splitn(10, ',')
            .nth(9)
            .map(str::to_owned)
    }

    fn assert_fixture_glyph(name: &str, character: char, kind: FontCharmap, glyph_id: u16) {
        let Some(data) = fixture(name) else {
            eprintln!("skipping: official libass compatible_0.17.5 fixture is unavailable");
            return;
        };
        let face = Face::parse(&data, 0).expect("official fixture parses");
        assert_eq!(font_face_charmap(&face), kind);
        assert_eq!(
            font_face_glyph_index(&face, character),
            Some(GlyphId(glyph_id))
        );
        assert!(font_face_uses_legacy_charmap(&face));
        assert_eq!(face.glyph_index(character), None);
    }

    #[test]
    fn libass_multibyte_encodings_pack_exact_legacy_codes() {
        assert_eq!(
            map_codepoint(FontCharmap::ShiftJis, '君' as u32),
            Some(0x8C4E)
        );
        assert_eq!(
            map_codepoint(FontCharmap::Gb2312, '中' as u32),
            Some(0xD6D0)
        );
        assert_eq!(map_codepoint(FontCharmap::Big5, '訐' as u32), Some(0xB050));
        assert_eq!(
            map_codepoint(FontCharmap::Wansung, '한' as u32),
            Some(0xC7D1)
        );
        assert_eq!(map_codepoint(FontCharmap::Johab, '한' as u32), Some(0xD065));
        assert_eq!(map_codepoint(FontCharmap::Johab, '中' as u32), Some(0xF3E9));
        assert_eq!(map_codepoint(FontCharmap::Johab, 'ㄱ' as u32), Some(0x8841));
        assert_eq!(map_codepoint(FontCharmap::ShiftJis, '😀' as u32), None);
    }

    #[test]
    fn symbol_and_arabic_charset_maps_match_libass_generated_data() {
        assert_eq!(
            map_codepoint(FontCharmap::Symbol { charset: None }, 'A' as u32),
            Some(0xF041)
        );
        assert_eq!(
            map_codepoint(
                FontCharmap::Symbol {
                    charset: Some(ARABIC_CHARSET_SIMPLIFIED),
                },
                'ج' as u32,
            ),
            Some(0xF151),
        );
        assert_eq!(
            map_codepoint(
                FontCharmap::Symbol {
                    charset: Some(ARABIC_CHARSET_TRADITIONAL),
                },
                'ج' as u32,
            ),
            Some(0xF258),
        );
    }

    #[test]
    fn official_simplified_arabic_fixture_resolves_unicode_to_real_glyph() {
        assert_fixture_glyph(
            "legacy-arabic-simplified-SimplifiedArabic.ttf",
            'ج',
            FontCharmap::Symbol {
                charset: Some(ARABIC_CHARSET_SIMPLIFIED),
            },
            56,
        );
    }

    #[test]
    fn official_traditional_arabic_fixture_resolves_unicode_to_real_glyph() {
        assert_fixture_glyph(
            "legacy-arabic-traditional-AGACairoRegular.ttf",
            'ج',
            FontCharmap::Symbol {
                charset: Some(ARABIC_CHARSET_TRADITIONAL),
            },
            91,
        );
    }

    #[test]
    fn official_shift_jis_fixture_resolves_unicode_to_real_glyph() {
        assert_fixture_glyph(
            "shiftjis_Reishoreiryu.ttf",
            '君',
            FontCharmap::ShiftJis,
            1439,
        );
    }

    #[test]
    fn official_big5_hkscs_fixture_resolves_unicode_to_real_glyph() {
        assert_fixture_glyph("big5-hkscs_SingYi-Ultra.ttf", '訐', FontCharmap::Big5, 2628);
    }

    #[test]
    fn font_support_accepts_all_official_nonunicode_regression_text() {
        let cases = [
            (
                "legacy-arabic-simplified-SimplifiedArabic.ttf",
                "legacy-arabic-simplified.ass",
            ),
            (
                "legacy-arabic-traditional-AGACairoRegular.ttf",
                "legacy-arabic-traditional.ass",
            ),
            ("shiftjis_Reishoreiryu.ttf", "shiftjis.ass"),
            ("big5-hkscs_SingYi-Ultra.ttf", "big5-hkscs.ass"),
        ];
        for (font_name, ass_name) in cases {
            let (Some(path), Some(text)) =
                (fixture_path(font_name), fixture_dialogue_text(ass_name))
            else {
                eprintln!("skipping: official libass compatible_0.17.5 fixture is unavailable");
                return;
            };
            let font = FontMatch {
                family: font_name.to_owned(),
                path: Some(path),
                face_index: None,
                style: None,
                synthetic_bold: false,
                synthetic_italic: false,
                provider: FontProviderKind::DefaultFile,
            };
            assert!(
                font_match_supports_text(&font, &text),
                "legacy face must advertise coverage for {ass_name}: {text:?}",
            );
        }
    }
}
