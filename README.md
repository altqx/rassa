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

- Rust 1.85 or newer.
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

The native shared object is produced by Cargo as:

```text
target/release/librassa.so
```

Build the release libass-compatible shared object only when an application expects `libass.so` or `-lass`:

```sh
cargo build --release -p rassa-libass-capi
```

The compatibility shared object is produced by Cargo as:

```text
target/release/libass.so
```

The compatibility crate also sets the ELF SONAME to `libass.so` for libass-style dynamic linking.

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

The Rust API is not the same as either shared-library package. Use the `rassa` crate directly for native Rust integration, `librassa.so` for new C/C++ integrations, and `rassa-libass-capi` only when you need the `libass.so` package name.

## Verification commands

Common local checks:

```sh
cargo fmt --all
cargo test -q
cargo clippy --all-targets -- -D warnings
cargo build --release -p rassa-check -p rassa -p rassa-libass-capi
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

- Keep this project a Rust rewrite. Do not vendor, compile, or wrap upstream libass C source in production code.
- Upstream libass fixtures, public headers, and generated reference artifacts may be used as compatibility test oracles when their licensing allows it.
- Keep ABI compatibility and rendering parity separate when reporting status: a symbol may exist before its behavior is fully compatible.
- Add tests for compatibility fixes. Prefer small focused tests plus corpus coverage for renderer changes.
- Run formatting, tests, clippy, and the relevant pixel-diff gates before submitting changes.

## License

This repository is licensed under the MIT License. See `LICENSE` for details.

Some public compatibility headers keep their original notices where applicable.
