use super::*;
use rassa_fonts::{
    FontProvider, FontProviderKind, FontQuery, FontconfigProvider, NullFontProvider,
};
use rassa_layout::LayoutLine;
use rassa_parse::{ParsedAnimatedStyle, ParsedKaraokeSpan, ParsedSpanTransform, parse_script_text};

fn config(
    frame_width: i32,
    frame_height: i32,
    margins: rassa_core::Margins,
    use_margins: bool,
) -> RendererConfig {
    RendererConfig {
        frame: Size {
            width: frame_width,
            height: frame_height,
        },
        margins,
        use_margins,
        ..RendererConfig::default()
    }
}

struct BundledFontProvider {
    path: std::path::PathBuf,
}

impl BundledFontProvider {
    fn aileron_regular() -> Self {
        Self {
            path: std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../rassa-test/fixtures/libass/compare/test/font2.otf"),
        }
    }
}

impl FontProvider for BundledFontProvider {
    fn resolve(&self, query: &FontQuery) -> FontMatch {
        FontMatch {
            family: query.family.clone(),
            path: Some(self.path.clone()),
            face_index: Some(0),
            style: query.style.clone(),
            synthetic_bold: false,
            synthetic_italic: false,
            provider: FontProviderKind::Fontconfig,
        }
    }
}

fn total_plane_area(planes: &[ImagePlane]) -> i32 {
    planes
        .iter()
        .map(|plane| plane.size.width * plane.size.height)
        .sum()
}

#[test]
fn glyph_compositing_adds_overlapping_coverage_like_libass() {
    let glyphs = vec![
        RasterGlyph {
            width: 1,
            height: 1,
            stride: 1,
            bitmap: vec![100],
            ..RasterGlyph::default()
        },
        RasterGlyph {
            width: 1,
            height: 1,
            stride: 1,
            bitmap: vec![180],
            ..RasterGlyph::default()
        },
    ];

    let plane =
        combined_image_plane_from_glyphs(&glyphs, 0, 0, Some(0), 0, ass::ImageType::Character, 0)
            .expect("overlapping glyphs create an image");

    assert_eq!(plane.bitmap, vec![255]);
}

#[test]
fn glyph_compositing_applies_fixed_point_shaped_positions() {
    let glyphs = vec![
        RasterGlyph {
            width: 1,
            height: 1,
            stride: 1,
            advance_x_26_6: 96,
            advance_y_26_6: 64,
            bitmap: vec![255],
            ..RasterGlyph::default()
        },
        RasterGlyph {
            width: 1,
            height: 1,
            stride: 1,
            offset_x_26_6: 32,
            bitmap: vec![255],
            ..RasterGlyph::default()
        },
    ];

    let plane =
        combined_image_plane_from_glyphs(&glyphs, 0, 0, Some(0), 0, ass::ImageType::Character, 0)
            .expect("positioned glyphs create an image");

    assert_eq!(plane.destination, Point { x: 0, y: -1 });
    assert_eq!(
        plane.size,
        Size {
            width: 3,
            height: 2
        }
    );
    assert_eq!(plane.bitmap, vec![0, 0, 255, 255, 0, 0]);
}

#[test]
fn be_transform_interpolation_uses_libass_integer_rounding() {
    assert_eq!(interpolate_be(0.0, 0.49, 1.0), 0.0);
    assert_eq!(interpolate_be(0.0, 0.5, 1.0), 1.0);
    assert_eq!(interpolate_be(0.0, 4.6, 0.5), 2.0);
    assert_eq!(interpolate_be(5.0, 0.0, 0.5), 3.0);
    assert_eq!(interpolate_be(0.0, 200.0, 1.0), 127.0);
    assert_eq!(interpolate_be(0.0, -1.0, 1.0), 0.0);
}

#[test]
fn blur_transform_interpolation_clamps_after_lerp_like_libass() {
    assert_eq!(interpolate_blur(0.0, 200.0, 0.25), 50.0);
    assert_eq!(interpolate_blur(0.0, 200.0, 0.5), 100.0);
    assert_eq!(interpolate_blur(20.0, -20.0, 0.25), 10.0);
    assert_eq!(interpolate_blur(20.0, -20.0, 0.75), 0.0);
}

#[test]
fn nonnegative_transform_interpolation_clamps_after_lerp_like_libass() {
    assert_eq!(interpolate_nonnegative(10.0, -30.0, 0.1), 6.0);
    assert_eq!(interpolate_nonnegative(10.0, -30.0, 0.25), 0.0);
    assert_eq!(interpolate_nonnegative(2.0, -2.0, 0.25), 1.0);
    assert_eq!(interpolate_nonnegative(2.0, -2.0, 0.75), 0.0);
}

#[test]
fn color_transform_interpolation_truncates_channels_like_libass() {
    assert_eq!(
        interpolate_color(0x0000_0000, 0x00FF_FFFF, 0.5),
        0x007F_7F7F
    );
    assert_eq!(
        interpolate_color(0x0000_0000, 0xFF00_0000, 0.5),
        0x7F00_0000
    );
    assert_eq!(
        interpolate_color(0x0033_6699, 0x00CC_9966, 0.5),
        0x007F_7F7F
    );
}

#[test]
fn fad_uses_libass_truncating_alpha_interpolation() {
    let event = ParsedEvent {
        start: 0,
        duration: 4000,
        ..ParsedEvent::default()
    };

    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 1000,
                fade_out_ms: 1000,
            },
            Some(&event),
            500,
        ),
        127
    );
    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 1000,
                fade_out_ms: 1000,
            },
            Some(&event),
            3500,
        ),
        127
    );
}

#[test]
fn fad_uses_libass_wrapping_out_start_when_fade_out_exceeds_duration() {
    let event = ParsedEvent {
        start: 0,
        duration: 800,
        ..ParsedEvent::default()
    };

    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 100,
                fade_out_ms: 1000,
            },
            Some(&event),
            100,
        ),
        76
    );
    assert_eq!(
        compute_fad_alpha(
            ParsedFade::Simple {
                fade_in_ms: 100,
                fade_out_ms: 1000,
            },
            Some(&event),
            400,
        ),
        153
    );
}

#[test]
fn fade_alpha_combines_with_existing_colour_alpha() {
    assert_eq!(with_fade_alpha(0xFF00_0080, 0), 0xFF00_0080);
    assert_eq!(with_fade_alpha(0xFF00_0000, 127), 0xFF00_007F);
    assert_eq!(with_fade_alpha(0xFF00_0080, 127), 0xFF00_00BF);
}

#[test]
fn fade_alpha_wraps_over_255_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let fade = ParsedFade::Complex {
        alpha1: 300,
        alpha2: 0,
        alpha3: 0,
        t1_ms: 0,
        t2_ms: 100,
        t3_ms: 100,
        t4_ms: 200,
    };

    assert_eq!(compute_fad_alpha(fade, Some(&event), 0), 300);
    assert_eq!(with_fade_alpha(0xFF00_0000, 300), 0xFF00_002C);
}

#[test]
fn fade_alpha_nonpositive_values_do_not_modify_colour_like_libass() {
    assert_eq!(with_fade_alpha(0xFF00_0080, -20), 0xFF00_0080);
    assert_eq!(with_fade_alpha(0xFF00_0080, 0), 0xFF00_0080);
}

#[test]
fn move_interpolation_swaps_reversed_times_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 500,
        ..ParsedEvent::default()
    };

    assert_eq!(
        interpolate_move(
            ParsedMovement {
                start: (0, 0),
                end: (100, 0),
                t1_ms: 200,
                t2_ms: 0,
            },
            Some(&event),
            100,
        ),
        (50, 0)
    );
}

#[test]
fn move_interpolation_preserves_negative_start_time_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 500,
        ..ParsedEvent::default()
    };

    assert_eq!(
        interpolate_move(
            ParsedMovement {
                start: (0, 0),
                end: (100, 0),
                t1_ms: -100,
                t2_ms: 100,
            },
            Some(&event),
            0,
        ),
        (50, 0)
    );
    assert_eq!(
        interpolate_move_exact(
            ParsedMovementExact {
                start: (0.0, 0.0),
                end: (100.0, 0.0),
                t1_ms: -100,
                t2_ms: 100,
            },
            Some(&event),
            0,
        ),
        (50, 0)
    );
}

#[test]
fn move_interpolation_uses_signed_wrapping_delta_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };

    assert_eq!(
        interpolate_move(
            ParsedMovement {
                start: (0, 0),
                end: (1, 0),
                t1_ms: i32::MIN,
                t2_ms: i32::MAX,
            },
            Some(&event),
            0,
        ),
        (i32::MAX, 0)
    );
    assert_eq!(
        interpolate_move_exact(
            ParsedMovementExact {
                start: (0.0, 0.0),
                end: (1.0, 0.0),
                t1_ms: i32::MIN,
                t2_ms: i32::MAX,
            },
            Some(&event),
            0,
        ),
        (i32::MAX, 0)
    );
}

#[test]
fn scaled_clip_rect_rounds_edges_like_libass() {
    let mapping = EventMapping {
        explicit: true,
        use_margins: false,
        scale_x: 0.5,
        scale_y: 1.5,
        margin_left: 0.0,
        margin_top: 0.0,
        frame_w: 960.0,
        frame_h: 1620.0,
        fit_w: 960.0,
        fit_h: 1620.0,
        play_res_x: 1920.0,
        play_res_y: 1080.0,
    };
    assert_eq!(
        scale_clip_rect_exact(
            ParsedRectF64 {
                x_min: 659.0,
                y_min: 35.0,
                x_max: 1261.0,
                y_max: 48.0,
            },
            &mapping,
        ),
        Rect {
            x_min: 330,
            y_min: 53,
            x_max: 631,
            y_max: 72,
        }
    );
}

#[test]
fn scaled_exact_clip_rect_rounds_after_mapping_like_libass() {
    let mapping = EventMapping {
        explicit: true,
        use_margins: false,
        scale_x: 2.0,
        scale_y: 2.0,
        margin_left: 0.0,
        margin_top: 0.0,
        frame_w: 200.0,
        frame_h: 200.0,
        fit_w: 200.0,
        fit_h: 200.0,
        play_res_x: 100.0,
        play_res_y: 100.0,
    };
    assert_eq!(
        scale_clip_rect_exact(
            ParsedRectF64 {
                x_min: 0.25,
                y_min: 0.25,
                x_max: 10.25,
                y_max: 20.25,
            },
            &mapping,
        ),
        Rect {
            x_min: 1,
            y_min: 1,
            x_max: 21,
            y_max: 41,
        }
    );
}

#[test]
fn animated_clip_resolution_keeps_fractional_edges_until_mapping() {
    let event = LayoutEvent {
        clip_rect: Some(Rect {
            x_min: 0,
            y_min: 0,
            x_max: 100,
            y_max: 100,
        }),
        lines: vec![LayoutLine {
            runs: vec![LayoutGlyphRun {
                transforms: vec![ParsedSpanTransform {
                    start_ms: 0,
                    end_ms: Some(1000),
                    accel: 1.0,
                    style: ParsedAnimatedStyle {
                        clip_rect: Some(ParsedRectF64 {
                            x_min: 1.0,
                            y_min: 1.0,
                            x_max: 11.0,
                            y_max: 21.0,
                        }),
                        clip_inverse: Some(false),
                        ..Default::default()
                    },
                }],
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let source = ParsedEvent {
        start: 0,
        duration: 1000,
        ..Default::default()
    };
    let track = ParsedTrack {
        play_res_x: 100,
        play_res_y: 100,
        ..Default::default()
    };
    let mapping = EventMapping {
        explicit: true,
        use_margins: false,
        scale_x: 2.0,
        scale_y: 2.0,
        margin_left: 0.0,
        margin_top: 0.0,
        frame_w: 200.0,
        frame_h: 200.0,
        fit_w: 200.0,
        fit_h: 200.0,
        play_res_x: 100.0,
        play_res_y: 100.0,
    };

    let (clip, inverse) =
        resolve_rect_clip(&event, &track, Some(&source), 500).expect("animated clip");
    assert!(!inverse);
    assert_eq!(scale_clip_rect_exact(clip, &mapping).x_min, 1);
}

#[test]
fn transform_accel_preserves_libass_power_value() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            font_size: 10.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: -1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                font_size: Some(20.0),
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 500).font_size, 30.0);
}

#[test]
fn transform_zero_end_time_uses_event_duration_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            font_size: 10.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(0),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                font_size: Some(20.0),
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 500).font_size, 15.0);
}

#[test]
fn transform_negative_start_time_is_not_clamped_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            font_size: 10.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: -100,
            end_ms: Some(100),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                font_size: Some(20.0),
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 0).font_size, 15.0);
}

#[test]
fn transform_bare_font_size_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            font_size: 40.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                font_size_steps: vec![ParsedFontSizeTransform::Reset { reset: 20.0 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 0).font_size, 20.0);
}

#[test]
fn transform_repeated_relative_font_size_compounds_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            font_size: 20.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                font_size_steps: vec![
                    ParsedFontSizeTransform::Relative {
                        value: 5.0,
                        reset: 20.0,
                    },
                    ParsedFontSizeTransform::Relative {
                        value: 5.0,
                        reset: 20.0,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 500).font_size, 31.25);
}

#[test]
fn transform_repeated_absolute_font_size_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            font_size: 20.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                font_size_steps: vec![
                    ParsedFontSizeTransform::Absolute {
                        value: 40.0,
                        reset: 20.0,
                    },
                    ParsedFontSizeTransform::Absolute {
                        value: 50.0,
                        reset: 20.0,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 500).font_size, 40.0);
}

#[test]
fn transform_bare_scale_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            scale_x: 4.0,
            scale_y: 5.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                scale_x_steps: vec![ParsedScaleTransform::Reset { reset: 1.2 }],
                scale_y_steps: vec![ParsedScaleTransform::Reset { reset: 0.8 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 0);
    assert_eq!(resolved.scale_x, 1.2);
    assert_eq!(resolved.scale_y, 0.8);
}

#[test]
fn transform_repeated_scale_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            scale_x: 1.0,
            scale_y: 1.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                scale_x_steps: vec![
                    ParsedScaleTransform::Absolute {
                        value: 2.0,
                        reset: 1.0,
                    },
                    ParsedScaleTransform::Absolute {
                        value: 3.0,
                        reset: 1.0,
                    },
                ],
                scale_y_steps: vec![
                    ParsedScaleTransform::Absolute {
                        value: 0.5,
                        reset: 1.0,
                    },
                    ParsedScaleTransform::Absolute {
                        value: 0.25,
                        reset: 1.0,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 500);
    assert_eq!(resolved.scale_x, 2.25);
    assert_eq!(resolved.scale_y, 0.5);
}

#[test]
fn transform_bare_spacing_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            spacing: 20.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                spacing_steps: vec![ParsedLinearTransform::Reset { reset: 4.0 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 0).spacing, 4.0);
}

#[test]
fn transform_repeated_spacing_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            spacing: 4.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                spacing_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 8.0,
                        reset: 4.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 12.0,
                        reset: 4.0,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(resolve_run_style(&run, Some(&event), 500).spacing, 9.0);
}

#[test]
fn transform_bare_rotation_shear_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            rotation_x: 9.0,
            rotation_y: 8.0,
            rotation_z: 90.0,
            shear_x: 0.2,
            shear_y: 0.3,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                rotation_x_steps: vec![ParsedLinearTransform::Reset { reset: 0.0 }],
                rotation_y_steps: vec![ParsedLinearTransform::Reset { reset: 0.0 }],
                rotation_z_steps: vec![ParsedLinearTransform::Reset { reset: 15.0 }],
                shear_x_steps: vec![ParsedLinearTransform::Reset { reset: 0.0 }],
                shear_y_steps: vec![ParsedLinearTransform::Reset { reset: 0.0 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 0);
    assert_eq!(resolved.rotation_x, 0.0);
    assert_eq!(resolved.rotation_y, 0.0);
    assert_eq!(resolved.rotation_z, 15.0);
    assert_eq!(resolved.shear_x, 0.0);
    assert_eq!(resolved.shear_y, 0.0);
}

#[test]
fn transform_repeated_rotation_shear_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            rotation_x: 0.0,
            rotation_y: 0.0,
            rotation_z: 15.0,
            shear_x: 0.1,
            shear_y: 0.2,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                rotation_x_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 10.0,
                        reset: 0.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 20.0,
                        reset: 0.0,
                    },
                ],
                rotation_y_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: -5.0,
                        reset: 0.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 5.0,
                        reset: 0.0,
                    },
                ],
                rotation_z_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 30.0,
                        reset: 15.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 60.0,
                        reset: 15.0,
                    },
                ],
                shear_x_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 0.2,
                        reset: 0.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 0.3,
                        reset: 0.0,
                    },
                ],
                shear_y_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 0.4,
                        reset: 0.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 0.5,
                        reset: 0.0,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 500);
    assert_eq!(resolved.rotation_x, 12.5);
    assert_eq!(resolved.rotation_y, 1.25);
    assert_eq!(resolved.rotation_z, 41.25);
    assert_eq!(resolved.shear_x, 0.225);
    assert_eq!(resolved.shear_y, 0.4);
}

#[test]
fn transform_bare_blur_be_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            blur: 5.0,
            be: 6.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                blur_steps: vec![ParsedLinearTransform::Reset { reset: 0.0 }],
                be_steps: vec![ParsedLinearTransform::Reset { reset: 0.0 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 0);
    assert_eq!(resolved.blur, 0.0);
    assert_eq!(resolved.be, 0.0);
}

#[test]
fn transform_repeated_blur_be_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            blur: 0.0,
            be: 0.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                blur_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 20.0,
                        reset: 0.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 40.0,
                        reset: 0.0,
                    },
                ],
                be_steps: vec![
                    ParsedLinearTransform::Absolute {
                        value: 2.0,
                        reset: 0.0,
                    },
                    ParsedLinearTransform::Absolute {
                        value: 4.0,
                        reset: 0.0,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 500);
    assert_eq!(resolved.blur, 25.0);
    assert_eq!(resolved.be, 3.0);
}

#[test]
fn transform_bare_border_shadow_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            border: 9.0,
            border_x: 10.0,
            border_y: 11.0,
            shadow: 12.0,
            shadow_x: 13.0,
            shadow_y: 14.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                border_x_steps: vec![ParsedAxisTransform::Reset { reset: 2.0 }],
                border_y_steps: vec![ParsedAxisTransform::Reset { reset: 2.0 }],
                shadow_x_steps: vec![ParsedAxisTransform::Reset { reset: 3.0 }],
                shadow_y_steps: vec![ParsedAxisTransform::Reset { reset: 3.0 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 0);
    assert_eq!(resolved.border, 2.0);
    assert_eq!(resolved.border_x, 2.0);
    assert_eq!(resolved.border_y, 2.0);
    assert_eq!(resolved.shadow, 3.0);
    assert_eq!(resolved.shadow_x, 3.0);
    assert_eq!(resolved.shadow_y, 3.0);
}

#[test]
fn transform_repeated_border_shadow_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            border: 2.0,
            border_x: 2.0,
            border_y: 2.0,
            shadow: 3.0,
            shadow_x: 3.0,
            shadow_y: 3.0,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                border_x_steps: vec![
                    ParsedAxisTransform::Absolute {
                        value: 8.0,
                        reset: 2.0,
                        clamp: true,
                    },
                    ParsedAxisTransform::Absolute {
                        value: -20.0,
                        reset: 2.0,
                        clamp: true,
                    },
                ],
                border_y_steps: vec![
                    ParsedAxisTransform::Absolute {
                        value: 8.0,
                        reset: 2.0,
                        clamp: true,
                    },
                    ParsedAxisTransform::Absolute {
                        value: 6.0,
                        reset: 2.0,
                        clamp: true,
                    },
                ],
                shadow_x_steps: vec![
                    ParsedAxisTransform::Absolute {
                        value: -5.0,
                        reset: 3.0,
                        clamp: true,
                    },
                    ParsedAxisTransform::Absolute {
                        value: -2.0,
                        reset: 3.0,
                        clamp: false,
                    },
                ],
                shadow_y_steps: vec![
                    ParsedAxisTransform::Absolute {
                        value: -5.0,
                        reset: 3.0,
                        clamp: true,
                    },
                    ParsedAxisTransform::Absolute {
                        value: 7.0,
                        reset: 3.0,
                        clamp: false,
                    },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let resolved = resolve_run_style(&run, Some(&event), 500);
    assert_eq!(resolved.border_x, 0.0);
    assert_eq!(resolved.border_y, 5.5);
    assert_eq!(resolved.border, 5.5);
    assert_eq!(resolved.shadow_x, -1.0);
    assert_eq!(resolved.shadow_y, 3.5);
    assert_eq!(resolved.shadow, 3.5);
}

#[test]
fn transform_bare_colour_alpha_reset_ignores_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            primary_colour: 0xFF00_0000,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 100,
            end_ms: Some(200),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                primary_colour_steps: vec![
                    ParsedColourTransform::ResetRgb { reset: 0x2010_2030 },
                    ParsedColourTransform::ResetAlpha { reset: 0x20 },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(
        resolve_run_style(&run, Some(&event), 0).primary_colour,
        0x2010_2030
    );
}

#[test]
fn transform_repeated_colour_alpha_uses_ordered_power_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            primary_colour: 0x2010_2030,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                primary_colour_steps: vec![
                    ParsedColourTransform::Rgb { value: 0x000000 },
                    ParsedColourTransform::Rgb { value: 0xFFFFFF },
                    ParsedColourTransform::Alpha { value: 0x80 },
                    ParsedColourTransform::Alpha { value: 0x40 },
                    ParsedColourTransform::Alpha { value: 0x20 },
                ],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    assert_eq!(
        resolve_run_style(&run, Some(&event), 500).primary_colour,
        0x3483_878b
    );
}

#[test]
fn transform_alpha_interpolates_raw_int_before_byte_truncation_like_libass() {
    let event = ParsedEvent {
        start: 0,
        duration: 1000,
        ..ParsedEvent::default()
    };
    let run = LayoutGlyphRun {
        style: ParsedSpanStyle {
            primary_colour: 0x0010_2030,
            secondary_colour: 0x0010_2030,
            ..ParsedSpanStyle::default()
        },
        transforms: vec![rassa_parse::ParsedSpanTransform {
            start_ms: 0,
            end_ms: Some(1000),
            accel: 1.0,
            style: rassa_parse::ParsedAnimatedStyle {
                primary_colour_steps: vec![ParsedColourTransform::Alpha { value: 0x100 }],
                secondary_colour_steps: vec![ParsedColourTransform::Alpha { value: -1 }],
                ..Default::default()
            },
        }],
        ..LayoutGlyphRun::default()
    };

    let halfway = resolve_run_style(&run, Some(&event), 500);
    assert_eq!(halfway.primary_colour, 0x8010_2030);
    assert_eq!(halfway.secondary_colour, 0xFF10_2030);

    let finished = resolve_run_style(&run, Some(&event), 1000);
    assert_eq!(finished.primary_colour, 0x0010_2030);
    assert_eq!(finished.secondary_colour, 0x0010_2030);
}

#[test]
fn vsfilter_alpha_clipping_wraps_only_after_transform_interpolation() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 120\nPlayResY: 120\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,93,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\\1a0\\t(\\1a1FF)}A";
    let track = parse_script_text(script).expect("alpha clipping fixture should parse");
    let provider = BundledFontProvider::aileron_regular();
    let renderer = RenderEngine::new();

    for (time_ms, expected_alpha) in [(2040, 0xD0), (2560, 0x05), (4960, 0xFA)] {
        let planes = renderer.render_frame_with_provider(&track, &provider, time_ms);
        let character = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("alpha clipping fixture should render a character plane");
        assert_eq!(
            character.color.0,
            0xFFFF_FF00 | expected_alpha,
            "VSFilter truncates the interpolated raw alpha to u8 at {time_ms} ms"
        );
    }
}

fn vertical_span(planes: &[ImagePlane]) -> i32 {
    let min_y = planes
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("plane");
    let max_y = planes
        .iter()
        .map(|plane| plane.destination.y + plane.size.height)
        .max()
        .expect("plane");
    max_y - min_y
}

fn kind_bounds(planes: &[ImagePlane], kind: ass::ImageType) -> Option<Rect> {
    let mut matching_planes = planes.iter().filter(|plane| plane.kind == kind);
    let first = matching_planes.next()?;
    let mut bounds = Rect {
        x_min: first.destination.x,
        y_min: first.destination.y,
        x_max: first.destination.x + first.size.width,
        y_max: first.destination.y + first.size.height,
    };
    for plane in matching_planes {
        bounds.x_min = bounds.x_min.min(plane.destination.x);
        bounds.y_min = bounds.y_min.min(plane.destination.y);
        bounds.x_max = bounds.x_max.max(plane.destination.x + plane.size.width);
        bounds.y_max = bounds.y_max.max(plane.destination.y + plane.size.height);
    }
    Some(bounds)
}

fn character_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    kind_bounds(planes, ass::ImageType::Character)
}

fn visible_bounds(planes: &[ImagePlane]) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for plane in planes {
        let stride = plane.stride.max(0) as usize;
        if stride == 0 {
            continue;
        }
        for y in 0..plane.size.height.max(0) as usize {
            for x in 0..plane.size.width.max(0) as usize {
                if plane.bitmap[y * stride + x] == 0 {
                    continue;
                }
                let px = plane.destination.x + x as i32;
                let py = plane.destination.y + y as i32;
                match &mut bounds {
                    Some(rect) => {
                        rect.x_min = rect.x_min.min(px);
                        rect.y_min = rect.y_min.min(py);
                        rect.x_max = rect.x_max.max(px + 1);
                        rect.y_max = rect.y_max.max(py + 1);
                    }
                    None => {
                        bounds = Some(Rect {
                            x_min: px,
                            y_min: py,
                            x_max: px + 1,
                            y_max: py + 1,
                        });
                    }
                }
            }
        }
    }
    bounds
}

fn visible_kind_bounds(planes: &[ImagePlane], kind: ass::ImageType) -> Option<Rect> {
    let matching = planes
        .iter()
        .filter(|plane| plane.kind == kind)
        .cloned()
        .collect::<Vec<_>>();
    visible_bounds(&matching)
}

fn drawing_alignment_script(alignment: i32, override_tags: &str, event_margins: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,32,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,{alignment},30,50,15,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,{event_margins},,{{{override_tags}\\p1}}m 0 0 l 40 0 40 20 0 20\n"
    )
}

fn render_drawing_bounds(script: &str) -> Rect {
    let track = parse_script_text(script).expect("alignment probe script should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    visible_bounds(&planes).expect("drawing probe should produce visible pixels")
}

fn text_alignment_script(alignment: i32, event_margins: &str) -> String {
    format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 180\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,32,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,{alignment},30,50,15,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,{event_margins},,Margin\n"
    )
}

fn render_text_bounds(script: &str) -> Option<Rect> {
    let track = parse_script_text(script).expect("text alignment probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    visible_bounds(&planes)
}

fn render_text_bounds_with_config(script: &str, config: &RendererConfig) -> Option<Rect> {
    let track = parse_script_text(script).expect("text alignment probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(&track, &provider, 500, config);
    visible_bounds(&planes)
}

fn baseline_fontconfig_matches_dejavu_fallback(family: &str) -> bool {
    baseline_fontconfig_family_contains(family, "DejaVu")
}

fn baseline_fontconfig_family_contains(family: &str, expected: &str) -> bool {
    let provider = FontconfigProvider::new();
    provider
        .resolve(&FontQuery::new(family))
        .family
        .contains(expected)
}

fn render_text_plane_bounds(script: &str) -> Option<Rect> {
    render_text_plane_bounds_at(script, 500)
}

fn render_text_plane_bounds_at(script: &str, now_ms: i64) -> Option<Rect> {
    render_text_kind_bounds_at(script, now_ms, ass::ImageType::Character)
}

fn render_text_kind_bounds_at(script: &str, now_ms: i64, kind: ass::ImageType) -> Option<Rect> {
    let track = parse_script_text(script).expect("text plane probe script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, now_ms);
    kind_bounds(&planes, kind)
}

fn assert_rect_near(actual: Option<Rect>, expected: Rect, tolerance: i32, context: &str) {
    let actual = actual.unwrap_or_else(|| panic!("{context}: expected {expected:?}, got None"));
    assert!(
        (actual.x_min - expected.x_min).abs() <= tolerance
            && (actual.y_min - expected.y_min).abs() <= tolerance
            && (actual.x_max - expected.x_max).abs() <= tolerance
            && (actual.y_max - expected.y_max).abs() <= tolerance,
        "{context}: actual={actual:?} expected={expected:?} tolerance={tolerance}"
    );
}

#[test]
fn decimal_positioned_drawing_uses_exact_coordinates() {
    let decimal = drawing_alignment_script(7, "\\pos(100.6,50.6)", "0,0,0");
    let integer = drawing_alignment_script(7, "\\pos(101,51)", "0,0,0");

    assert_eq!(
        render_drawing_bounds(&decimal),
        render_drawing_bounds(&integer)
    );
}

#[test]
fn decimal_move_interpolates_from_exact_coordinates() {
    let decimal = drawing_alignment_script(7, "\\move(10.5,20.5,110.5,120.5,0,1000)", "0,0,0");
    let integer = drawing_alignment_script(7, "\\move(61,71,61,71)", "0,0,0");

    assert_eq!(
        render_drawing_bounds(&decimal),
        render_drawing_bounds(&integer)
    );
}

#[test]
fn downscaled_positioned_text_scales_font_and_anchor_like_libass() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,42,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\an5\\pos(320,180)}POS\n";
    let config = RendererConfig {
        frame: Size {
            width: 320,
            height: 180,
        },
        storage: Size {
            width: 320,
            height: 180,
        },
        pixel_aspect: 1.0,
        shaping: ass::ShapingLevel::Complex,
        ..Default::default()
    };
    let actual = render_text_bounds_with_config(script, &config)
        .expect("positioned text should render in downscaled frame");
    let expected = Rect {
        x_min: 141,
        y_min: 83,
        x_max: 179,
        y_max: 97,
    };

    assert!(
        (actual.x_min - expected.x_min).abs() <= 2 && (actual.y_min - expected.y_min).abs() <= 1,
        "downscaled \\pos anchor should stay in libass position: actual={actual:?} expected={expected:?}"
    );
    assert!(
        (actual.width() - expected.width()).abs() <= 2
            && (actual.height() - expected.height()).abs() <= 2,
        "downscaled \\pos text must scale glyph dimensions like libass: actual={actual:?} expected={expected:?}"
    );
}

#[test]
fn positioned_center_text_anchors_visible_ink_not_layout_advance() {
    if !baseline_fontconfig_matches_dejavu_fallback("Againts") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs140\\bord0\\blur1\\fnAgaints\\pos(947.46,191.6)}ท่านคาชิวากิ อาซาฮิ\n";
    let actual = render_text_bounds(script).expect("baseline positioned text should render");
    let center_x = (actual.x_min + actual.x_max) / 2;

    assert!(
        (center_x - 947).abs() <= 8,
        "\\pos center anchor must use visible rendered text width, not stale layout advance: bounds={actual:?} center_x={center_x}"
    );
    assert!(
        (actual.y_min - 80).abs() <= 4,
        "bottom-aligned \\pos text must reserve libass-like descender space below visible glyphs: bounds={actual:?}"
    );
}

#[test]
fn positioned_multiline_text_uses_libass_like_line_gap_and_descender_space() {
    if !baseline_fontconfig_matches_dejavu_fallback("Raphtalia") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs100\\bord0\\blur1\\fnRaphtalia\\b1\\pos(944.4,752.8)}จงเตรียมตัว\\Nให้พร้อมสรรพก่อนมา\n";
    let actual =
        render_text_bounds(script).expect("baseline multiline positioned text should render");

    assert!(
        (actual.y_min - 570).abs() <= 6,
        "multiline bottom-aligned \\pos text should use libass-like vertical block metrics: bounds={actual:?}"
    );
    assert!(
        (actual.height() - 158).abs() <= 8,
        "multiline positioned text should keep libass-like line gap: bounds={actual:?}"
    );
}

#[test]
fn positioned_multiline_text_aligns_deep_glyph_bottoms_like_libass() {
    if !baseline_fontconfig_matches_dejavu_fallback("Raphtalia") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs100\\bord0\\blur1\\fnRaphtalia\\b1\\pos(928,992)}ห้ามท่านหนี\\Nจากคำขอนี้เป็นอันขาด\n";
    let actual =
        render_text_bounds(script).expect("baseline multiline positioned text should render");

    assert!(
        (actual.y_min - 808).abs() <= 4,
        "top of deep-glyph multiline block should match libass baseline line 1270: bounds={actual:?}"
    );
    assert!(
        (actual.y_max - 968).abs() <= 4,
        "bottom-aligned \\pos should keep deep Thai glyphs above the libass descender gap: bounds={actual:?}"
    );
    assert!(
        (actual.height() - 160).abs() <= 6,
        "deep-glyph multiline block should keep libass-like visible-bottom line spacing: bounds={actual:?}"
    );
}

#[test]
fn positioned_thai_deep_glyphs_keep_libass_like_bottom_anchor() {
    if !baseline_fontconfig_family_contains("K2D ExtraBold", "K2D") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 400\nPlayResY: 240\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED TH2,K2D ExtraBold,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0,0,2,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,ED TH2,,0,0,0,,{\\an2\\pos(200,180)\\bord0\\shad0\\blur0}อุ อู ญ ฐ ฏ ฎ\n";
    let actual = render_text_bounds(script).expect("Thai positioned text should render");

    assert!(
        (actual.y_min - 132).abs() <= 4,
        "Thai lower vowels and descender glyphs should not be raised above libass-like bottom anchor: bounds={actual:?}"
    );
    assert!(
        (actual.y_max - 173).abs() <= 4,
        "Thai deep glyph bottom should stay near libass-like descender plane: bounds={actual:?}"
    );
}

#[test]
fn lower_ed_th2_positioned_per_glyph_line_matches_libass_bounds() {
    if !baseline_fontconfig_family_contains("K2D ExtraBold", "K2D") {
        return;
    }
    let provider = FontconfigProvider::new();
    let script = r#"[Script Info]
ScriptType: v4.00+
WrapStyle: 0
PlayResX: 1920
PlayResY: 1080
ScaledBorderAndShadow: yes
YCbCr Matrix: TV.709

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED TH2,K2D ExtraBold,75,&H00FFFFFF,&H0094FDFF,&H00000000,&H00B5B7B7,-1,0,0,0,100,100,0,0,1,0.7,3,2,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(677.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(0,160,\alpha&H00&)\t(4790,\alpha&HFF&)}ฉั
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(703.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(20,180,\alpha&H00&)\t(4810,\alpha&HFF&)}น
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(728.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(40,200,\alpha&H00&)\t(4830,\alpha&HFF&)}คื
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(752.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(60,220,\alpha&H00&)\t(4850,\alpha&HFF&)}อ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(775.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(80,240,\alpha&H00&)\t(4870,\alpha&HFF&)}ส
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(797.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(100,260,\alpha&H00&)\t(4890,\alpha&HFF&)}า
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(818.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(120,280,\alpha&H00&)\t(4910,\alpha&HFF&)}ว
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(840.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(140,300,\alpha&H00&)\t(4930,\alpha&HFF&)}แ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(863.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(160,320,\alpha&H00&)\t(4950,\alpha&HFF&)}ก
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(887.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(180,340,\alpha&H00&)\t(4970,\alpha&HFF&)}ร่
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(909.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(200,360,\alpha&H00&)\t(4990,\alpha&HFF&)}ง
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(931.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(220,380,\alpha&H00&)\t(5010,\alpha&HFF&)}ผู้
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(952.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(240,400,\alpha&H00&)\t(5030,\alpha&HFF&)}ไ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(972.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(260,420,\alpha&H00&)\t(5050,\alpha&HFF&)}ร้
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(990.8,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(280,440,\alpha&H00&)\t(5070,\alpha&HFF&)}เ
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1010,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(300,460,\alpha&H00&)\t(5090,\alpha&HFF&)}ที
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1034.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(320,480,\alpha&H00&)\t(5110,\alpha&HFF&)}ย
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1059.5,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(340,500,\alpha&H00&)\t(5130,\alpha&HFF&)}ม
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1085.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(360,520,\alpha&H00&)\t(5150,\alpha&HFF&)}ท
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1108.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(380,540,\alpha&H00&)\t(5170,\alpha&HFF&)}า
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1131.3,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(400,560,\alpha&H00&)\t(5190,\alpha&HFF&)}น
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1149.2,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(420,580,\alpha&H00&)\t(5210,\alpha&HFF&)}
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1167.9,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(440,600,\alpha&H00&)\t(5230,\alpha&HFF&)}A
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1192.6,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(460,620,\alpha&H00&)\t(5250,\alpha&HFF&)}h
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1208.7,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(480,640,\alpha&H00&)\t(5270,\alpha&HFF&)}
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1224.4,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(500,660,\alpha&H00&)\t(5290,\alpha&HFF&)}a
Dialogue: 0,0:21:45.28,0:21:50.57,ED TH2,,0,0,0,fx,{\an2\pos(1246.1,1050)\bord0.7\shad3\blur0\c&HFFFFFF&\3c&H000000&\4c&HB5B7B7&\fad(200,400)\alpha&HFF&\t(520,680,\alpha&H00&)\t(5310,\alpha&HFF&)}h
"#;
    let track = parse_script_text(script).expect("lower ED TH2 regression script should parse");
    let engine = RenderEngine::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 1_308_800);
    assert_eq!(
        planes.len(),
        75,
        "lower ED TH2 fixture should emit one shadow, outline, and character plane per visible glyph"
    );
    let actual =
        visible_bounds(&planes).expect("lower ED TH2 fixture should render visible pixels");
    let expected = Rect {
        x_min: 663,
        y_min: 986,
        x_max: 1267,
        y_max: 1045,
    };

    assert_rect_near(
        Some(actual),
        expected,
        5,
        "lower ED TH2 logic should keep glyph count and visible bounds near libass while rasterizer parity is out of scope",
    );
}

#[test]
fn rotated_positioned_text_keeps_libass_like_transparent_frz_plane() {
    if !baseline_fontconfig_family_contains("Raphtalia", "Raphtalia") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 2\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\fs66\\shad\\bord0\\blur1\\fnRaphtalia\\c&H070707&\\b0\\fscx99\\fscy107\\frz345.2\\pos(1258.48,593.06)}หลังเลิกเรียน จะรอที่\n";
    let actual = render_text_plane_bounds(script).expect("rotated positioned text should render");
    let expected = Rect {
        x_min: 1091,
        y_min: 499,
        x_max: 1461,
        y_max: 626,
    };

    assert!(
        (actual.x_min - expected.x_min).abs() <= 3
            && (actual.y_min - expected.y_min).abs() <= 3
            && (actual.x_max - expected.x_max).abs() <= 3
            && (actual.y_max - expected.y_max).abs() <= 3,
        "rotated positioned text should keep libass-like transparent \\frz plane: actual={actual:?} expected={expected:?}"
    );
}

#[test]
fn decimal_clipped_transformed_single_char_keeps_libass_like_plane() {
    if !baseline_fontconfig_matches_dejavu_fallback("OFL Sorts Mill Goudy TT") {
        return;
    }
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nWrapStyle: 0\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 8,0:00:00.00,0:00:01.00,ED2,,0,0,0,fx,{\\move(727.1,73,727.1,65)\\org(637.1,-25)\\t(53.571428571429,107.14285714286,\\frz4)\\t(107.14285714286,160.71428571429,\\frz-4)\\t(160.71428571429,214.28571428571,\\frz4\\t(214.28571428571,267.85714285714,\\frz-4\\t(267.85714285714,321.42857142857,\\frz4\\t(321.42857142857,375,\\frz-4\\t(375,428.57142857143,\\frz4\\t(857.14285714286,482.14285714286,\\frz-4\\t(482.14285714286,535.71428571429,\\frz4\\t(535.71428571429,589.28571428571,\\frz-4\\t(589.28571428571,642.85714285714,\\frz4\\t(642.85714285714,696.42857142857,\\frz-4\\t(696.42857142857,750,\\frz0)))))))))))\\b0\\bord0\\blur0.2\\shad0\\an5\\fs80\\t(0,750,\\fs70\\frz0)\\clip(659.3,63.6,1260.8,77.4)\\c&H9DD9FC&}I\n";
    let actual = render_text_plane_bounds(script)
        .expect("02.ass-style decimal clipped transformed glyph should emit a plane");

    assert_eq!(
        actual,
        Rect {
            x_min: 721,
            y_min: 63,
            x_max: 745,
            y_max: 77,
        },
        "decimal rectangular clip over transformed one-char text should keep the current libass ASS_Image plane geometry"
    );
}

#[test]
fn clipped_org_move_single_char_slice_keeps_libass_like_plane() {
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,Arial,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.93,ED2,,0,0,0,fx,{\move(1072.3,57,1072.3,65)\org(982.3,-25)\t(66.428571428571,132.85714285714,\frz4)\t(132.85714285714,199.28571428571,\frz-4)\t(199.28571428571,265.71428571429,\frz4\t(265.71428571429,332.14285714286,\frz-4\t(332.14285714286,398.57142857143,\frz4\t(398.57142857143,465,\frz-4\t(465,531.42857142857,\frz4\t(1062.8571428571,597.85714285714,\frz-4\t(597.85714285714,664.28571428571,\frz4\t(664.28571428571,730.71428571429,\frz-4\t(730.71428571429,797.14285714286,\frz4\t(797.14285714286,863.57142857143,\frz-4\t(863.57142857143,930,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,930,\fs70\frz0)\clip(659.3,32.4,1260.8,45.8)\c&HDEF2FE&}A
"#;
    assert_rect_near(
        render_text_plane_bounds_at(script, 870),
        Rect {
            x_min: 1046,
            y_min: 39,
            x_max: 1102,
            y_max: 45,
        },
        // rassa transforms rendered bitmaps while libass transforms outlines,
        // so edge coverage spreads differently; guard placement, not exact
        // coverage extent.
        8,
        "02.ass line 577-style clipped org/move transformed glyph should retain libass-like placement",
    );
}

#[test]
fn clipped_org_move_empty_edge_slices_keep_libass_like_planes() {
    if !baseline_fontconfig_matches_dejavu_fallback("OFL Sorts Mill Goudy TT") {
        return;
    }

    let script = |clip: &str, text: &str, move_x: &str, move_y: &str, org_x: &str| {
        format!(
            r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,OFL Sorts Mill Goudy TT,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 8,0:00:00.00,0:00:00.93,ED2,,0,0,0,fx,{{\move({move_x},{move_y},{move_x},65)\org({org_x},-25)\t(66.428571428571,132.85714285714,\frz4)\t(132.85714285714,199.28571428571,\frz-4)\t(199.28571428571,265.71428571429,\frz4\t(265.71428571429,332.14285714286,\frz-4\t(332.14285714286,398.57142857143,\frz4\t(398.57142857143,465,\frz-4\t(465,531.42857142857,\frz4\t(1062.8571428571,597.85714285714,\frz-4\t(597.85714285714,664.28571428571,\frz4\t(664.28571428571,730.71428571429,\frz-4\t(730.71428571429,797.14285714286,\frz4\t(797.14285714286,863.57142857143,\frz-4\t(863.57142857143,930,\frz0)))))))))))\b0\bord0\blur0.2\shad0\an5\fs80\t(0,930,\fs70\frz0){clip}\c&H62C3FA&}}{text}
"#
        )
    };

    assert_rect_near(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,92.2,1260.8,106.36666666667)",
                "A",
                "1072.3",
                "57",
                "982.3",
            ),
            870,
        ),
        Rect {
            x_min: 1047,
            y_min: 92,
            x_max: 1103,
            y_max: 93,
        },
        1,
        "02.ass lower empty clipped A slice should keep libass transparent ASS_Image plane geometry",
    );
    assert_rect_near(
        render_text_plane_bounds_at(
            &script(
                "\\clip(659.3,27.2,1260.8,40.533333333333)",
                "h",
                "1106.8",
                "73",
                "1016.8",
            ),
            870,
        ),
        Rect {
            x_min: 1088,
            y_min: 36,
            x_max: 1128,
            y_max: 40,
        },
        1,
        "02.ass upper empty clipped h slice should keep libass transparent ASS_Image plane geometry",
    );
}

#[test]
fn transformed_move_origin_single_char_keeps_libass_like_plane_padding() {
    // Bitmap-space transforms spread AA coverage differently from libass's
    // outline-space transforms; placement is asserted, exact extent is not.
    if !baseline_fontconfig_family_contains("Arial", "Liberation") {
        return;
    }
    let script = r#"[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 0
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: ED2,Arial,70,&H00FFAACD,&H00000000,&H00FFFFFF,&H00FFAACD,-1,0,0,0,100,100,0,0,1,3,3,8,30,30,30,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 7,0:00:00.00,0:00:01.00,ED2,,0,0,0,fx,{\move(808.8,73,808.8,65)\org(718.8,-25)\t(27.857142857143,55.714285714286,\frz4)\t(55.714285714286,83.571428571429,\frz-4)\t(83.571428571429,111.42857142857,\frz4\t(111.42857142857,139.28571428571,\frz-4\t(139.28571428571,167.14285714286,\frz4\t(167.14285714286,195,\frz-4\t(195,222.85714285714,\frz4\t(445.71428571429,250.71428571429,\frz-4\t(250.71428571429,278.57142857143,\frz4\t(278.57142857143,306.42857142857,\frz-4\t(306.42857142857,334.28571428571,\frz4\t(334.28571428571,362.14285714286,\frz-4\t(362.14285714286,390,\frz0)))))))))))\b0\bord3.5\blur1.5\fs80\an5\c&HFFFFFF&\3c&HFFFFFF&\t(0,390,\fs70\frz0)\1a&H70&}s
"#;
    assert_rect_near(
        render_text_kind_bounds_at(script, 195, ass::ImageType::Shadow),
        Rect {
            x_min: 785,
            y_min: 57,
            x_max: 841,
            y_max: 113,
        },
        12,
        "shadow ASS_Image plane should stay near libass for the 02.ass move/origin transform fixture",
    );
    assert_rect_near(
        render_text_kind_bounds_at(script, 195, ass::ImageType::Outline),
        Rect {
            x_min: 782,
            y_min: 54,
            x_max: 838,
            y_max: 110,
        },
        12,
        "outline ASS_Image plane should stay near libass for the 02.ass move/origin transform fixture",
    );
    assert_rect_near(
        render_text_kind_bounds_at(script, 195, ass::ImageType::Character),
        Rect {
            x_min: 789,
            y_min: 61,
            x_max: 821,
            y_max: 109,
        },
        12,
        "character ASS_Image plane should stay near libass for the 02.ass move/origin transform fixture",
    );
}

#[test]
fn positioned_drawing_fry_uses_libass_like_projective_camera() {
    let script = |override_tags: &str| {
        format!(
            "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{{{override_tags}\\p1}}m 0 0 l 710 0 710 18 0 18\n"
        )
    };
    let plain = parse_script_text(&script("\\an2\\pos(953,563)"))
        .expect("plain positioned drawing should parse");
    let projected = parse_script_text(&script("\\an2\\pos(953,563)\\frx14\\fry4"))
        .expect("projected positioned drawing should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let plain_bounds = character_bounds(&engine.render_frame_with_provider(&plain, &provider, 500))
        .expect("plain drawing should render");
    let projected_bounds =
        character_bounds(&engine.render_frame_with_provider(&projected, &provider, 500))
            .expect("projected drawing should render");

    assert!(
        projected_bounds.x_min <= plain_bounds.x_min - 24,
        "libass \\fry perspective shifts bottom-centered drawings left: plain={plain_bounds:?} projected={projected_bounds:?}"
    );
    assert!(
        (projected_bounds.x_min - 568).abs() <= 4,
        "projective camera should match libass-probed left edge for this fixture: projected={projected_bounds:?}"
    );
    assert!(
        (projected_bounds.y_min - 544).abs() <= 2,
        "projective transform should preserve libass-probed vertical placement: projected={projected_bounds:?}"
    );
}

#[test]
fn borderstyle3_opaque_box_follows_text_transform() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,Arial,42,&H00000000,&H000000FF,&H00FFFFFF,&H00000000,0,0,0,0,100,100,0,0,3,4,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Box,,0,0,0,,{\\pos(320,180)\\frz-18\\fax0.25}TRANSFORM BOX\n";
    let track = parse_script_text(script).expect("borderstyle transform script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let box_bounds = kind_bounds(&planes, ass::ImageType::Outline)
        .expect("BorderStyle=3 should emit an opaque box outline plane");

    assert!(
        box_bounds.height() > 90,
        "opaque box must be transformed with the rotated/sheared text, got bounds {box_bounds:?}"
    );
}

#[test]
fn borderstyle3_uses_inline_box_geometry_colours_and_axis_shadow() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 500\nPlayResY: 180\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,DejaVu Sans,30,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,3,2,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Box,,0,0,0,,{\\an5\\pos(250,90)}A{\\xbord6\\ybord1\\3c&H0000FF&\\xshad-4\\yshad5\\4c&H00FF00&}B\n";
    let track = parse_script_text(script).expect("inline BorderStyle=3 script should parse");
    let planes =
        RenderEngine::new().render_frame_with_provider(&track, &FontconfigProvider::new(), 500);

    let overridden_box = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0xFF00_0000)
        .expect("inline \\3c must colour its own opaque-box run");
    let base_box = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0000_0000)
        .expect("the base-style run must retain its own opaque-box colour");
    let overridden_shadow = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x00FF_0000)
        .expect("inline \\4c and axis shadow must create a run-specific box shadow");

    assert!(
        overridden_box.size.width >= base_box.size.width + 4,
        "the larger inline \\xbord must widen only the overridden box: base={base_box:?} override={overridden_box:?}"
    );
    assert_eq!(
        overridden_box.size.height, 32,
        "inline \\ybord1 must replace the base two-pixel vertical padding"
    );
    assert_eq!(
        overridden_shadow.destination.x,
        overridden_box.destination.x - 4,
        "negative \\xshad must move the run-specific shadow left"
    );
    assert_eq!(
        overridden_shadow.destination.y,
        overridden_box.destination.y + 5,
        "positive \\yshad must move the run-specific shadow down"
    );
}

#[test]
fn positioned_drawing_an_anchors_match_libass_for_all_alignments() {
    // Expected boxes were probed from libass/ffmpeg for a 40x20 vector drawing at \pos(x,y):
    // bottom align => y - 20, middle align => y - 10, top align => y.
    let cases = [
        (
            1,
            "\\an1\\pos(60,60)",
            Rect {
                x_min: 60,
                y_min: 40,
                x_max: 100,
                y_max: 60,
            },
        ),
        (
            2,
            "\\an2\\pos(160,60)",
            Rect {
                x_min: 140,
                y_min: 40,
                x_max: 180,
                y_max: 60,
            },
        ),
        (
            3,
            "\\an3\\pos(260,60)",
            Rect {
                x_min: 220,
                y_min: 40,
                x_max: 260,
                y_max: 60,
            },
        ),
        (
            4,
            "\\an4\\pos(60,100)",
            Rect {
                x_min: 60,
                y_min: 90,
                x_max: 100,
                y_max: 110,
            },
        ),
        (
            5,
            "\\an5\\pos(160,100)",
            Rect {
                x_min: 140,
                y_min: 90,
                x_max: 180,
                y_max: 110,
            },
        ),
        (
            6,
            "\\an6\\pos(260,100)",
            Rect {
                x_min: 220,
                y_min: 90,
                x_max: 260,
                y_max: 110,
            },
        ),
        (
            7,
            "\\an7\\pos(60,140)",
            Rect {
                x_min: 60,
                y_min: 140,
                x_max: 100,
                y_max: 160,
            },
        ),
        (
            8,
            "\\an8\\pos(160,140)",
            Rect {
                x_min: 140,
                y_min: 140,
                x_max: 180,
                y_max: 160,
            },
        ),
        (
            9,
            "\\an9\\pos(260,140)",
            Rect {
                x_min: 220,
                y_min: 140,
                x_max: 260,
                y_max: 160,
            },
        ),
    ];

    for (alignment, override_tags, expected) in cases {
        let script = drawing_alignment_script(alignment, override_tags, "0,0,0");
        assert_eq!(
            render_drawing_bounds(&script),
            expected,
            "\\an{alignment} positioned drawing anchor should match libass"
        );
    }
}

#[test]
fn moved_drawing_an_anchors_match_libass_for_all_alignments_at_midpoint() {
    let cases = [
        (
            1,
            "\\an1\\move(40,60,80,60)",
            Rect {
                x_min: 60,
                y_min: 40,
                x_max: 100,
                y_max: 60,
            },
        ),
        (
            2,
            "\\an2\\move(140,60,180,60)",
            Rect {
                x_min: 140,
                y_min: 40,
                x_max: 180,
                y_max: 60,
            },
        ),
        (
            3,
            "\\an3\\move(240,60,280,60)",
            Rect {
                x_min: 220,
                y_min: 40,
                x_max: 260,
                y_max: 60,
            },
        ),
        (
            4,
            "\\an4\\move(40,100,80,100)",
            Rect {
                x_min: 60,
                y_min: 90,
                x_max: 100,
                y_max: 110,
            },
        ),
        (
            5,
            "\\an5\\move(140,100,180,100)",
            Rect {
                x_min: 140,
                y_min: 90,
                x_max: 180,
                y_max: 110,
            },
        ),
        (
            6,
            "\\an6\\move(240,100,280,100)",
            Rect {
                x_min: 220,
                y_min: 90,
                x_max: 260,
                y_max: 110,
            },
        ),
        (
            7,
            "\\an7\\move(40,140,80,140)",
            Rect {
                x_min: 60,
                y_min: 140,
                x_max: 100,
                y_max: 160,
            },
        ),
        (
            8,
            "\\an8\\move(140,140,180,140)",
            Rect {
                x_min: 140,
                y_min: 140,
                x_max: 180,
                y_max: 160,
            },
        ),
        (
            9,
            "\\an9\\move(240,140,280,140)",
            Rect {
                x_min: 220,
                y_min: 140,
                x_max: 260,
                y_max: 160,
            },
        ),
    ];

    for (alignment, override_tags, expected) in cases {
        let script = drawing_alignment_script(alignment, override_tags, "0,0,0");
        assert_eq!(
            render_drawing_bounds(&script),
            expected,
            "\\an{alignment} moved drawing anchor should match libass at the event midpoint"
        );
    }
}

#[test]
fn margin_positioned_text_uses_style_and_event_margins_like_libass() {
    let cases = [
        (
            1,
            "0,0,0",
            Rect {
                x_min: 32,
                y_min: 138,
                x_max: 116,
                y_max: 165,
            },
        ),
        (
            2,
            "0,0,0",
            Rect {
                x_min: 108,
                y_min: 138,
                x_max: 192,
                y_max: 165,
            },
        ),
        (
            3,
            "0,0,0",
            Rect {
                x_min: 184,
                y_min: 138,
                x_max: 269,
                y_max: 165,
            },
        ),
        (
            5,
            "0,0,0",
            Rect {
                x_min: 108,
                y_min: 79,
                x_max: 192,
                y_max: 106,
            },
        ),
        (
            7,
            "0,0,0",
            Rect {
                x_min: 32,
                y_min: 20,
                x_max: 116,
                y_max: 47,
            },
        ),
        (
            8,
            "0,0,0",
            Rect {
                x_min: 108,
                y_min: 20,
                x_max: 192,
                y_max: 47,
            },
        ),
        (
            9,
            "7,9,11",
            Rect {
                x_min: 225,
                y_min: 16,
                x_max: 310,
                y_max: 43,
            },
        ),
    ];

    for (alignment, event_margins, expected) in cases {
        let script = text_alignment_script(alignment, event_margins);
        let Some(actual) = render_text_bounds(&script) else {
            return;
        };
        // Text rasterization can have a few pixels of coverage-width drift from libass even
        // with the same Fontconfig face. This regression guards the placement bug: the
        // effective style/event margin anchor must no longer be shifted left or sunk.
        assert!(
            (actual.x_min - expected.x_min).abs() <= 1,
            "text style/event margins and \\an{alignment} x placement should match libass within raster rounding: actual={actual:?} expected={expected:?}"
        );
        assert_eq!(
            actual.y_min, expected.y_min,
            "text style/event margins and \\an{alignment} vertical anchor should match libass"
        );
        assert!(
            (actual.y_max - expected.y_max).abs() <= 1,
            "text style/event margins and \\an{alignment} visible height may drift by one raster row: actual={actual:?} expected={expected:?}"
        );
    }
}

#[test]
fn margin_positioned_drawing_uses_style_and_event_margins_like_libass() {
    // Expected boxes were probed from libass/ffmpeg for a 40x20 vector drawing with
    // style margins L=30/R=50/V=15. Event margins of 0 should fall back to style margins.
    let cases = [
        (
            1,
            Rect {
                x_min: 30,
                y_min: 145,
                x_max: 70,
                y_max: 165,
            },
        ),
        (
            2,
            Rect {
                x_min: 130,
                y_min: 145,
                x_max: 170,
                y_max: 165,
            },
        ),
        (
            3,
            Rect {
                x_min: 230,
                y_min: 145,
                x_max: 270,
                y_max: 165,
            },
        ),
        (
            4,
            Rect {
                x_min: 30,
                y_min: 80,
                x_max: 70,
                y_max: 100,
            },
        ),
        (
            5,
            Rect {
                x_min: 130,
                y_min: 80,
                x_max: 170,
                y_max: 100,
            },
        ),
        (
            6,
            Rect {
                x_min: 230,
                y_min: 80,
                x_max: 270,
                y_max: 100,
            },
        ),
        (
            7,
            Rect {
                x_min: 30,
                y_min: 15,
                x_max: 70,
                y_max: 35,
            },
        ),
        (
            8,
            Rect {
                x_min: 130,
                y_min: 15,
                x_max: 170,
                y_max: 35,
            },
        ),
        (
            9,
            Rect {
                x_min: 230,
                y_min: 15,
                x_max: 270,
                y_max: 35,
            },
        ),
    ];

    for (alignment, expected) in cases {
        let script = drawing_alignment_script(alignment, "", "0,0,0");
        assert_eq!(
            render_drawing_bounds(&script),
            expected,
            "style margins and \\an{alignment} should match libass when no explicit position exists"
        );
    }

    let script = drawing_alignment_script(7, "", "7,9,11");
    assert_eq!(
        render_drawing_bounds(&script),
        Rect {
            x_min: 7,
            y_min: 11,
            x_max: 47,
            y_max: 31
        },
        "non-zero event margins should override style margins for top-left alignment"
    );
}

#[test]
fn projective_transform_keeps_frx_and_fry_axes_distinct() {
    let origin = (320.0, 180.0);
    let frx = ProjectiveMatrix::from_ass_transform_at_origin(
        EventTransform {
            rotation_x: 45.0,
            ..EventTransform::default()
        },
        origin.0,
        origin.1,
        1.0,
    );
    let fry = ProjectiveMatrix::from_ass_transform_at_origin(
        EventTransform {
            rotation_y: 45.0,
            ..EventTransform::default()
        },
        origin.0,
        origin.1,
        1.0,
    );

    let (frx_x, frx_y) = frx.transform_point(320.0, 140.0);
    let (fry_x, fry_y) = fry.transform_point(360.0, 180.0);

    assert!(
        (frx_x - 320.0).abs() < 0.5,
        "frx must not act like fry: {frx_x}"
    );
    assert!(
        frx_y > 140.0,
        "positive frx should pitch the top edge downward: {frx_y}"
    );
    assert!(
        fry_x < 360.0,
        "positive fry should yaw the right edge leftward: {fry_x}"
    );
    assert!(
        (fry_y - 180.0).abs() < 0.5,
        "fry must not act like frx: {fry_y}"
    );
}

#[test]
fn projective_transform_rescales_ass_shear_for_anisotropic_fsc() {
    let fax = ProjectiveMatrix::from_ass_transform_at_origin(
        EventTransform {
            shear_x: 0.4,
            scale_x: 2.0,
            scale_y: 0.5,
            pixel_aspect: 2.0,
            ..EventTransform::default()
        },
        0.0,
        0.0,
        1.0,
    );
    let fay = ProjectiveMatrix::from_ass_transform_at_origin(
        EventTransform {
            shear_y: 0.4,
            scale_x: 2.0,
            scale_y: 0.5,
            pixel_aspect: 2.0,
            ..EventTransform::default()
        },
        0.0,
        0.0,
        1.0,
    );

    let (fax_x, fax_y) = fax.transform_point(0.0, 10.0);
    let (fay_x, fay_y) = fay.transform_point(100.0, 0.0);
    assert!((fax_x - 32.0).abs() < 1.0e-6 && (fax_y - 10.0).abs() < 1.0e-6);
    assert!((fay_x - 100.0).abs() < 1.0e-6 && (fay_y - 5.0).abs() < 1.0e-6);
}

#[test]
fn projective_transform_composes_shear_rotation_and_offcenter_org_like_libass() {
    let matrix = ProjectiveMatrix::from_ass_transform_at_origin_with_shear_base(
        EventTransform {
            rotation_x: 30.0,
            rotation_y: -20.0,
            rotation_z: 15.0,
            shear_x: 0.2,
            scale_x: 1.75,
            scale_y: 0.6,
            ..EventTransform::default()
        },
        120.0,
        160.0,
        80.0,
        80.0,
        1.0,
    );
    let corners = [
        ((80.0, 80.0), (81.975_914, 110.276_864)),
        ((255.0, 80.0), (236.595_498, 67.328_567)),
        ((255.0, 104.0), (257.791_992, 79.568_907)),
        ((80.0, 104.0), (93.873_252, 123.198_758)),
    ];

    for ((x, y), (expected_x, expected_y)) in corners {
        let (actual_x, actual_y) = matrix.transform_point(x, y);
        assert!((actual_x - expected_x).abs() < 1.0e-5);
        assert!((actual_y - expected_y).abs() < 1.0e-5);
    }
}

#[test]
fn high_resolution_projective_drawing_uses_storage_camera_distance() {
    let track = parse_script_text(include_str!(
        "../../rassa-test/fixtures/libass/compare/edge/vector_transform.ass"
    ))
    .expect("vector transform fixture should parse");
    let config = RendererConfig {
        frame: Size {
            width: 1920,
            height: 1080,
        },
        storage: Size {
            width: 1920,
            height: 1080,
        },
        ..RendererConfig::default()
    };
    let planes = RenderEngine::new().render_frame_with_provider_and_config(
        &track,
        &NullFontProvider,
        500,
        &config,
    );
    let projected = planes
        .into_iter()
        .filter(|plane| plane.destination.x > 900 && plane.destination.y > 400)
        .collect::<Vec<_>>();

    // Fresh libass 3087d2b renders visible ink at 1047,462..1315,486.
    // Rassa's rasterizer may move antialiased edges by a pixel, but the
    // projection geometry and camera distance must remain the same.
    assert_rect_near(
        visible_bounds(&projected),
        Rect {
            x_min: 1047,
            y_min: 462,
            x_max: 1315,
            y_max: 486,
        },
        2,
        "storage resolution, not PlayRes scaling, sets libass's projection camera distance",
    );
}

#[test]
fn anisotropic_shear_fixture_tracks_libass_geometry() {
    let track = parse_script_text(include_str!(
        "../../rassa-test/fixtures/libass/compare/edge/anisotropic_shear.ass"
    ))
    .expect("anisotropic shear fixture should parse");
    let engine = RenderEngine::new();
    let expected = [
        (
            500,
            Rect {
                x_min: 80,
                y_min: 79,
                x_max: 312,
                y_max: 101,
            },
        ),
        (
            1500,
            Rect {
                x_min: 80,
                y_min: 79,
                x_max: 280,
                y_max: 121,
            },
        ),
        (
            2500,
            Rect {
                x_min: 81,
                y_min: 63,
                x_max: 256,
                y_max: 124,
            },
        ),
    ];

    for (now_ms, libass_bounds) in expected {
        let planes = engine.render_frame_with_provider(&track, &NullFontProvider, now_ms);
        assert_rect_near(
            visible_bounds(&planes),
            libass_bounds,
            2,
            "anisotropic fax/fay plus combined 3D transform should follow libass geometry",
        );
    }
}

#[test]
fn projective_transform_uses_deep_org_as_perspective_lever_arm() {
    let transform = EventTransform {
        rotation_x: 55.0,
        ..EventTransform::default()
    };
    let shallow = ProjectiveMatrix::from_ass_transform_at_origin(transform, 320.0, 240.0, 1.0);
    let deep = ProjectiveMatrix::from_ass_transform_at_origin(transform, 320.0, 420.0, 1.0);

    let (_, shallow_y) = shallow.transform_point(320.0, 240.0);
    let (_, deep_y) = deep.transform_point(320.0, 240.0);

    assert!((shallow_y - 240.0).abs() < 0.5);
    assert!(
        deep_y > shallow_y + 70.0,
        "deep \\org below text should pull frx text substantially downward like libass, got shallow={shallow_y} deep={deep_y}"
    );
}

#[test]
fn prepare_frame_only_keeps_active_events() {
    let track = parse_script_text("[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,20,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,First\nDialogue: 0,0:00:02.00,0:00:03.00,Default,,0000,0000,0000,,Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let frame = engine.prepare_frame(&track, &provider, 500);

    assert_eq!(frame.active_events.len(), 1);
    assert_eq!(frame.active_events[0].text, "First");
}

#[test]
fn render_frame_produces_image_planes_for_active_text() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
    assert!(planes.iter().all(|plane| plane.size.width >= 0));
    assert!(planes.iter().all(|plane| plane.size.height >= 0));
}

#[test]
fn render_frame_supports_multiple_override_runs() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fnDejaVu Sans}Hi{\\fnArial} there").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
}

#[test]
fn render_frame_uses_axis_specific_shadow_offsets() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(30,30)\\xshad9\\yshad3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let character_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .cloned()
        .collect::<Vec<_>>();
    let shadow_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Shadow)
        .cloned()
        .collect::<Vec<_>>();

    let character = visible_bounds(&character_planes).expect("character bounds");
    let shadow = visible_bounds(&shadow_planes).expect("axis-specific shadow should render");
    assert_eq!(shadow.x_min - character.x_min, 9);
    assert_eq!(shadow.y_min - character.y_min, 3);
}

#[test]
fn render_frame_renders_underline_and_strikeout_decorations() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(30,30)\\u1\\s1}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    // libass draws decorations into the glyph bitmap itself (ass_font.c), so
    // they may share a plane with the text.  Assert coverage instead: the
    // underline and strikeout bars span (nearly) the full advance width,
    // producing full-width rows both through the glyph band and below it.
    let character_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .cloned()
        .collect::<Vec<_>>();
    let ink = visible_bounds(&character_planes).expect("text ink");
    let full_width_rows = (ink.y_min..ink.y_max)
        .filter(|&row| {
            let mut covered = 0;
            for plane in &character_planes {
                let local_y = row - plane.destination.y;
                if local_y < 0 || local_y >= plane.size.height {
                    continue;
                }
                let stride = plane.stride as usize;
                let row_pixels = &plane.bitmap[local_y as usize * stride
                    ..local_y as usize * stride + plane.size.width as usize];
                covered += row_pixels.iter().filter(|value| **value > 0).count() as i32;
            }
            covered >= ink.width() * 9 / 10
        })
        .collect::<Vec<_>>();

    assert!(
        full_width_rows.len() >= 2,
        "\\u1\\s1 should draw full-advance underline and strikeout bars; full rows: {full_width_rows:?} ink: {ink:?}"
    );
    let span = full_width_rows.last().unwrap() - full_width_rows.first().unwrap();
    assert!(
        span >= ink.height() / 3,
        "underline and strikeout bars should sit in different vertical bands; rows: {full_width_rows:?}"
    );
}

#[test]
fn render_frame_uses_override_colors_and_shadow_planes() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\1c&H112233&\\4c&H445566&\\shad3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x6655_4400)
    );
}

#[test]
fn render_frame_orders_events_by_layer_then_read_order() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 5,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\1c&H0000FF&}High\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,40)\\1c&H00FF00&}Low").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let first_character = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("character plane");
    assert_eq!(first_character.color.0, 0x00FF_0000);
}

#[test]
fn render_frame_orders_shadow_outline_before_character_within_event() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00111111,&H0000FFFF,&H00222222,&H00333333,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let kinds = planes.iter().map(|plane| plane.kind).collect::<Vec<_>>();

    let first_shadow = kinds
        .iter()
        .position(|kind| *kind == ass::ImageType::Shadow)
        .expect("shadow plane");
    let first_outline = kinds
        .iter()
        .position(|kind| *kind == ass::ImageType::Outline)
        .expect("outline plane");
    let first_character = kinds
        .iter()
        .position(|kind| *kind == ass::ImageType::Character)
        .expect("character plane");

    assert!(first_shadow < first_outline);
    assert!(first_outline < first_character);
}

#[test]
fn render_frame_emits_outline_planes_for_border_override() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord3\\3c&H0A0B0C&}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0C0B_0A00)
    );
}

#[test]
fn render_frame_applies_anisotropic_borders() {
    // libass strokes borders with independent x/y radii: \xbord4\ybord0
    // grows ink horizontally only (ass_outline stroker / get_outline_glyph).
    let script = |bord: &str| {
        format!(
            "[Script Info]\nPlayResX: 320\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00010203,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(40,40){bord}}}Hi"
        )
    };
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let bounds = |script_text: &str, kind: ass::ImageType| {
        let track = parse_script_text(script_text).expect("bord script parses");
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        planes
            .iter()
            .filter(|plane| plane.kind == kind)
            .filter_map(plane_ink_bounds)
            .reduce(|a, b| Rect {
                x_min: a.x_min.min(b.x_min),
                y_min: a.y_min.min(b.y_min),
                x_max: a.x_max.max(b.x_max),
                y_max: a.y_max.max(b.y_max),
            })
    };

    let fill = bounds(&script("\\xbord4\\ybord0"), ass::ImageType::Character).expect("fill ink");
    let outline =
        bounds(&script("\\xbord4\\ybord0"), ass::ImageType::Outline).expect("outline ink");
    assert!(
        (outline.width() - (fill.width() + 8)).abs() <= 1,
        "\\xbord4 grows outline ink 4px per horizontal side: fill={fill:?} outline={outline:?}"
    );
    assert!(
        (outline.height() - fill.height()).abs() <= 1,
        "\\ybord0 must not grow outline ink vertically: fill={fill:?} outline={outline:?}"
    );

    let outline_v =
        bounds(&script("\\xbord0\\ybord4"), ass::ImageType::Outline).expect("vertical outline ink");
    let fill_v =
        bounds(&script("\\xbord0\\ybord4"), ass::ImageType::Character).expect("vertical fill ink");
    assert!(
        (outline_v.height() - (fill_v.height() + 8)).abs() <= 1,
        "\\ybord4 grows outline ink 4px per vertical side: fill={fill_v:?} outline={outline_v:?}"
    );
    assert!(
        (outline_v.width() - fill_v.width()).abs() <= 1,
        "\\xbord0 must not grow outline ink horizontally: fill={fill_v:?} outline={outline_v:?}"
    );
}

#[test]
fn render_frame_distinguishes_be_from_blur() {
    // libass \be N applies N passes of a light [1,2,1] box blur (variance
    // N/2) while \blur is a gaussian of sigma = blur; both combine by
    // variance addition.  The gaussian implementation quantizes small radii
    // into shared kernel buckets, so plane padding and ink extent can tie;
    // the alpha mass that bleeds a fixed distance OUTSIDE the sharp ink is
    // monotone in sigma and discriminates the kernels reliably.
    let script = |blur_tag: &str| {
        format!(
            "[Script Info]\nPlayResX: 320\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(60,40){blur_tag}}}Hi"
        )
    };
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let render = |script_text: &str| {
        let track = parse_script_text(script_text).expect("blur script parses");
        engine.render_frame_with_provider(&track, &provider, 500)
    };
    let sharp_ink = visible_bounds(&render(&script(""))).expect("sharp ink");
    // Sum the alpha one pixel left of the sharp ink. libass reserves exactly
    // one pixel for \be1, while the gaussian reaches farther at \blur1.
    let left_bleed = |script_text: &str| {
        let planes = render(script_text);
        let probe_x = sharp_ink.x_min - 1;
        planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| {
                let local_x = probe_x - plane.destination.x;
                if local_x < 0 || local_x >= plane.size.width {
                    return 0_u32;
                }
                (0..plane.size.height as usize)
                    .map(|row| {
                        u32::from(plane.bitmap[row * plane.stride as usize + local_x as usize])
                    })
                    .sum()
            })
            .sum::<u32>()
    };

    let sharp = left_bleed(&script(""));
    let be1 = left_bleed(&script("\\be1"));
    let blur1 = left_bleed(&script("\\blur1"));
    let both = left_bleed(&script("\\be4\\blur1"));
    assert!(
        be1 > sharp,
        "\\be1 must bleed alpha past the sharp edge: sharp={sharp} be1={be1}"
    );
    assert!(
        blur1 > be1,
        "\\blur1 (sigma 1) spreads more than \\be1 (sigma ~0.7): be1={be1} blur1={blur1}"
    );
    assert!(
        both > blur1,
        "\\be4\\blur1 (sigma ~1.7) spreads more than \\blur1: blur1={blur1} both={both}"
    );
}

#[test]
fn render_frame_emits_background_box_for_border_style_4() {
    // libass add_background (ass_render.c): BorderStyle 4 draws one solid
    // box in the back colour behind the whole event, expanded by positive
    // shadow offsets, and suppresses the shadow bitmaps themselves.
    let track = parse_script_text("[Script Info]\nPlayResX: 500\nPlayResY: 160\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,DejaVu Sans,30,&H00FFFFFF,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,4,0,4,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Box,,0000,0000,0000,,{\\an5\\pos(250,80)}Background").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let background = planes.first().expect("BorderStyle 4 event renders planes");
    assert_eq!(
        background.color.0, 0x1111_1100,
        "the background box is drawn first in the back colour"
    );
    let text_ink = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .filter_map(plane_ink_bounds)
        .reduce(|a, b| Rect {
            x_min: a.x_min.min(b.x_min),
            y_min: a.y_min.min(b.y_min),
            x_max: a.x_max.max(b.x_max),
            y_max: a.y_max.max(b.y_max),
        })
        .expect("text ink");
    let bg = plane_rect(background);
    assert!(
        bg.x_min <= text_ink.x_min
            && bg.y_min <= text_ink.y_min
            && bg.x_max >= text_ink.x_max
            && bg.y_max >= text_ink.y_max,
        "the background box covers the text: bg={bg:?} ink={text_ink:?}"
    );
    // The line box is 30 tall; \shad4 expands the box but produces no
    // offset shadow copy of the glyphs.
    assert!(
        bg.height() >= 30 + 8,
        "the box is expanded by the shadow size: {bg:?}"
    );
    assert_eq!(
        planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Shadow)
            .count(),
        1,
        "no glyph shadow bitmaps besides the background box"
    );
}

#[test]
fn render_frame_emits_opaque_box_for_border_style_3() {
    let track = parse_script_text("[Script Info]\nPlayResX: 500\nPlayResY: 160\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Box,DejaVu Sans,30,&H00000000,&H0000FFFF,&H00000000,&H00111111,0,0,0,0,100,100,0,0,3,2,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Box,,0000,0000,0000,,{\\an5\\pos(250,80)}BorderStyle=3 opaque box").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let character_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .cloned()
        .collect::<Vec<_>>();
    let outline_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
        .cloned()
        .collect::<Vec<_>>();

    assert_eq!(
        outline_planes.len(),
        1,
        "BorderStyle=3 should emit only the opaque box outline plane, not a separate stroked glyph outline"
    );
    let _character = visible_bounds(&character_planes).expect("character bounds");
    let outline = outline_planes
        .iter()
        .find(|plane| plane.color.0 == 0x0000_0000 && plane.bitmap.contains(&255))
        .expect("opaque border-style box plane uses outline colour");
    assert!(outline.size.width > 0);
    assert!(outline.size.height > 0);
    let bounds = visible_bounds(std::slice::from_ref(outline)).expect("opaque box bounds");
    let center_x = (bounds.x_min + bounds.x_max) / 2;
    assert!(
        (center_x - 250).abs() <= 2,
        "opaque box should stay centered at \\pos, got {bounds:?}"
    );
    let center_y = (bounds.y_min + bounds.y_max) / 2;
    assert!(
        (center_y - 80).abs() <= 1,
        "opaque box should stay vertically centered at \\pos like libass, got {bounds:?}"
    );
    // libass get_outline_glyph OUTLINE_BOX: the box spans -asc-bord_y ..
    // desc+bord_y, i.e. font size (asc+desc under REAL_DIM sizing) plus one
    // border on each side: 30 + 2*2 = 34.
    assert_eq!(
        bounds.height(),
        34,
        "BorderStyle=3 box plane height should be font size plus two borders like libass"
    );
    assert!(
        bounds.width() < 370,
        "opaque box should use actual raster advance like libass, not inflated layout width: {bounds:?}"
    );
}

#[test]
fn render_frame_blurs_outline_and_shadow_layers() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00010203,&H00111111,0,0,0,0,100,100,0,0,1,2,2,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\bord2\\blur2\\3c&H0A0B0C&\\shad2}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline
                && plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Shadow
                && plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
}

#[test]
fn render_frame_blurs_fill_only_without_outline_or_shadow() {
    let base = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)}Hi").expect("script should parse");
    let blurred = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\blur3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let base_planes = engine.render_frame_with_provider(&base, &provider, 500);
    let blurred_planes = engine.render_frame_with_provider(&blurred, &provider, 500);
    let base_character = visible_bounds(
        &base_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .cloned()
            .collect::<Vec<_>>(),
    )
    .expect("base character bounds");
    let blurred_character = visible_bounds(
        &blurred_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .cloned()
            .collect::<Vec<_>>(),
    )
    .expect("blurred character bounds");

    assert!(blurred_character.x_min < base_character.x_min);
    assert!(blurred_character.x_max > base_character.x_max);
    assert!(blurred_character.y_min < base_character.y_min);
    assert!(blurred_character.y_max > base_character.y_max);
}

#[test]
fn render_frame_does_not_blur_fill_when_outline_or_shadow_exists() {
    let base = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)}Hi").expect("script should parse");
    let blurred = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\blur3}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let base_planes = engine.render_frame_with_provider(&base, &provider, 500);
    let blurred_planes = engine.render_frame_with_provider(&blurred, &provider, 500);
    let character_bounds = |planes: &[ImagePlane]| {
        visible_bounds(
            &planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character)
                .cloned()
                .collect::<Vec<_>>(),
        )
        .expect("character bounds")
    };

    assert_eq!(
        character_bounds(&blurred_planes),
        character_bounds(&base_planes)
    );
    assert!(
        blurred_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Outline)
            .any(|plane| plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
    assert!(
        blurred_planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Shadow)
            .any(|plane| plane.bitmap.iter().any(|value| *value > 0 && *value < 255))
    );
}

#[test]
fn render_frame_applies_rectangular_clip() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(0,0,64,64)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
    assert!(planes.iter().all(|plane| plane.destination.x >= 0));
    assert!(planes.iter().all(|plane| plane.destination.y >= 0));
    assert!(
        planes
            .iter()
            .all(|plane| plane.destination.x + plane.size.width <= 64)
    );
    assert!(
        planes
            .iter()
            .all(|plane| plane.destination.y + plane.size.height <= 64)
    );
}

#[test]
fn render_frame_accepts_renderer_shaping_mode() {
    let track = parse_script_text("[Script Info]\nPlayResX: 320\nPlayResY: 180\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,48,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,20,20,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,office").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let simple = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            shaping: ass::ShapingLevel::Simple,
            ..default_renderer_config(&track)
        },
    );
    let complex = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            shaping: ass::ShapingLevel::Complex,
            ..default_renderer_config(&track)
        },
    );

    assert!(!simple.is_empty());
    assert!(!complex.is_empty());
}

#[test]
fn render_frame_interpolates_animated_clip_rect() {
    // libass interpolates rectangular \clip coordinates inside \t
    // (ass_parse.c): the visible ink window grows monotonically between the
    // transform's start and end times.
    let track = parse_script_text("[Script Info]\nPlayResX: 320\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(20,30)\\clip(20,0,40,120)\\t(0,1000,\\clip(20,0,300,120))}Clipping").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let width_at = |now_ms: i64| {
        let planes = engine.render_frame_with_provider(&track, &provider, now_ms);
        visible_bounds(&planes)
            .map(|rect| rect.width())
            .unwrap_or(0)
    };

    let start = width_at(10);
    let middle = width_at(200);
    let near_end = width_at(400);
    assert!(
        start < middle && middle < near_end,
        "animated \\t(\\clip) must widen the visible window over time: {start} < {middle} < {near_end}"
    );
}

#[test]
fn render_frame_interpolates_animated_inverse_clip_rect() {
    // libass parses \t(\iclip(...)) through the same animated rectangular
    // clip path as \clip, but switches clip mode to inverse for the event.
    let track = parse_script_text("[Script Info]\nPlayResX: 320\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(20,30)\\t(0,1000,\\iclip(0,0,60,120))}AnimatedInverseClip").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let width_at = |now_ms: i64| {
        let planes = engine.render_frame_with_provider(&track, &provider, now_ms);
        visible_bounds(&planes)
            .map(|rect| rect.width())
            .unwrap_or(0)
    };

    let start = width_at(10);
    let middle = width_at(500);
    let end = width_at(999);
    assert!(
        start < middle && middle <= end,
        "animated \\t(\\iclip) must reveal the inverse region as the excluded rectangle shrinks: {start} < {middle} <= {end}"
    );
}

#[test]
fn render_frame_applies_transform_vector_clip_like_static_vector_clip() {
    let script = |tag: &str| {
        format!(
            "[Script Info]\nPlayResX: 320\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(20,30){tag}}}VectorClipParity"
        )
    };
    let vector = "m 20 0 l 160 0 160 120 20 120";
    let bounds_for = |script_text: String| {
        render_text_bounds(&script_text).expect("vector-clipped text should render")
    };

    assert_eq!(
        bounds_for(script(format!("\\clip({vector})").as_str())),
        bounds_for(script(format!("\\t(0,1000,\\clip({vector}))").as_str())),
        "libass applies vector \\clip inside \\t as a clip side effect"
    );
    assert_eq!(
        bounds_for(script(format!("\\iclip({vector})").as_str())),
        bounds_for(script(format!("\\t(0,1000,\\iclip({vector}))").as_str())),
        "libass applies vector \\iclip inside \\t as an inverse clip side effect"
    );
}

#[test]
fn render_frame_applies_inverse_rectangular_clip() {
    let plane = ImagePlane {
        size: Size {
            width: 6,
            height: 4,
        },
        stride: 6,
        color: RgbaColor(0x00FF_FFFF),
        destination: Point { x: 0, y: 0 },
        kind: ass::ImageType::Character,
        bitmap: vec![255; 24],
    };
    let parts = inverse_clip_plane(
        plane,
        Rect {
            x_min: 2,
            y_min: 1,
            x_max: 4,
            y_max: 3,
        },
    );

    assert_eq!(parts.len(), 4);
    assert_eq!(
        parts.iter().map(|plane| plane.bitmap.len()).sum::<usize>(),
        20
    );
}

fn solid_test_plane(width: i32, height: i32, destination: Point) -> ImagePlane {
    ImagePlane {
        size: Size { width, height },
        stride: width,
        color: RgbaColor(0x00FF_FFFF),
        destination,
        kind: ass::ImageType::Character,
        bitmap: vec![255; (width * height) as usize],
    }
}

fn assert_valid_frame_planes(planes: &[ImagePlane], frame_width: i32, frame_height: i32) {
    for plane in planes {
        assert!(plane.size.width >= 0 && plane.size.height >= 0);
        assert!(plane.stride >= plane.size.width && plane.stride >= 0);
        let stride = usize::try_from(plane.stride).expect("nonnegative stride");
        let height = usize::try_from(plane.size.height).expect("nonnegative height");
        let required = stride.checked_mul(height).expect("plane size fits usize");
        assert!(
            required <= plane.bitmap.len(),
            "plane bitmap is shorter than stride * height: {plane:?}"
        );
        let right = i64::from(plane.destination.x) + i64::from(plane.size.width);
        let bottom = i64::from(plane.destination.y) + i64::from(plane.size.height);
        assert!(plane.destination.x >= 0 && plane.destination.y >= 0);
        assert!(right <= i64::from(frame_width) && bottom <= i64::from(frame_height));
    }
}

fn rectangular_vector_clip(x_min: i32, y_min: i32, x_max: i32, y_max: i32) -> ParsedVectorClip {
    ParsedVectorClip {
        scale: 1,
        polygons: vec![vec![
            Point { x: x_min, y: y_min },
            Point { x: x_max, y: y_min },
            Point { x: x_max, y: y_max },
            Point { x: x_min, y: y_max },
        ]],
    }
}

#[test]
fn regular_vector_clip_keeps_empty_overlap_plane_like_libass() {
    let plane = solid_test_plane(6, 4, Point { x: 0, y: 0 });
    let clipped =
        mask_plane_with_vector_clip(plane, &rectangular_vector_clip(20, 20, 30, 30), false)
            .expect("valid vector clip with no overlap keeps an empty ASS_Image node");

    assert_eq!(clipped.size, Size::default());
    assert_eq!(clipped.stride, 0);
    assert_eq!(clipped.destination, Point { x: 0, y: 0 });
    assert!(clipped.bitmap.is_empty());
}

#[test]
fn regular_vector_clip_crops_to_mask_overlap_not_visible_pixels() {
    let mut plane = solid_test_plane(8, 6, Point { x: 0, y: 0 });
    plane.bitmap.fill(0);
    plane.bitmap[0] = 255;

    let clipped = mask_plane_with_vector_clip(plane, &rectangular_vector_clip(2, 1, 6, 5), false)
        .expect("regular vector clip should keep the mask overlap geometry");

    assert_eq!(clipped.destination, Point { x: 2, y: 1 });
    assert_eq!(
        clipped.size,
        Size {
            width: 4,
            height: 4,
        }
    );
    assert_eq!(clipped.stride, 4);
    assert!(clipped.bitmap.iter().all(|value| *value == 0));
}

#[test]
fn inverse_vector_clip_keeps_original_plane_when_mask_covers_everything() {
    let plane = solid_test_plane(6, 4, Point { x: 10, y: 20 });
    let clipped = mask_plane_with_vector_clip(
        plane.clone(),
        &rectangular_vector_clip(10, 20, 16, 24),
        true,
    )
    .expect("inverse vector clip keeps the ASS_Image node even when all alpha is masked");

    assert_eq!(clipped.size, plane.size);
    assert_eq!(clipped.stride, plane.stride);
    assert_eq!(clipped.destination, plane.destination);
    assert!(clipped.bitmap.iter().all(|value| *value == 0));
}

#[test]
fn inverse_vector_clip_keeps_original_plane_when_mask_does_not_overlap() {
    let plane = solid_test_plane(6, 4, Point { x: 10, y: 20 });
    let clipped = mask_plane_with_vector_clip(
        plane.clone(),
        &rectangular_vector_clip(40, 40, 50, 50),
        true,
    )
    .expect("inverse vector clip without overlap leaves the ASS_Image unchanged");

    assert_eq!(clipped, plane);
}

#[test]
fn invalid_programmatic_vector_clip_is_unapplied_for_regular_and_inverse_modes() {
    let plane = solid_test_plane(6, 4, Point { x: 10, y: 20 });
    let invalid = ParsedVectorClip {
        scale: 1,
        polygons: vec![vec![
            Point { x: i32::MIN, y: 0 },
            Point { x: 20, y: 0 },
            Point { x: 20, y: 20 },
        ]],
    };

    for inverse in [false, true] {
        assert_eq!(
            mask_plane_with_vector_clip(plane.clone(), &invalid, inverse),
            Some(plane.clone()),
            "libass skips an invalid vector outline instead of applying {inverse:?} clipping"
        );
    }
}

#[test]
fn render_frame_applies_vector_clip() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)\\clip(m 0 0 l 32 0 32 32 0 32)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(!planes.is_empty());
    assert!(
        planes
            .iter()
            .all(|plane| plane.bitmap.iter().any(|value| *value > 0))
    );
    assert!(planes.iter().all(|plane| plane.destination.x >= 0));
    assert!(planes.iter().all(|plane| plane.destination.y >= 0));
}

#[test]
fn master_outline_extremes_do_not_clip_regular_or_inverse_text() {
    let render = |clip_tag: &str| {
        let script = format!(
            "[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(20,20){clip_tag}}}OutlineRange"
        );
        let track = parse_script_text(&script).expect("adversarial clip script parses");
        RenderEngine::new().render_frame_with_provider(&track, &FontconfigProvider::new(), 500)
    };

    let baseline = render("");
    assert!(!baseline.is_empty());
    let baseline_bounds = visible_bounds(&baseline);
    for tag in [
        "\\clip(m 0 0 l -33554432 0 20 20 0 20)",
        "\\iclip(m 0 0 l -33554432 0 20 20 0 20)",
        "\\clip(m 0 0 s -2147483648 0 10 10 20 20 p 30 30 c)",
        "\\iclip(m 0 0 s -2147483648 0 10 10 20 20 p 30 30 c)",
    ] {
        let planes = render(tag);
        assert_eq!(
            visible_bounds(&planes),
            baseline_bounds,
            "invalid vector outline must be left unapplied for {tag:?}"
        );
        assert_valid_frame_planes(&planes, 200, 100);
    }
}

#[test]
fn hostile_vector_drawings_are_rejected_before_bitmap_allocation() {
    let render = |drawing_tags: &str, drawing: &str| {
        let script = format!(
            "[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(10,10)\\p1{drawing_tags}}}{drawing}"
        );
        let track = parse_script_text(&script).expect("adversarial drawing script parses");
        RenderEngine::new().render_frame_with_provider(&track, &FontconfigProvider::new(), 500)
    };

    let cases = [
        ("", "m 0 0 l -33554432 0 0 10"),
        ("", "m 0 0 s -2147483648 0 10 10 20 20 p 30 30 c"),
        (
            "\\fscx1000000000000\\fscy1000000000000",
            "m 0 0 l 10 0 10 10 0 10",
        ),
        // Coordinates are individually valid, but the eager 12k-square bitmap
        // would exceed libass's 128 MiB default bitmap-cache budget.
        ("", "m 0 0 l 12000 0 12000 12000 0 12000"),
    ];
    for (tags, drawing) in cases {
        let planes = render(tags, drawing);
        assert!(
            planes.is_empty(),
            "hostile drawing must not produce a partial or gigantic plane: tags={tags:?} drawing={drawing:?}"
        );
        assert_valid_frame_planes(&planes, 200, 100);
    }
}

#[test]
fn render_frame_applies_vector_clip_scale_clamping_and_empty_masks_like_libass() {
    fn render_with_tag(tag: &str) -> Vec<ImagePlane> {
        let script = format!(
            "[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\pos(100,50){tag}}}Hi"
        );
        let track = parse_script_text(&script).expect("script should parse");
        let engine = RenderEngine::new();
        let provider = FontconfigProvider::new();
        engine.render_frame_with_provider(&track, &provider, 500)
    }

    assert!(!render_with_tag("").is_empty());
    assert!(render_with_tag("\\clip(not drawing)").is_empty());
    assert!(!render_with_tag("\\clip(0,m 0 0 l 200 0 200 100 0 100)").is_empty());
    assert!(!render_with_tag("\\iclip(not drawing)").is_empty());
    // A fully masked inverse clip still retains its zero-coverage ASS_Image;
    // 0.17.5 filters colour-alpha 0xFF nodes, not empty bitmap masks.
    assert!(!render_with_tag("\\iclip(0,m 0 0 l 200 0 200 100 0 100)").is_empty());
}

#[test]
fn render_frame_clips_to_frame_bounds() {
    let plane = ImagePlane {
        size: Size {
            width: 20,
            height: 20,
        },
        stride: 20,
        color: RgbaColor(0x00FF_FFFF),
        destination: Point { x: 50, y: 50 },
        kind: ass::ImageType::Character,
        bitmap: vec![255; 400],
    };
    let clipped = apply_event_clip(
        vec![plane],
        Rect {
            x_min: 0,
            y_min: 0,
            x_max: 60,
            y_max: 60,
        },
        false,
    );

    assert_eq!(clipped.len(), 1);
    assert_eq!(clipped[0].size.width, 10);
    assert_eq!(clipped[0].size.height, 10);
}

#[test]
fn render_frame_remaps_events_into_full_frame_when_margins_used() {
    // libass use_margins (ass_render.c x2scr_left/y2scr family): margins do
    // NOT hard-clip normal subtitles; instead the content frame is
    // aspect-fitted into the full frame, so a top-aligned subtitle anchors
    // at the very top of the frame (inside the top margin/black bar), while
    // a positioned event still maps onto the content area offset by the
    // margins.
    let script = |text: &str| {
        format!(
            "[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,8,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{text}"
        )
    };
    let margin_config = config(
        100,
        120,
        rassa_core::Margins {
            top: 10,
            bottom: 10,
            left: 0,
            right: 0,
        },
        true,
    );
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let normal_track = parse_script_text(&script("Hi")).expect("script should parse");
    let normal =
        engine.render_frame_with_provider_and_config(&normal_track, &provider, 500, &margin_config);
    let normal_top = visible_bounds(&normal).expect("normal subtitle ink").y_min;
    // Content is 100x100 fitted into the 100x120 frame: fit_h = 100 with no
    // vertical offset for toptitles, so the line box top sits at y = 0, well
    // inside the 10px top margin.
    assert!(
        normal_top < 10,
        "a top-aligned normal subtitle anchors inside the top margin under use_margins; got y_min={normal_top}"
    );

    let positioned_track =
        parse_script_text(&script("{\\an8\\pos(50,0)}Hi")).expect("script should parse");
    let positioned = engine.render_frame_with_provider_and_config(
        &positioned_track,
        &provider,
        500,
        &margin_config,
    );
    let positioned_top = visible_bounds(&positioned).expect("positioned ink").y_min;
    // \pos maps onto the content frame offset by the top margin, and
    // explicit events clip to the content area.
    assert!(
        positioned_top >= 10,
        "a positioned event maps into the margin-offset content area; got y_min={positioned_top}"
    );

    let bare_drawing_mode_track =
        parse_script_text(&script("{\\p0}Hi")).expect("script should parse");
    let bare_drawing_mode = engine.render_frame_with_provider_and_config(
        &bare_drawing_mode_track,
        &provider,
        500,
        &margin_config,
    );
    let bare_drawing_mode_top = visible_bounds(&bare_drawing_mode)
        .expect("bare \\p0 hard override ink")
        .y_min;
    assert!(
        bare_drawing_mode_top >= 10,
        "a bare \\p0 tag is a libass hard override and maps into the content area; got y_min={bare_drawing_mode_top}"
    );

    let hard_clip_track =
        parse_script_text(&script("{\\clip(0,0,0,0)}Hi")).expect("script should parse");
    let hard_clip = engine.render_frame_with_provider_and_config(
        &hard_clip_track,
        &provider,
        500,
        &margin_config,
    );
    assert!(
        visible_bounds(&hard_clip).is_none(),
        "a normal \\clip tag is a libass hard override and should apply its zero-size clip"
    );

    let spaced_clip_track =
        parse_script_text(&script("{\\ clip(0,0,0,0)}Hi")).expect("script should parse");
    let spaced_clip = engine.render_frame_with_provider_and_config(
        &spaced_clip_track,
        &provider,
        500,
        &margin_config,
    );
    assert!(
        visible_bounds(&spaced_clip).is_some(),
        "libass parses spaced \\ clip but its hard scan does not mark the event explicit under use_margins"
    );
}

#[test]
fn render_frame_maps_into_content_area_when_margins_are_not_used() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &config(
            120,
            120,
            rassa_core::Margins {
                top: 10,
                bottom: 10,
                left: 10,
                right: 10,
            },
            false,
        ),
    );

    assert!(!planes.is_empty());
    let bounds = visible_bounds(&planes).expect("visible bounds");
    assert!(
        bounds.x_min >= 10,
        "visible bounds should start inside content area: {bounds:?}"
    );
    assert!(
        bounds.y_min >= 9,
        "libass-style antialiasing may allocate one guard row above the content area: {bounds:?}"
    );
    assert!(
        bounds.x_max <= 110,
        "visible bounds should end inside content area: {bounds:?}"
    );
    assert!(
        bounds.y_max <= 110,
        "visible bounds should end inside content area: {bounds:?}"
    );
}

#[test]
fn render_frame_keeps_border_closer_to_device_size_when_scaled_border_is_disabled() {
    let enabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
    let disabled = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\nScaledBorderAndShadow: no\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,4,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}I").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let config = RendererConfig {
        storage: Size {
            width: 200,
            height: 200,
        },
        ..config(200, 200, rassa_core::Margins::default(), true)
    };
    let enabled_planes =
        engine.render_frame_with_provider_and_config(&enabled, &provider, 500, &config);
    let disabled_planes =
        engine.render_frame_with_provider_and_config(&disabled, &provider, 500, &config);
    let enabled_outline_area: i32 = enabled_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
        .map(|plane| plane.size.width * plane.size.height)
        .sum();
    let disabled_outline_area: i32 = disabled_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Outline)
        .map(|plane| plane.size.width * plane.size.height)
        .sum();

    assert!(disabled_outline_area > 0);
    assert!(disabled_outline_area < enabled_outline_area);
}

#[test]
fn render_frame_applies_font_scale_to_output() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let scaled = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 120,
            },
            font_scale: 2.0,
            ..RendererConfig::default()
        },
    );

    assert!(!baseline.is_empty());
    assert!(!scaled.is_empty());
    assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
}

#[test]
fn render_frame_zero_font_scale_collapses_output_like_libass() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let zero_scaled = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 120,
            },
            font_scale: 0.0,
            ..RendererConfig::default()
        },
    );

    assert!(zero_scaled.is_empty());
}

#[test]
fn render_frame_applies_text_scale_overrides() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}Scale").expect("script should parse");
    let stretched = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50}Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let scaled = engine.render_frame_with_provider(&stretched, &provider, 500);
    let baseline_width = baseline
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.destination.x + plane.size.width)
        .max()
        .expect("baseline max x")
        - baseline
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.x)
            .min()
            .expect("baseline min x");
    let scaled_width = scaled
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.destination.x + plane.size.width)
        .max()
        .expect("scaled max x")
        - scaled
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.x)
            .min()
            .expect("scaled min x");

    assert!(scaled_width > baseline_width);
    assert!(total_plane_area(&scaled) < total_plane_area(&baseline) * 2);
}

#[test]
fn render_frame_applies_drawing_scale_overrides() {
    let baseline = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 10 0 10 10 0 10").expect("script should parse");
    let scaled = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fscx200\\fscy50\\p1}m 0 0 l 10 0 10 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
    let scaled_planes = engine.render_frame_with_provider(&scaled, &provider, 500);
    let baseline_plane = baseline_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("baseline drawing plane");
    let scaled_plane = scaled_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("scaled drawing plane");

    assert!(scaled_plane.size.width > baseline_plane.size.width);
    assert!(scaled_plane.size.height < baseline_plane.size.height);
    assert_eq!(scaled_plane.destination, Point { x: 10, y: 10 });
}

#[test]
fn non_positioned_drawing_does_not_receive_positioned_overhang_compensation() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\p1}m 0 0 l 10 0 10 10 0 10{\\p0}").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let plane = engine
        .render_frame_with_provider(&track, &provider, 500)
        .into_iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");

    assert_eq!(
        plane.size.width, 11,
        "libass-style positioned overhang compensation is specific to explicit \\pos vector drawings"
    );
}

#[test]
fn render_frame_applies_drawing_baseline_offset() {
    // libass (ass_render.c get_bitmap_glyph + measure_text): a drawing
    // contributes asc = height - pbo and desc = pbo to its line, and its ink
    // bottom sits at baseline + pbo.  For this top-anchored (\an7\pos) mixed
    // text+drawing line the fs24 text ascent (~19, REAL_DIM win metrics)
    // dominates while pbo <= 10 never exceeds it, so positive \pbo moves the
    // drawing down by exactly pbo.  \pbo-12 makes the drawing ascent
    // (10 + 12 = 22) exceed the text ascent, lowering the baseline and
    // netting a shift of 10 - text_asc (about -9).
    fn pbo_track(pbo_tag: &str) -> ParsedTrack {
        parse_script_text(&format!("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(10,40)}}X{{{pbo_tag}\\p1\\1c&H44FF44&}}m 0 0 l 10 0 10 10 0 10{{\\p0\\1c&H332211&}}X"))
                .expect("script should parse")
    }

    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let drawing_plane = |track: &ParsedTrack| {
        engine
            .render_frame_with_provider(track, &provider, 500)
            .into_iter()
            .find(|plane| {
                plane.kind == ass::ImageType::Character
                    && plane.size.width == 11
                    && plane.size.height == 11
            })
            .expect("drawing plane")
    };
    let baseline_drawing = drawing_plane(&pbo_track(""));
    let pbo5_drawing = drawing_plane(&pbo_track("\\pbo5"));
    let shifted_drawing = drawing_plane(&pbo_track("\\pbo12"));
    let negative_drawing = drawing_plane(&pbo_track("\\pbo-12"));

    assert_eq!(pbo5_drawing.destination.x, baseline_drawing.destination.x);
    assert_eq!(
        pbo5_drawing.destination.y,
        baseline_drawing.destination.y + 5,
        "\\pbo5 must move the drawing down by 5 (libass: bottom = baseline + pbo)"
    );
    assert_eq!(
        shifted_drawing.destination.y,
        baseline_drawing.destination.y + 12,
        "\\pbo12 must move the drawing down by 12"
    );
    let negative_delta = negative_drawing.destination.y - baseline_drawing.destination.y;
    assert!(
        (-10..=-8).contains(&negative_delta),
        "\\pbo-12 raises the line ascent to 22, netting 10 - text_asc (about -9); got {negative_delta}"
    );
}

#[test]
fn render_frame_applies_banner_effect_motion() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Banner;25;0;0,Banner").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let early = character_bounds(&engine.render_frame_with_provider(&track, &provider, 100))
        .expect("early banner bounds");
    let late = character_bounds(&engine.render_frame_with_provider(&track, &provider, 1500))
        .expect("late banner bounds");

    assert!(
        late.x_min < early.x_min,
        "right-to-left banner should move left over time"
    );
    assert!(
        (194..=198).contains(&early.x_min),
        "libass positions a right-to-left banner by PlayResX - elapsed/delay, got {early:?}"
    );
}

#[test]
fn effect_values_use_c_atoi_whitespace_like_libass() {
    assert_eq!(
        effect_values("Banner; \t\n\r\u{000b}\u{000c}+25;\u{00a0}25; -3;abc;99"),
        vec![25, 0, -3, 0]
    );
}

#[test]
fn invalid_scroll_effect_does_not_disable_collision_like_libass() {
    let mut event = ParsedEvent {
        effect: "Scroll up;20;100".to_string(),
        ..ParsedEvent::default()
    };
    assert!(!transition_effect_disables_collision(&event));

    event.effect = "Scroll up;20;100;25".to_string();
    assert!(transition_effect_disables_collision(&event));

    event.effect = "Banner;".to_string();
    assert!(transition_effect_disables_collision(&event));
}

#[test]
fn banner_effect_delay_uses_layout_scale_not_render_supersampling() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Banner;25;0;0,Banner").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let bounds = character_bounds(&engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        1500,
        &RendererConfig {
            frame: Size {
                width: 1600,
                height: 800,
            },
            storage: Size {
                width: 200,
                height: 100,
            },
            ..RendererConfig::default()
        },
    ))
    .expect("supersampled banner bounds");

    assert!(
        bounds.x_min >= 1112,
        "Banner delay should be based on layout/storage resolution rather than render supersampling; got {bounds:?}"
    );
}

#[test]
fn render_frame_applies_scroll_effect_motion() {
    let up = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Scroll up;20;100;25;0,Scroll").expect("script should parse");
    let down = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,Scroll down;20;100;25;0,Scroll").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let up_early = character_bounds(&engine.render_frame_with_provider(&up, &provider, 100))
        .expect("early scroll-up bounds");
    let up_late = character_bounds(&engine.render_frame_with_provider(&up, &provider, 1500))
        .expect("late scroll-up bounds");
    // At 100ms a scroll-down box bottom sits at y0 + 4 with the glyph ink
    // still above the y0..y1 clip window (libass shows nothing yet), so
    // sample once the text has entered the window.
    let down_early = character_bounds(&engine.render_frame_with_provider(&down, &provider, 500))
        .expect("early scroll-down bounds");
    let down_late = character_bounds(&engine.render_frame_with_provider(&down, &provider, 1500))
        .expect("late scroll-down bounds");

    assert!(
        up_late.y_min < up_early.y_min,
        "scroll up should move upward"
    );
    assert!(
        down_late.y_min > down_early.y_min,
        "scroll down should move downward"
    );
}

#[test]
fn render_frame_applies_text_spacing_override() {
    let baseline = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)}IIII").expect("script should parse");
    let spaced = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\fsp8}IIII").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
    let spaced_planes = engine.render_frame_with_provider(&spaced, &provider, 500);
    let baseline_width = character_bounds(&baseline_planes)
        .expect("baseline bounds")
        .width();
    let spaced_width = character_bounds(&spaced_planes)
        .expect("spaced bounds")
        .width();

    assert!(spaced_width > baseline_width);
}

#[test]
fn render_frame_scales_output_to_frame_size() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Scale").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let scaled = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            ..default_renderer_config(&track)
        },
    );

    assert!(total_plane_area(&baseline) > 0);
    assert!(total_plane_area(&scaled) > total_plane_area(&baseline));
}

#[test]
fn render_frame_applies_pixel_aspect_horizontally() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}I").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 120,
            },
            ..default_renderer_config(&track)
        },
    );
    let widened = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 120,
            },
            pixel_aspect: 2.0,
            ..default_renderer_config(&track)
        },
    );

    let baseline_bounds = character_bounds(&baseline).expect("baseline character bounds");
    let widened_bounds = character_bounds(&widened).expect("widened character bounds");
    assert!(
        widened_bounds.x_min > baseline_bounds.x_min,
        "pixel aspect should affect horizontal placement: baseline={baseline_bounds:?} widened={widened_bounds:?}"
    );
}

#[test]
fn render_frame_derives_pixel_aspect_from_storage_size_when_unset() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Storage").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            ..default_renderer_config(&track)
        },
    );
    let storage_adjusted = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            storage: Size {
                width: 400,
                height: 120,
            },
            ..default_renderer_config(&track)
        },
    );

    assert!(total_plane_area(&baseline) > 0);
    assert!(total_plane_area(&storage_adjusted) < total_plane_area(&baseline));
}

#[test]
fn render_frame_layout_resolution_takes_precedence_over_storage_and_explicit_aspect() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\nLayoutResX: 400\nLayoutResY: 240\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,18,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(0,0)}Layout").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            ..default_renderer_config(&track)
        },
    );
    let overridden_inputs = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 400,
                height: 240,
            },
            storage: Size {
                width: 400,
                height: 120,
            },
            pixel_aspect: 2.0,
            ..default_renderer_config(&track)
        },
    );

    assert_eq!(
        character_bounds(&overridden_inputs),
        character_bounds(&baseline),
        "LayoutRes forces derived pixel aspect and overrides storage and explicit PAR"
    );
}

#[test]
fn render_frame_applies_line_position_to_subtitles() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,Shift").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let shifted = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 120,
            },
            line_position: 50.0,
            ..RendererConfig::default()
        },
    );

    let baseline_y = baseline
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("baseline plane");
    let shifted_y = shifted
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("shifted plane");

    assert!(shifted_y < baseline_y);
}

#[test]
fn render_frame_applies_line_spacing_to_multiline_subtitles() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,One\\NTwo").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let baseline = engine.render_frame_with_provider(&track, &provider, 500);
    let spaced = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 200,
                height: 140,
            },
            line_spacing: 20.0,
            ..RendererConfig::default()
        },
    );

    assert!(vertical_span(&spaced) > vertical_span(&baseline));
}

#[test]
fn render_frame_avoids_basic_bottom_collision_for_unpositioned_events() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,First\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let mut ys = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.destination.y)
        .collect::<Vec<_>>();
    ys.sort_unstable();
    ys.dedup();

    assert!(ys.len() >= 2);
    assert!(ys.last().expect("max y") - ys.first().expect("min y") >= 20);
}

#[test]
fn zero_advance_combining_mark_events_do_not_participate_in_collision_layout() {
    // libass-tests zero-area/zero-area.ass: a standalone combining mark has
    // visible ink but a zero-width advance bbox. VSFilter's intersection-area
    // rule cannot collide it, and libass therefore skips it in fix_collisions
    // regardless of whether it appears before or after a normal event.
    // Aileron contains U+0326 as a zero-advance combining mark, making this
    // regression self-contained with the repository's bundled font.
    let track = parse_script_text("[Script Info]\nScriptType: v4.00+\nPlayResX: 1280\nPlayResY: 720\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Aileron,96,&H000000FF,&H80FFFF00,&H00000000,&H0000FF00,0,0,0,0,100,100,0,0,1,0,0,5,0,10,20,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\\1c&H0000FF&}\u{326}\nDialogue: 0,0:00:01.00,0:00:05.00,Default,,0,0,0,,{\\1c&H00FF00&}A\nDialogue: 0,0:00:06.00,0:00:10.00,Default,,0,0,0,,{\\1c&H00FF00&}A\nDialogue: 0,0:00:06.00,0:00:10.00,Default,,0,0,0,,{\\1c&H0000FF&}\u{326}")
        .expect("zero-area collision fixture parses");
    let provider = BundledFontProvider::aileron_regular();
    let engine = RenderEngine::new();

    let prepared = engine.prepare_frame(&track, &provider, 2000);
    let mark = prepared
        .active_events
        .iter()
        .find(|event| event.event_index == 0)
        .expect("combining-mark event is active");
    let config = default_renderer_config(&track);
    assert_eq!(
        rendered_text_alignment_width(
            &mark.lines[0],
            track.events.first(),
            2000,
            &track,
            &config,
            RenderScale { x: 1.0, y: 1.0 },
            1.0,
        ),
        0,
        "a visible combining-only line retains its zero advance bbox: {mark:?}"
    );

    let early = engine.render_frame_with_provider(&track, &provider, 2000);
    let late = engine.render_frame_with_provider(&track, &provider, 8000);
    let colour_bounds = |planes: &[ImagePlane], colour| {
        let matching = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == colour)
            .cloned()
            .collect::<Vec<_>>();
        visible_bounds(&matching).expect("coloured event renders visible ink")
    };

    assert_eq!(
        colour_bounds(&early, 0xFF00_0000),
        colour_bounds(&late, 0xFF00_0000),
        "the combining mark stays on the same baseline in either read order"
    );
    assert_eq!(
        colour_bounds(&early, 0x00FF_0000),
        colour_bounds(&late, 0x00FF_0000),
        "the normal event is not displaced by a preceding zero-area event"
    );
}

#[test]
fn collision_positions_stay_stable_across_frames_like_libass() {
    // libass keeps a per-event render_priv rect: an event placed by
    // fix_collisions keeps its position in later frames while its height is
    // unchanged, even after other events end (ass_render.c get_render_priv).
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:00.50,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,{\\1c&H00FF00&}Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let second_y = |planes: &[ImagePlane]| {
        planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
            .map(|plane| plane.destination.y)
            .min()
            .expect("second event plane")
    };

    let both_active = engine.render_frame_with_provider(&track, &provider, 200);
    let first_gone = engine.render_frame_with_provider(&track, &provider, 1000);
    assert_eq!(
        second_y(&both_active),
        second_y(&first_gone),
        "an event keeps its collision-assigned position after the other event ends"
    );
}

#[test]
fn collision_cache_invalidates_after_in_place_track_mutation() {
    let mut track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 150\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,First\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0,0,0,,Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    let before = engine.render_frame_with_provider(&track, &provider, 500);
    track.events[0].text = "First\\Nline two\\Nline three".to_string();
    let after = engine.render_frame_with_provider(&track, &provider, 500);

    let visible_y = |planes: &[ImagePlane]| {
        planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character)
            .map(|plane| plane.destination.y)
            .min()
            .expect("visible character plane")
    };
    assert_ne!(
        visible_y(&before),
        visible_y(&after),
        "in-place track changes must invalidate cached collision rectangles"
    );
}

#[test]
fn collision_fix_groups_records_by_layer_like_libass() {
    let record = |event_index| RenderedEvent {
        event_index,
        planes: Vec::new(),
        collision_rect: Some(Rect {
            x_min: 0,
            y_min: 0,
            x_max: 100,
            y_max: 10,
        }),
        detect_collisions: true,
        shift_direction: 1,
        frame_clip: Rect {
            x_min: 0,
            y_min: 0,
            x_max: 100,
            y_max: 100,
        },
    };
    let same_layer = ParsedTrack {
        events: vec![
            ParsedEvent {
                layer: 0,
                ..Default::default()
            },
            ParsedEvent {
                layer: 0,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let mut same_layer_records = vec![record(0), record(1)];
    let mut same_layer_cache = std::collections::HashMap::new();
    fix_collisions_by_layer(&mut same_layer_cache, &mut same_layer_records, &same_layer);
    assert_eq!(same_layer_cache.get(&0).map(|rect| rect.y_min), Some(0));
    assert_eq!(same_layer_cache.get(&1).map(|rect| rect.y_min), Some(10));

    let different_layers = ParsedTrack {
        events: vec![
            ParsedEvent {
                layer: 0,
                ..Default::default()
            },
            ParsedEvent {
                layer: 1,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let mut different_layer_records = vec![record(0), record(1)];
    let mut different_layer_cache = std::collections::HashMap::new();
    fix_collisions_by_layer(
        &mut different_layer_cache,
        &mut different_layer_records,
        &different_layers,
    );
    assert_eq!(
        different_layer_cache.get(&0).map(|rect| rect.y_min),
        Some(0)
    );
    assert_eq!(
        different_layer_cache.get(&1).map(|rect| rect.y_min),
        Some(0)
    );
}

#[test]
fn render_frame_allows_collision_across_layers_like_libass() {
    // libass ass_render_frame sorts by layer, then calls fix_collisions for
    // each same-layer group. Different layers only affect z-order.
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 1,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H00FF00&}Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let layer0_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("layer 0 character plane");
    let layer1_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("layer 1 character plane");

    assert_eq!(
        layer0_y, layer1_y,
        "events on different layers should not collision-shift each other: layer0_y={layer0_y} layer1_y={layer1_y}"
    );
}

#[test]
fn banner_effect_does_not_participate_in_collision_layout_like_libass() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,Banner;25;0;0,{\\1c&H00FF00&}Second").expect("script should parse");
    let banner_only = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,Banner;25;0;0,{\\1c&H00FF00&}Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 100);
    let solo_planes = engine.render_frame_with_provider(&banner_only, &provider, 100);

    let banner_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("banner character plane");
    let solo_banner_y = solo_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("solo banner character plane");

    assert_eq!(banner_y, solo_banner_y);
}

#[test]
fn transform_tag_does_not_participate_in_collision_layout_like_libass() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\t\\1c&H00FF00&}Second").expect("script should parse");
    let transform_only = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\t\\1c&H00FF00&}Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 100);
    let solo_planes = engine.render_frame_with_provider(&transform_only, &provider, 100);

    let transform_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("transform-tag character plane");
    let solo_transform_y = solo_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("solo transform-tag character plane");

    assert_eq!(transform_y, solo_transform_y);
}

#[test]
fn origin_tag_does_not_participate_in_collision_layout_like_libass() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\1c&H0000FF&}First\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\org(120,0)\\1c&H00FF00&}Second").expect("script should parse");
    let origin_only = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,0,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{\\org(120,0)\\1c&H00FF00&}Second").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 100);
    let solo_planes = engine.render_frame_with_provider(&origin_only, &provider, 100);

    let origin_y = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("origin-tag character plane");
    let solo_origin_y = solo_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .map(|plane| plane.destination.y)
        .min()
        .expect("solo origin-tag character plane");

    assert_eq!(origin_y, solo_origin_y);
}

#[test]
fn render_frame_interpolates_move_position() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\move(0,0,100,0,0,1000)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

    let start_x = start_planes
        .iter()
        .map(|plane| plane.destination.x)
        .min()
        .expect("start plane");
    let mid_x = mid_planes
        .iter()
        .map(|plane| plane.destination.x)
        .min()
        .expect("mid plane");
    let end_x = end_planes
        .iter()
        .map(|plane| plane.destination.x)
        .min()
        .expect("end plane");

    assert!(start_x <= mid_x);
    assert!(mid_x <= end_x);
    assert!(end_x - start_x >= 80);
}

#[test]
fn center_positioned_move_without_geometric_transform_anchors_like_pos() {
    let script = |tag: &str| {
        format!(
            "[Script Info]\nPlayResX: 240\nPlayResY: 160\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,72,&H00FFFFFF,&H0000FFFF,&H00FFFFFF,&H00000000,0,0,0,0,100,100,0,0,1,3.5,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an5{tag}\\blur1.5}}S"
        )
    };
    let pos = parse_script_text(&script("\\pos(120,80)")).expect("pos script should parse");
    let movement = parse_script_text(&script("\\move(120,80,120,80)\\org(80,-20)"))
        .expect("move script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let pos_bounds = visible_bounds(&engine.render_frame_with_provider(&pos, &provider, 20))
        .expect("pos should render");
    let move_bounds = visible_bounds(&engine.render_frame_with_provider(&movement, &provider, 20))
        .expect("move should render");

    assert!(
        (move_bounds.y_min - pos_bounds.y_min).abs() <= 2,
        "libass treats a zero-distance \\move and unused \\org like an equivalent \\pos for \\an5 center anchoring; pos={pos_bounds:?} move={move_bounds:?}"
    );
}

#[test]
fn render_frame_applies_z_rotation_to_event_planes() {
    let baseline = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
    let rotated = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\frz90\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let baseline_planes = engine.render_frame_with_provider(&baseline, &provider, 500);
    let rotated_planes = engine.render_frame_with_provider(&rotated, &provider, 500);
    let baseline_bounds = character_bounds(&baseline_planes).expect("baseline bounds");
    let rotated_bounds = character_bounds(&rotated_planes).expect("rotated bounds");

    assert!(baseline_bounds.width() > baseline_bounds.height());
    assert!(rotated_bounds.height() > rotated_bounds.width());
}

#[test]
fn positioned_drawing_uses_position_y_before_compare_supersample_offset() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(20,24)\\p1}m 0 0 l 42 0 42 12 0 12{\\p0}").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider_and_config(
        &track,
        &provider,
        500,
        &RendererConfig {
            frame: Size {
                width: 1760,
                height: 1120,
            },
            storage: Size {
                width: 220,
                height: 140,
            },
            ..RendererConfig::default()
        },
    );
    let bounds = character_bounds(&planes).expect("positioned drawing bounds");
    let visible = visible_bounds(&planes).expect("positioned drawing visible bounds");

    assert_eq!(
        bounds.y_min,
        24 * 8,
        "libass keeps top-aligned positioned vector drawings anchored at \\pos y before final supersample offset; got {bounds:?}"
    );
    // libass's outline rasterizer bleeds one subpixel-thin antialias sample
    // past each geometric drawing edge (probed ink 159..497 for geometry
    // 160..496); rassa's rasterizer keeps sharp edges, so allow that margin.
    assert!(
        (bounds.x_min - 19 * 8).abs() <= 8,
        "positioned vector drawing plane should start near the libass anchor; got {bounds:?}"
    );
    assert!(
        (visible.x_min - 20 * 8).abs() <= 1,
        "positioned vector drawing ink must start at the \\pos anchor; got visible {visible:?}"
    );
    assert!(
        (visible.x_max - 62 * 8).abs() <= 1,
        "positioned vector drawing ink must end at the scaled drawing width; got visible {visible:?}"
    );
}

#[test]
fn render_frame_shears_positioned_drawing_from_run_baseline_not_org() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 140\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,28,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(120,24)\\org(120,80)\\frx45\\fax0.25\\p1}m 0 0 l 50 0 50 14 0 14{\\p0}")
            .expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let bounds = planes_bounds(&planes).expect("drawing plane should render");

    assert!(
        bounds.x_min >= 116,
        "libass applies \\fax in drawing-local baseline space before \\org perspective; global \\org shear pulls this too far left: {bounds:?}"
    );
}

#[test]
fn render_frame_applies_z_rotation_per_override_run() {
    let track = parse_script_text("[Script Info]\nPlayResX: 220\nPlayResY: 300\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,32,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,220)\\c&H0000FF&}MMMM{\\frz90\\c&H00FF00&}MMMM").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let red_planes = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
        .collect::<Vec<_>>();
    let green = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x00FF_0000)
        .expect("rotated green drawing plane");

    assert!(!red_planes.is_empty(), "expected unrotated red glyph plane");
    let red_y_min = red_planes
        .iter()
        .map(|plane| plane.destination.y)
        .min()
        .expect("red y min");
    let red_y_max = red_planes
        .iter()
        .map(|plane| plane.destination.y)
        .max()
        .expect("red y max");
    assert!(
        red_y_max - red_y_min <= 1,
        "unrotated run should stay on a horizontal baseline: {red_planes:?}"
    );
    assert!(
        green.size.height >= green.size.width,
        "rotated run should become vertical-ish: {green:?}"
    );
}

#[test]
fn render_frame_interpolates_z_rotation_transform() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(40,40)\\t(0,1000,\\frz90)\\p1}m 0 0 l 40 0 40 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);
    let start_bounds = character_bounds(&start_planes).expect("start bounds");
    let end_bounds = character_bounds(&end_planes).expect("end bounds");

    assert!(start_bounds.width() > start_bounds.height());
    assert!(end_bounds.height() > end_bounds.width());
}

#[test]
fn render_frame_applies_fad_alpha() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fad(200,200)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

    // Since libass 0.17.5, fully transparent images are omitted from the
    // returned list rather than exposed with an alpha byte of 0xFF.
    assert!(start_planes.is_empty());
    let mid_alpha = mid_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("mid alpha");
    let end_alpha = end_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("end alpha");

    assert!(end_alpha > mid_alpha);
}

#[test]
fn render_frame_applies_full_fade_alpha() {
    let track = parse_script_text("[Script Info]\nPlayResX: 200\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\fade(255,0,128,0,200,700,1000)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let middle_planes = engine.render_frame_with_provider(&track, &provider, 400);
    let late_planes = engine.render_frame_with_provider(&track, &provider, 850);

    assert!(start_planes.is_empty());
    let middle_alpha = middle_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("middle alpha");
    let late_alpha = late_planes
        .iter()
        .map(|plane| plane.color.0 & 0xFF)
        .max()
        .expect("late alpha");

    assert!(late_alpha > middle_alpha);
    assert!(late_alpha < 0xFF);
}

#[test]
fn render_frame_switches_karaoke_fill_after_elapsed_span() {
    // libass ass_parse.c process_karaoke_effects: \k sets tm_end = tm_start,
    // so a syllable turns primary at its START.  At 200ms the first \k50
    // syllable (start 0) is already primary while the second (start 500ms)
    // is still secondary; at 700ms both are primary.
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\k50}Ka{\\k50}ra").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
    let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

    assert!(
        early_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100),
        "an active \\k syllable is primary from its start time"
    );
    assert!(
        early_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x6655_4400),
        "an upcoming \\k syllable stays secondary until it starts"
    );
    assert!(
        late_planes
            .iter()
            .all(|plane| plane.kind != ass::ImageType::Character || plane.color.0 == 0x3322_1100),
        "all syllables are primary once started"
    );
}

#[test]
fn render_frame_sweeps_karaoke_fill_during_active_span() {
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\K100}Kara").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        mid_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
    assert!(
        mid_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x6655_4400)
    );
}

#[test]
fn sweep_emits_both_libass_colours_at_the_exact_start_boundary() {
    // libass anchors a \kf split at the leftmost transformed outline.  At
    // exact progress zero the rounded edge still leaves a visible primary
    // antialias column; the rest of the word remains secondary.  This is the
    // boundary exercised at 2000/6000/8000/10000 ms by libass-tests v4++/kt.ass.
    let track = parse_script_text("[Script Info]\nPlayResX: 320\nPlayResY: 100\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: D,BundledAileron,35,&HFFFFFF,&H0000FF,&H000000,&H000000,0,0,0,0,100,100,0,0,1,0,0,7,20,5,4,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,D,,0,0,0,,{\\kf100}Kara{\\kt50\\kf100}oke")
        .expect("kt boundary fixture parses");
    let planes = RenderEngine::new().render_frame_with_provider(
        &track,
        &BundledFontProvider::aileron_regular(),
        0,
    );
    let character_colours = planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character)
        .map(|plane| plane.color.0)
        .collect::<std::collections::HashSet<_>>();

    assert!(character_colours.contains(&0xFFFF_FF00));
    assert!(character_colours.contains(&0xFF00_0000));
}

#[test]
fn official_karaoke_runsplit_keeps_implicit_word_secondary_until_prior_word_ends() {
    let track = parse_script_text("[Script Info]\nPlayResX: 640\nPlayResY: 120\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,BundledAileron,40,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,5,0,5,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:50.00,Default,,0,0,0,,{\\k162}hodie{\\i1}que{\\r} |{\\k118}{\\board1\\c&HFF9920&}cael{\\b1\\u1}um{\\r} |{\\k24}est |{\\k156}{\\board1\\c&HFF9920&}candid{\\b1\\u1}um")
        .expect("official runsplit fixture parses");
    let engine = RenderEngine::new();
    let provider = BundledFontProvider::aileron_regular();
    let before_end = engine.render_frame_with_provider(&track, &provider, 3120);
    let at_end = engine.render_frame_with_provider(&track, &provider, 4600);
    let has_secondary = |planes: &[ImagePlane]| {
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000)
    };

    assert!(
        has_secondary(&before_end),
        "the final implicit word stays secondary during the preceding \\k156 word"
    );
    assert!(
        !has_secondary(&at_end),
        "the zero-duration implicit word switches at the preceding word's end"
    );
}

#[test]
fn render_frame_keeps_k0_text_in_current_karaoke_word_like_libass() {
    // libass split_style_runs ignores effect_skip_timing and a zero
    // effect_timing does not start a new karaoke word by itself.
    let track = parse_script_text("[Script Info]\nPlayResX: 360\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\k100}A{\\k0}BBBB{\\k50}C").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let color_width = |color: u32| {
        let rects = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == color)
            .filter_map(plane_ink_bounds)
            .collect::<Vec<_>>();
        let min = rects.iter().map(|rect| rect.x_min).min()?;
        let max = rects.iter().map(|rect| rect.x_max).max()?;
        Some(max - min)
    };

    let primary_width = color_width(0x3322_1100).expect("primary karaoke text");
    let secondary_width = color_width(0x6655_4400).expect("upcoming karaoke text");
    assert!(
        primary_width > secondary_width * 2,
        "\\k0 text should stay in the current primary karaoke word: primary={primary_width}, secondary={secondary_width}"
    );
}

#[test]
fn render_frame_keeps_kt_text_in_current_sweep_until_run_break_like_libass() {
    // A \kt after text only affects the next karaoke word once a later run
    // break occurs; it does not stop the active \K sweep for following glyphs.
    let track = parse_script_text("[Script Info]\nPlayResX: 420\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\K100}A{\\kt50}WWWWWWWW").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let color_width = |color: u32| {
        let rects = planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == color)
            .filter_map(plane_ink_bounds)
            .collect::<Vec<_>>();
        let min = rects.iter().map(|rect| rect.x_min).min()?;
        let max = rects.iter().map(|rect| rect.x_max).max()?;
        Some(max - min)
    };

    let primary_width = color_width(0x3322_1100).expect("primary sweep text");
    let secondary_width = color_width(0x6655_4400).expect("secondary sweep text");
    assert!(
        secondary_width > primary_width / 2,
        "\\kt text should remain inside the active \\K sweep: primary={primary_width}, secondary={secondary_width}"
    );
}

#[test]
fn render_frame_fills_zero_and_negative_kf_at_syllable_start_like_libass() {
    // libass ass_parse.c process_karaoke_effects marks \kf fully swept when
    // tm_current >= tm_end.  For zero or negative durations, tm_end is not
    // after tm_start, so the syllable is primary right at its start time.
    let script = |tag: &str| {
        format!(
            "[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{{\\an7\\pos(20,20){tag}}}K"
        )
    };
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();

    for tag in ["\\K0", "\\kf0", "\\K-1", "\\kf-1"] {
        let track = parse_script_text(&script(tag)).expect("script should parse");
        let planes = engine.render_frame_with_provider(&track, &provider, 0);
        assert!(
            planes
                .iter()
                .any(|plane| plane.kind == ass::ImageType::Character),
            "{tag} should render a character plane"
        );
        assert!(
            planes.iter().all(
                |plane| plane.kind != ass::ImageType::Character || plane.color.0 == 0x3322_1100
            ),
            "{tag} should be fully primary at the syllable start"
        );
    }
}

#[test]
fn render_frame_retimes_pending_karaoke_with_kt_like_libass() {
    // libass \kt sets effect_skip_timing and clears effect_timing without
    // clearing the active karaoke type.  When this happens before the next
    // glyph is emitted, that glyph is a zero-duration karaoke word at the
    // \kt timestamp.
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\k10\\kt50}K").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let before_start = engine.render_frame_with_provider(&track, &provider, 200);
    let after_start = engine.render_frame_with_provider(&track, &provider, 600);

    assert!(
        before_start
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character),
        "the retimed karaoke syllable should render before its start"
    );
    assert!(
        before_start
            .iter()
            .all(|plane| plane.kind != ass::ImageType::Character || plane.color.0 == 0x6655_4400),
        "\\kt-retimed pending karaoke stays secondary before the \\kt timestamp"
    );
    assert!(
        after_start
            .iter()
            .all(|plane| plane.kind != ass::ImageType::Character || plane.color.0 == 0x3322_1100),
        "\\kt-retimed pending karaoke becomes primary at the \\kt timestamp"
    );
}

#[test]
fn karaoke_elapsed_time_keeps_libass_long_long_range() {
    let event = ParsedEvent {
        start: 0,
        duration: i64::from(i32::MAX) + 50_000,
        ..ParsedEvent::default()
    };
    let now_ms = i64::from(i32::MAX) + 20_000;
    let mut style = ParsedSpanStyle {
        primary_colour: 0x0011_2233,
        secondary_colour: 0x0044_5566,
        ..ParsedSpanStyle::default()
    };
    let started_fill = LayoutGlyphRun {
        karaoke: Some(ParsedKaraokeSpan {
            start_ms: 10_000,
            duration_ms: 10_000,
            mode: ParsedKaraokeMode::FillSwap,
        }),
        ..LayoutGlyphRun::default()
    };
    assert_eq!(
        resolve_run_fill_color(&started_fill, &style, Some(&event), now_ms),
        style.primary_colour,
        "large elapsed times stay after the \\k syllable start"
    );

    let started_outline = LayoutGlyphRun {
        karaoke: Some(ParsedKaraokeSpan {
            start_ms: 10_000,
            duration_ms: 10_000,
            mode: ParsedKaraokeMode::OutlineToggle,
        }),
        ..LayoutGlyphRun::default()
    };
    assert!(
        !karaoke_hides_outline(&started_outline, Some(&event), now_ms),
        "large elapsed times do not wrap into the pre-start \\ko outline state"
    );

    style.rotation_z = 0.0;
    let started_sweep = LayoutGlyphRun {
        karaoke: Some(ParsedKaraokeSpan {
            start_ms: 10_000,
            duration_ms: 10_000,
            mode: ParsedKaraokeMode::Sweep,
        }),
        ..LayoutGlyphRun::default()
    };
    let planes = apply_karaoke_to_character_planes(
        vec![ImagePlane {
            size: Size {
                width: 8,
                height: 1,
            },
            stride: 8,
            color: RgbaColor(0),
            destination: Point { x: 0, y: 0 },
            kind: ass::ImageType::Character,
            bitmap: vec![255; 8],
        }],
        &started_sweep,
        &style,
        Some(&event),
        now_ms,
        0,
        8,
    );
    assert!(
        planes
            .iter()
            .all(|plane| plane.kind != ass::ImageType::Character || plane.color.0 == 0x3322_1100),
        "large elapsed times stay after the \\kf sweep end"
    );
}

#[test]
fn render_frame_reverses_kf_sweep_for_flipped_rotation() {
    // libass ass_parse.c process_karaoke_effects: when fmod(\\frz, 360) lies
    // in (90, 270), \\kf fills right-to-left with swapped colors in glyph
    // space, so after the 180-degree rotation the sweep still appears
    // left-to-right on screen.  Without the reversal the flipped sweep would
    // appear right-to-left.
    let script = |frz: &str| {
        format!(
            "[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{{\\an5\\pos(120,50){frz}\\kf100}}Kara"
        )
    };
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let half_centers = |script_text: &str| {
        let track = parse_script_text(script_text).expect("kf script should parse");
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let center = |color: u32| {
            let rects = planes
                .iter()
                .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == color)
                .filter_map(plane_ink_bounds)
                .collect::<Vec<_>>();
            let min = rects.iter().map(|rect| rect.x_min).min()?;
            let max = rects.iter().map(|rect| rect.x_max).max()?;
            Some((min + max) / 2)
        };
        (center(0x3322_1100), center(0x6655_4400))
    };

    let (upright_primary, upright_secondary) = half_centers(&script(""));
    assert!(
        upright_primary.expect("upright primary half")
            < upright_secondary.expect("upright secondary half"),
        "an upright \\kf fills left-to-right at the midpoint"
    );

    let (flipped_primary, flipped_secondary) = half_centers(&script("\\frz180"));
    assert!(
        flipped_primary.expect("flipped primary half")
            < flipped_secondary.expect("flipped secondary half"),
        "the \\frz180 \\kf sweep still appears left-to-right on screen thanks to the libass right-to-left reversal"
    );
}

#[test]
fn render_frame_hides_outline_for_ko_until_span_ends() {
    // libass render_text: a \ko outline is skipped only while
    // effect_timing <= 0, i.e. before the syllable starts.  The first \ko50
    // syllable's outline is visible from t=0; the second (start 500ms) has
    // no outline at 200ms and gains it at 700ms.
    let track = parse_script_text("[Script Info]\nPlayResX: 240\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H00445566,&H000A0B0C,&H00000000,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Default,,0000,0000,0000,,{\\an7\\pos(20,20)\\ko50}Ko{\\ko50}ra").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let early_planes = engine.render_frame_with_provider(&track, &provider, 200);
    let late_planes = engine.render_frame_with_provider(&track, &provider, 700);

    let outline_ink_width = |planes: &[ImagePlane]| {
        planes
            .iter()
            .filter(|plane| plane.kind == ass::ImageType::Outline)
            .filter_map(plane_ink_bounds)
            .fold(0, |acc, rect| acc + rect.width())
    };
    let early_width = outline_ink_width(&early_planes);
    let late_width = outline_ink_width(&late_planes);
    assert!(
        early_width > 0,
        "a started \\ko syllable keeps its outline from t=0"
    );
    assert!(
        late_width > early_width,
        "the second \\ko syllable's outline appears once it starts: early={early_width} late={late_width}"
    );
}

#[test]
fn vertical_font_raster_advances_rotate_bitmap_like_libass_vertical_faces() {
    // libass DECO_ROTATE (ass_get_glyph_outline + ass_outline_rotate_90):
    // FreeType point (x, y) -> (offs.x + y, offs.y - x), but libass first
    // imports the outline as (x, -y). The resulting screen bitmap is rotated
    // counterclockwise. Without face metrics, offs=(font size, 0), so the new
    // bearings are left'=50-9=41, top'=2-4=-2.
    let glyph = RasterGlyph {
        width: 2,
        height: 3,
        stride: 2,
        left: 4,
        top: 9,
        offset_x: 1,
        offset_y: 2,
        advance_x: 7,
        bitmap: vec![1, 2, 3, 4, 5, 6],
        ..RasterGlyph::default()
    };
    let style = ParsedSpanStyle {
        font_name: "@Vertical".to_string(),
        font_size: 50.0,
        ..ParsedSpanStyle::default()
    };
    let font = FontMatch::unresolved("Vertical", None, FontProviderKind::Null);

    let glyph_infos = [GlyphInfo {
        vertical_rotation_eligible: true,
        ..GlyphInfo::default()
    }];
    let glyphs = apply_vertical_font_raster_advances(vec![glyph], &glyph_infos, &style, &font);
    let rotated = &glyphs[0];

    assert_eq!(rotated.width, 3);
    assert_eq!(rotated.height, 2);
    assert_eq!(rotated.stride, 3);
    assert_eq!(rotated.bitmap, vec![2, 4, 6, 1, 3, 5]);
    assert_eq!(rotated.left, 41);
    assert_eq!(rotated.top, -2);
    assert_eq!(rotated.advance_x, 50);
    assert_eq!(rotated.advance_y, 0);
    assert_eq!(rotated.advance_x_26_6, 50 * 64);
}

#[test]
fn vertical_font_mixed_run_rotates_only_eligible_glyphs() {
    let glyph = RasterGlyph {
        width: 2,
        height: 3,
        stride: 2,
        left: 4,
        top: 9,
        advance_x: 7,
        advance_x_26_6: 7 * 64,
        bitmap: vec![1, 2, 3, 4, 5, 6],
        ..RasterGlyph::default()
    };
    let glyph_infos = [
        GlyphInfo {
            // ASCII remains upright in a vertical face.
            vertical_rotation_eligible: false,
            ..GlyphInfo::default()
        },
        GlyphInfo {
            // U+02F1 is libass's inclusive DECO_ROTATE lower bound.
            vertical_rotation_eligible: true,
            ..GlyphInfo::default()
        },
    ];
    let style = ParsedSpanStyle {
        font_name: "@Vertical".to_string(),
        font_size: 50.0,
        ..ParsedSpanStyle::default()
    };
    let font = FontMatch::unresolved("Vertical", None, FontProviderKind::Null);

    let glyphs = apply_vertical_font_raster_advances(
        vec![glyph.clone(), glyph],
        &glyph_infos,
        &style,
        &font,
    );

    assert_eq!(glyphs[0].width, 2);
    assert_eq!(glyphs[0].height, 3);
    assert_eq!(glyphs[0].bitmap, vec![1, 2, 3, 4, 5, 6]);
    assert_eq!(glyphs[0].advance_x, 7);
    assert_eq!(glyphs[1].width, 3);
    assert_eq!(glyphs[1].height, 2);
    assert_eq!(glyphs[1].bitmap, vec![2, 4, 6, 1, 3, 5]);
    assert_eq!(glyphs[1].advance_x, 50);
}

#[test]
fn bundled_vertical_karaoke_runs_share_one_event_rotation_pivot() {
    // Mirrors libass-tests karaoke/216-vertical.ass with a small bundled font:
    // U+02F1 is libass's first DECO_ROTATE codepoint. An @font rotates each
    // eligible glyph outline, while the style's 270-degree angle turns the
    // line advance vertical. Every karaoke syllable is a separate style run,
    // but libass calculates one implicit pivot from the whole event bbox.
    let track = parse_script_text("[Script Info]\nScriptType: v4.00+\nPlayResX: 320\nPlayResY: 320\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Vertical,@BundledAileron,40,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,270,1,1,0,7,100,0,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:02.00,Vertical,,0,0,0,,{\\K25}˱{\\K25}˱{\\K25}˱{\\K25}˱{\\K25}˱{\\K25}˱")
        .expect("vertical karaoke fixture parses");
    let engine = RenderEngine::new();
    let sweep_planes =
        engine.render_frame_with_provider(&track, &BundledFontProvider::aileron_regular(), 510);
    let primary = sweep_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFFFF_FF00);
    let secondary = sweep_planes
        .iter()
        .filter(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0xFF00_0000);
    assert!(
        primary.clone().any(|left| {
            secondary.clone().any(|right| {
                left.destination.y == right.destination.y
                    && left.size.height == right.size.height
                    && left.destination.x + left.size.width == right.destination.x
            })
        }),
        "an in-progress quarter-turned syllable must retain libass's screen-horizontal primary/secondary split"
    );

    let planes =
        engine.render_frame_with_provider(&track, &BundledFontProvider::aileron_regular(), 700);

    let bounds = visible_bounds(&planes).expect("vertical karaoke fixture renders");
    assert_eq!(
        bounds,
        Rect {
            x_min: 58,
            y_min: 11,
            x_max: 78,
            y_max: 203,
        }
    );
    assert_eq!(
        visible_kind_bounds(&planes, ass::ImageType::Outline),
        Some(Rect {
            x_min: 58,
            y_min: 11,
            x_max: 78,
            y_max: 203,
        })
    );
    assert_eq!(
        visible_kind_bounds(&planes, ass::ImageType::Character),
        Some(Rect {
            x_min: 59,
            y_min: 12,
            x_max: 77,
            y_max: 202,
        })
    );
}

#[test]
fn quarter_turn_karaoke_sweep_stays_horizontal_and_tracks_progress() {
    let plane = ImagePlane {
        size: Size {
            width: 48,
            height: 64,
        },
        stride: 48,
        color: RgbaColor(0),
        destination: Point { x: 20, y: 80 },
        kind: ass::ImageType::Character,
        bitmap: vec![255; 48 * 64],
    };
    let run = LayoutGlyphRun {
        karaoke: Some(ParsedKaraokeSpan {
            start_ms: 0,
            duration_ms: 400,
            mode: ParsedKaraokeMode::Sweep,
        }),
        ..LayoutGlyphRun::default()
    };
    let style = ParsedSpanStyle {
        primary_colour: 0x0000_00FF,
        secondary_colour: 0x00FF_FFFF,
        rotation_z: 270.0,
        ..ParsedSpanStyle::default()
    };
    let event = ParsedEvent {
        start: 30,
        duration: 2_000,
        ..ParsedEvent::default()
    };

    let at_start = apply_quarter_turn_karaoke_sweep_after_transform(
        vec![plane.clone()],
        &run,
        &style,
        Some(&event),
        30,
        62,
    );
    assert_eq!(at_start[0].destination, Point { x: 20, y: 80 });
    assert_eq!(
        at_start[0].size,
        Size {
            width: 1,
            height: 64
        }
    );
    assert_eq!(at_start[0].color, RgbaColor(0xFF00_0000));
    assert_eq!(at_start[1].destination, Point { x: 21, y: 80 });
    assert_eq!(
        at_start[1].size,
        Size {
            width: 47,
            height: 64
        }
    );
    assert_eq!(at_start[1].color, RgbaColor(0xFFFF_FF00));

    let mid_sweep = apply_quarter_turn_karaoke_sweep_after_transform(
        vec![plane],
        &run,
        &style,
        Some(&event),
        130,
        62,
    );
    assert_eq!(
        mid_sweep[0].size,
        Size {
            width: 17,
            height: 64
        }
    );
    assert_eq!(mid_sweep[1].destination, Point { x: 37, y: 80 });
    assert_eq!(
        mid_sweep[1].size,
        Size {
            width: 31,
            height: 64
        }
    );
}

#[test]
fn blurred_vector_drawing_expands_fill_plane_like_libass() {
    let script = "[Script Info]\nScriptType: v4.00+\nPlayResX: 1920\nPlayResY: 1080\nScaledBorderAndShadow: yes\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Placas,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,2,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Placas,,0,0,0,,{\\blur6\\p1\\c&HC6BECA&\\fscx165\\fscy138\\pos(948,1324)}m 0 0 b -3 -28 -6 -56 -9 -84 b -18 -113 -6 -135 -5 -160 b -3 -184 1 -208 3 -232 b 125 -233 248 -235 370 -236 b 377 -220 386 -204 393 -188 b 397 -167 403 -146 407 -125 b 409 -109 411 -93 413 -77 b 421 -61 431 -44 439 -28 b 440 -18 441 -7 442 3 b 295 3 147 1 0 1\n";
    let track = parse_script_text(script).expect("track parses");
    let engine = RenderEngine::new();
    let planes = engine.render_frame_with_provider(&track, &NullFontProvider, 500);

    assert_eq!(planes.len(), 1);
    let plane = &planes[0];
    // \blur6 pads the plane by the blur kernel; per-point polygon scaling can
    // round the unblurred extent by a pixel or two vs libass.
    assert!(
        (plane.destination.y - 650).abs() <= 4,
        "y={}",
        plane.destination.y
    );
    assert!(
        (plane.size.width - 788).abs() <= 4,
        "width={}",
        plane.size.width
    );
    assert!(
        (plane.size.height - 372).abs() <= 4,
        "height={}",
        plane.size.height
    );
}

#[test]
fn render_frame_renders_drawing_plane() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Character && plane.color.0 == 0x3322_1100)
    );
    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert_eq!(plane.destination.x, 10);
    assert_eq!(plane.destination.y, 10);
    assert!(plane.bitmap.contains(&255));
}

#[test]
fn render_frame_collapses_drawing_when_scale_base_is_nonpositive_like_libass() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p32}m 0 0 l 64 0 64 64 0 64").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = NullFontProvider;
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes.is_empty(),
        "libass lshiftwrapi makes \\p32 collapse to a zero-scale drawing"
    );
}

#[test]
fn render_frame_renders_drawing_holes_with_nonzero_winding() {
    // libass rasterizes drawings with its nonzero-winding rasterizer
    // (ass_rasterizer.c): nested same-direction squares fill solid, while an
    // opposite-direction inner square punches a hole.
    let script = |inner: &str| {
        format!(
            "[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{{\\an7\\pos(10,10)\\p1}}m 0 0 l 20 0 20 20 0 20 {inner}"
        )
    };
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let center_alpha = |script_text: &str| {
        let track = parse_script_text(script_text).expect("drawing script parses");
        let planes = engine.render_frame_with_provider(&track, &provider, 500);
        let plane = planes
            .iter()
            .find(|plane| plane.kind == ass::ImageType::Character)
            .expect("drawing plane")
            .clone();
        let local_x = 20 - plane.destination.x;
        let local_y = 20 - plane.destination.y;
        plane.bitmap[local_y as usize * plane.stride as usize + local_x as usize]
    };

    let same_direction = center_alpha(&script("m 5 5 l 15 5 15 15 5 15"));
    assert_eq!(
        same_direction, 255,
        "same-direction nested squares fill solid under nonzero winding"
    );
    let opposite_direction = center_alpha(&script("m 5 5 l 5 15 15 15 15 5"));
    assert_eq!(
        opposite_direction, 0,
        "an opposite-direction inner square leaves a hole under nonzero winding"
    );
}

#[test]
fn render_frame_antialiases_vector_drawing_edges() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 20 0 20 20").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);
    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");

    assert!(
        plane.bitmap.iter().any(|value| *value > 0 && *value < 255),
        "vector drawing edges should keep libass-like partial coverage instead of binary rasterization"
    );
}

#[test]
fn render_frame_renders_bezier_drawing_plane() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 b 10 0 10 10 0 10").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert!(plane.bitmap.contains(&255));
    assert!(plane.size.width >= 8);
    assert!(plane.size.height >= 8);
}

#[test]
fn render_frame_emits_outline_and_shadow_for_drawings() {
    let track = parse_script_text("[Script Info]\nPlayResX: 100\nPlayResY: 100\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H000A0B0C,&H00445566,0,0,0,0,100,100,0,0,1,2,3,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline && plane.color.0 == 0x0C0B_0A00)
    );
    assert!(
        planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Shadow && plane.color.0 == 0x6655_4400)
    );
}

#[test]
fn render_frame_renders_spline_drawing_plane() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 s 10 0 10 10 0 10 p -5 5 c").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert!(plane.bitmap.contains(&255));
    assert!(plane.size.width >= 10);
    assert!(plane.size.height >= 10);
}

#[test]
fn render_frame_renders_non_closing_move_subpaths() {
    let track = parse_script_text("[Script Info]\nPlayResX: 120\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H00112233,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\p1}m 0 0 l 8 0 8 8 0 8 n 20 20 l 28 20 28 28 20 28").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let planes = engine.render_frame_with_provider(&track, &provider, 500);

    let plane = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("drawing plane");
    assert!(plane.bitmap.contains(&255));
    assert!(plane.size.width >= 28);
    assert!(plane.size.height >= 28);
}

#[test]
fn render_frame_applies_timed_transform_style() {
    let track = parse_script_text("[Script Info]\nPlayResX: 160\nPlayResY: 120\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,sans,24,&H000000FF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:00.00,0:00:01.00,Default,,0000,0000,0000,,{\\an7\\pos(10,10)\\t(0,1000,\\1c&H00112233&\\fs48\\bord4)}Hi").expect("script should parse");
    let engine = RenderEngine::new();
    let provider = FontconfigProvider::new();
    let start_planes = engine.render_frame_with_provider(&track, &provider, 0);
    let mid_planes = engine.render_frame_with_provider(&track, &provider, 500);
    let end_planes = engine.render_frame_with_provider(&track, &provider, 999);

    assert!(
        !start_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );
    assert!(
        mid_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );
    assert!(
        end_planes
            .iter()
            .any(|plane| plane.kind == ass::ImageType::Outline)
    );

    let start_fill = start_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("start fill")
        .color
        .0;
    let end_fill = end_planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("end fill")
        .color
        .0;
    assert_ne!(start_fill, end_fill);
    assert!(total_plane_area(&end_planes) > total_plane_area(&start_planes));
}

#[test]
fn decimal_thin_ring_keeps_latest_libass_geometry_and_continuity() {
    let track = parse_script_text(include_str!(
        "../../rassa-test/fixtures/libass/compare/edge/decimal_thin_ring.ass"
    ))
    .expect("decimal thin-ring fixture parses");
    let planes = RenderEngine::new().render_frame_with_provider(&track, &NullFontProvider, 500);
    let character = planes
        .iter()
        .find(|plane| plane.kind == ass::ImageType::Character)
        .expect("the sub-pixel ring must retain visible fill coverage");

    assert_eq!(
        visible_bounds(std::slice::from_ref(character)),
        Some(Rect {
            x_min: 947,
            y_min: 527,
            x_max: 973,
            y_max: 553,
        }),
        "latest libass 3087d2b has the same 26x26 visible character geometry"
    );
    let lit_pixels = character.bitmap.iter().filter(|alpha| **alpha > 0).count();
    let alpha_mass: u64 = character.bitmap.iter().map(|alpha| u64::from(*alpha)).sum();
    assert!(
        (100..=130).contains(&lit_pixels),
        "latest libass has 103 lit character pixels; got {lit_pixels}"
    );
    assert!(
        (4_800..=5_800).contains(&alpha_mass),
        "latest libass character alpha mass is 5341; got {alpha_mass}"
    );

    let width = usize::try_from(character.size.width).expect("positive width");
    let height = usize::try_from(character.size.height).expect("positive height");
    let stride = usize::try_from(character.stride).expect("positive stride");
    let mut visited = vec![false; stride * height];
    let mut components = 0;
    for y in 0..height {
        for x in 0..width {
            let index = y * stride + x;
            if character.bitmap[index] == 0 || visited[index] {
                continue;
            }
            components += 1;
            let mut pending = vec![(x, y)];
            visited[index] = true;
            while let Some((current_x, current_y)) = pending.pop() {
                for delta_y in -1_isize..=1 {
                    for delta_x in -1_isize..=1 {
                        if delta_x == 0 && delta_y == 0 {
                            continue;
                        }
                        let Some(next_x) = current_x.checked_add_signed(delta_x) else {
                            continue;
                        };
                        let Some(next_y) = current_y.checked_add_signed(delta_y) else {
                            continue;
                        };
                        if next_x >= width || next_y >= height {
                            continue;
                        }
                        let next = next_y * stride + next_x;
                        if character.bitmap[next] > 0 && !visited[next] {
                            visited[next] = true;
                            pending.push((next_x, next_y));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(components, 1, "the bubble ring must not break into dots");
}

#[test]
fn fixed_point_vector_clip_preserves_fractional_mask_and_inverse_complement() {
    // These two 26.6 contours are only 20/64 px apart. Rounding each source
    // coordinate to an integer makes them identical and erases the ring.
    let exact_clip = ParsedVectorClip {
        scale: 1,
        polygons: vec![
            vec![
                Point { x: 6, y: 6 },
                Point { x: 506, y: 6 },
                Point { x: 506, y: 506 },
                Point { x: 6, y: 506 },
            ],
            vec![
                Point { x: 26, y: 26 },
                Point { x: 26, y: 486 },
                Point { x: 486, y: 486 },
                Point { x: 486, y: 26 },
            ],
        ],
    };
    let source = solid_test_plane(8, 8, Point { x: 0, y: 0 });
    let regular = apply_vector_clip_d6(vec![source.clone()], &exact_clip, false)
        .into_iter()
        .next()
        .expect("regular exact clip");
    let inverse = apply_vector_clip_d6(vec![source], &exact_clip, true)
        .into_iter()
        .next()
        .expect("inverse exact clip");

    assert_eq!(
        regular.size,
        Size {
            width: 8,
            height: 8
        }
    );
    assert_eq!(inverse.size, regular.size);
    assert!(regular.bitmap[4] > 0 && regular.bitmap[4] < 255);
    assert_eq!(regular.bitmap[4 * 8 + 4], 0);
    for (regular_alpha, inverse_alpha) in regular.bitmap.iter().zip(&inverse.bitmap) {
        assert_eq!(u16::from(*regular_alpha) + u16::from(*inverse_alpha), 255);
    }

    let rounded_away = ParsedVectorClip {
        scale: 1,
        polygons: vec![
            vec![
                Point { x: 0, y: 0 },
                Point { x: 8, y: 0 },
                Point { x: 8, y: 8 },
                Point { x: 0, y: 8 },
            ],
            vec![
                Point { x: 0, y: 0 },
                Point { x: 0, y: 8 },
                Point { x: 8, y: 8 },
                Point { x: 8, y: 0 },
            ],
        ],
    };
    let collapsed = mask_plane_with_vector_clip(
        solid_test_plane(8, 8, Point { x: 0, y: 0 }),
        &rounded_away,
        false,
    )
    .expect("a valid but empty rounded clip retains an empty image node");
    assert_eq!(collapsed.size, Size::default());
}

#[cfg(all(unix, not(target_os = "macos"), not(target_arch = "wasm32")))]
#[test]
fn portable_font_vertical_metrics_match_freetype_path() {
    let fixture_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../rassa-test/fixtures/libass/compare");
    for fixture in [
        "test/font1.ttf",
        "test/font2.otf",
        "broad/font1.ttf",
        "broad/font2.otf",
    ] {
        let path = fixture_root.join(fixture);
        let data = std::fs::read(&path).expect("fixture font readable");
        let font = FontMatch {
            path: Some(path),
            face_index: Some(0),
            ..FontMatch::unresolved("fixture", None, FontProviderKind::Fontconfig)
        };
        for size_26_6 in [1, 64, 640, 1216, 2400, 7777, 131_072] {
            let freetype = font_vertical_metrics(&font, size_26_6);
            let portable = font_vertical_metrics_from_data(&data, 0, size_26_6);
            assert!(freetype.is_some(), "{fixture} at size {size_26_6}");
            assert_eq!(freetype, portable, "{fixture} at size {size_26_6}");
        }
    }
}
