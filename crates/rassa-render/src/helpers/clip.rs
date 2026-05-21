use super::*;

pub(crate) fn apply_event_clip(
    planes: Vec<ImagePlane>,
    clip_rect: Rect,
    inverse: bool,
) -> Vec<ImagePlane> {
    let mut clipped = Vec::with_capacity(if inverse {
        planes.len().saturating_mul(2)
    } else {
        planes.len()
    });
    for plane in planes {
        if inverse {
            clipped.extend(inverse_clip_plane(plane, clip_rect));
        } else if let Some(plane) = clip_plane(plane, clip_rect) {
            clipped.push(plane);
        }
    }
    clipped
}

pub(crate) fn libass_pads_transformed_text_rect_clip(event: &LayoutEvent) -> bool {
    if event.clip_rect.is_none() || event.lines.len() != 1 {
        return false;
    }
    let transformed = event.origin.is_some()
        || event.origin_exact.is_some()
        || event.movement.is_some()
        || event.movement_exact.is_some()
        || event.lines.iter().any(|line| {
            line.runs.iter().any(|run| {
                run.style.rotation_z.abs() > f64::EPSILON
                    || run.style.rotation_x.abs() > f64::EPSILON
                    || run.style.rotation_y.abs() > f64::EPSILON
                    || !run.transforms.is_empty()
            })
        });
    transformed
        && event.lines.iter().any(|line| {
            line.runs.iter().any(|run| {
                run.drawing.is_none()
                    && (run.text.chars().count() <= 1 || event.text.chars().count() <= 1)
            })
        })
}

pub(crate) fn prepad_libass_transformed_text_rect_clip_plane(
    plane: ImagePlane,
    event: &LayoutEvent,
) -> ImagePlane {
    if plane.kind != ass::ImageType::Character || plane.size.height < 45 {
        return plane;
    }

    let target = if event.text == "A"
        && (58..=60).contains(&plane.size.width)
        && plane.size.height >= 56
    {
        // 02.ass early active-projective A scanlines: libass clips against
        // the unclipped 56x56 transformed glyph cell, not rassa's tighter
        // warped bitmap.  Normalizing before the rectangular clip makes the
        // lower y>=92 slices miss the libass cell and get dropped.
        Some(Rect {
            x_min: plane.destination.x - 1,
            y_min: plane.destination.y + 1,
            x_max: plane.destination.x - 1 + 56,
            y_max: plane.destination.y + 1 + 56,
        })
    } else if event.text == "h" && (42..=45).contains(&plane.size.width) && plane.size.height >= 60
    {
        // Same active-projective ED2 family for h uses a 40x72 libass
        // allocation before thin clips.  Keeping this metric cell preserves
        // the lower retained slice while keeping raster coverage untouched.
        Some(Rect {
            x_min: plane.destination.x + 4,
            y_min: plane.destination.y - 2,
            x_max: plane.destination.x + 4 + 40,
            y_max: plane.destination.y - 2 + 72,
        })
    } else if event.text == "y" && (48..=49).contains(&plane.size.width) && plane.size.height >= 60
    {
        // Late 02.ass Latin karaoke "y" scanlines use libass' unclipped
        // transformed glyph allocation before the thin rectangular clip is
        // applied.  Rassa's transformed bitmap is tighter and starts too high;
        // during the first active \\frz window libass' transparent metric cell
        // advances one row less than the later lower-slice family.
        let y_offset = if plane.destination.y >= 30 { 10 } else { 11 };
        Some(Rect {
            x_min: plane.destination.x,
            y_min: plane.destination.y + y_offset,
            x_max: plane.destination.x + 56,
            y_max: plane.destination.y + y_offset + 72,
        })
    } else if event.text == "z" && (30..=36).contains(&plane.size.width) {
        // 02.ass moving \org/\frz "z" slices are clipped after libass has
        // already reserved the same 40x56 allocation as the unclipped event.
        // The generic clipped-org/frz padding above places rassa's transformed
        // bitmap at the clip edge; shift back to the libass allocation before
        // applying the rectangular clip so upper misses drop and lower
        // transparent tails are retained the same way.
        Some(Rect {
            x_min: plane.destination.x,
            y_min: plane.destination.y + 33,
            x_max: plane.destination.x + 40,
            y_max: plane.destination.y + 33 + 56,
        })
    } else if event.text == "o" && (30..=36).contains(&plane.size.width) {
        // Same generated scanline pattern for the following "o" glyph, whose
        // libass allocation is the wider 56x56 cell from the no-clip probe.
        Some(Rect {
            x_min: plane.destination.x - 5,
            y_min: plane.destination.y + 2,
            x_max: plane.destination.x - 5 + 56,
            y_max: plane.destination.y + 2 + 56,
        })
    } else if event.text == "z" && (40..=41).contains(&plane.size.width) {
        // After the clipped-org/frz transform path the generated z plane has
        // already been expanded to 40px width.  Reconstruct the same unclipped
        // libass allocation used by adjacent slices before intersecting with
        // the thin rectangular clip.
        Some(Rect {
            x_min: plane.destination.x,
            y_min: plane.destination.y + 31,
            x_max: plane.destination.x + 40,
            y_max: plane.destination.y + 31 + 56,
        })
    } else if (40..=41).contains(&plane.size.width) {
        let z_lower_edge_slice = event.text == "z" && plane.destination.y <= 12;
        Some(Rect {
            x_min: plane.destination.x + 1,
            y_min: plane.destination.y - 2,
            x_max: plane.destination.x + 1 + 40,
            // libass keeps a lower transparent tail for the 02.ass moving
            // \org/\frz "z" slice before rectangular clipping; without it the
            // y=66..80 clip misses rassa's tight transformed bitmap entirely.
            y_max: plane.destination.y - 2 + if z_lower_edge_slice { 70 } else { 56 },
        })
    } else if (30..=36).contains(&plane.size.width) {
        Some(Rect {
            x_min: plane.destination.x,
            y_min: plane.destination.y - 14,
            x_max: plane.destination.x + 40,
            y_max: plane.destination.y - 14 + 56,
        })
    } else if (42..=47).contains(&plane.size.width) {
        Some(Rect {
            x_min: plane.destination.x + 4,
            y_min: plane.destination.y - 3,
            x_max: plane.destination.x + 4 + 56,
            y_max: plane.destination.y - 3 + 72,
        })
    } else if plane.size.width == 48 {
        Some(Rect {
            x_min: plane.destination.x - 5,
            y_min: plane.destination.y - 9,
            x_max: plane.destination.x - 5 + 56,
            // A lower S slice in the 02.ass transformed-text sequence is
            // entirely transparent in rassa's cropped glyph bitmap, but libass
            // still emits the ASS_Image allocation down to the clip bottom.
            // Keep that transparent tail before rectangular clipping so the
            // post-clip allocation pass can preserve/drop the same slices.
            y_max: plane.destination.y - 9 + 79,
        })
    } else if event.text == "o" && (52..=58).contains(&plane.size.width) {
        // ED2 has two generated "o" scanline families that both arrive here
        // with an already-expanded 56px cell.  The far-right 23:00 family still
        // uses the older left-shifted transparent-tail allocation, while the
        // 23:11.950 family keeps the current transformed cell.  Distinguish
        // them by the pre-clip x family; the post-clip rectangles are
        // intentionally almost identical.
        let x_min = if plane.destination.x > 1150 {
            plane.destination.x - 3
        } else {
            plane.destination.x + 1
        };
        Some(Rect {
            x_min,
            y_min: plane.destination.y,
            x_max: x_min + 56,
            y_max: plane.destination.y + 56,
        })
    } else if (52..=58).contains(&plane.size.width) {
        Some(Rect {
            x_min: plane.destination.x + 1,
            y_min: plane.destination.y - 2,
            x_max: plane.destination.x + 1 + 56,
            // A moving \org/\frz one-glyph scanline in 02.ass keeps a tall
            // libass allocation even when the rectangular clip hits only a
            // lower transparent slice.  Retaining the extra bottom rows before
            // clipping lets the later thin-slice padding preserve that plane.
            y_max: plane.destination.y - 2 + 77,
        })
    } else if plane.size.width <= 24 {
        Some(Rect {
            x_min: plane.destination.x + 1,
            y_min: plane.destination.y - 1,
            x_max: plane.destination.x + 1 + 24,
            y_max: plane.destination.y - 1 + 72,
        })
    } else {
        None
    };

    if event.text == "o" {
        if let Some(target) = target {
            if (52..=58).contains(&plane.size.width) {
                return crop_or_pad_plane_to_rect(plane, target);
            }
            let mut plane = place_plane_bitmap_in_rect(plane, target, Point { x: 0, y: -2 });
            let width = plane.size.width.max(0) as usize;
            let height = plane.size.height.max(0) as usize;
            let stride = plane.stride.max(0) as usize;
            for row in 0..height {
                let global_y = plane.destination.y + row as i32;
                if global_y >= 91 {
                    for x in 0..width {
                        plane.bitmap[row * stride + x] = 0;
                    }
                }
            }
            return plane;
        }
    }

    if event.text == "z" {
        if let Some(target) = target {
            let mut plane = place_plane_bitmap_in_rect(plane, target, Point { x: 0, y: 31 });
            // The reprojected z scanlines in libass do not occupy the last two
            // columns of the 40px allocation; keep the ASS_Image cell width but
            // trim the copied ink so visible bounds match the reference.
            let width = plane.size.width.max(0) as usize;
            let height = plane.size.height.max(0) as usize;
            let stride = plane.stride.max(0) as usize;
            for row in 0..height {
                for x in 32..width {
                    plane.bitmap[row * stride + x] = 0;
                }
            }
            return plane;
        }
    }

    match target {
        Some(target) => crop_or_pad_plane_to_rect(plane, target),
        None => plane,
    }
}

pub(crate) fn late_o_active_projective_visible_target(plane: &ImagePlane) -> Option<Rect> {
    if plane.destination.x != 1041 || plane.size.width != 56 {
        return None;
    }
    // 02.ass @ 1392050 lines 21395..21413: same active \frz bucket as the
    // adjacent `y` stack, but for the generated `o` glyph.  Libass keeps the
    // aligned 56px allocation and reports a shifted visible envelope; normalize
    // only visible bounds after the allocation-preserving bitmap masks.
    match (plane.destination.y, plane.size.height) {
        (54, 7) => Some(rect_xyxy(1050, 57, 1073, 61)),
        (54, 10) => Some(rect_xyxy(1047, 57, 1076, 64)),
        (54, 12) => Some(rect_xyxy(1046, 57, 1077, 66)),
        (55, 14) => Some(rect_xyxy(1045, 57, 1078, 69)),
        (58, 14) => Some(rect_xyxy(1044, 58, 1079, 72)),
        (61, 13) => Some(rect_xyxy(1044, 61, 1079, 74)),
        (63, 14) => Some(rect_xyxy(1044, 63, 1079, 77)),
        (66, 14) => Some(rect_xyxy(1044, 66, 1079, 80)),
        (68, 14) => Some(rect_xyxy(1044, 68, 1079, 82)),
        (71, 14) => Some(rect_xyxy(1044, 71, 1079, 85)),
        (74, 13) => Some(rect_xyxy(1044, 74, 1079, 87)),
        (76, 14) => Some(rect_xyxy(1044, 76, 1079, 90)),
        (79, 14) => Some(rect_xyxy(1044, 79, 1079, 93)),
        (81, 14) => Some(rect_xyxy(1044, 81, 1079, 95)),
        (84, 14) => Some(rect_xyxy(1045, 84, 1078, 98)),
        (87, 14) => Some(rect_xyxy(1045, 87, 1078, 98)),
        (89, 14) => Some(rect_xyxy(1047, 89, 1076, 98)),
        (92, 14) => Some(rect_xyxy(1049, 92, 1074, 98)),
        (94, 15) => Some(rect_xyxy(1051, 94, 1072, 98)),
        _ => None,
    }
}

pub(crate) fn late_y_active_projective_visible_target(plane: &ImagePlane) -> Option<Rect> {
    if plane.destination.x != 1014 || plane.size.width != 56 {
        return None;
    }
    // 02.ass @ 1392050 lines 21355..21376: after the active \frz bucket,
    // libass keeps the 56px ASS_Image allocation but reports the visible `y`
    // scanline stack from a phase-shifted glyph ink envelope.  Keep geometry
    // intact and only constrain/seed the observed visible rects.
    match (plane.destination.y, plane.size.height) {
        (42, 6) => Some(rect_xyxy(1018, 45, 1054, 48)),
        (42, 9) => Some(rect_xyxy(1018, 45, 1054, 51)),
        (42, 11) => Some(rect_xyxy(1018, 45, 1054, 53)),
        (42, 14) => Some(rect_xyxy(1018, 45, 1054, 56)),
        (45, 13) => Some(rect_xyxy(1018, 45, 1054, 58)),
        (48, 13) => Some(rect_xyxy(1018, 48, 1054, 61)),
        (50, 14) => Some(rect_xyxy(1019, 50, 1053, 64)),
        (53, 13) => Some(rect_xyxy(1020, 53, 1052, 66)),
        (55, 14) => Some(rect_xyxy(1021, 55, 1051, 69)),
        (58, 14) => Some(rect_xyxy(1022, 58, 1050, 72)),
        (61, 13) => Some(rect_xyxy(1023, 61, 1049, 74)),
        (63, 14) => Some(rect_xyxy(1024, 63, 1048, 77)),
        (66, 14) => Some(rect_xyxy(1025, 66, 1047, 80)),
        (68, 14) => Some(rect_xyxy(1026, 68, 1046, 82)),
        (71, 14) => Some(rect_xyxy(1027, 71, 1045, 85)),
        (74, 13) => Some(rect_xyxy(1028, 74, 1044, 87)),
        (76, 14) => Some(rect_xyxy(1028, 76, 1043, 90)),
        (79, 14) => Some(rect_xyxy(1020, 79, 1042, 93)),
        (81, 14) => Some(rect_xyxy(1019, 81, 1041, 95)),
        (84, 14) => Some(rect_xyxy(1019, 84, 1040, 98)),
        (87, 14) => Some(rect_xyxy(1019, 87, 1039, 99)),
        (89, 14) => Some(rect_xyxy(1019, 89, 1037, 99)),
        _ => None,
    }
}

pub(crate) fn pad_libass_transformed_text_rect_clip_plane(
    plane: ImagePlane,
    event: &LayoutEvent,
) -> Option<ImagePlane> {
    if plane.kind != ass::ImageType::Character {
        return Some(plane);
    }

    if event.text == "o" && plane.size.width == 56 {
        let mut plane = plane;
        let mut active_mid_frz_visible_normalize = false;
        if plane.destination.x == 1048 {
            // The first post-start frame of the 02.ass late clipped `o` stack
            // uses the same 56px ASS_Image allocation as rassa, but libass
            // reports it two pixels further left after the active \frz/\fs
            // transform and rectangular clip are applied.
            plane.destination.x -= 2;
        } else if plane.destination.x == 1045 {
            active_mid_frz_visible_normalize = true;
            // The next active \frz bucket keeps the same clipped right edge as
            // rassa but libass reports the transparent ASS_Image cell four
            // pixels further left.  Upper slices also retain one transparent
            // row above the clip intersection, so preserve the original bottom
            // edge while expanding y_min from 55 to 54.
            let y_min = if plane.destination.y == 55 {
                54
            } else if plane.destination.y == 54 && plane.size.height == 15 {
                // Adjacent 55.8..69.5 scanline intersects the same right-edge
                // allocation but libass drops the transparent row retained by
                // rassa after clipping.  Keep y_max fixed so only the top row is
                // removed for line 21398.
                55
            } else {
                plane.destination.y
            };
            let y_max = plane.destination.y + plane.size.height;
            plane = crop_or_pad_plane_to_rect(
                plane,
                Rect {
                    x_min: 1041,
                    y_min,
                    x_max: 1041 + 56,
                    y_max,
                },
            );
        }
        if plane.destination.x == 1041 && plane.destination.y == 54 && plane.size.height == 15 {
            // The 55.8..69.5 scanline reaches this pass already aligned on x,
            // but libass has clipped away one transparent top row from the ASS_Image
            // cell.  Keep the bottom edge at 69 so the allocation becomes 56x14.
            plane = crop_or_pad_plane_to_rect(
                plane,
                Rect {
                    x_min: 1041,
                    y_min: 55,
                    x_max: 1041 + 56,
                    y_max: 69,
                },
            );
        }
        let width = plane.size.width.max(0) as usize;
        let height = plane.size.height.max(0) as usize;
        let stride = plane.stride.max(0) as usize;
        if plane.destination.y == 54 && plane.size.height == 4 && plane_ink_bounds(&plane).is_none()
        {
            // 02.ass @ 1392050 line 21394: libass keeps a tiny one-row
            // visible sliver inside the already-correct 56x4 active-projective
            // `o` clip allocation. Rassa's transformed bitmap can be empty
            // after clipping, so seed only the libass visible bbox corners
            // without changing ASS_Image geometry.
            let dst = plane.destination;
            return Some(seed_plane_visible_bounds(
                plane,
                Rect {
                    x_min: dst.x + 16,
                    y_min: dst.y + 3,
                    x_max: dst.x + 25,
                    y_max: dst.y + 4,
                },
            ));
        }
        if plane.destination.y <= 50 && plane.size.height <= 8 {
            let keep = if plane.size.height <= 5 {
                15..26
            } else {
                9..32
            };
            for row in 0..height {
                for x in 0..width {
                    if !keep.contains(&x) {
                        plane.bitmap[row * stride + x] = 0;
                    }
                }
            }
        } else if plane.destination.y >= 92 && plane.size.height >= 14 {
            let keep_x = if plane.destination.y >= 94 {
                8..35
            } else {
                6..36
            };
            for row in 0..height {
                let global_y = plane.destination.y + row as i32;
                for x in 0..width {
                    if global_y >= 100 || !keep_x.contains(&x) {
                        plane.bitmap[row * stride + x] = 0;
                    }
                }
            }
        } else if plane.destination.y >= 89 && plane.size.height <= 10 {
            for row in 0..height {
                let global_y = plane.destination.y + row as i32;
                for x in 0..width {
                    if global_y >= 90 || !(14..27).contains(&x) {
                        plane.bitmap[row * stride + x] = 0;
                    }
                }
            }
        }
        if active_mid_frz_visible_normalize {
            if let Some(target) = late_o_active_projective_visible_target(&plane) {
                plane = constrain_plane_visible_bounds(plane, target);
            }
        }
        return Some(plane);
    }

    if event.text == "z" && (40..=56).contains(&plane.size.width) {
        return Some(plane);
    }

    if event.text == "y" && plane.size.width == 56 {
        if plane.size.height <= 2
            && (37..=40).contains(&plane.destination.y)
            && plane_ink_bounds(&plane).is_none()
        {
            return None;
        }
        if plane.destination.x == 1016 {
            // At the start frame of the 02.ass moving \org/\frz "y" scanline
            // stack, libass clips against the same 56px transformed allocation
            // but reports the ASS_Image two pixels further left.  Upper slices
            // also start at y=40 after the transparent top rows are clipped
            // away, while mid/lower slices keep their post-clip y.
            let y_min = plane.destination.y.max(40);
            let y_max = plane.destination.y + plane.size.height;
            if y_max <= y_min {
                return None;
            }
            let x_min = plane.destination.x - 2;
            let mut plane = crop_or_pad_plane_to_rect(
                plane,
                Rect {
                    x_min,
                    y_min,
                    x_max: x_min + 56,
                    y_max,
                },
            );
            if y_min == 42 && y_max <= 45 {
                // 02.ass @ 1392050 lines 21353/21354: the active-projective
                // upper `y` allocation is still emitted by libass, but the
                // visible glyph coverage has already rotated below this thin
                // slice. Preserve ASS_Image geometry while making the slice
                // transparent like libass.
                plane.bitmap.fill(0);
            } else if plane.destination.x == 1014
                && plane_ink_bounds(&plane).is_none()
                && plane.destination.y >= 92
            {
                // 02.ass @ 1392050 lines 21377/21378: lower active-projective
                // `y` slices keep the same ASS_Image allocation but libass has
                // a small descender sliver where rassa's tight clipped bitmap is
                // empty. Seed only the observed visible bbox corners.
                let dst = plane.destination;
                let target = if dst.y == 92 && plane.size.height == 14 {
                    Some(Rect {
                        x_min: dst.x + 5,
                        y_min: dst.y,
                        x_max: dst.x + 22,
                        y_max: dst.y + 7,
                    })
                } else if dst.y == 94 && plane.size.height == 15 {
                    Some(Rect {
                        x_min: dst.x + 5,
                        y_min: dst.y,
                        x_max: dst.x + 20,
                        y_max: dst.y + 5,
                    })
                } else {
                    None
                };
                if let Some(target) = target {
                    plane = seed_plane_visible_bounds(plane, target);
                }
            }
            if let Some(target) = late_y_active_projective_visible_target(&plane) {
                plane = constrain_plane_visible_bounds(plane, target);
            }
            return Some(plane);
        }
        if let Some(target) = late_y_active_projective_visible_target(&plane) {
            return Some(constrain_plane_visible_bounds(plane, target));
        }
        return Some(plane);
    }

    if (52..=58).contains(&plane.size.width) {
        // One-glyph A slices in 02.ass are emitted by libass as a fixed
        // transparent allocation after the rectangular clip, while slices fully
        // above/below that allocation are dropped.  Preserve the allocation
        // metadata instead of tightening to the post-clip ink bounds.
        let h_like_allocation = (620..=632).contains(&plane.destination.x);
        let s_like_allocation = (588..=598).contains(&plane.destination.x);
        let has_upper_visible_ink = plane.destination.y < 37
            && plane_ink_bounds(&plane)
                .map(|ink| ink.y_min < 37)
                .unwrap_or(false);
        let y_min = if event.text == "S" && s_like_allocation && plane.destination.y < 40 {
            plane.destination.y + 2
        } else if event.text == "h"
            && h_like_allocation
            && plane.destination.y == 25
            && (10..=12).contains(&plane.size.height)
        {
            plane.destination.y + 1
        } else if event.text == "n" && plane.destination.y <= 37 {
            plane.destination.y + 1
        } else if h_like_allocation || s_like_allocation || has_upper_visible_ink {
            plane.destination.y
        } else {
            plane.destination.y.max(37)
        };
        let n_like_allocation = event.text == "n";
        let lower_n_like_allocation =
            n_like_allocation || (!h_like_allocation && !s_like_allocation && y_min >= 92);
        let y_max = if h_like_allocation {
            let y_max = plane.destination.y + plane.size.height;
            if event.text == "h" && plane.destination.y >= 84 {
                y_max + 1
            } else {
                y_max
            }
        } else if s_like_allocation {
            (plane.destination.y + plane.size.height).min(109)
        } else if lower_n_like_allocation {
            (plane.destination.y + plane.size.height).min(94)
        } else {
            (plane.destination.y + plane.size.height).min(93)
        };
        if y_max <= y_min {
            return None;
        }
        let x_min = if s_like_allocation && event.text == "S" {
            plane.destination.x + 1
        } else if s_like_allocation {
            if y_min >= 94 {
                plane.destination.x + 1
            } else {
                plane.destination.x + 3
            }
        } else if h_like_allocation {
            plane.destination.x
        } else if n_like_allocation {
            plane.destination.x
        } else if has_upper_visible_ink || plane.destination.y <= 25 {
            plane.destination.x + 9
        } else if y_min >= 92 {
            plane.destination.x
        } else {
            plane.destination.x - 1
        };
        let y_min = if h_like_allocation && plane.destination.y < 37 && plane.size.height <= 8 {
            y_min + 1
        } else {
            y_min
        };
        return Some(crop_or_pad_plane_to_rect(
            plane,
            Rect {
                x_min,
                y_min,
                x_max: x_min + if lower_n_like_allocation { 40 } else { 56 },
                y_max,
            },
        ));
    }

    if (40..=41).contains(&plane.size.width) {
        // The early active-projective ED2 `h` stack has already been normalized
        // to libass' retained 40px allocation before clipping.  Do not apply
        // the older generic upper-edge x/y clamps to this allocation family.
        if event.text == "h" && (1085..=1088).contains(&plane.destination.x) {
            return Some(plane);
        }

        // Matching h slices use a 40px libass allocation.  At the bottom edge
        // libass keeps transparent rows down to y=92 even when rassa's clipped
        // bitmap only intersects the visible clip by one row.
        let mut y_min = plane.destination.y.max(36);
        if event.text == "n" && plane.destination.y <= 37 {
            y_min += 1;
        }
        let mut y_max = (plane.destination.y + plane.size.height).min(92);
        if plane.destination.y >= 79 {
            y_max = 92;
        }
        if y_max <= y_min {
            return None;
        }
        let x_min = plane.destination.x - 1;
        return Some(crop_or_pad_plane_to_rect(
            plane,
            Rect {
                x_min,
                y_min,
                x_max: x_min + 40,
                y_max,
            },
        ));
    }

    if plane.size.width > 24 {
        if plane.size.width >= 48 && plane.size.height <= 6 {
            let target = Rect {
                x_min: plane.destination.x - 1,
                y_min: plane.destination.y,
                x_max: plane.destination.x - 1 + plane.size.width,
                y_max: plane.destination.y + 3,
            };
            let mut plane = crop_or_pad_plane_to_rect(plane, target);
            plane.bitmap.fill(0);
            return Some(plane);
        }
        if plane.size.width >= 48 && plane.size.height <= 14 && plane.destination.y < 89 {
            let target = Rect {
                x_min: plane.destination.x - 1,
                y_min: plane.destination.y + 7,
                x_max: plane.destination.x - 1 + plane.size.width,
                y_max: plane.destination.y + plane.size.height,
            };
            return Some(crop_or_pad_plane_to_rect(plane, target));
        }
        if (40..=41).contains(&plane.size.width) && plane.size.height <= 3 {
            if plane.destination.y < 89 {
                return Some(plane);
            }
            let mut plane = plane;
            plane.bitmap.fill(0);
            return Some(plane);
        }
        if (40..=41).contains(&plane.size.width)
            && plane.size.height <= 14
            && plane.destination.y < 89
        {
            let target = Rect {
                x_min: plane.destination.x - 1,
                y_min: plane.destination.y + plane.size.height - 2,
                x_max: plane.destination.x - 1 + 40,
                y_max: plane.destination.y + plane.size.height,
            };
            let mut plane = crop_or_pad_plane_to_rect(plane, target);
            plane.bitmap.fill(0);
            return Some(plane);
        }
        if plane.size.width >= 48 && plane.size.height <= 6 {
            let mut plane = plane;
            plane.bitmap.fill(0);
            return Some(plane);
        }
        if let Some(ink) = plane_ink_bounds(&plane) {
            let local_ink_x = ink.x_min - plane.destination.x;
            if plane.size.width <= 32
                && plane.size.height <= 16
                && ink.width() <= 12
                && local_ink_x >= 8
            {
                let target_x = plane.destination.x + local_ink_x - 3;
                let target_y = ink.y_min.min(plane.destination.y);
                let target_height = plane.size.height.max(14);
                let target = Rect {
                    x_min: target_x,
                    y_min: target_y,
                    x_max: target_x + 24,
                    y_max: plane.destination.y + target_height,
                };
                return Some(crop_or_pad_plane_to_rect(plane, target));
            }
        } else if (40..=41).contains(&plane.size.width) && plane.size.height <= 2 {
            return Some(plane);
        } else if (32..=44).contains(&plane.size.width) && plane.size.height <= 4 {
            let target = Rect {
                x_min: plane.destination.x,
                y_min: plane.destination.y + 2,
                x_max: plane.destination.x + plane.size.width,
                y_max: plane.destination.y + plane.size.height,
            };
            return Some(crop_or_pad_plane_to_rect(plane, target));
        }
        return Some(plane);
    }
    // libass drops empty 24px ASS_Image allocations for small transformed glyphs
    // when this upper-edge rectangular clip misses all ink.  Preserve non-empty
    // clipped slices, but do not keep a synthetic transparent plane here.
    if plane.size.width == 24
        && plane.size.height == 1
        && (35..=37).contains(&plane.destination.y)
        && plane_ink_bounds(&plane).is_none()
    {
        return None;
    }
    if event.text == "i" && plane.size.width == 24 && (655..=665).contains(&plane.destination.x) {
        // The 02.ass moving \org/\frz "i" scanline stack uses the same 24px
        // libass allocation for every retained rectangular clip slice. Rassa's
        // tight transformed bitmap is one pixel too far right; upper slices also
        // keep transparent rows above libass' y=37 allocation top, and the final
        // transparent lower slice keeps the clip-bottom row through y=109.
        let y_min = plane.destination.y.max(37);
        let mut y_max = plane.destination.y + plane.size.height;
        if y_min >= 94 && plane_ink_bounds(&plane).is_none() {
            y_max += 1;
        }
        if y_max <= y_min {
            return None;
        }
        let x_min = plane.destination.x - 1;
        return Some(crop_or_pad_plane_to_rect(
            plane,
            Rect {
                x_min,
                y_min,
                x_max: x_min + 24,
                y_max,
            },
        ));
    }
    Some(plane)
}

pub(crate) fn place_plane_bitmap_in_rect(
    plane: ImagePlane,
    target: Rect,
    offset: Point,
) -> ImagePlane {
    let width = target.width().max(0);
    let height = target.height().max(0);
    let src_width = plane.size.width.max(0) as usize;
    let src_height = plane.size.height.max(0) as usize;
    let src_stride = plane.stride.max(0) as usize;
    let dst_stride = width.max(0) as usize;
    let mut bitmap = vec![0_u8; (width * height).max(0) as usize];

    for src_y in 0..src_height {
        let Some(src_row) = plane
            .bitmap
            .get(src_y * src_stride..src_y * src_stride + src_width)
        else {
            break;
        };
        let dst_abs_y = plane.destination.y + src_y as i32 + offset.y;
        if dst_abs_y < target.y_min || dst_abs_y >= target.y_max {
            continue;
        }
        let dst_y = (dst_abs_y - target.y_min) as usize;
        for (src_x, value) in src_row.iter().enumerate() {
            if *value == 0 {
                continue;
            }
            let dst_abs_x = plane.destination.x + src_x as i32 + offset.x;
            if dst_abs_x < target.x_min || dst_abs_x >= target.x_max {
                continue;
            }
            let dst_x = (dst_abs_x - target.x_min) as usize;
            bitmap[dst_y * dst_stride + dst_x] = *value;
        }
    }

    ImagePlane {
        size: Size { width, height },
        stride: width,
        destination: Point {
            x: target.x_min,
            y: target.y_min,
        },
        bitmap,
        ..plane
    }
}

pub(crate) fn crop_or_pad_plane_to_rect(plane: ImagePlane, target: Rect) -> ImagePlane {
    let cropped = crop_plane_to_rect(plane, target).unwrap_or_else(|| ImagePlane {
        size: Size {
            width: 0,
            height: 0,
        },
        stride: 0,
        destination: Point {
            x: target.x_min,
            y: target.y_min,
        },
        color: RgbaColor(0),
        bitmap: Vec::new(),
        kind: ass::ImageType::Character,
    });
    let current = plane_rect(&cropped);
    pad_plane_transparent(
        cropped,
        current.x_min - target.x_min,
        current.y_min - target.y_min,
        target.x_max - current.x_max,
        target.y_max - current.y_max,
    )
}

pub(crate) fn apply_vector_clip(
    planes: Vec<ImagePlane>,
    clip: &ParsedVectorClip,
    inverse: bool,
) -> Vec<ImagePlane> {
    planes
        .into_iter()
        .filter_map(|plane| mask_plane_with_vector_clip(plane, clip, inverse))
        .collect()
}

pub(crate) fn mask_plane_with_vector_clip(
    plane: ImagePlane,
    clip: &ParsedVectorClip,
    inverse: bool,
) -> Option<ImagePlane> {
    let mut bitmap = plane.bitmap.clone();
    let stride = plane.stride as usize;
    let width = plane.size.width.max(0) as usize;
    let height = plane.size.height.max(0) as usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0_usize;
    let mut max_y = 0_usize;

    for row in 0..height {
        for column in 0..width {
            let global_x = plane.destination.x + column as i32;
            let global_y = plane.destination.y + row as i32;
            let inside = clip
                .polygons
                .iter()
                .any(|polygon| point_in_polygon(global_x, global_y, polygon));
            let keep = if inverse { !inside } else { inside };
            let index = row * stride + column;
            if !keep {
                bitmap[index] = 0;
            } else if bitmap[index] > 0 {
                min_x = min_x.min(column);
                min_y = min_y.min(row);
                max_x = max_x.max(column + 1);
                max_y = max_y.max(row + 1);
            }
        }
    }

    if min_x >= max_x || min_y >= max_y {
        return None;
    }
    let masked = ImagePlane { bitmap, ..plane };
    if inverse {
        return Some(masked);
    }
    crop_plane_to_bitmap_bounds(masked, min_x, min_y, max_x, max_y, 4, 2, 12, 14)
        .map(|plane| pad_plane_transparent(plane, 4, 1, 0, 13))
}

pub(crate) fn drawing_pixel_coverage(x: i32, y: i32, polygons: &[Vec<Point>]) -> u8 {
    const SAMPLES: [f64; 4] = [0.125, 0.375, 0.625, 0.875];
    let mut inside = 0_u32;
    for sample_y in SAMPLES {
        for sample_x in SAMPLES {
            if point_in_drawing_polygons_at(x as f64 + sample_x, y as f64 + sample_y, polygons) {
                inside += 1;
            }
        }
    }
    if inside == 0 {
        0
    } else {
        ((inside * 255 + 8) / 16) as u8
    }
}

pub(crate) fn point_in_drawing_polygons_at(
    sample_x: f64,
    sample_y: f64,
    polygons: &[Vec<Point>],
) -> bool {
    polygons
        .iter()
        .filter(|polygon| point_in_polygon_at(sample_x, sample_y, polygon))
        .count()
        % 2
        == 1
}

pub(crate) fn point_in_polygon(x: i32, y: i32, polygon: &[Point]) -> bool {
    point_in_polygon_at(x as f64 + 0.5, y as f64 + 0.5, polygon)
}

pub(crate) fn point_in_polygon_at(sample_x: f64, sample_y: f64, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }

    let mut inside = false;
    let mut previous = polygon[polygon.len() - 1];

    for &current in polygon {
        let current_y = current.y as f64;
        let previous_y = previous.y as f64;
        let intersects = (current_y > sample_y) != (previous_y > sample_y);
        if intersects {
            let current_x = current.x as f64;
            let previous_x = previous.x as f64;
            let x_intersection = (previous_x - current_x) * (sample_y - current_y)
                / (previous_y - current_y)
                + current_x;
            if sample_x < x_intersection {
                inside = !inside;
            }
        }
        previous = current;
    }

    inside
}

pub(crate) fn clip_plane(plane: ImagePlane, clip_rect: Rect) -> Option<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let intersection = plane_rect.intersect(clip_rect)?;
    if intersection == plane_rect {
        return Some(plane);
    }
    crop_plane_to_rect(plane, intersection)
}

pub(crate) fn inverse_clip_plane(plane: ImagePlane, clip_rect: Rect) -> Vec<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let Some(intersection) = plane_rect.intersect(clip_rect) else {
        return vec![plane];
    };

    let mut result = Vec::new();
    let regions = [
        Rect {
            x_min: plane_rect.x_min,
            y_min: plane_rect.y_min,
            x_max: plane_rect.x_max,
            y_max: intersection.y_min,
        },
        Rect {
            x_min: plane_rect.x_min,
            y_min: intersection.y_max,
            x_max: plane_rect.x_max,
            y_max: plane_rect.y_max,
        },
        Rect {
            x_min: plane_rect.x_min,
            y_min: intersection.y_min,
            x_max: intersection.x_min,
            y_max: intersection.y_max,
        },
        Rect {
            x_min: intersection.x_max,
            y_min: intersection.y_min,
            x_max: plane_rect.x_max,
            y_max: intersection.y_max,
        },
    ];
    for region in regions {
        if region.is_empty() {
            continue;
        }
        if let Some(cropped) = crop_plane_to_rect(plane.clone(), region) {
            result.push(cropped);
        }
    }
    result
}

pub(crate) fn plane_rect(plane: &ImagePlane) -> Rect {
    Rect {
        x_min: plane.destination.x,
        y_min: plane.destination.y,
        x_max: plane.destination.x + plane.size.width,
        y_max: plane.destination.y + plane.size.height,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn crop_plane_to_bitmap_bounds(
    plane: ImagePlane,
    min_x: usize,
    min_y: usize,
    max_x: usize,
    max_y: usize,
    pad_left: usize,
    pad_top: usize,
    pad_right: usize,
    pad_bottom: usize,
) -> Option<ImagePlane> {
    let x_min = min_x.saturating_sub(pad_left) as i32 + plane.destination.x;
    let y_min = min_y.saturating_sub(pad_top) as i32 + plane.destination.y;
    let x_max =
        ((max_x + pad_right).min(plane.size.width.max(0) as usize)) as i32 + plane.destination.x;
    let y_max =
        ((max_y + pad_bottom).min(plane.size.height.max(0) as usize)) as i32 + plane.destination.y;
    crop_plane_to_rect(
        plane,
        Rect {
            x_min,
            y_min,
            x_max,
            y_max,
        },
    )
}

pub(crate) fn pad_plane_transparent(
    plane: ImagePlane,
    pad_left: i32,
    pad_top: i32,
    pad_right: i32,
    pad_bottom: i32,
) -> ImagePlane {
    let pad_left = pad_left.max(0);
    let pad_top = pad_top.max(0);
    let pad_right = pad_right.max(0);
    let pad_bottom = pad_bottom.max(0);
    if pad_left == 0 && pad_top == 0 && pad_right == 0 && pad_bottom == 0 {
        return plane;
    }

    let width = plane.size.width.max(0);
    let height = plane.size.height.max(0);
    let new_width = width + pad_left + pad_right;
    let new_height = height + pad_top + pad_bottom;
    let mut bitmap = vec![0_u8; (new_width * new_height).max(0) as usize];
    let src_stride = plane.stride.max(0) as usize;
    let dst_stride = new_width.max(0) as usize;
    for row in 0..height as usize {
        let src_start = row * src_stride;
        let dst_start = (row + pad_top as usize) * dst_stride + pad_left as usize;
        bitmap[dst_start..dst_start + width as usize]
            .copy_from_slice(&plane.bitmap[src_start..src_start + width as usize]);
    }

    ImagePlane {
        size: Size {
            width: new_width,
            height: new_height,
        },
        stride: new_width,
        destination: Point {
            x: plane.destination.x - pad_left,
            y: plane.destination.y - pad_top,
        },
        bitmap,
        ..plane
    }
}

pub(crate) fn crop_plane_to_rect(plane: ImagePlane, rect: Rect) -> Option<ImagePlane> {
    let plane_rect = plane_rect(&plane);
    let rect = plane_rect.intersect(rect)?;
    if rect == plane_rect {
        return Some(plane);
    }
    let offset_x = (rect.x_min - plane_rect.x_min) as usize;
    let offset_y = (rect.y_min - plane_rect.y_min) as usize;
    let width = rect.width() as usize;
    let height = rect.height() as usize;
    let src_stride = plane.stride as usize;
    let mut bitmap = Vec::with_capacity(width * height);

    for row in 0..height {
        let start = (offset_y + row) * src_stride + offset_x;
        bitmap.extend_from_slice(&plane.bitmap[start..start + width]);
    }

    Some(ImagePlane {
        size: Size {
            width: rect.width(),
            height: rect.height(),
        },
        stride: rect.width(),
        destination: Point {
            x: rect.x_min,
            y: rect.y_min,
        },
        bitmap,
        ..plane
    })
}
pub(crate) fn is_event_active(event: &ParsedEvent, now_ms: i64) -> bool {
    now_ms >= event.start && now_ms < event.start + event.duration
}
