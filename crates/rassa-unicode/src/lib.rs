#![allow(unsafe_op_in_unsafe_fn)]

use std::{ops::Range, os::raw::c_int};

use rassa_core::RassaResult;
use rassa_unibreak::{BreakAnalysis, LineBreakOpportunity, WordBreakOpportunity, analyze_breaks};

const FRIBIDI_MASK_RTL: u32 = 0x0000_0001;
const FRIBIDI_MASK_WEAK: u32 = 0x0000_0020;
const FRIBIDI_PAR_ON: u32 = 0x0000_0040;

#[cfg(fribidi_available)]
type FriBidiChar = u32;
#[cfg(fribidi_available)]
type FriBidiStrIndex = c_int;
#[cfg(fribidi_available)]
type FriBidiParType = u32;
#[cfg(fribidi_available)]
type FriBidiLevel = u8;

#[cfg(fribidi_available)]
unsafe extern "C" {
    fn fribidi_log2vis(
        input: *const FriBidiChar,
        len: FriBidiStrIndex,
        base_dir: *mut FriBidiParType,
        visual_str: *mut FriBidiChar,
        positions_l_to_v: *mut FriBidiStrIndex,
        positions_v_to_l: *mut FriBidiStrIndex,
        embedding_levels: *mut FriBidiLevel,
    ) -> FriBidiLevel;
}

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

    #[cfg(fribidi_available)]
    {
        analyze_bidi_with_fribidi(text)
    }

    #[cfg(not(fribidi_available))]
    {
        Ok(fallback_bidi_analysis(text))
    }
}

#[cfg(fribidi_available)]
fn analyze_bidi_with_fribidi(text: &str) -> RassaResult<BidiAnalysis> {
    let logical = text.chars().map(u32::from).collect::<Vec<_>>();
    let mut visual = vec![0_u32; logical.len()];
    let mut logical_to_visual = vec![0_i32; logical.len()];
    let mut visual_to_logical = vec![0_i32; logical.len()];
    let mut embedding_levels = vec![0_u8; logical.len()];
    let mut base_dir = FRIBIDI_PAR_ON;

    let max_level = unsafe {
        fribidi_log2vis(
            logical.as_ptr(),
            logical.len() as FriBidiStrIndex,
            &mut base_dir,
            visual.as_mut_ptr(),
            logical_to_visual.as_mut_ptr(),
            visual_to_logical.as_mut_ptr(),
            embedding_levels.as_mut_ptr(),
        )
    };

    if max_level == 0 {
        return Ok(fallback_bidi_analysis(text));
    }

    Ok(BidiAnalysis {
        direction: map_bidi_direction(base_dir),
        visual_text: utf32_to_string(&visual),
        logical_to_visual: normalize_indices(&logical_to_visual),
        visual_to_logical: normalize_indices(&visual_to_logical),
        embedding_levels,
    })
}

#[cfg(not(fribidi_available))]
fn analyze_bidi_with_fribidi(text: &str) -> RassaResult<BidiAnalysis> {
    Ok(fallback_bidi_analysis(text))
}

fn fallback_bidi_analysis(text: &str) -> BidiAnalysis {
    let char_count = text.chars().count();
    BidiAnalysis {
        direction: BidiDirection::Neutral,
        visual_text: text.to_string(),
        logical_to_visual: (0..char_count).collect(),
        visual_to_logical: (0..char_count).collect(),
        embedding_levels: vec![0; char_count],
    }
}

fn map_bidi_direction(value: u32) -> BidiDirection {
    if value == FRIBIDI_PAR_ON {
        BidiDirection::Neutral
    } else if value & FRIBIDI_MASK_WEAK != 0 {
        if value & FRIBIDI_MASK_RTL != 0 {
            BidiDirection::WeakRightToLeft
        } else {
            BidiDirection::WeakLeftToRight
        }
    } else if value & FRIBIDI_MASK_RTL != 0 {
        BidiDirection::RightToLeft
    } else {
        BidiDirection::LeftToRight
    }
}

fn normalize_indices(indices: &[i32]) -> Vec<usize> {
    indices
        .iter()
        .map(|index| (*index).max(0) as usize)
        .collect()
}

fn utf32_to_string(codepoints: &[u32]) -> String {
    codepoints
        .iter()
        .filter_map(|codepoint| char::from_u32(*codepoint))
        .collect()
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
}
