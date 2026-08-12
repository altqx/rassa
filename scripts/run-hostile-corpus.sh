#!/bin/sh
set -eu

workspace_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
tests_commit=${RASSA_LIBASS_TESTS_COMMIT:-9498737388cbd78cbab6b703821adc213a335995}
tests_repository=https://github.com/libass/libass-tests.git
temporary=

cleanup() {
    if [ -n "$temporary" ]; then
        rm -rf -- "$temporary"
    fi
}
trap cleanup EXIT HUP INT TERM

if [ "$#" -gt 0 ]; then
    set -- "$@"
elif [ -n "${RASSA_LIBASS_TESTS_DIR:-}" ]; then
    tests_dir=$RASSA_LIBASS_TESTS_DIR
    if [ "${RASSA_ALLOW_UNPINNED_TESTS:-0}" != "1" ]; then
        actual=$(git -C "$tests_dir" rev-parse HEAD^{commit})
        expected=$(git -C "$tests_dir" rev-parse "$tests_commit^{commit}")
        if [ "$actual" != "$expected" ]; then
            echo "error: libass-tests checkout is $actual, expected $expected" >&2
            exit 2
        fi
    fi
    set -- "$tests_dir/crash" "$tests_dir/regression"
else
    temporary=$(mktemp -d "${TMPDIR:-/tmp}/rassa-libass-tests.XXXXXX")
    git clone --quiet --no-checkout "$tests_repository" "$temporary/libass-tests"
    git -C "$temporary/libass-tests" fetch --quiet --depth 1 origin "$tests_commit"
    git -C "$temporary/libass-tests" checkout --quiet --detach FETCH_HEAD
    set -- "$temporary/libass-tests/crash" "$temporary/libass-tests/regression"
fi

exec cargo run --manifest-path "$workspace_root/Cargo.toml" \
    --release --quiet -p rassa-test --bin rassa-corpus-check -- --quiet "$@"
