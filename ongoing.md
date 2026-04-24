# Ongoing Work

Append new session entries at the top of this file.

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