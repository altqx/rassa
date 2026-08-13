fn main() {
    println!("cargo:rerun-if-changed=src/message_shim.c");
    println!("cargo:rerun-if-env-changed=RASSA_WASM_MESSAGE_CALLBACK_TEST");
    println!("cargo:rustc-check-cfg=cfg(rassa_wasm_message_callback_test)");

    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32")
        && std::env::var("RASSA_WASM_MESSAGE_CALLBACK_TEST").as_deref() == Ok("1")
    {
        // Enable the e2e probe only in the explicit test build.
        println!("cargo:rustc-cfg=rassa_wasm_message_callback_test");
    }

    // Keep va_list construction in C so wasm32 uses Clang's ABI.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let host = std::env::var("HOST").unwrap_or_default();
    let target = std::env::var("TARGET").unwrap_or_default();
    let mut build = cc::Build::new();
    build.file("src/message_shim.c").warnings(true);
    // Cross-compile Apple targets with Clang; host GCC rejects Apple `-arch` flags.
    if matches!(target_os.as_str(), "macos" | "ios") && host != target {
        build.compiler("clang");
    }
    build.compile("rassa-message-shim");
}
