#![allow(dead_code)]

use std::ffi::{c_char, CString};

use rassa_core::{RassaError, RassaResult};
use rassa_unibreak_sys as sys;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineBreakOpportunity {
    Mandatory,
    Allowed,
    #[default]
    Prohibited,
    InsideCharacter,
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WordBreakOpportunity {
    Break,
    #[default]
    NoBreak,
    InsideCharacter,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BreakAnalysis {
    pub line_breaks: Vec<LineBreakOpportunity>,
    pub word_breaks: Vec<WordBreakOpportunity>,
}

pub fn libunibreak_linked() -> bool {
    sys::LIBUNIBREAK_LINKED
}

pub fn analyze_breaks(text: &str, language: Option<&str>) -> RassaResult<BreakAnalysis> {
    Ok(BreakAnalysis {
        line_breaks: classify_line_breaks(text, language)?,
        word_breaks: classify_word_breaks(text, language)?,
    })
}

pub fn classify_line_breaks(text: &str, language: Option<&str>) -> RassaResult<Vec<LineBreakOpportunity>> {
    classify_line_breaks_with_libunibreak(text, language)
}

pub fn classify_word_breaks(text: &str, language: Option<&str>) -> RassaResult<Vec<WordBreakOpportunity>> {
    classify_word_breaks_with_libunibreak(text, language)
}

fn fallback_line_breaks(text: &str) -> Vec<LineBreakOpportunity> {
    let chars = text.chars().collect::<Vec<_>>();
    chars
        .iter()
        .enumerate()
        .map(|(index, character)| {
            if *character == '\n' || *character == '\r' {
                LineBreakOpportunity::Mandatory
            } else if character.is_whitespace() {
                LineBreakOpportunity::Allowed
            } else if index + 1 == chars.len() {
                LineBreakOpportunity::Indeterminate
            } else {
                LineBreakOpportunity::Prohibited
            }
        })
        .collect()
}

fn fallback_word_breaks(text: &str) -> Vec<WordBreakOpportunity> {
    let chars = text.chars().collect::<Vec<_>>();
    chars
        .iter()
        .enumerate()
        .map(|(index, character)| {
            if character.is_whitespace() || is_basic_word_boundary(*character) {
                WordBreakOpportunity::Break
            } else if index + 1 == chars.len() {
                WordBreakOpportunity::Break
            } else {
                WordBreakOpportunity::NoBreak
            }
        })
        .collect()
}

fn is_basic_word_boundary(character: char) -> bool {
    matches!(character, '-' | '/' | '\\' | '.' | ',' | ';' | ':' | '!' | '?')
}

fn to_utf32(text: &str) -> Vec<u32> {
    text.chars().map(u32::from).collect()
}

fn classify_line_breaks_with_libunibreak(text: &str, language: Option<&str>) -> RassaResult<Vec<LineBreakOpportunity>> {
    let codepoints = to_utf32(text);
    let mut output = vec![sys::LINEBREAK_NOBREAK; codepoints.len()];
    let language = make_lang_cstring(language)?;

    let linked = unsafe {
        sys::analyze_linebreaks_utf32(
            codepoints.as_ptr(),
            codepoints.len(),
            language.as_ref().map_or(std::ptr::null(), |lang| lang.as_ptr()),
            output.as_mut_ptr(),
        )
    };

    if !linked {
        return Ok(fallback_line_breaks(text));
    }

    Ok(output.into_iter().map(map_line_break).collect())
}

fn classify_word_breaks_with_libunibreak(text: &str, language: Option<&str>) -> RassaResult<Vec<WordBreakOpportunity>> {
    let codepoints = to_utf32(text);
    let mut output = vec![sys::WORDBREAK_NOBREAK; codepoints.len()];
    let language = make_lang_cstring(language)?;

    let linked = unsafe {
        sys::analyze_wordbreaks_utf32(
            codepoints.as_ptr(),
            codepoints.len(),
            language.as_ref().map_or(std::ptr::null(), |lang| lang.as_ptr()),
            output.as_mut_ptr(),
        )
    };

    if !linked {
        return Ok(fallback_word_breaks(text));
    }

    Ok(output.into_iter().map(map_word_break).collect())
}

fn make_lang_cstring(language: Option<&str>) -> RassaResult<Option<CString>> {
    language
        .filter(|language| !language.is_empty())
        .map(|language| CString::new(language).map_err(|_| RassaError::new("language contains interior NUL byte")))
        .transpose()
}

fn map_line_break(value: c_char) -> LineBreakOpportunity {
    match value {
        x if x == sys::LINEBREAK_MUSTBREAK => LineBreakOpportunity::Mandatory,
        x if x == sys::LINEBREAK_ALLOWBREAK => LineBreakOpportunity::Allowed,
        x if x == sys::LINEBREAK_INSIDEACHAR => LineBreakOpportunity::InsideCharacter,
        x if x == sys::LINEBREAK_INDETERMINATE => LineBreakOpportunity::Indeterminate,
        _ => LineBreakOpportunity::Prohibited,
    }
}

fn map_word_break(value: c_char) -> WordBreakOpportunity {
    match value {
        x if x == sys::WORDBREAK_BREAK => WordBreakOpportunity::Break,
        x if x == sys::WORDBREAK_INSIDEACHAR => WordBreakOpportunity::InsideCharacter,
        _ => WordBreakOpportunity::NoBreak,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn break_analysis_matches_character_count() {
        let analysis = analyze_breaks("hello world", Some("en")).expect("analysis should succeed");
        assert_eq!(analysis.line_breaks.len(), 11);
        assert_eq!(analysis.word_breaks.len(), 11);
    }

    #[test]
    fn newline_is_always_mandatory_break() {
        let breaks = classify_line_breaks("a\nb", Some("en")).expect("line break analysis should succeed");
        assert_eq!(breaks[1], LineBreakOpportunity::Mandatory);
    }
}
