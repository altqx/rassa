//! Safe Rust API and native `librassa.so` target for rassa.

pub use rassa_core::{
    ImagePlane, Margins, Point, RassaError, RassaResult, Rect, RendererConfig, RgbaColor, Size, ass,
};
pub use rassa_fonts::{AttachedFontProvider, FontAttachment, FontProvider, FontconfigProvider};
pub use rassa_parse::{
    ParsedAttachment, ParsedEvent, ParsedSpanStyle, ParsedStyle, ParsedTrack, parse_script_text,
};
pub use rassa_render::{PreparedFrame, RenderEngine, RenderSelection, default_renderer_config};

/// C ABI symbols exported by `librassa.so`.
pub mod capi {
    pub use rassa_capi::*;
}

/// Parsed ASS/SSA subtitle script for the safe Rust API.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    track: ParsedTrack,
}

impl Script {
    /// Parse an ASS/SSA script from UTF-8 text.
    pub fn parse(text: &str) -> RassaResult<Self> {
        parse_script_text(text).map(|track| Self { track })
    }

    /// Wrap an already parsed track.
    pub fn from_track(track: ParsedTrack) -> Self {
        Self { track }
    }

    /// Borrow the underlying parsed track for advanced users.
    pub fn track(&self) -> &ParsedTrack {
        &self.track
    }

    /// Consume this script and return the underlying parsed track.
    pub fn into_track(self) -> ParsedTrack {
        self.track
    }

    /// ASS PlayRes as a `Size`.
    pub fn play_res(&self) -> Size {
        Size {
            width: self.track.play_res_x,
            height: self.track.play_res_y,
        }
    }

    /// Default renderer configuration derived from this script.
    pub fn default_config(&self) -> RendererConfig {
        default_renderer_config(&self.track)
    }
}

/// Rendered frame returned by the safe Rust API.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    pub now_ms: i64,
    pub planes: Vec<ImagePlane>,
}

/// Safe Rust subtitle renderer facade.
#[derive(Default)]
pub struct Renderer {
    engine: RenderEngine,
}

impl Renderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Render using the default fontconfig-backed provider.
    pub fn render_frame(&self, script: &Script, now_ms: i64) -> RassaResult<Frame> {
        let provider = FontconfigProvider::new();
        self.render_frame_with_provider(script, &provider, now_ms)
    }

    /// Render using an explicit font provider.
    pub fn render_frame_with_provider<P: FontProvider>(
        &self,
        script: &Script,
        provider: &P,
        now_ms: i64,
    ) -> RassaResult<Frame> {
        Ok(Frame {
            now_ms,
            planes: self
                .engine
                .render_frame_with_provider(script.track(), provider, now_ms),
        })
    }

    /// Render using an explicit font provider and renderer config.
    pub fn render_frame_with_config<P: FontProvider>(
        &self,
        script: &Script,
        provider: &P,
        now_ms: i64,
        config: &RendererConfig,
    ) -> RassaResult<Frame> {
        Ok(Frame {
            now_ms,
            planes: self.engine.render_frame_with_provider_and_config(
                script.track(),
                provider,
                now_ms,
                config,
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_facade_parses_and_renders() {
        let script = Script::parse("[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Rust ABI")
            .expect("script should parse");

        let frame = Renderer::new()
            .render_frame(&script, 500)
            .expect("frame should render");

        assert_eq!(
            script.play_res(),
            Size {
                width: 320,
                height: 180
            }
        );
        assert_eq!(frame.now_ms, 500);
        assert!(!frame.planes.is_empty());
    }
}
