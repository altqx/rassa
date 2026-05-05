fn main() {
    let target_family = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // SONAME is an ELF dynamic-linker concept. Passing this flag to wasm-ld,
    // link.exe, or the Darwin linker breaks cross-target cdylib builds.
    if target_family == "unix" && !matches!(target_os.as_str(), "macos" | "ios") {
        println!("cargo:rustc-link-arg-cdylib=-Wl,-soname,libass.so");
    }
}
