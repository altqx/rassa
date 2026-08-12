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
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let host = std::env::var("HOST").unwrap_or_default();
    let target = std::env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    build.file("src/message_shim.c").warnings(true);
    // cc-rs otherwise selects the Linux host GCC for an Apple check target and
    // feeds it Apple-only `-arch` flags. Clang can emit the required Mach-O
    // object directly without an SDK because this shim only needs stdarg.h.
    if matches!(target_os.as_str(), "macos" | "ios") && host != target {
        build.compiler("clang");
    }
    build.compile("rassa-message-shim");
}
