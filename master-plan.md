## 1. Overview
### Goal
Reimplement `libass` in pure Rust while preserving:
- public C API behavior
- rendering output semantics
- subtitle format compatibility
- shaping, bidi, font lookup, and rasterization behavior
- platform/provider feature coverage
### Non-Negotiable Constraints
- **No feature loss**
- Rust-first implementation for all project-owned logic
- Use:
  - `harfrust` for HarfBuzz-equivalent shaping
  - `fribidi` for bidi processing
  - `freetype-rs` for glyph loading/rasterization integration
  - `fontconfig-rs` for Fontconfig integration
- Create a **Rust wrapper crate for `libunibreak`**
- Preserve compatibility with the `libass/libass/ass.h` API contract
- Target drop-in replacement behavior where practical
### Primary Deliverable
A Rust workspace that can:
1. expose a compatible C ABI layer,
2. match libass feature coverage,
3. produce rendering-equivalent output for a validation corpus.
---
## 2. Scope
### In Scope
- ASS/SSA parsing and track model
- override tags and style handling
- bidi and line-breaking pipeline
- font discovery, selection, fallback, and attachments
- simple and complex shaping modes
- glyph caching, outline processing, bitmap generation
- layout, positioning, margins, collision handling
- karaoke, transforms, clipping, drawing primitives
- image list generation equivalent to `ASS_Image`
- C ABI compatibility layer
- parity/performance validation infrastructure
### Out of Scope (Initial Phase)
- refactoring upstream libass C code into Rust mechanically
- introducing new end-user features before parity
- changing public API semantics
- performance-only optimizations that risk behavior drift before parity is proven
---
## 3. Architecture
### Workspace Layout
- `rassa-core` — shared types, errors, geometry, colors, timing, fixed-point helpers
- `rassa-parse` — ASS/SSA parser, styles, events, override tags, track model
- `rassa-unicode` — bidi integration, segmentation, line/word break integration
- `rassa-unibreak-sys` / `rassa-unibreak` — low-level and safe wrapper crates for `libunibreak`
- `rassa-fonts` — font discovery, fallback, attachments, provider abstraction
- `rassa-shape` — shaping abstraction with `harfrust` and simple-shaping compatibility mode
- `rassa-raster` — glyph loading, outline processing, stroking, blur, bitmap generation
- `rassa-layout` — line layout, alignment, margins, collision, karaoke timing
- `rassa-render` — frame rendering pipeline and image list assembly
- `rassa-capi` — C ABI compatibility layer matching `ass.h`
- `rassa-test` — golden tests, render diffing, corpus tests, fuzz/property tests
### Architectural Principles
- small, focused crates
- explicit interfaces and ownership
- pure functions where possible
- isolation of unsafe code to FFI boundaries
- validation at each boundary
- compatibility-first behavior
---
## 4. External Dependency Strategy
### Required Dependencies
- `fribidi`
  - bidi ordering and Unicode bidi behavior
- `freetype-rs`
  - glyph loading
  - outlines
  - hinting
  - bitmap generation
- `fontconfig-rs`
  - system font discovery and pattern matching
- `harfrust`
  - complex shaping replacement for HarfBuzz
- `libunibreak`
  - wrapped via Rust FFI crate because no suitable crate exists
### Dependency Notes
- `fontconfig-rs` should be treated as an external binding/wrapper dependency
- `libunibreak` integration should be split into:
  - raw `-sys` crate
  - safe ergonomic wrapper crate
- platform font-provider abstraction must allow later CoreText / DirectWrite support even if Linux-first delivery comes first
---
## 5. Compatibility Targets
### Public API Compatibility
- Match exported symbols and behavior from `libass/libass/ass.h`
- Preserve:
  - struct layout expectations where ABI requires it
  - enum semantics
  - callback behavior
  - ownership/lifetime expectations
  - image ordering and composition behavior
### Behavioral Compatibility
- Match libass parsing quirks where compatibility matters
- Preserve VSFilter-compatible rendering semantics where libass already does
- Preserve shaping mode behavior:
  - `ASS_SHAPING_SIMPLE`
  - `ASS_SHAPING_COMPLEX`
- Preserve hinting modes, style override behavior, margin/alignment logic, and line wrapping semantics
### Rendering Compatibility
- Target parity in:
  - glyph positions
  - bitmap extents
  - outline/shadow composition
  - blur behavior
  - clipping and drawing
  - frame-time event selection
---
## 6. Component Plan
### Component 1: API Inventory and Parity Contract
**Objective**
Define the compatibility contract before implementation.
**Tasks**
- inventory all public symbols in `ass.h`
- map structs, enums, callbacks, ownership, versioning
- define parity checklist
- document platform/provider expectations
- define acceptance criteria for “no feature loss”
**Outputs**
- API inventory
- behavior matrix
- parity contract document
**Dependencies**
- none
---
### Component 2: Core Types and Shared Infrastructure
**Objective**
Create foundational types and utilities for all downstream crates.
**Tasks**
- define core geometry, color, timing, error, and scalar types
- define shared result/error patterns
- define config and feature-gate surfaces
- establish crate conventions for ownership and immutability
**Outputs**
- `rassa-core`
**Dependencies**
- Component 1
---
### Component 3: Parser and Track Model
**Objective**
Implement ASS/SSA parsing and in-memory track representation.
**Tasks**
- parse script info, styles, events, attachments
- parse override tags and drawing commands
- preserve compatibility-relevant malformed-input behavior
- build normalized internal track/event model
**Outputs**
- `rassa-parse`
**Dependencies**
- Component 2
---
### Component 4: Unicode, Bidi, and Break Processing
**Objective**
Implement text preprocessing needed for shaping and layout.
**Tasks**
- integrate `fribidi`
- build `libunibreak` wrapper crates
- implement segmentation and boundary helpers
- define compatibility behavior for bidi/bracket pairing/line break handling
**Outputs**
- `rassa-unibreak-sys`
- `rassa-unibreak`
- `rassa-unicode`
**Dependencies**
- Component 2
- Component 3
---
### Component 5: Font Provider Layer
**Objective**
Implement font discovery and selection infrastructure.
**Tasks**
- integrate `fontconfig-rs`
- build font provider abstraction
- support family matching and fallback
- support embedded/attached fonts
- prepare provider interface for non-Linux backends
**Outputs**
- `rassa-fonts`
**Dependencies**
- Component 2
- Component 3
---
### Component 6: Shaping Layer
**Objective**
Implement text shaping with compatibility modes.
**Tasks**
- define shaping abstraction
- integrate `harfrust` for complex shaping
- implement simple shaping mode
- connect bidi/script segmentation output to shaping input
- validate cluster and positioning behavior
**Outputs**
- `rassa-shape`
**Dependencies**
- Component 3
- Component 4
- Component 5
---
### Component 7: Rasterization Layer
**Objective**
Generate glyph bitmaps and outlines equivalent to libass behavior.
**Tasks**
- integrate `freetype-rs`
- implement glyph loading and scaling
- implement outline, border, shadow, blur
- implement cache layers for glyphs/outlines/bitmaps
- preserve hinting behavior modes
**Outputs**
- `rassa-raster`
**Dependencies**
- Component 2
- Component 5
- Component 6
---
### Component 8: Layout Engine
**Objective**
Place shaped runs into final screen positions.
**Tasks**
- implement line wrapping
- implement alignment, margins, and justify behavior
- implement event positioning and collision handling
- implement karaoke timing/layout behavior
- implement clipping and drawing layout rules
**Outputs**
- `rassa-layout`
**Dependencies**
- Component 3
- Component 4
- Component 6
- Component 7
---
### Component 9: Rendering Pipeline
**Objective**
Assemble final frame output equivalent to libass image generation.
**Tasks**
- implement event selection by timestamp
- compose raster output into ordered image lists
- match `ASS_Image` semantics
- preserve image ordering and clipping behavior
**Outputs**
- `rassa-render`
**Dependencies**
- Component 7
- Component 8
---
### Component 10: C ABI Compatibility Layer
**Objective**
Expose a public interface compatible with libass consumers.
**Tasks**
- implement C ABI shim matching `ass.h`
- define opaque handle/lifetime model
- expose version and configuration APIs
- ensure ABI-safe struct interactions
**Outputs**
- `rassa-capi`
**Dependencies**
- Components 1–9
---
### Component 11: Validation and Parity Hardening
**Objective**
Prove parity and stabilize behavior.
**Tasks**
- golden render corpus against upstream libass
- parser snapshots over real-world ASS corpus
- API compatibility tests
- fuzz/property tests
- regression suite for bug-for-bug compatibility where required
- performance baselines and hotspots
**Outputs**
- `rassa-test`
- parity report
- remaining gap tracker
**Dependencies**
- Components 1–10
---
## 7. Dependency Order
1. API Inventory and Parity Contract
2. Core Types and Shared Infrastructure
3. Parser and Track Model
4. Unicode, Bidi, and Break Processing
5. Font Provider Layer
6. Shaping Layer
7. Rasterization Layer
8. Layout Engine
9. Rendering Pipeline
10. C ABI Compatibility Layer
11. Validation and Parity Hardening
---
## 8. Validation Strategy
### Functional Validation
- compare parser outputs against known subtitle samples
- compare rendering output against upstream libass across a representative corpus
- verify style overrides, karaoke, transforms, clipping, and drawing tags
### ABI Validation
- validate exported API surface against `ass.h`
- test lifecycle and memory ownership expectations
- confirm struct and enum compatibility assumptions
### Quality Validation
- fuzz parser and override-tag handling
- property test internal transformations
- snapshot-test edge-case scripts
- benchmark hot paths after correctness is established
---
## 9. Major Risks
### Risk: Shaping Parity Drift
`harfrust` may differ from HarfBuzz behavior in edge cases.
**Mitigation**
- explicit shaping abstraction
- corpus-based comparison
- isolate shaping differences in targeted tests
### Risk: Font Fallback Differences
Fontconfig matching and fallback behavior may drift from current libass expectations.
**Mitigation**
- build deterministic test corpus
- capture fallback decisions during parity testing
- keep provider abstraction narrow and testable
### Risk: Raster Output Mismatch
Border, shadow, blur, and hinting may differ subtly.
**Mitigation**
- pixel diff testing with tolerances only where justified
- isolate raster stages for component-level tests
### Risk: Compatibility Quirks
Malformed input and legacy behavior may be relied on by users.
**Mitigation**
- document compatibility-sensitive behaviors early
- preserve bug-compatible behavior where needed behind explicit policies
---
## 10. Milestones
### Milestone 1: Planning and Compatibility Lock
- API inventory complete
- parity checklist defined
- workspace and crate boundaries approved
### Milestone 2: Text Pipeline Foundation
- parser complete
- bidi and line-break pipeline complete
- font provider abstraction working on Linux
### Milestone 3: Rendering Core
- shaping, rasterization, and layout integrated
- first renderable frame pipeline working
### Milestone 4: C ABI Drop-In Layer
- public C API operational
- consumer smoke tests passing
### Milestone 5: Parity Stabilization
- corpus diff coverage in place
- remaining rendering/API gaps tracked and resolved
### Milestone 6: Platform and Performance Expansion
- non-Linux provider roadmap implemented or planned
- performance tuning after correctness lock
---
## 11. Success Criteria
The rewrite is successful when:
- all core libass features are implemented with no known feature loss
- the C API is compatible enough for intended downstream consumers
- the validation corpus shows rendering parity or documented acceptable deltas
- dependency replacements are fully integrated:
  - `harfrust`
  - `fribidi`
  - `freetype-rs`
  - `fontconfig-rs`
  - custom Rust wrapper for `libunibreak`