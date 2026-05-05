# Ongoing Work

Append new session entries at the top of this file.

## 2026-05-04

### Strict Broad Exact-Pixel Continuation (shadow/karaoke geometry pass)
- Continued the strict broad exact-pixel pass without touching CoreText/DirectWrite and without editing upstream references/comparator behavior to hide mismatches.
- Retained a renderer-side fix where text shadow planes use the outline mask when a border is present, matching libass `ASS_Image` behavior for bordered shadow runs. This moved `broad_overrides @ 500 ms` from severe under-rendering (`actual_nontransparent=5104`) to near-target coverage (`actual_nontransparent=5898`, `target_nontransparent=5903`, bbox `Some((49,74,269,112))` vs `Some((48,74,269,112))`).
- Added a karaoke-line ascender correction so `broad_karaoke @ 500 ms` high-res plane geometry now matches the libass probe exactly (`Outline1 x=623 y=615 w=368 h=240`, `Outline2 x=1007 y=663 w=368 h=192`, `Character1 x=631 y=623 w=352 h=224`, `Character2 x=1015 y=671 w=352 h=176`, `Character3 x=1399 y=623 w=544 h=224`). The remaining karaoke mismatch is coverage/composition intensity, not gross placement.
- Rejected and reverted experiments that had no benefit or caused regressions: global ascender offsets (`+2/-2`), karaoke no-shared-ascender, broad `254->255` mask normalization, and raster correction-table `254->255` tweaks.
- Verified retained changes: `cargo fmt --all`, `cargo test -p rassa-render -q`, `cargo test -p rassa-raster -q`, and strict `sub2` all pass (`/tmp/sub2_retained.log`). Strict broad remains red with 12 frames (`/tmp/strict_retained.log`): `broad_box`, `broad_static`, `broad_karaoke`, and `broad_overrides` still require exact coverage/composition fixes.

### Strict Broad Exact-Pixel Continuation (libass geometry probe pass)
- Continued strict broad exact-pixel work without touching CoreText/DirectWrite and without changing upstream reference PNGs/comparator behavior to hide renderer mismatches.
- Added a temporary `/tmp/libass_probe.c` oracle program against the checked-out libass build (test-only, not production) to print upstream `ASS_Image` geometry for the broad fixtures. It showed `broad_static` geometry already matches (`x=671 y=591 w=1264 h=368`) and narrowed that fixture to raster mask coverage (`rassa lit_pixels=109200` vs upstream `108720`).
- The same probe showed `broad_box` has a separate `BorderStyle=3` text-plane geometry issue: upstream character image is `x=1070 y=630 w=454 h=208`, while rassa had emitted a much wider/left-shifted character plane. A focused renderer change now brings the `broad_box` character-plane geometry to `x=1070 y=630 w=454 h=208`; the remaining `broad_box` output still has exact-pixel edge/composition deltas (`actual_nontransparent=3878`, `target_nontransparent=3878`, matching bbox, alpha `243329928` vs `243330809`).
- Removed the temporary `RASSA_DUMP_COMPARE_RAW`/`.rgba16le` diagnostic hook from `rassa-test`; `RASSA_DUMP_COMPARE` PNG dumping remains available.
- Re-ran the strict broad gate at `/tmp/strict_resume.log`: still 12 failing frames. Focused follow-up split the remaining roots: `broad_static` is rectilinear raster AA/coverage phase (turning rectilinear AA off proves the base mask is too small, current correction over-spreads weak fringe); `broad_karaoke`/`broad_overrides` are outline anchor/coverage interactions where a fill-ascender outline anchor improves karaoke but regresses scaled overrides, so that heuristic was not retained; `broad_box` edge/side alpha sweeps (`edge_alpha` and `side_edge_alpha`) cannot hit exact target with a simple constant change and were reverted. A run-level outline/shadow plane-combination cleanup was retained because it matches the intended libass-style image-plane grouping better and keeps tests green, but it does not change the remaining broad pixel metrics.
- Verified after retained changes: `cargo fmt --all`, `cargo test -p rassa-render -q`, `cargo test -p rassa-raster -q`, and `cargo test -p rassa-test upstream_compare_reference_sub2_is_pixel_perfect -- --ignored --nocapture` pass. Strict broad is still red with 12 frames; latest strict logs `/tmp/strict_current.log`, `/tmp/strict_resume.log`, and `/tmp/strict_after_combined_run.log`.

### Strict Broad Exact-Pixel Continuation (box edge/raw diagnostics pass)
- Continued the strict broad exact-pixel pass without touching CoreText/DirectWrite and kept `upstream_compare_reference_sub2_is_pixel_perfect` as the regression guard.
- Added `RASSA_BROAD_FILTER` support to the broad-corpus harness so individual fixture root causes can be tested without rerunning every broad frame; used it for `broad_box` and `broad_static` focused loops.
- Improved `BorderStyle=3` opaque-box edge composition by expanding the box plane with a narrow antialiased side fringe while preserving the already-correct bbox and nontransparent count. Current `broad_box @ 500 ms`: `actual_nontransparent=3878`, `target_nontransparent=3878`, bbox `Some((126,64,197,119))` on both sides, alpha now `243329928` vs `243330809` (down from a much larger alpha gap earlier), but exact pixels still differ.
- Added temporary raw RGBA16 diagnostics during the investigation, then removed those debug-only hooks before leaving the tree. Raw analysis showed `broad_static` is a rectilinear glyph edge-coverage/phase issue (mostly inverse-alpha `64→32` edge pixels plus 59 fringe pixels), not a placement issue.
- Tried disabling rectilinear boundary antialiasing and changing its deltas as root-cause tests; those worsened/failed to improve `broad_static`, so the production raster path was restored.
- Current verified focused checks pass: `cargo fmt --all`, `cargo test -p rassa-render -q`, `cargo test -p rassa-raster -q`, and `cargo test -p rassa-test upstream_compare_reference_sub2_is_pixel_perfect -- --ignored --nocapture` (`/tmp/sub2_after_cleanup.log`). Strict broad remains red with 12 frames (`/tmp/strict_after_cleanup.log`): `broad_box`, `broad_static`, `broad_karaoke`, and `broad_overrides` at the broad timestamps.

### Strict Broad Exact-Pixel Continuation (scaled outline radius pass)
- Continued strict broad exact-pixel work without touching CoreText/DirectWrite.
- Added a raster regression ensuring `rassa-raster` preserves shaped glyph advances even when the glyph bitmap cache is reused; this prevents cached FreeType advance values from overriding HarfBuzz/layout advances.
- Changed scaled outline generation to scale the bitmap-dilation outline radius for `\\fscx/\\fscy` runs using the combined x/y style scale. This made the largest remaining fixture (`broad_overrides`) substantially closer: `broad_overrides @ 500 ms` moved to `actual_nontransparent=5884` vs `target_nontransparent=5903`, `actual_alpha_sum=350908536` vs `target_alpha_sum=352761459`, `actual_bbox=Some((49,75,269,112))` vs `target_bbox=Some((48,74,269,112))`.
- Kept the previous exact-bbox improvement for `broad_box @ 500 ms` (`actual_nontransparent=3878`, `target_nontransparent=3878`, bbox `Some((126,64,197,119))` on both sides), but its edge/composition alpha still differs.
- Tried additional vertical positioning and outline-radius overshoot experiments; reverted the ones that worsened bbox/alpha or broke compilation. Current verified focused checks pass: `cargo fmt --all`, `cargo test -p rassa-render -q`, `cargo test -p rassa-raster -q`, and strict `sub2` (`/tmp/sub2_current_final.log`).
- Strict broad remains red with 12 exact-pixel mismatching frames; current log: `/tmp/rassa_strict_current_final.log`. Remaining root causes are now mostly subpixel/coverage exactness: scaled outline edge coverage/top-left extent, karaoke outline/fill split edge placement, static glyph edge coverage, and `BorderStyle=3` antialiased rectangle/composition intensity.

### Strict Broad Exact-Pixel Continuation (current)
- Continued the strict broad exact-pixel pass without touching CoreText/DirectWrite.
- Verified the latest `BorderStyle=3` y-edge experiment; strict broad remained red. Replaced the hard full-255 top/bottom rows of opaque box planes with a thin antialiased edge strip and adjusted the box side coverage. This brought `broad_box @ 500 ms` to matching nontransparent pixel count and bbox (`actual_nontransparent=3878`, `target_nontransparent=3878`, `actual_bbox=Some((126,64,197,119))`, `target_bbox=Some((126,64,197,119))`), but exact alpha still differs (`actual_alpha_sum=242836145`, `target_alpha_sum=243330809`).
- Tried switching normal glyph fill to FreeType's rendered bitmap as a root-cause test. It regressed the broad static bbox (`actual=Some((84,74,240,118))` vs target `Some((83,73,241,119))`), so that experiment was reverted to keep the existing custom raster bbox parity.
- Switched text outlines from bitmap dilation toward the existing FreeType stroker path for non-drawing text runs, with dilation fallback. This improved `broad_karaoke @ 500 ms` coverage (`actual_nontransparent=2708` vs `target_nontransparent=2755`, `actual_alpha_sum=149245915` vs `149217515`) while preserving focused renderer tests and strict `sub2`.
- Replaced nearest-neighbor post-raster glyph scaling with bilinear resampling for scaled glyph bitmaps. This slightly improved the scaled override bbox (`broad_overrides @ 500 ms` moved from top 76 to 75, target 74) but strict broad remains red with 12 mismatching frames.
- Current verified focused checks: `cargo fmt --all`, `cargo test -p rassa-render -q`, and `cargo test -p rassa-test upstream_compare_reference_sub2_is_pixel_perfect -- --ignored --nocapture` pass. Strict broad log: `/tmp/rassa_strict_bilinear.log`.

### Strict Broad Exact-Pixel Attempt
- Continued the strict broad exact-pixel pass without touching CoreText/DirectWrite.
- Added optional compare-frame debug dumping for broad diagnostics via `RASSA_DUMP_COMPARE=/tmp/path`, writing actual/target PNG pairs for mismatching frames.
- Normalized fully transparent downsampled pixels to transparent black in the broad compare compositor; this removes visually irrelevant RGB residue from exact comparisons without hiding any nontransparent alpha/color differences.
- Re-tested an experimental shared-ascender placement change for positioned/outlined text, but reverted it because it regressed `broad_static` vertical placement while `sub2` must remain green.
- Current focused verification remains green: `cargo fmt --all`, `cargo test -p rassa-render -q`, and `cargo test -p rassa-test upstream_compare_reference_sub2_is_pixel_perfect -- --ignored --nocapture`.
- Strict broad is still red with 12 exact-pixel mismatching frames (`/tmp/rassa_strict_current.log`). Current root cause is subpixel/fixed-point coverage rather than gross placement: hard-edged `BorderStyle=3` boxes, nearest-neighbor post-raster scaling for `\\fscx/\\fscy`, and integer-rounded glyph/clip placement still diverge from upstream edge coverage.

### Strict Broad Pixel-Diff Continuation Follow-up
- Continued the strict broad-corpus root-cause pass without touching CoreText/DirectWrite.
- Added a positioned-text vertical correction for explicit `\\pos`/`\\move` events that preserves the existing strict `sub2-153000` fixture while bringing broad positioned text closer to libass line metrics:
  - `broad_static @ 500 ms` now has matching bbox `actual=Some((83,73,241,119))` vs `target=Some((83,73,241,119))` and near-identical alpha sum (`106953120` vs `106970208`); remaining mismatch is raster edge coverage.
  - `broad_overrides @ 500 ms` improved enough that it remains the main outline/shadow/raster-intensity gap rather than a gross placement failure.
  - `broad_karaoke @ 500 ms` is now down to roughly a one-row top-edge bbox delta (`actual=Some((77,77,242,107))` vs `target=Some((77,76,242,107))`) plus karaoke/outline intensity deltas.
- Tightened `BorderStyle=3` line box right-edge geometry so `broad_box @ 500 ms` now matches the upstream bbox horizontally and vertically: `actual=Some((126,64,197,119))` vs `target=Some((126,64,197,119))`; remaining box delta is edge alpha/composition intensity.
- Re-ran focused renderer tests, the strict `sub2-153000` ignored fixture, and the normal workspace test suite after these changes; all stayed green. Strict broad mode remains red with 12 exact-pixel mismatching frames.

### Strict Broad Pixel-Diff Continuation
- Re-ran the strict broad corpus gate and grouped the current failures by root cause:
  - all 12 broad frames still differ in exact pixel mode;
  - `broad_static` is now a small glyph/raster edge mismatch with matching horizontal bbox and about a one-pixel vertical extent delta;
  - `broad_karaoke` is primarily outline/raster row-distribution and karaoke split intensity, not timing order;
  - `broad_overrides` is dominated by outline/shadow/raster intensity for scaled/bordered inline overrides;
  - `broad_box` exposed remaining `BorderStyle=3` opaque-box geometry differences.
- Tightened `BorderStyle=3` opaque-box generation to use line-level ASS box rectangles instead of deriving the box solely from visible character glyph bounds. This moves the broad box fixture much closer to upstream: the 500 ms bbox improved to approximately `actual=Some((126,64,196,119))` vs `target=Some((126,64,197,119))`.
- Kept the existing strict upstream `sub2-153000` pixel fixture green by preserving the prior line-height calibration; an attempted line-height tweak improved some broad rows but regressed that stricter reference and was reverted.

### Verified This Continuation
- `cargo fmt --all` completed successfully.
- `cargo test -p rassa-render -q` passes.
- `cargo test -p rassa-test upstream_compare_reference_sub2_is_pixel_perfect -- --ignored --nocapture` passes.
- Full workspace `cargo test -q` passes.
- Broad corpus report mode still passes and reports 12 mismatching frames; strict broad mode remains the active red gate for further renderer parity work.

### Broad Upstream Pixel-Diff Corpus Pass
- Added a generated upstream-reference corpus under `crates/rassa-test/fixtures/libass/compare/broad/` with 4 ASS fixtures and 12 upstream libass-rendered PNG frames:
  - static centered `\\pos` text
  - inline override/color/scale/border/shadow coverage
  - karaoke timing coverage including `\\kt`/`\\kf`/`\\ko`
  - `BorderStyle=3` opaque-box coverage
- Added ignored `rassa-test` harness `upstream_generated_broad_corpus_pixel_diff_report` that renders every broad-corpus frame through rassa, composites the emitted image planes to RGBA, compares against the upstream PNG, and prints high-signal diagnostics: plane summaries, nontransparent counts, alpha sums, bboxes, row summaries, and first differing pixel.
- The broad harness defaults to report mode so it can stay in `cargo test -- --ignored` without blocking unrelated work; setting `RASSA_STRICT_BROAD_PIXEL_DIFF=1` turns broad-corpus mismatches into a failing strict gate.
- Ran the broad corpus in strict/red form first and confirmed it exposed parity deltas, then fixed the largest geometry issue found: explicit `\\pos`/`\\move` placement now honors ASS alignment anchors horizontally and vertically instead of treating positioned coordinates as top-left line origins.
- After the anchor fix, centered static text aligns horizontally with upstream and the broad corpus shows much smaller placement deltas; remaining mismatches are mainly raster/outline/blur/composition/line-metric precision rather than the original gross `\\pos` anchoring error.

### Verified
- `cargo test -p rassa-test upstream_generated_broad_corpus_pixel_diff_report -- --ignored --nocapture` passes in report mode and currently reports 12 mismatching broad frames.
- The strict mode is intentionally available but not yet green: `RASSA_STRICT_BROAD_PIXEL_DIFF=1 cargo test -p rassa-test upstream_generated_broad_corpus_pixel_diff_report -- --ignored --nocapture` remains the next renderer pixel-parity gate.

### Feature-Completion Pass (excluding CoreText/DirectWrite)
- Audited the remaining ASS/libass parity surface while explicitly keeping CoreText/DirectWrite backend work out of scope.
- Closed parser override gaps aligned with upstream `ass_parse.c` semantics:
  - Added `\kt` karaoke timing reset support so subsequent `\k`/`\kf`/`\ko` spans can start from an absolute centisecond cursor.
  - Aligned inline `\fs` handling with libass-style relative font-size semantics (`+/-` values scale from the current size by tenths) and reset invalid/zero sizes to the base style size.
  - Added `\fsc` reset behavior for paired scale reset back to the base style `ScaleX`/`ScaleY`.
- Added renderer support for style `BorderStyle=3` opaque subtitle boxes: text events now replace outline planes with an opaque box around the rendered character bounds and optionally emit the matching shadow box from `BackColour`.
- Added focused regressions for `\kt`, relative/reset font-size and scale overrides, and opaque-box border rendering.

### Verified
- `cargo fmt --all` completed successfully.
- `cargo test -p rassa-parse parses_ -- --nocapture` passes with the new parser regressions.
- `cargo test -p rassa-render render_frame_emits_opaque_box_for_border_style_3 -- --nocapture` passes.
- Full workspace `cargo test -q` passes.
- Ignored upstream/parity tests pass via `cargo test -- --ignored -q`, including the strict `rassa-test` reference fixtures.
- Release build passes via `cargo build --release -q`.
- Exported public C ABI inventory still reports 50 `ass_*` symbols from `target/release/libass.so`.

### Current Gaps
- CoreText/DirectWrite remain intentionally out of scope for this pass.
- The available automated corpus and strict fixture tests pass, but absolute feature-complete confidence still depends on growing the upstream pixel-diff corpus beyond the current fixtures and continuing exact libass quirk matching for rare edge cases.

## 2026-05-03

### Pixel-Parity Continuation
- Tightened the strict upstream `libass/compare/test/sub2-153000.png` parity harness with richer diagnostics for differing pixels, emitted plane summaries, nontransparent counts, bboxes, and first-diff samples.
- Fixed additional root causes exposed by the strict harness: ASS style `Alignment` values are now normalized through `\an`-style libass alignment bits; renderer default shaping now follows the C API's complex-shaping default; HarfBuzz advances/offsets are scaled from font units by the requested span font size; glyph rasterization now uses HarfBuzz glyph indices on the complex path; `\blur` is now applied to fill planes, not only outline/shadow planes; and large-font centered layout now uses a font-size-derived line height instead of the fixed `40px` placeholder.
- Current strict `sub2-153000` parity status is much closer but still not pixel-perfect: bbox improved from approximately `actual=(163,70,320,127)` vs `target=(38,62,288,137)` to `actual=(38,62,288,138)` vs `target=(38,62,288,137)`. Remaining mismatch is now mainly blur/filter/composition intensity and a one-pixel vertical extent, not gross placement or glyph-selection.

### Pixel-Parity Verification This Pass
- Focused parser alignment regression passed earlier: `cargo test -p rassa-parse normalizes_style_alignment_numbers_to_libass_bits -- --nocapture`.
- Focused strict parity still fails as expected while tracking the remaining blur/composition delta: `cargo test -p rassa-test upstream_compare_reference_sub2_is_pixel_perfect -- --nocapture`.
- Latest diagnostic: `planes=[x=38..288], actual_nontransparent=7675, target_nontransparent=7257, actual_bbox=Some((38, 62, 288, 138)), target_bbox=Some((38, 62, 288, 137)), first_diff=Some((19878, [0,0,771,64764], [0,0,0,65535]))`.

### Added This Session
- Cloned `altqx/rassa` into `~/Work/rassa` and cloned upstream `libass/libass` into `~/Work/rassa/libass` as the local parity reference.
- Fixed the renderer fade-alpha regression by applying computed `\fad`/`\fade` event alpha directly to emitted image-plane colors while preserving color channels.
- Exported the previously internal `ass_flush_events` symbol, completing the current public `libass/libass/ass.h` function-symbol inventory.
- Renamed the `rassa-capi` cdylib output to `libass.so` so release builds produce a library with the expected drop-in linker name.
- Kept Rust tests importing the C API crate through a package alias after the library name change.
- Added drop-in include headers under `include/ass/` from the local upstream reference and added a `pkgconfig/libass.pc` file pointing at `target/release/libass.so` and `include`.
- Added the first automated `libass/compare/test` validation harness in `rassa-test`: it loads upstream `sub1.ass`/`sub2.ass`, reads the bundled reference PNG dimensions directly from IHDR bytes, renders the corresponding frame timestamps, and asserts generated image planes are non-empty, visible, time-varying, and inside the reference frame.
- Closed the newly reproduced rotation gap by parsing inline/style `\frz`/`\fr` Z-rotation and timed `\t(...\frz...)` transforms, interpolating rotation at render time, and rotating all event image planes around the event bounds before clipping/fade/output scaling.

### Verified
- `cargo fmt` completed successfully after the parser/renderer/harness changes.
- Focused parser coverage passes: `cargo test -p rassa-parse parses_z_rotation_overrides_and_transforms -- --nocapture`.
- Focused renderer coverage passes: `cargo test -p rassa-render render_frame_applies_z_rotation_to_event_planes -- --nocapture` and `cargo test -p rassa-render render_frame_interpolates_z_rotation_transform -- --nocapture`.
- Upstream fixture harness passes: `cargo test -p rassa-test upstream_compare_reference_png_matrix_renders_within_frame -- --nocapture`.
- `cargo test` passes for the full workspace.
- `cargo build --release -p rassa-capi` produces `target/release/libass.so`.
- Exported-symbol inventory matches upstream `ass.h`: 50 prototypes, 50 exported `ass_*` symbols, no missing/extra public function symbols.
- A C smoke program including `<ass/ass.h>` links against `target/release/libass.so`, initializes/frees library/renderer/track objects, and runs successfully with output `24133632`.

### Current Gaps
- The currently automated gap checks all pass, including the new upstream compare-fixture smoke harness and Z-rotation regressions.
- Full libass parity is still not complete in the strict pixel-exact sense: the new harness validates fixture coverage and image-list sanity, but does not yet compare rendered pixels against upstream `libass` output byte-for-byte. Broader exact pixel parity, full layout/collision parity, and exhaustive validation corpus comparison remain future hardening work.
- The included headers are copied from the checked-out upstream reference; keep them in sync when changing the C ABI surface.

### Recommended Next Step
- Upgrade the `libass/compare/test` harness from image-list/frame sanity checks to an upstream-vs-rassa pixel diff once rassa has a stable RGBA composition path or a dependency-light PNG decoder/encoder in the test stack.

## 2026-04-28

### Added This Session
- Checked the current implementation stage against `master-plan.md`: the workspace is in late Component 9/10 with Component 11 validation started. The exported `rassa-capi` public function names now match the `libass/libass/ass.h` function inventory, but behavioral parity is still incomplete.
- Implemented the next renderer-geometry parity gap from the previous session notes: `RendererConfig.storage` is now materially used by `rassa-render` when `pixel_aspect` is unset.
- Added libass-aligned aspect derivation for storage/layout geometry: valid `LayoutResX`/`LayoutResY` takes precedence over both API storage size and explicit pixel aspect, otherwise valid `ass_set_storage_size` contributes to default pixel-aspect calculation from the frame content ratio.
- Added renderer regression coverage for storage-derived aspect behavior and `LayoutRes*` precedence, plus a C ABI regression proving `ass_set_storage_size` changes rendered output through `ass_render_frame`.
- Added a first pass at libass-style content-area mapping for configured margins when `use_margins` is disabled: render output now scales into the frame content area and applies left/top margin offsets instead of treating margins as clip-only metadata.
- Added renderer and C ABI regression coverage for default margin mapping through `ass_set_margins`, `ass_set_use_margins`, and `ass_render_frame`.
- Aligned C API shaper defaults and invalid-input behavior with upstream libass: renderers now default to complex shaping, and unsupported `ass_set_shaper` values normalize to complex instead of simple.
- Extended C ABI shaping coverage to assert invalid shaper values render identically to explicit complex shaping.
- Aligned more renderer-setting sanitization with upstream libass: invalid frame/storage size pairs now reset both dimensions, and negative pixel aspect values reset to default.
- Added C ABI regressions for invalid frame size reset, invalid storage size reset, and negative pixel-aspect reset behavior.
- Implemented the `ass_track_set_feature` post-render lock from the public `ass.h` contract: feature changes now return `-1` after a track has been rendered.
- Added C ABI coverage for feature locking after `ass_render_frame`.
- Aligned active-event/image ordering with upstream libass by sorting active events by `Layer`, then `ReadOrder`, before layout and image-list assembly.
- Added renderer and C ABI regressions proving lower-layer images are emitted first even when the lower-layer event appears later in the track.
- Aligned font-provider C API behavior with `ass.h`: `ass_get_available_font_providers` now reports providers in libass order, unsupported `ass_set_fonts` provider IDs behave like `ASS_FONTPROVIDER_NONE`, and `default_font` is used as a final file fallback when provider lookup cannot resolve a face.
- Added provider and C ABI regressions for available-provider ordering, invalid-provider fallback, and rendering through `default_font` while system providers are disabled.
- Replaced the simplified `ass_step_sub` start-time lookup with libass' event-selection behavior, including backward movement based on event end times and movement `0` resolving the nearest earlier subtitle start.
- Extended dynamic-event C ABI coverage for `ass_step_sub` backward stepping and movement `0` behavior.
- Tightened `ass_set_check_readorder` to match upstream's documented/implemented behavior: only `1` enables read-order checking, while other values disable it.
- Extended C ABI read-order coverage for a non-`1` value.
- Reworked per-event renderer output ordering to follow libass' `render_text` order: all shadow images first, then outlines, then character images, while keeping event ordering by `Layer` and `ReadOrder`.
- Added renderer and C ABI regressions for within-event `ASS_Image` ordering.
- Added first-pass `ScaledBorderAndShadow: no` compensation so border, shadow, and blur radii stay closer to device-space size when output geometry is scaled.
- Added renderer regression coverage proving disabled scaled-border mode produces a smaller outline footprint under a 2x frame scale.
- Improved raster behavior: glyph bitmaps are now cached by font path, glyph id, pixel size, and hinting mode; outline expansion uses a rounder mask; blur now expands bitmap bounds instead of only softening inside the original glyph box.
- Added raster regressions for cache reuse and expanded blur bounds.
- Aligned the basic collision pass with libass layer grouping: unpositioned events now only avoid collisions against earlier events in the same `Layer`, while different layers are allowed to overlap.
- Added renderer and C ABI regressions proving cross-layer subtitles keep the same vertical placement while existing same-layer collision avoidance remains intact.
- Extended timed transform support to include `\fs` font-size animation, carrying it through `ParsedAnimatedStyle` and applying it during render-time style interpolation.
- Added parser and renderer regressions for `\t(...\fs...)`.
- Checked another libass feature surface against the current implementation: style `ScaleX`/`ScaleY` were parsed into `ParsedStyle` but not carried into span style, layout, render, or C ABI render output.
- Wired `ScaleX`/`ScaleY` and inline `\fscx`/`\fscy` through `ParsedSpanStyle`, timed transforms, layout width calculation, and glyph bitmap/advance scaling during render.
- Added parser, renderer, and C ABI regressions proving `\fscx`/`\fscy` affect parsed spans and rendered bounds.
- Extended the same `ScaleX`/`ScaleY` path to visible `\p` drawing geometry so drawing planes now scale their polygon bounds instead of only advancing the text pen.
- Added renderer and C ABI regressions proving `\fscx`/`\fscy` change emitted drawing `ASS_Image` bounds.
- Mirrored the next upstream style surface for text spacing: style `Spacing`, inline `\fsp`, and timed `\t(...\fsp...)` now flow through parsed span state, layout width, render-time interpolation, glyph advances, and public C API output.
- Added parser, renderer, and C ABI regressions proving text spacing changes parsed transforms and rendered image width while remaining text-only, matching libass' non-drawing `\fsp` behavior.

### Verified
- `cargo test` passes for the whole workspace after the storage-size, margin geometry, C API shaper, setting sanitization, feature-lock, image-ordering, font-provider, dynamic-event, scaled-border, output-ordering, raster cache/blur, layer-collision, timed-font-size transform, text-scale, drawing-scale, and text-spacing updates.
- Targeted font-provider tests pass: `cargo test -p rassa-test capi_available_font_providers_match_libass_order`, `cargo test -p rassa-test capi_invalid_font_provider_behaves_like_none`, `cargo test -p rassa-fonts default_font_file_provider_falls_back_to_configured_path`, and `cargo test -p rassa-test capi_default_font_path_renders_when_system_providers_are_disabled`.
- Targeted dynamic-event test passes: `cargo test -p rassa-test capi_chunk_and_prune_manage_event_timeline`.
- Targeted read-order test passes: `cargo test -p rassa-test capi_check_readorder_affects_chunk_insertions`.
- Targeted renderer/raster tests pass: `cargo test -p rassa-render`, `cargo test -p rassa-raster`, and `cargo test -p rassa-test capi_render_frame_orders_shadow_outline_before_character`.
- Targeted collision/transform tests pass: `cargo test -p rassa-render render_frame_allows_basic_collision_across_different_layers`, `cargo test -p rassa-test capi_render_frame_allows_collision_across_different_layers`, `cargo test -p rassa-parse parses_timed_transform_overrides`, and `cargo test -p rassa-render render_frame_applies_timed_transform_style`.
- Targeted text-scale tests pass: `cargo test -p rassa-render render_frame_applies_text_scale_overrides`, `cargo test -p rassa-test capi_text_scale_overrides_change_rendered_bounds`, `cargo test -p rassa-parse parses_dialogue_overrides_into_spans_and_event_metadata`, and `cargo test -p rassa-parse parses_timed_transform_overrides`.
- Targeted drawing-scale and spacing tests pass: `cargo test -p rassa-render render_frame_applies_drawing_scale_overrides`, `cargo test -p rassa-test capi_drawing_scale_overrides_change_rendered_bounds`, `cargo test -p rassa-render render_frame_applies_text_spacing_override`, and `cargo test -p rassa-test capi_text_spacing_override_changes_rendered_width`.

### Current Gaps
- Renderer geometry is improved, but still not fully libass-equivalent: full `use_margins` placement semantics, script-to-screen mapping, exact PAR compensation, exact libass `ScaledBorderAndShadow` anisotropic scaling, and explicit-position edge cases still need a deeper upstream-parity pass.
- Event ordering and collision layer grouping now follow libass more closely, but collision handling remains basic and does not yet mirror libass' persistent render-private placement reuse or full free-space fitting behavior.
- Font-provider parity is improved for C API defaults, invalid provider IDs, attachments, and default file fallback, but provider coverage is still Linux-first and lacks CoreText/DirectWrite backends.
- Drawing, clipping, transform, karaoke, and raster behavior are broader than before, but still need upstream pixel-diff validation, more animated override tags such as rotation/shear, and exact libass outline stroking/blur/composite-cache parity.
- Validation is still partial: there are corpus-backed smoke tests and ABI regressions, but no automated pixel-diff harness against upstream libass outputs, fuzz/property coverage, or generated parity report.

### Recommended Next Step
- Continue Component 11 by adding an upstream render diff harness for `libass/compare/test` fixtures, or continue the renderer-geometry branch by aligning full `use_margins` placement and `ScaledBorderAndShadow` scaling semantics with upstream `ass_render.c`.

## 2026-04-24

### Added This Session
- Wired renderer `hinting` through the live raster path: `rassa-render` now passes `RendererConfig.hinting` into `rassa-raster`, and `rassa-raster` now maps ASS hinting modes to real FreeType load flags instead of always using the same render flag.
- Added raster and C ABI regression coverage for hinting, including a FreeType load-flag mapping unit test and a public API smoke test proving `ass_set_hinting` now affects a real render path instead of only mutating stored state.
- Added a first-pass output scaling path in `rassa-render` so rendered image planes are scaled against renderer frame size and pixel aspect before final frame clipping.
- Added renderer- and C ABI-level regression coverage for output scaling, including tests showing `ass_set_frame_size` changes rendered image size and `ass_set_pixel_aspect` widens output under a frame configuration that leaves room for horizontal scaling.

### Verified
- `cargo test` passes for the whole workspace after the hinting implementation and the first-pass frame-size/pixel-aspect scaling path.

### Current Gaps
- Hinting is no longer a stored-only renderer field, but parity is still incomplete: the current mapping is a focused FreeType-load-flag implementation rather than a libass-validated hinting parity pass.
- Frame size and pixel aspect are no longer mostly inert, but the current behavior is still a first-pass output-plane scaling model rather than full libass-equivalent script-to-screen coordinate mapping with storage-size semantics.
- `storage_size` is still not materially honored in rendering, so output geometry parity remains incomplete.
- Collision handling, broader layout parity, fuller drawing semantics, raster/cache parity, and validation/parity-hardening work are still incomplete.

### Recommended Next Step
- Continue the renderer-geometry parity branch by implementing `storage_size` and tightening frame/pixel-aspect mapping toward upstream libass behavior, or switch back to validation/parity-hardening if broader corpus diffing is the next priority.

## 2026-04-24

### Added This Session
- Wired renderer shaping mode through the live layout path: `rassa-render` now passes `RendererConfig.shaping` into `rassa-layout`, so `ass_set_shaper` is no longer a stored-only renderer field.
- Added layout/render/C ABI regression coverage for explicit shaping-mode handling to ensure both simple and complex shaping modes are accepted and rendering still succeeds through the public API.
- Added renderer- and C ABI-level tests proving `line_position` changes subtitle placement for the supported unpositioned subtitle case.
- Wired renderer `font_scale` through the live render path: `rassa-render` now scales effective style fields during rendering and uses scaled line widths for horizontal origin calculations, so `ass_set_font_scale` now changes actual output size instead of only mutating stored state.
- Implemented `ass_process_force_style` in `rassa-capi` for a focused set of upstream-compatible track/style overrides, including header overrides like `PlayResX`, `PlayResY`, `Timer`, `WrapStyle`, `ScaledBorderAndShadow`, and common style fields such as `FontName`, `FontSize`, colors, margins, outline/shadow, blur, and basic attributes.
- Parse/load entry points now automatically apply library style overrides after `ass_process_data`, `ass_read_file`, `ass_read_memory`, and `ass_read_styles`, so force-style behavior is no longer a stored-but-inert API.
- Added `ass_flush_events` behavior in `rassa-capi` so buffered events are actually freed and removed, plus render-time automatic pruning when `ass_configure_prune` is enabled to match libass' dynamic-event flow more closely.
- Expanded `rassa-test` with dynamic-event ABI coverage for `ass_flush_events`, `ass_configure_prune`, `ass_alloc_style`, `ass_alloc_event`, `ass_free_style`, `ass_free_event`, and `ass_set_check_readorder`.
- Added support for the ASS drawing `n` command in `rassa-parse`, which now starts a new non-closing subpath in `\p` drawings and vector clips; parser and renderer tests cover the new behavior.
- Extended karaoke metadata and rendering behavior: `rassa-parse` now distinguishes `\k`, `\K`/`\kf`, and `\ko`, and `rassa-render` now supports progressive sweep fill for `\K`/`\kf` plus temporary outline suppression for `\ko` while keeping the earlier post-span color swap behavior for `\k`.
- Added a dedicated `rassa-test` workspace crate with reusable parser/render regression helpers, deterministic render summaries, and corpus-backed tests using the cloned upstream `libass/compare/test/sub1.ass` and `sub2.ass` fixtures.
- Expanded `rassa-test` C ABI coverage with lifecycle and behavior checks for `ass_process_data`, `ass_process_chunk`, `ass_prune_events`, `ass_step_sub`, `ass_render_frame` change detection, and renderer frame clipping through the public `rassa-capi` API.
- Expanded `rassa-test` again with file-based and utility ABI checks covering `ass_read_file`, `ass_read_styles`, `ass_track_set_feature`, `ass_library_version`, and `ass_malloc`/`ass_free`, so more of the `ass.h`-style public surface is now exercised through tests.
- Added simple `\fad(in,out)` and full `\fade(a1,a2,a3,t1,t2,t3,t4)` parsing in `rassa-parse`, propagated fade metadata through `rassa-layout`, and applied render-time alpha fading to generated image planes in `rassa-render`.
- Added minimal karaoke timing support for `\k`/`\K`/`\kf`/`\ko`: `rassa-parse` now stores karaoke timing per text span, `rassa-layout` carries it through shaped runs, and `rassa-render` now switches character fill from secondary to primary color once a karaoke span has elapsed.
- Added minimal vector clip support for simple drawing paths: `rassa-parse` now accepts `\clip(...)`/`\iclip(...)` drawing syntax with `m`/`l` polygons, `rassa-layout` carries vector clip metadata, and `rassa-render` now masks image planes against vector clip polygons.
- Added visible drawing support for `\p` mode with simple `m`/`l` polygons: `rassa-parse` now marks drawing spans, `rassa-layout` carries them as drawing runs, and `rassa-render` now rasterizes those polygons into character image planes.
- Extended simple drawing support to cubic `b` commands by approximating bezier segments into polygons that flow through the same clip and visible drawing pipeline.
- Added outline and shadow layering for visible drawing planes so `\p` drawings now participate in the same fill/outline/shadow composition path as text, including blur on drawing outline/shadow layers.
- Extended drawing support to spline-style `s`/`p`/`c` commands by approximating spline segments into polygons that feed both vector clips and visible drawing rendering.
- Added a first render-time `\t(...)` implementation for non-layout-affecting properties: `rassa-parse` now keeps timed transform metadata intact across override parsing, `rassa-layout` carries those transforms on runs, and `rassa-render` interpolates fill/outline/shadow colors plus border/shadow/blur over time.
- Added parser, layout, and render tests covering movement metadata, move interpolation, fade metadata, fade alpha behavior, karaoke span timing, karaoke fill-color switching, vector clip masking, visible drawing output, cubic drawing output, drawing outline/shadow layering, and spline drawing output.
- Added parser, layout, and render tests covering timed transform metadata and render-time transform interpolation.

### Verified
- `cargo test` passes for the whole workspace after the new shaping-mode threading, selective style override path, render-time `line_spacing`, `line_position`, and `font_scale` behavior, force-style implementation, dynamic-event compatibility work, broader `rassa-capi` ABI coverage, and `n` drawing-subpath support.

### Current Gaps
- Timing/animation support is still partial: `\move`, `\fad`, full `\fade`, richer karaoke timing/sweep/outline behavior, and a render-time subset of `\t(...)` now work, but transform coverage is still limited to non-layout-affecting color/border/shadow/blur properties and broader animated overrides remain unimplemented.
- Clipping support is still partial: vector clips now work for simple `m`/`l`, cubic `b`, and spline-style `s`/`p`/`c` geometry, but broader libass clip behavior is still unimplemented.
- Drawing support is still partial: visible `\p` drawings now work for simple `m`/`l`, cubic `b`, and spline-style `s`/`p`/`c` geometry with outline/shadow layering, but fuller libass drawing semantics are still unimplemented.
- Collision handling and broader layout parity are still partial: the renderer now avoids basic overlap for simultaneous unpositioned blocks, but libass-equivalent collision resolution, multi-line stacking nuances, and broader layout parity are still unimplemented.
- `rassa-raster` still lacks true libass stroking, hinting parity, and cache layers.
- Validation and parity-hardening infrastructure is now started with `rassa-test`, regression helpers, corpus-backed parser/render tests, and a growing set of `rassa-capi` lifecycle/render/file-loading/dynamic-event/force-style/font-scale/line-position/line-spacing/selective-style-override/shaping-mode compatibility checks, but corpus diffing against upstream outputs, fuzz/property tests, broader API compatibility checks, and a parity report are still unimplemented.

### Recommended Next Step
- Continue on the validation branch with upstream output diffing or additional API compatibility checks, or switch back to unresolved rendering parity gaps such as collision handling and fuller drawing semantics.

## 2026-04-24

### Added This Session
- Added rectangular `\clip(x1,y1,x2,y2)` and `\iclip(...)` parsing in `rassa-parse`, including propagation of clip metadata through `rassa-layout`.
- Added rectangular clip application in `rassa-render`, including inverse clip splitting for image planes.
- Fixed an event-local render bug where clip handling was incorrectly reapplying to previously accumulated planes from earlier events.
- Added `RendererConfig`-driven frame and margin clipping in `rassa-render` and routed `rassa-capi::ass_render_frame` through that configured render path, so `ASS_Renderer` frame size, margins, and `use_margins` now affect output.
- Added parser, layout, and render tests covering rectangular clip tags, inverse clipping, frame clipping, and margin clipping.

### Verified
- `cargo test` passes for the whole workspace after the clipping and renderer-config update.

### Current Gaps
- Clipping support is still partial: vector clips and drawing-based clip paths are still unimplemented.
- Drawing commands are still not parsed into shapes or rendered.
- Karaoke timing/layout, collision handling, and broader layout parity are still unimplemented.
- `rassa-raster` still lacks true libass stroking, hinting parity, and cache layers.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks, parity report) is still unimplemented.

### Recommended Next Step
- Continue on the remaining clipping/drawing branch by implementing drawing command parsing and vector clip support, or switch to karaoke/collision behavior if render-visible parity is the next priority.

## 2026-04-24

### Added This Session
- Added parser-side `[Fonts]` support in `rassa-parse`, including libass-style embedded font payload decoding and attachment collection on `ParsedTrack`.
- Wired `rassa-capi` ingestion so `ass_process_data`, `ass_read_file`, and `ass_read_memory` now extract parsed font attachments into `ASS_Library` when `extract_fonts` is enabled.
- Added a parser test covering embedded font decoding from a `[Fonts]` section.

### Verified
- `cargo test` passes for the whole workspace after the embedded font parsing/extraction update.

### Current Gaps
- Embedded font extraction now works for parsed `[Fonts]` sections and `ass_add_font`, but provider parity is still incomplete: non-Linux backends, libass-equivalent metadata matching, and duplicate/eviction behavior are still missing.
- Override coverage is still partial: transforms, movement, clipping, drawing commands, karaoke, secondary/outline behavior parity, and many libass quirks are still unimplemented.
- `rassa-raster` now owns first-pass outline and blur helpers, but it still does not implement true libass outline stroking, border/shadow composition, hinting parity, or caching.
- `rassa-layout` and `rassa-render` still lack collision handling, karaoke timing/layout, clipping parity, drawing support, and libass-equivalent image ordering/composition behavior.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks, parity report) is still unimplemented.

### Recommended Next Step
- Move to the next large parity gap in layout/render behavior: clipping and drawing command support, or karaoke timing/layout, before tackling the validation crate and corpus diffing path.

## 2026-04-24

### Added This Session
- Added attached-font resolution support in `rassa-fonts`: attachments are now materialized to files, inspected via FreeType for family/style metadata, and resolved through a new `AttachedFontProvider`.
- Added `MergedFontProvider` so rendering can prefer embedded/attached fonts and fall back to Fontconfig only when needed.
- Wired `rassa-capi::ass_render_frame` to build a provider stack from `ASS_Library` font attachments plus renderer font-provider settings, so `ass_add_font` now affects shaping/raster/render output.
- Added font-layer tests covering attached-font resolution and merged fallback behavior.

### Verified
- `cargo test` passes for the whole workspace after the attached-font provider update.

### Current Gaps
- Attached fonts added through `ass_add_font` now participate in resolution, but parser-driven script attachment extraction is still missing, so `extract_fonts` does not yet ingest `[Fonts]` sections from subtitle data.
- Override coverage is still partial: transforms, movement, clipping, drawing commands, karaoke, secondary/outline behavior parity, and many libass quirks are still unimplemented.
- `rassa-raster` now owns first-pass outline and blur helpers, but it still does not implement true libass outline stroking, border/shadow composition, hinting parity, or caching.
- `rassa-layout` and `rassa-render` still lack collision handling, karaoke timing/layout, clipping parity, drawing support, and libass-equivalent image ordering/composition behavior.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks, parity report) is still unimplemented.

### Recommended Next Step
- Continue the attachment/font path by parsing script font attachments into `rassa-parse` and honoring `extract_fonts`, then move on to higher-impact layout/render parity gaps such as clipping, drawing commands, and karaoke.

## 2026-04-24

### Added This Session
- Added a first-pass outline rendering path for `\bord`: the renderer now expands rasterized glyph bitmaps and emits `ImageType::Outline` planes using the run outline color.
- Extended the first override parser slice to include `\1c`/`\c`, `\3c`, `\4c`, `\alpha`, `\1a`, `\3a`, `\4a`, `\bord`, `\shad`, and `\blur`.
- Propagated per-span color and shadow metadata through layout and into render, so glyph image planes now use run-level colors and can emit basic shadow planes for `\shad`.
- Added parser and render tests covering color packing, alpha handling, and shadow-plane generation from inline override tags.
- Added a render test covering outline-plane generation from `\bord` plus `\3c` overrides.

### Verified
- `cargo test` passes for the whole workspace after the visible override-tag update.

### Current Gaps
- Border and blur values are now parsed, and `\bord` has a temporary renderer-side bitmap inflation path, but raster/render still do not implement true libass outline stroking, blur kernels, or border/shadow composition semantics.
- Override coverage is still partial: transforms, movement, clipping, drawing commands, karaoke, secondary/outline behavior parity, and many libass quirks are still unimplemented.
- `rassa-raster` still does not implement outlines, borders, shadows, blur, stroking, hinting parity, or caching; it still renders only base glyph bitmaps.
- `rassa-render` now handles basic multi-run glyph composition plus simple shadow duplication and temporary outline expansion, but libass-equivalent ordering/composition parity, collision handling, karaoke timing, and drawing support are still missing.
- Embedded font attachments are still not part of provider-backed font resolution.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks) is still unimplemented.

### Recommended Next Step
- Move outline and shadow generation out of the renderer hack and into `rassa-raster`, then add blur handling on top of those raster products so `\bord`, `\shad`, and `\blur` start to behave like real libass raster stages.

## 2026-04-24

### Added This Session
- Added a first override-tag parser in `rassa-parse` for `\fn`, `\fs`, `\b`, `\i`, `\an`, `\pos`, and `\r`, plus dialogue text segmentation into styled spans and logical lines.
- Refactored `rassa-layout` to shape per-span runs instead of treating dialogue text as a single uniformly styled line.
- Updated `rassa-render` to rasterize and position multiple styled runs within the same line, including event-level position and alignment overrides from parsed dialogue tags.
- Added parser, layout, and render tests covering inline font overrides, reset behavior, event metadata extraction, and multi-run rendering.

### Verified
- `cargo test` passes for the whole workspace after the override-tag parsing and per-run layout/render update.

### Current Gaps
- Override-tag coverage is still very partial: colors, alpha, transforms, movement, clipping, drawing commands, borders, shadows, blur tags, karaoke, and many libass quirks are still unimplemented.
- `rassa-raster` still does not implement outlines, borders, shadows, blur, stroking, hinting parity, or caching; it only renders basic glyph bitmaps.
- `rassa-render` now handles basic multi-run glyph composition, but libass-equivalent ordering/composition parity, clipping, collision handling, karaoke timing, and drawing support are still missing.
- Embedded font attachments are still not part of provider-backed font resolution.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks) is still unimplemented.

### Recommended Next Step
- Continue parser and render parity by implementing the next high-value override tags that materially affect visible output, starting with color/alpha overrides and `\bord`/`\shad`/`\blur`, then teach raster/render to use those values.

## 2026-04-24

### Added This Session
- Integrated `harfrust` into `rassa-shape` for a real `ShapingMode::Complex` path backed by resolved font files.
- Added a HarfRust-based shaping path that loads font bytes through `read-fonts`, shapes through `UnicodeBuffer` and `ShaperData`, and translates HarfRust glyph ids and positions back into `rassa-shape::GlyphInfo`.
- Preserved fallback behavior so unresolved fonts or shaping failures still degrade cleanly to the simple shaper.
- Added a complex-shaping test that exercises the new path through the existing Fontconfig provider.

### Verified
- `cargo test` passes for the whole workspace after the HarfRust integration.

### Current Gaps
- `rassa-raster` still does not implement outlines, borders, shadows, blur, stroking, hinting parity, or caching; it only renders basic glyph bitmaps.
- `rassa-render` now emits basic glyph image planes, but ordering/composition parity with libass is still incomplete and clipping, collision handling, karaoke, drawings, and override-tag semantics are still missing.
- Embedded font attachments are still not part of provider-backed font resolution.
- Override tags and drawing commands are still not parsed into real shaping/layout/render behavior.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks) is still unimplemented.

### Recommended Next Step
- Continue parser and layout parity by implementing override-tag parsing and propagating those overrides into shaping and rendering, so the new complex-shaping and image-plane path can handle more than plain dialogue text.

## 2026-04-24

### Added This Session
- Extended `rassa-layout` to retain shaped glyph data and resolved font metadata in `LayoutLine` and `LayoutEvent` so downstream rasterization can use actual shaped output.
- Extended `rassa-raster` with glyph-based rasterization helpers keyed by `FontMatch`, so render can rasterize layout output without reconstructing shaped runs.
- Implemented `rassa-render::render_frame` to convert active events into `ImagePlane` glyph bitmaps with basic horizontal and vertical placement derived from margins and alignment.
- Implemented owned `ASS_Image` list storage in `rassa-capi::ass_render_frame`, including conversion from the FFI track model back into a parsed track and stable lifetime management across calls.
- Added render tests that verify actual `ImagePlane` output for active text.

### Verified
- `cargo test` passes for the whole workspace after the render and C API image-output update.

### Current Gaps
- `rassa-shape` still does not integrate `harfrust`, so `ShapingMode::Complex` still follows the simple path.
- `rassa-raster` still does not implement outlines, borders, shadows, blur, stroking, hinting parity, or caching; it only renders basic glyph bitmaps.
- `rassa-render` now emits basic glyph image planes, but ordering/composition parity with libass is still far from complete and clipping, collision handling, karaoke, drawings, and override-tag semantics are still missing.
- Embedded font attachments are still not part of provider-backed font resolution.
- Validation and parity-hardening infrastructure (`rassa-test`, corpus diffing, fuzzing, API compatibility checks) is still unimplemented.

### Recommended Next Step
- Continue the shaping and rendering parity path by integrating `harfrust` for complex shaping, then add override-tag parsing and richer layout semantics so the new image-plane path can approach libass behavior instead of only rendering plain dialogue text.

## 2026-04-24

### Added This Session
- Replaced the `rassa-raster` placeholder with a Linux-first FreeType-backed rasterizer using `freetype-rs`.
- Added `RasterOptions`, richer `RasterGlyph` metrics, bitmap copying with pitch normalization, and `rasterize_run` for shaped runs backed by resolved font paths.
- Added raster tests that shape text with the existing Fontconfig provider and confirm real system-font glyph bitmaps are produced.

### Verified
- `cargo test` passes for the whole workspace after the rasterization update.

### Current Gaps
- `rassa-shape` still does not integrate `harfrust`, so `ShapingMode::Complex` still follows the simple path.
- `rassa-raster` still does not implement outline extraction, borders, shadows, blur, stroking, or caching; it only renders basic glyph bitmaps.
- `rassa-render::render_frame` still returns no `ImagePlane` output and `rassa-capi::ass_render_frame` still returns no `ASS_Image` list.
- Embedded font attachments are still not part of provider-backed font resolution.
- Override tags, drawing commands, karaoke behavior, clipping, collision handling, and parity validation infrastructure are still unimplemented.

### Recommended Next Step
- Feed `LayoutEvent` output through `rassa-raster` inside `rassa-render` so `render_frame` starts producing actual `ImagePlane` data, then bridge that into `rassa-capi::ass_render_frame`.

## 2026-04-24

### Added This Session
- Implemented real style-name to style-index resolution in `rassa-parse` for event parsing.
- Replaced the `rassa-layout` glyph-count placeholder with `LayoutEvent` and `LayoutLine` output derived from shaped runs, style-selected fonts, direction metadata, and resolved margins.
- Added `RenderEngine::prepare_frame` so `rassa-render` now converts active events into prepared layout data instead of stopping at event selection.
- Added parser, layout, and render tests covering style resolution, style-driven font selection, margin fallback, line splitting, and active-event preparation.

### Verified
- `cargo test` passes for the whole workspace after the parser, layout, and render updates.

### Current Gaps
- `rassa-shape` still does not integrate `harfrust`, so `ShapingMode::Complex` still follows the simple path.
- `rassa-raster` is still a scaffold and there is still no glyph bitmap, outline, border, shadow, or blur generation.
- `rassa-render::render_frame` still returns no `ImagePlane` output and `rassa-capi::ass_render_frame` still returns no `ASS_Image` list.
- Embedded font attachments are still not part of provider-backed font resolution.
- Override tags, drawing commands, karaoke behavior, clipping, collision handling, and parity validation infrastructure are still unimplemented.

### Recommended Next Step
- Continue Component 7 by integrating `freetype-rs` into `rassa-raster`, then feed `LayoutEvent` output into rasterization so `rassa-render::render_frame` can start producing real image planes.

## 2026-04-24

### Added This Session
- Replaced the `rassa-shape` placeholder with a usable shaping API built around `ShapeRequest`, `ShapeEngine`, `ShapedText`, and `ShapedRun`.
- Connected `rassa-shape` to `rassa-unicode` and `rassa-fonts`, so shaping now resolves a font, runs Unicode analysis, splits runs on mandatory breaks, and emits simple glyph runs with direction-aware ordering.
- Added shaping tests covering single-line shaping and multi-line run splitting.

### Verified
- `cargo test` passes for the whole workspace after the shaping-layer update.

### Current Gaps
- `rassa-shape` still does not integrate `harfrust`, so `ShapingMode::Complex` currently follows the simple path.
- Font attachments are still not part of font resolution.
- `rassa-raster`, `rassa-layout`, and `rassa-render` are still scaffolds.
- `rassa-capi::ass_render_frame` still returns no `ASS_Image` list.
- Style-name to style-index resolution in parsed events is still a placeholder.

### Recommended Next Step
- Continue Component 6 by integrating `harfrust` into `rassa-shape`, then flow shaped glyph data into `rassa-layout` instead of placeholder glyph counts.

## 2026-04-24

### Added This Session
- Implemented a Linux-first `FontconfigProvider` in `rassa-fonts` using `fontconfig-rs`.
- Added `FontQuery`, `FontProviderKind`, richer `FontMatch` metadata, and tests that resolve an actual system font through Fontconfig.

### Verified
- `cargo test` passes for the whole workspace after the Fontconfig integration.

### Current Gaps
- `rassa-fonts` still does not map embedded attachments into a provider-backed lookup path.
- `rassa-shape`, `rassa-raster`, `rassa-layout`, and `rassa-render` are still scaffolds.
- `rassa-capi::ass_render_frame` still returns no `ASS_Image` list.
- Style-name to style-index resolution in parsed events is still a placeholder.

### Recommended Next Step
- Start Component 6 by connecting `rassa-fonts` and `rassa-unicode` into a real shaping layer, with simple shaping first and HarfBuzz-replacement work after that.

## 2026-04-24

### Added This Session
- Added build-time `fribidi` probing in `rassa-unicode` and integrated a minimal FriBidi-backed bidi analysis path.
- `UnicodeAnalysis` now includes bidi results: paragraph direction, visual text, logical-to-visual mapping, visual-to-logical mapping, and embedding levels.
- Added a bidi metadata test while preserving the mandatory line-break segmentation tests.

### Verified
- `cargo test` passes for the whole workspace after the FriBidi integration.

### Current Gaps
- Bidi analysis is paragraph-level only and does not yet expose script-run segmentation.
- `rassa-shape`, `rassa-raster`, `rassa-layout`, and `rassa-render` are still scaffolds.
- `rassa-capi::ass_render_frame` still returns no `ASS_Image` list.
- Style-name to style-index resolution in parsed events is still a placeholder.

### Recommended Next Step
- Start Component 5 by implementing a Linux-first `fontconfig-rs` provider in `rassa-fonts`, then feed that into `rassa-shape`.

## 2026-04-24

### Added This Session
- Converted the repository into a multi-crate workspace with the planned crate boundaries.
- Implemented `rassa-core` with shared libass-aligned constants, enums, and basic renderer data types.
- Implemented `rassa-parse` with a working ASS/SSA parser for Script Info, Styles, and Dialogue events.
- Implemented `rassa-capi` with the first C ABI slice: library and renderer lifecycle, track lifecycle, parsing entry points, style and event allocation, memory helpers, and configuration stubs.
- Replaced the `rassa-unibreak-sys` placeholder with real `libunibreak` detection through `pkg-config` and FFI bindings for UTF-32 line and word break analysis.
- Replaced the `rassa-unibreak` placeholder with safe wrappers that return line break and word break classifications, with fallback logic if `libunibreak` is unavailable.
- Expanded `rassa-unicode` to produce break analysis plus segmented text ranges split on mandatory line breaks.

### Verified
- `cargo test` passes for the whole workspace.
- Local environment has `libunibreak 7.0` and `fribidi 1.0.16` available through `pkg-config`.

### Current Gaps
- `rassa-unicode` still does not perform bidi analysis or script-run segmentation.
- `rassa-shape`, `rassa-raster`, `rassa-layout`, and `rassa-render` remain scaffolds.
- `rassa-capi::ass_render_frame` still returns no `ASS_Image` list.
- Style-name to style-index resolution in parsed events is still a placeholder.

### Recommended Next Step
- Continue Component 4 by adding a small `fribidi` integration layer in `rassa-unicode` for paragraph direction, logical-to-visual mapping, and embedding levels before wiring shaping.
