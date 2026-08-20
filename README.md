# rassa

`rassa` is a Rust rewrite of the ASS subtitle rendering stack. The project is split into small Rust crates for parsing, font lookup, shaping, rasterization, layout, rendering, and C ABI packaging.

The preferred integration target for new applications is the rassa-branded shared library, `librassa.so`, built from the `rassa` crate. A separate compatibility package can still build `libass.so` for applications that specifically need a libass-shaped linker target.

> Status: experimental. `librassa.so` is the forward-looking ABI target and may evolve with rassa. `libass.so` is provided as a compatibility target, but this project does not promise to track every upstream libass behavior forever.

## Repository layout

- `crates/rassa-core` — shared public Rust data types.
- `crates/rassa-parse` — ASS/SSA parser and style/event model.
- `crates/rassa-fonts` — font provider integration.
- `crates/rassa-shape` — text shaping.
- `crates/rassa-raster` — Rust glyph rasterization helpers.
- `crates/rassa-layout` — subtitle event layout.
- `crates/rassa-render` — image-plane renderer.
- `crates/rassa` — Rust API crate and native shared-library target, producing `librassa.so`.
- `crates/rassa-capi` — C ABI implementation exported from Rust.
- `crates/rassa-libass-capi` — libass-compatible shared-library target named `ass`, producing `libass.so`.
- `crates/rassa-check` — command-line checker/debug utility.
- `crates/rassa-test` — compatibility and pixel-diff tests.
- `include/ass` — current public C headers.
- `pkgconfig/rassa.pc` — pkg-config metadata for the native rassa shared library.
- `pkgconfig/libass.pc` — pkg-config metadata for the libass-compatible target.

## Requirements

- Rust 1.90 or newer.
- A Unix-like development environment for the current C ABI packaging flow.
- A C compiler if you want to compile C smoke tests or downstream C/C++ applications.

## Build

Build all Rust crates:

```sh
cargo build --workspace
```

Build the release rassa shared object for new applications:

```sh
cargo build --release -p rassa
```

The native shared library is produced by Cargo as:

```text
# Linux
target/release/librassa.so

# Windows GNU target
target/x86_64-pc-windows-gnu/release/rassa.dll

# Web/wasm target
target/wasm32-unknown-unknown/release/rassa.wasm
```

Build the release libass-compatible shared object only when an application expects `libass.so` or `-lass`:

```sh
cargo build --release -p rassa-libass-capi
```

The compatibility shared library is produced by Cargo as:

```text
# Linux
target/release/libass.so

# Windows GNU target
target/x86_64-pc-windows-gnu/release/ass.dll

# Web/wasm target
target/wasm32-unknown-unknown/release/ass.wasm
```

The compatibility crate sets the ELF SONAME to upstream's ABI name
`libass.so.9` and emits a `libass.so.9` link next to `libass.so`. Windows DLL,
Darwin dylib/check, and WebAssembly builds do not receive ELF-only linker
flags.

Build the checker utility and C ABI crates used during development:

```sh
cargo build --release -p rassa-check -p rassa -p rassa-libass-capi
```

## Native C ABI usage (`librassa.so`)

Use this path for new applications that want to depend on rassa without making a strict libass compatibility promise:

- Header include path: `include/`
- Current public headers: `include/ass/ass.h` and `include/ass/ass_types.h`
- Library name: `librassa.so`
- Linker flag: `-lrassa`
- pkg-config file: `pkgconfig/rassa.pc`

For local development from the repository root:

```sh
cargo build --release -p rassa
export PKG_CONFIG_PATH="$PWD/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

Compile a C file against the rassa shared object:

```sh
cc example.c $(pkg-config --cflags --libs rassa) -Wl,-rpath,"$PWD/target/release" -o example
```

Or without pkg-config:

```sh
cc example.c -Iinclude -Ltarget/release -lrassa -Wl,-rpath,"$PWD/target/release" -o example
```

The current native C ABI reuses the same public entry points as the compatibility layer. New applications should link to `librassa.so` / `-lrassa` so their dependency is on rassa itself, not on the `libass.so` drop-in package name.

## Compatibility C ABI usage (`libass.so`)

Use this path only for applications or plugins that already expect a libass-shaped package:

- Header include path: `include/`
- Public headers: `include/ass/ass.h` and `include/ass/ass_types.h`
- Library name: `libass.so`
- Linker flag: `-lass`
- pkg-config file: `pkgconfig/libass.pc`

For local development from the repository root:

```sh
cargo build --release -p rassa-libass-capi
export PKG_CONFIG_PATH="$PWD/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
```

Compile a C file against rassa's libass-compatible ABI:

```sh
cc example.c $(pkg-config --cflags --libs libass) -Wl,-rpath,"$PWD/target/release" -o example
```

Or without pkg-config:

```sh
cc example.c -Iinclude -Ltarget/release -lass -Wl,-rpath,"$PWD/target/release" -o example
```

Minimal C smoke test:

```c
#include <ass/ass.h>
#include <stdio.h>

int main(void) {
    ASS_Library *library = ass_library_init();
    if (!library) {
        return 1;
    }

    ASS_Renderer *renderer = ass_renderer_init(library);
    if (!renderer) {
        ass_library_done(library);
        return 1;
    }

    ass_set_frame_size(renderer, 640, 360);
    printf("libass ABI version: 0x%x\n", ass_library_version());

    ass_renderer_done(renderer);
    ass_library_done(library);
    return 0;
}
```

Build and run it from the repository root:

```sh
cat > /tmp/rassa-smoke.c <<'C'
#include <ass/ass.h>
#include <stdio.h>

int main(void) {
    ASS_Library *library = ass_library_init();
    if (!library) return 1;
    ASS_Renderer *renderer = ass_renderer_init(library);
    if (!renderer) {
        ass_library_done(library);
        return 1;
    }
    ass_set_frame_size(renderer, 640, 360);
    printf("libass ABI version: 0x%x\n", ass_library_version());
    ass_renderer_done(renderer);
    ass_library_done(library);
    return 0;
}
C

cargo build --release -p rassa-libass-capi
cc /tmp/rassa-smoke.c -Iinclude -Ltarget/release -lass -Wl,-rpath,"$PWD/target/release" -o /tmp/rassa-smoke
/tmp/rassa-smoke
```

## Drop-in linking notes

For consumers that currently link against libass:

1. Build `rassa-libass-capi` in release mode.
2. Put `target/release/libass.so` and its generated ABI-name link
   `target/release/libass.so.9` in the runtime library search path used by your
   application. The shared object advertises SONAME `libass.so.9`, matching
   upstream libass for prebuilt consumers.
3. Put `include/` in the C/C++ compiler include path.
4. Put `pkgconfig/` in `PKG_CONFIG_PATH` if your build uses `pkg-config --libs libass`.
5. Rebuild your downstream application and run its subtitle-rendering tests.

Example:

```sh
cargo build --release -p rassa-libass-capi
export PKG_CONFIG_PATH="$PWD/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$PWD/target/release${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
```

Then build the downstream project as usual. If it uses pkg-config, it should see:

```sh
pkg-config --cflags --libs libass
```

Expected output shape:

```text
-I.../include -L.../target/release -lass
```

## Rust API usage

Use the `rassa` crate for native Rust applications. The public facade exports the parser, renderer, font provider traits, core geometry/color types, and a small safe `Script`/`Renderer` API.

Add the crate from crates.io:

```toml
[dependencies]
rassa = "0.7.1"
```

Or, inside this repository, use a path dependency while developing:

```toml
[dependencies]
rassa = { path = "crates/rassa" }
```

Minimal render example:

```rust
use rassa::{Renderer, Script};

fn main() -> rassa::RassaResult<()> {
    let script = Script::parse(r#"[Script Info]
PlayResX: 640
PlayResY: 360

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,sans,48,&H00FFFFFF,&H0000FFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,1,5,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0000,0000,0000,,Hello from rassa
"#)?;

    let frame = Renderer::new().render_frame(&script, 500)?;
    println!("rendered {} image plane(s)", frame.planes.len());

    for plane in &frame.planes {
        println!(
            "plane at ({}, {}) size {}x{} stride {} color 0x{:08x}",
            plane.destination.x,
            plane.destination.y,
            plane.size.width,
            plane.size.height,
            plane.stride,
            plane.color.0,
        );
    }

    Ok(())
}
```

If you need control over font lookup or frame sizing, build a provider/config explicitly:

```rust
use rassa::{FontconfigProvider, Renderer, RendererConfig, Script, Size};

fn main() -> rassa::RassaResult<()> {
    let ass_text = std::fs::read_to_string("subtitle.ass")
        .expect("failed to read subtitle.ass");
    let script = Script::parse(&ass_text)?;
    let provider = FontconfigProvider::new();
    let config = RendererConfig {
        frame: Size { width: 1920, height: 1080 },
        storage: script.play_res(),
        ..script.default_config()
    };

    let frame = Renderer::new().render_frame_with_config(&script, &provider, 12_500, &config)?;
    println!("{} plane(s)", frame.planes.len());
    Ok(())
}
```

The Rust API is not the same as either shared-library package. Use the `rassa` crate directly for native Rust integration, `librassa.so` for new C/C++ integrations, and `rassa-libass-capi` only when you need the `libass.so` package name.

## Published crates and docs

The current published version is `0.7.1`.

| Crate | Purpose | Docs |
| --- | --- | --- |
| `rassa` | Safe Rust facade and native `librassa.so` target | <https://docs.rs/rassa> |
| `rassa-core` | Shared data types and ASS enums | <https://docs.rs/rassa-core> |
| `rassa-parse` | ASS/SSA parser | <https://docs.rs/rassa-parse> |
| `rassa-fonts` | Cross-platform font provider traits and discovery | <https://docs.rs/rassa-fonts> |
| `rassa-unicode` | Pure-Rust Unicode bidi, line/word breaking, and segmentation helpers | <https://docs.rs/rassa-unicode> |
| `rassa-shape` | Text shaping | <https://docs.rs/rassa-shape> |
| `rassa-raster` | Glyph rasterization helpers | <https://docs.rs/rassa-raster> |
| `rassa-layout` | Subtitle event layout | <https://docs.rs/rassa-layout> |
| `rassa-render` | Image-plane renderer | <https://docs.rs/rassa-render> |
| `rassa-capi` | Shared C ABI implementation | <https://docs.rs/rassa-capi> |
| `rassa-libass-capi` | `libass.so` compatibility target | <https://docs.rs/rassa-libass-capi> |
| `rassa-check` | CLI/library smoke checker and PNG/JPEG/PGM renderer | <https://docs.rs/rassa-check> |

Generate local docs for the whole workspace:

```sh
cargo doc --workspace --no-deps --open
```

Generate docs for only the public facade:

```sh
cargo doc -p rassa --no-deps --open
```

## Checker CLI usage

Install the checker from crates.io:

```sh
cargo install rassa-check
```

Render the built-in smoke script to PNG:

```sh
rassa-check --output rassa-check.png --time-ms 500 --width 640 --height 360
```

Render your own ASS/SSA file:

```sh
rassa-check --input subtitles.ass --output frame.png --time-ms 12500 --width 1920 --height 1080
```

Supported output formats are inferred from the file extension: `.png`, `.jpg`/`.jpeg`, or `.pgm`. You can also set them explicitly with `--format png|jpg|pgm`.

## Verification commands

Common local checks:

```sh
cargo fmt --all
cargo test -q
cargo clippy --all-targets -- -D warnings
cargo build --release -p rassa-check -p rassa -p rassa-libass-capi
```

Check that the safe Rust API and C API produce exactly the same image-plane
metadata and bitmap bytes, then measure warmed dynamic rendering and repeated
same-frame calls:

```sh
cargo run --release -p rassa-test --bin rassa-perf -- \
  --iterations 1000 --samples 9 --warmup 200
```

The benchmark excludes parser/provider setup, emits diff-friendly key/value
records, and fails before timing if any Rust/C output differs. For a quick
correctness-only gate, pass `--verify-only`. Pinning the process to one CPU with
`taskset -c <cpu>` can reduce scheduler noise on Linux.

Non-pixel compatibility gates use pinned upstream revisions and fail on API or
memory/layout invariants, not raster differences. Verify that the public C
headers structurally match libass master `3087d2b2ffda76602a17f9b09d25cb8addc8d313`
and that all declared `ass_*` functions are exported:

```sh
scripts/check-libass-public-api.sh
```

Reuse an existing checkout with `RASSA_LIBASS_UPSTREAM=/path/to/libass`, or set
`RASSA_LIBASS_COMMIT` when intentionally updating the pin. The checker clones
the pinned revision into a temporary directory when no checkout is supplied.

Run the deterministic ASS corpus gate, based on libass `fuzz/fuzz.c`.
It parses the complete pinned libass-tests crash and regression corpus at commit
`9498737388cbd78cbab6b703821adc213a335995`, renders every event at Start,
midpoint, and End-1, and checks dimensions, stride, allocation length, and
frame clipping:

```sh
scripts/run-hostile-corpus.sh
```

Use `RASSA_LIBASS_TESTS_DIR=/path/to/libass-tests` to reuse the exact pinned
checkout. Arbitrary local files or directories can also be checked directly:

```sh
cargo run --release -p rassa-test --bin rassa-corpus-check -- path/to/corpus
```

Run the pinned official semantic differential against current libass master and
the compatible libass-tests corpus:

```sh
scripts/e2e-libass-semantic.sh
```

The script clones and builds the pinned revisions when local checkouts are not
provided. Reuse existing checkouts with:

```sh
RASSA_LIBASS_TESTS_DIR=/path/to/libass-tests \
RASSA_LIBASS_BUILD_DIR=/path/to/libass-build/libass \
  scripts/e2e-libass-semantic.sh
```

This deliberately does not require pixel equality: Rassa has its own
rasterizer. It compares track/event timing, visibility, image kind and colour
sets, stable image-kind ordering, and per-kind visible geometry with a
documented 3% axis tolerance for raster support differences. It also runs
tighter 1080p drawing/transform probes and a full 24 fps, progress-aware reveal
curve comparison for the rotated Japanese karaoke fixture; these catch
animation failures that a single reference PNG cannot expose. Filter to a
fixture family or produce a diagnostic-only report with:

```sh
RASSA_LIBASS_TEST_FILTER=karaoke/ \
RASSA_LIBASS_REPORT_ONLY=1 \
  scripts/e2e-libass-semantic.sh
```

Exercise the WebAssembly C message-callback ABI, including the C `va_list`
bridge and a real indirect callback under Node:

```sh
scripts/e2e-wasm-message-callback.sh
```

## Contributing

We do not accept contributions yet. If you run into a bug or have a suggestion,
feel free to [open an issue](https://github.com/altqx/rassa/issues) — issue
reports are very welcome.

## License

This repository is licensed under the MIT License. See `LICENSE` for details.

Some public compatibility headers keep their original notices where applicable.

The `rassa-raster` crate is a fork of
[`crossfont`](https://github.com/alacritty/crossfont), which is licensed under
the Apache License, Version 2.0. Portions of that crate derived from crossfont
remain subject to the Apache-2.0 terms; see `crates/rassa-raster/LICENSE-APACHE`
and `crates/rassa-raster/NOTICE` for details.
