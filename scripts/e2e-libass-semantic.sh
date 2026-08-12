#!/usr/bin/env bash
set -euo pipefail

workspace_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
libass_commit=${RASSA_LIBASS_COMMIT:-3087d2b2ffda76602a17f9b09d25cb8addc8d313}
tests_commit=${RASSA_LIBASS_TESTS_COMMIT:-9498737388cbd78cbab6b703821adc213a335995}
temporary=

cleanup() {
  if [[ -n "$temporary" ]]; then
    rm -rf -- "$temporary"
  fi
}
trap cleanup EXIT HUP INT TERM

if [[ -n "${RASSA_LIBASS_TESTS_DIR:-}" ]]; then
  tests_dir=$RASSA_LIBASS_TESTS_DIR
else
  temporary=$(mktemp -d "${TMPDIR:-/tmp}/rassa-libass-semantic.XXXXXX")
  git clone --quiet --no-checkout https://github.com/libass/libass-tests.git "$temporary/libass-tests"
  git -C "$temporary/libass-tests" fetch --quiet --depth 1 origin "$tests_commit"
  git -C "$temporary/libass-tests" checkout --quiet --detach FETCH_HEAD
  tests_dir=$temporary/libass-tests
fi

if [[ -n "${RASSA_LIBASS_BUILD_DIR:-}" ]]; then
  upstream_dir=$RASSA_LIBASS_BUILD_DIR
else
  if [[ -z "$temporary" ]]; then
    temporary=$(mktemp -d "${TMPDIR:-/tmp}/rassa-libass-semantic.XXXXXX")
  fi
  git clone --quiet --no-checkout https://github.com/libass/libass.git "$temporary/libass"
  git -C "$temporary/libass" fetch --quiet --depth 1 origin "$libass_commit"
  git -C "$temporary/libass" checkout --quiet --detach FETCH_HEAD
  meson setup "$temporary/libass-build" "$temporary/libass" \
    --buildtype=release \
    -Ddefault_library=shared \
    -Dtest=disabled \
    -Dcompare=disabled \
    -Dprofile=disabled \
    -Dfuzz=disabled \
    -Dcheckasm=disabled
  meson compile -C "$temporary/libass-build"
  upstream_dir=$temporary/libass-build/libass
fi
probe="$workspace_root/target/libass-semantic-probe"

test -d "$tests_dir/regression"
test -f "$upstream_dir/libass.so.9"
cargo build --manifest-path "$workspace_root/Cargo.toml" --release -p rassa-libass-capi
cc -std=c11 -Wall -Wextra -Werror \
  -I"$workspace_root/include" \
  "$workspace_root/scripts/libass-semantic-probe.c" \
  -L"$workspace_root/target/release" -lass \
  -o "$probe"

args=(
  --probe "$probe"
  --rassa-lib-dir "$workspace_root/target/release"
  --libass-lib-dir "$upstream_dir"
  --libass-tests "$tests_dir"
)
if [[ -n "${RASSA_LIBASS_TEST_FILTER:-}" ]]; then
  args+=(--filter "$RASSA_LIBASS_TEST_FILTER")
fi
if [[ "${RASSA_LIBASS_REPORT_ONLY:-0}" == 1 ]]; then
  args+=(--report-only)
fi
python3 "$workspace_root/scripts/e2e-libass-semantic.py" "${args[@]}"
