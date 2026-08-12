fn main() {
    println!("cargo:rerun-if-changed=src/message_shim.c");
    println!("cargo:rerun-if-env-changed=RASSA_WASM_MESSAGE_CALLBACK_TEST");
    println!("cargo:rustc-check-cfg=cfg(rassa_wasm_message_callback_test)");

    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32")
        && std::env::var("RASSA_WASM_MESSAGE_CALLBACK_TEST").as_deref() == Ok("1")
    {
        // Expose a single end-to-end probe only in the explicit test build.
        // Normal native and wasm artifacts retain exactly the public libass ABI.
        println!("cargo:rustc-cfg=rassa_wasm_message_callback_test");
    }

    // Keep va_list construction and callback invocation in C on every target.
    // Clang represents wasm32 function pointers as indirect-table indices and
    // supplies the target's concrete va_list ABI, so this same bridge is valid
    // for wasm32-unknown-unknown without Rust forging either representation.
    cc::Build::new()
        .file("src/message_shim.c")
        .warnings(true)
        .compile("rassa-message-shim");
}
