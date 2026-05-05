#![allow(dead_code)]

use rassa_core::RassaResult;
use unicode_linebreak::{BreakOpportunity as UnicodeLineBreakOpportunity, linebreaks};
use unicode_segmentation::UnicodeSegmentation;

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
    false
}

pub fn analyze_breaks(text: &str, _language: Option<&str>) -> RassaResult<BreakAnalysis> {
    Ok(BreakAnalysis {
        line_breaks: classify_line_breaks(text, _language)?,
        word_breaks: classify_word_breaks(text, _language)?,
    })
}

pub fn classify_line_breaks(
    text: &str,
    _language: Option<&str>,
) -> RassaResult<Vec<LineBreakOpportunity>> {
    Ok(unicode_line_breaks(text))
}

pub fn classify_word_breaks(
    text: &str,
    _language: Option<&str>,
) -> RassaResult<Vec<WordBreakOpportunity>> {
    Ok(unicode_word_breaks(text))
}

fn unicode_line_breaks(text: &str) -> Vec<LineBreakOpportunity> {
    let mut breaks = vec![LineBreakOpportunity::Prohibited; text.chars().count()];
    if breaks.is_empty() {
        return breaks;
    }

    for (byte_index, opportunity) in linebreaks(text) {
        if let Some(char_index) = char_index_ending_at_byte(text, byte_index) {
            breaks[char_index] = match opportunity {
                UnicodeLineBreakOpportunity::Mandatory if byte_index == text.len() => {
                    LineBreakOpportunity::Indeterminate
                }
                UnicodeLineBreakOpportunity::Mandatory => LineBreakOpportunity::Mandatory,
                UnicodeLineBreakOpportunity::Allowed => LineBreakOpportunity::Allowed,
            };
        }
    }

    breaks
}

fn unicode_word_breaks(text: &str) -> Vec<WordBreakOpportunity> {
    let mut breaks = vec![WordBreakOpportunity::NoBreak; text.chars().count()];
    if breaks.is_empty() {
        return breaks;
    }

    for (start, segment) in text.split_word_bound_indices() {
        let end = start + segment.len();
        if let Some(char_index) = char_index_ending_at_byte(text, end) {
            breaks[char_index] = WordBreakOpportunity::Break;
        }
    }

    breaks
}

fn char_index_ending_at_byte(text: &str, byte_index: usize) -> Option<usize> {
    if byte_index == 0 || byte_index > text.len() {
        return None;
    }

    text.char_indices()
        .enumerate()
        .find_map(|(char_index, (start, character))| {
            (start + character.len_utf8() == byte_index).then_some(char_index)
        })
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
        let breaks =
            classify_line_breaks("a\nb", Some("en")).expect("line break analysis should succeed");
        assert_eq!(breaks[1], LineBreakOpportunity::Mandatory);
    }

    #[test]
    fn unicode_line_breaks_keep_cjk_break_opportunities() {
        let breaks = unicode_line_breaks("日本語");

        assert_eq!(breaks.len(), 3);
        assert_eq!(breaks[0], LineBreakOpportunity::Allowed);
        assert_eq!(breaks[1], LineBreakOpportunity::Allowed);
    }

    #[test]
    fn unicode_word_breaks_keep_apostrophe_words_together() {
        let breaks = unicode_word_breaks("can't stop");
        let chars = "can't stop".chars().collect::<Vec<_>>();
        let apostrophe = chars
            .iter()
            .position(|character| *character == '\'')
            .expect("fixture has apostrophe");

        assert_eq!(breaks[apostrophe], WordBreakOpportunity::NoBreak);
    }
}
