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
    let aspect = effective_pixel_aspect(track, config);

    f64::from(frame_width.max(1)) / f64::from(base_width) * aspect
}

pub(crate) fn output_scale_y(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    let frame_height = output_mapping_size(track, config).height;
    let base_height = track.play_res_y.max(1);

    f64::from(frame_height.max(1)) / f64::from(base_height)
}

pub(crate) fn effective_pixel_aspect(track: &ParsedTrack, config: &RendererConfig) -> f64 {
    if layout_resolution(track).is_some()
        || !(config.pixel_aspect.is_finite() && config.pixel_aspect > 0.0)
    {
        return derived_pixel_aspect(track, config).unwrap_or(1.0);
    }

    config.pixel_aspect
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
    if config.use_margins {
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
    } else {
        frame_content_size(track, config)
    }
}

pub(crate) fn output_offset(config: &RendererConfig) -> Point {
    if config.use_margins {
        Point { x: 0, y: 0 }
    } else {
        Point {
            x: config.margins.left.max(0),
            y: config.margins.top.max(0),
        }
    }
}
