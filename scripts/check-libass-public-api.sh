#!/bin/sh
set -eu

workspace_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

if [ "${RASSA_SKIP_LIBASS_BUILD:-0}" != "1" ]; then
    cargo build --manifest-path "$workspace_root/Cargo.toml" --release -p rassa-libass-capi
fi

exec python3 "$workspace_root/scripts/check-libass-public-api.py" \
    --local-include "$workspace_root/include/ass" \
    --library "${RASSA_LIBASS_LIBRARY:-$workspace_root/target/release/libass.so}" \
    "$@"
