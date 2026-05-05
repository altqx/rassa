# rassa

`rassa` is a clean-room Rust reimplementation of the libass subtitle rendering stack. The project is split into small Rust crates for parsing, font lookup, shaping, rasterization, layout, rendering, and C ABI compatibility.

The main compatibility goal is to provide a libass-style public C API and shared library that downstream applications can link against as `libass.so`, while keeping the implementation in Rust.

> Status: early parity work. The public ABI surface is intended to look like libass, but rendering and feature parity are still being actively improved. Treat this as experimental until your own integration tests pass.

## Repository layout

- `crates/rassa-core` — shared public Rust data types.
- `crates/rassa-parse` — ASS/SSA parser and style/event model.
- `crates/rassa-fonts` — font provider integration.
- `crates/rassa-shape` — text shaping.
- `crates/rassa-raster` — Rust glyph rasterization helpers.
- `crates/rassa-layout` — subtitle event layout.
- `crates/rassa-render` — image-plane renderer.
- `crates/rassa` — Rust API crate.
- `crates/rassa-capi` — C ABI implementation exported from Rust.
- `crates/rassa-libass-capi` — libass-compatible shared-library target named `ass`, producing `libass.so`.
- `crates/rassa-check` — command-line checker/debug utility.
- `crates/rassa-test` — compatibility and pixel-diff tests.
- `include/ass` — public C headers compatible with libass-style consumers.
- `pkgconfig/libass.pc` — pkg-config metadata for local development.

## Requirements

- Rust 1.85 or newer.
- A Unix-like development environment for the current C ABI packaging flow.
- A C compiler if you want to compile C smoke tests or downstream C/C++ applications.

## Build

Build all Rust crates:

```sh
cargo build --workspace
```

Build the release libass-compatible shared object:

```sh
cargo build --release -p rassa-libass-capi
```

The shared object is produced by Cargo as:

```text
target/release/libass.so
```

The crate also sets the ELF SONAME to `libass.so` for libass-style dynamic linking.

Build the checker utility and C ABI crates used during development:

```sh
cargo build --release -p rassa-check -p rassa-capi -p rassa-libass-capi
```

## C ABI usage (`libass.so`)

The open-source integration path is the libass-compatible C ABI:

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
2. Put `target/release/libass.so` in the runtime library search path used by your application.
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

The Rust API is exposed through the workspace crates. For internal development, use path dependencies, for example:

```toml
[dependencies]
rassa = { path = "crates/rassa" }
```

The Rust API is not the same as the libass C ABI. Use `rassa-libass-capi` when you need `ass_*` symbols and `libass.so`; use the Rust crates when you want native Rust integration.

## Verification commands

Common local checks:

```sh
cargo fmt --all
cargo test -q
cargo clippy --all-targets -- -D warnings
cargo build --release -p rassa-check -p rassa-capi -p rassa-libass-capi
```

Strict broad corpus pixel-diff gate:

```sh
RASSA_STRICT_BROAD_PIXEL_DIFF=1 \
  cargo test -p rassa-test upstream_generated_broad_corpus_pixel_diff_report -- --ignored --nocapture
```

Dump actual/target compare PNGs for debugging:

```sh
RASSA_DUMP_COMPARE=/tmp/rassa-compare-dump \
  cargo test -p rassa-test upstream_generated_broad_corpus_pixel_diff_report -- --ignored --nocapture
```

Filter the broad corpus by fixture name:

```sh
RASSA_BROAD_FILTER=broad_box RASSA_STRICT_BROAD_PIXEL_DIFF=1 \
  cargo test -p rassa-test upstream_generated_broad_corpus_pixel_diff_report -- --ignored --nocapture
```

## Open-source contribution guidelines

- Keep this project a clean-room Rust implementation. Do not vendor, compile, or wrap upstream libass C source in production code.
- Upstream libass fixtures, public headers, and generated reference artifacts may be used as compatibility test oracles when their licensing allows it.
- Keep ABI compatibility and rendering parity separate when reporting status: a symbol may exist before its behavior is fully compatible.
- Add tests for compatibility fixes. Prefer small focused tests plus corpus coverage for renderer changes.
- Run formatting, tests, clippy, and the relevant pixel-diff gates before submitting changes.

## License

This repository is licensed under the MIT License. See `LICENSE` for details.

Some public compatibility headers keep their original notices where applicable.
