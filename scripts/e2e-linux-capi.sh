#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo build --release -p rassa -p rassa-libass-capi -p rassa-check

cc scripts/e2e-capi-smoke.c \
  -Iinclude \
  -Ltarget/release \
  -lass \
  -Wl,-rpath,"$root/target/release" \
  -o target/release/rassa-capi-smoke

target/release/rassa-capi-smoke

test -f target/release/librassa.so
test -f target/release/libass.so
nm -D --defined-only target/release/librassa.so | grep -E ' ass_library_init$| ass_library_version$| ass_render_frame$'
nm -D --defined-only target/release/libass.so | grep -E ' ass_library_init$| ass_library_version$| ass_render_frame$'
