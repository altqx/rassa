use std::ops::Range;

use rassa_core::RassaResult;
use rassa_unibreak::{BreakAnalysis, LineBreakOpportunity, WordBreakOpportunity, analyze_breaks};
use unicode_bidi::{BidiClass, BidiInfo};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BidiDirection {
    #[default]
    Neutral,
    LeftToRight,
    RightToLeft,
    WeakLeftToRight,
    WeakRightToLeft,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BidiAnalysis {
    pub direction: BidiDirection,
    pub visual_text: String,
    pub logical_to_visual: Vec<usize>,
    pub visual_to_logical: Vec<usize>,
    pub embedding_levels: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextSegment {
    pub text: String,
    pub byte_range: Range<usize>,
    pub char_range: Range<usize>,
    pub line_breaks: Vec<LineBreakOpportunity>,
    pub word_breaks: Vec<WordBreakOpportunity>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnicodeAnalysis {
    pub text: String,
    pub break_analysis: BreakAnalysis,
    pub bidi_analysis: BidiAnalysis,
    pub segments: Vec<TextSegment>,
}

#[derive(Default)]
pub struct UnicodePipeline;

impl UnicodePipeline {
    pub fn analyze_text(&self, text: &str, language: Option<&str>) -> RassaResult<UnicodeAnalysis> {
        let break_analysis = analyze_breaks(text, language)?;
        let bidi_analysis = analyze_bidi(text)?;
        let segments = segment_by_mandatory_breaks(text, &break_analysis);

        Ok(UnicodeAnalysis {
            text: text.to_string(),
            break_analysis,
            bidi_analysis,
            segments,
        })
    }

    pub fn segment_text(
        &self,
        text: &str,
        language: Option<&str>,
    ) -> RassaResult<Vec<TextSegment>> {
        Ok(self.analyze_text(text, language)?.segments)
    }
}

pub fn analyze_bidi(text: &str) -> RassaResult<BidiAnalysis> {
    if text.is_empty() {
        return Ok(BidiAnalysis::default());
    }

    Ok(analyze_bidi_with_unicode_bidi(text))
}

fn analyze_bidi_with_unicode_bidi(text: &str) -> BidiAnalysis {
    let bidi_info = BidiInfo::new(text, None);
    let Some(paragraph) = bidi_info.paragraphs.first() else {
        return BidiAnalysis::default();
    };

    let levels = bidi_info.reordered_levels_per_char(paragraph, paragraph.range.clone());
    let visual_to_logical = BidiInfo::reorder_visual(&levels);
    let mut logical_to_visual = vec![0; visual_to_logical.len()];
    for (visual_index, logical_index) in visual_to_logical.iter().copied().enumerate() {
        if let Some(slot) = logical_to_visual.get_mut(logical_index) {
            *slot = visual_index;
        }
    }

    BidiAnalysis {
        direction: first_strong_direction(&bidi_info),
        visual_text: bidi_info
            .reorder_line(paragraph, paragraph.range.clone())
            .into_owned(),
        logical_to_visual,
        visual_to_logical,
        embedding_levels: levels.iter().map(|level| level.number()).collect(),
    }
}

fn first_strong_direction(bidi_info: &BidiInfo<'_>) -> BidiDirection {
    bidi_info
        .original_classes
        .iter()
        .find_map(|class| match class {
            BidiClass::L => Some(BidiDirection::LeftToRight),
            BidiClass::R | BidiClass::AL => Some(BidiDirection::RightToLeft),
            _ => None,
        })
        .unwrap_or(BidiDirection::Neutral)
}

fn segment_by_mandatory_breaks(text: &str, analysis: &BreakAnalysis) -> Vec<TextSegment> {
    let mut segments = Vec::new();
    let mut byte_start = 0;
    let mut char_start = 0;
    let chars = text.char_indices().collect::<Vec<_>>();

    for (index, (byte_index, character)) in chars.iter().copied().enumerate() {
        let should_break = matches!(
            analysis.line_breaks.get(index),
            Some(LineBreakOpportunity::Mandatory)
        );
        if should_break {
            let end_byte = byte_index + character.len_utf8();
            segments.push(build_segment(
                text,
                analysis,
                byte_start,
                end_byte,
                char_start,
                index + 1,
            ));
            byte_start = end_byte;
            char_start = index + 1;
        }
    }

    if char_start < chars.len() || text.is_empty() {
        segments.push(build_segment(
            text,
            analysis,
            byte_start,
            text.len(),
            char_start,
            chars.len(),
        ));
    }

    segments
        .into_iter()
        .filter(|segment| !segment.text.is_empty() || text.is_empty())
        .collect()
}

fn build_segment(
    text: &str,
    analysis: &BreakAnalysis,
    byte_start: usize,
    byte_end: usize,
    char_start: usize,
    char_end: usize,
) -> TextSegment {
    TextSegment {
        text: text[byte_start..byte_end].to_string(),
        byte_range: byte_start..byte_end,
        char_range: char_start..char_end,
        line_breaks: analysis.line_breaks[char_start..char_end].to_vec(),
        word_breaks: analysis.word_breaks[char_start..char_end].to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_text_on_mandatory_breaks() {
        let pipeline = UnicodePipeline;
        let segments = pipeline
            .segment_text("alpha\nbeta", Some("en"))
            .expect("unicode segmentation should succeed");

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "alpha\n");
        assert_eq!(segments[1].text, "beta");
    }

    #[test]
    fn bidi_analysis_returns_shape_metadata() {
        let analysis = analyze_bidi("abc").expect("bidi analysis should succeed");

        assert_eq!(analysis.visual_text.chars().count(), 3);
        assert_eq!(analysis.logical_to_visual.len(), 3);
        assert_eq!(analysis.visual_to_logical.len(), 3);
        assert_eq!(analysis.embedding_levels.len(), 3);
    }

    #[test]
    fn bidi_fallback_reorders_rtl_runs() {
        let analysis = analyze_bidi_with_unicode_bidi("abc אבג");

        assert_eq!(analysis.direction, BidiDirection::LeftToRight);
        assert_eq!(analysis.visual_text, "abc גבא");
        assert_ne!(analysis.logical_to_visual, vec![0, 1, 2, 3, 4, 5, 6]);
        assert!(analysis.embedding_levels.iter().any(|level| *level > 0));
    }

    #[test]
    fn bidi_fallback_detects_rtl_paragraph_direction() {
        let analysis = analyze_bidi_with_unicode_bidi("אבג abc");

        assert_eq!(analysis.direction, BidiDirection::RightToLeft);
        assert_ne!(analysis.visual_text, "אבג abc");
        assert!(
            analysis
                .embedding_levels
                .iter()
                .any(|level| *level % 2 == 1)
        );
    }
}
