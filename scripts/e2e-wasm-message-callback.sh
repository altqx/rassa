#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
wasm="$repo_root/target/wasm32-unknown-unknown/release/ass.wasm"
node_bin="${NODE:-node}"

RASSA_WASM_MESSAGE_CALLBACK_TEST=1 \
    cargo build \
        --manifest-path "$repo_root/Cargo.toml" \
        --target wasm32-unknown-unknown \
        --package rassa-libass-capi \
        --release

"$node_bin" - "$wasm" <<'JS'
const fs = require("fs");

const wasmPath = process.argv[2];
const bytes = fs.readFileSync(wasmPath);
const module_ = new WebAssembly.Module(bytes);
const imports = {};

for (const entry of WebAssembly.Module.imports(module_)) {
    if (entry.kind !== "function") {
        throw new Error(`unsupported wasm import ${entry.module}.${entry.name}: ${entry.kind}`);
    }
    imports[entry.module] ??= {};
    imports[entry.module][entry.name] = () => 0;
}

const instance = new WebAssembly.Instance(module_, imports);
const probe = instance.exports.rassa_wasm_message_callback_lifecycle_test;
if (typeof probe !== "function") {
    throw new Error("test-only wasm callback lifecycle export is missing");
}
if (probe() !== 1) {
    throw new Error("wasm callback did not receive the expected formatted message");
}

console.log("wasm message callback lifecycle: ok");
JS

# Rebuild without the test cfg and prove the lifecycle hook did not leak into
# the production libass-compatible export surface.
cargo build \
    --manifest-path "$repo_root/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --package rassa-libass-capi \
    --release

"$node_bin" - "$wasm" <<'JS'
const fs = require("fs");

const bytes = fs.readFileSync(process.argv[2]);
const exports_ = WebAssembly.Module.exports(new WebAssembly.Module(bytes));
if (exports_.some((entry) => entry.name === "rassa_wasm_message_callback_lifecycle_test")) {
    throw new Error("test-only callback lifecycle export leaked into production wasm");
}

console.log("wasm production callback ABI: ok");
JS
