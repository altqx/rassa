use super::*;

pub fn default_renderer_config(track: &ParsedTrack) -> RendererConfig {
    RendererConfig {
        frame: Size {
            width: track.play_res_x,
            height: track.play_res_y,
        },
        hinting: ass::Hinting::None,
        ..RendererConfig::default()
    }
}

pub(crate) fn output_scale_x(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    let frame_width = output_mapping_size(track, config).width;
    let base_width = track.play_res_x.max(1);

    f64::from(frame_width.max(1)) / f64::from(base_width)
}

pub(crate) fn output_scale_y(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    let frame_height = output_mapping_size(track, config).height;
    let base_height = track.play_res_y.max(1);

    f64::from(frame_height.max(1)) / f64::from(base_height)
}

pub(crate) fn effective_pixel_aspect(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    // libass always derives PAR from LayoutRes when both dimensions are
    // present; LayoutRes explicitly overrides ass_set_storage_size and an
    // explicit ass_set_pixel_aspect value. Without LayoutRes, a positive
    // explicit PAR wins and storage is only used when PAR is unset (zero).
    if layout_resolution(track).is_some() {
        return derived_pixel_aspect(track, config).unwrap_or(1.0);
    }
    if config.pixel_aspect.is_finite() && config.pixel_aspect > 0.0 {
        return config.pixel_aspect;
    }
    derived_pixel_aspect(track, config).unwrap_or(1.0)
}

pub(crate) fn derived_pixel_aspect(track: &ParsedTrack, config: &RendererConfig) -> Option<f64> {
    let layout = layout_resolution(track).or_else(|| storage_resolution(config))?;
    let frame = frame_content_size(track, config);
    if frame.width <= 0 || frame.height <= 0 || layout.width <= 0 || layout.height <= 0 {
        return None;
    }

    let display_aspect = f64::from(frame.width) / f64::from(frame.height);
    let source_aspect = f64::from(layout.width) / f64::from(layout.height);
    (source_aspect > 0.0).then_some(display_aspect / source_aspect)
}

pub(crate) fn layout_resolution(track: &ParsedTrack) -> Option<Size> {
    (track.layout_res_x > 0 && track.layout_res_y > 0).then_some(Size {
        width: track.layout_res_x,
        height: track.layout_res_y,
    })
}

pub(crate) fn storage_resolution(config: &RendererConfig) -> Option<Size> {
    (config.storage.width > 0 && config.storage.height > 0).then_some(config.storage)
}

pub(crate) fn frame_content_size(track: &ParsedTrack, config: &RendererConfig) -> Size {
    let frame_width = if config.frame.width > 0 {
        config.frame.width
    } else {
        track.play_res_x
    };
    let frame_height = if config.frame.height > 0 {
        config.frame.height
    } else {
        track.play_res_y
    };

    Size {
        width: (frame_width - config.margins.left - config.margins.right).max(0),
        height: (frame_height - config.margins.top - config.margins.bottom).max(0),
    }
}

pub(crate) fn output_mapping_size(track: &ParsedTrack, config: &RendererConfig) -> Size {
    // libass screen_scale always maps PlayRes onto the content frame (frame
    // minus margins); use_margins only changes where events are anchored.
    frame_content_size(track, config)
}

/// Resolution used by libass for blur and for unscaled borders/shadows.
/// Unlike text placement, an explicit pixel aspect can synthesize one axis
/// when neither LayoutRes nor a storage size was supplied.
pub(crate) fn filter_layout_resolution(track: &ParsedTrack, config: &RendererConfig) -> Size {
    if let Some(layout) = layout_resolution(track).or_else(|| storage_resolution(config)) {
        return layout;
    }

    let frame = frame_content_size(track, config);
    let par = config.pixel_aspect;
    if !par.is_finite()
        || par <= 0.0
        || (par - 1.0).abs() < f64::EPSILON
        || frame.width <= 0
        || frame.height <= 0
    {
        return Size {
            width: track.play_res_x.max(1),
            height: track.play_res_y.max(1),
        };
    }

    if par > 1.0 {
        Size {
            width: (f64::from(track.play_res_y.max(1)) * f64::from(frame.width)
                / f64::from(frame.height)
                / par)
                .round()
                .max(1.0) as i32,
            height: track.play_res_y.max(1),
        }
    } else {
        Size {
            width: track.play_res_x.max(1),
            height: (f64::from(track.play_res_x.max(1)) * f64::from(frame.height)
                / f64::from(frame.width)
                * par)
                .round()
                .max(1.0) as i32,
        }
    }
}

pub(crate) fn renderer_blur_scales(
    track: &ParsedTrack,
    config: &RendererConfig,
    font_scale: f64,
) -> (f64, f64) {
    let frame = frame_content_size(track, config);
    let layout = filter_layout_resolution(track, config);
    let font_scale = if font_scale.is_finite() {
        font_scale.max(0.0)
    } else {
        1.0
    };
    (
        style_scale(f64::from(frame.width.max(1)) / f64::from(layout.width.max(1))) * font_scale,
        style_scale(f64::from(frame.height.max(1)) / f64::from(layout.height.max(1))) * font_scale,
    )
}

pub(crate) fn frame_size(track: &ParsedTrack, config: &RendererConfig) -> Size {
    Size {
        width: if config.frame.width > 0 {
            config.frame.width
        } else {
            track.play_res_x
        },
        height: if config.frame.height > 0 {
            config.frame.height
        } else {
            track.play_res_y
        },
    }
}

/// Per-event script-to-screen coordinate mapping, mirroring the libass
/// x2scr/y2scr family (ass_render.c): positioned coordinates (and all
/// coordinates of explicit events, or every event when use_margins is off)
/// map onto the content frame offset by the margins; margin-anchored
/// coordinates of normal events under use_margins map onto the content
/// frame aspect-fitted into the full frame, letterboxed right/bottom-heavy
/// exactly like fit_width/fit_height.
#[derive(Clone, Copy, Debug)]
pub(crate) struct EventMapping {
    pub(crate) explicit: bool,
    pub(crate) use_margins: bool,
    pub(crate) scale_x: f64,
    pub(crate) scale_y: f64,
    pub(crate) margin_left: f64,
    pub(crate) margin_top: f64,
    pub(crate) frame_w: f64,
    pub(crate) frame_h: f64,
    pub(crate) fit_w: f64,
    pub(crate) fit_h: f64,
    pub(crate) play_res_x: f64,
    pub(crate) play_res_y: f64,
}

impl EventMapping {
    fn pos_like(&self) -> bool {
        self.explicit || !self.use_margins
    }

    pub(crate) fn map_x_pos(&self, x: f64) -> f64 {
        x * self.scale_x + self.margin_left
    }

    pub(crate) fn map_y_pos(&self, y: f64) -> f64 {
        y * self.scale_y + self.margin_top
    }

    pub(crate) fn x_left(&self, x: f64) -> f64 {
        if self.pos_like() {
            self.map_x_pos(x)
        } else {
            x * self.fit_w / self.play_res_x
        }
    }

    pub(crate) fn x_right(&self, x: f64) -> f64 {
        if self.pos_like() {
            self.map_x_pos(x)
        } else {
            x * self.fit_w / self.play_res_x + (self.frame_w - self.fit_w)
        }
    }

    pub(crate) fn y_top(&self, y: f64) -> f64 {
        if self.pos_like() {
            self.map_y_pos(y)
        } else {
            y * self.fit_h / self.play_res_y
        }
    }

    pub(crate) fn y_center(&self, y: f64) -> f64 {
        if self.pos_like() {
            self.map_y_pos(y)
        } else {
            y * self.fit_h / self.play_res_y + (self.frame_h - self.fit_h) * 0.5
        }
    }

    pub(crate) fn y_sub(&self, y: f64) -> f64 {
        if self.pos_like() {
            self.map_y_pos(y)
        } else {
            y * self.fit_h / self.play_res_y + (self.frame_h - self.fit_h)
        }
    }
}

pub(crate) fn event_mapping(
    track: &ParsedTrack,
    config: &RendererConfig,
    explicit: bool,
) -> EventMapping {
    let scale_x = output_scale_x(track, config);
    let scale_y = output_scale_y(track, config);
    let frame = frame_size(track, config);
    let frame_w = f64::from(frame.width.max(0));
    let frame_h = f64::from(frame.height.max(0));
    let content_w = f64::from(track.play_res_x.max(1)) * style_scale(scale_x);
    let content_h = f64::from(track.play_res_y.max(1)) * style_scale(scale_y);
    // libass ass_reconfigure: aspect-fit the content into the full frame.
    let (fit_w, fit_h) = if content_w * frame_h >= content_h * frame_w {
        (
            frame_w,
            if content_w > 0.0 {
                content_h * frame_w / content_w
            } else {
                frame_h
            },
        )
    } else {
        (
            if content_h > 0.0 {
                content_w * frame_h / content_h
            } else {
                frame_w
            },
            frame_h,
        )
    };
    EventMapping {
        explicit,
        use_margins: config.use_margins,
        scale_x: style_scale(scale_x),
        scale_y: style_scale(scale_y),
        margin_left: f64::from(config.margins.left),
        margin_top: f64::from(config.margins.top),
        frame_w,
        frame_h,
        fit_w,
        fit_h,
        play_res_x: f64::from(track.play_res_x.max(1)),
        play_res_y: f64::from(track.play_res_y.max(1)),
    }
}
