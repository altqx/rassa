fn main() {
    println!("cargo:rustc-check-cfg=cfg(fribidi_available)");

    let target = std::env::var("TARGET").unwrap_or_default();
    if uses_expected_fallback(&target) {
        return;
    }

    if pkg_config::Config::new().probe("fribidi").is_ok() {
        println!("cargo:rustc-cfg=fribidi_available");
    } else {
        println!(
            "cargo:warning=fribidi not found via pkg-config; rassa-unicode will use bidi fallback logic"
        );
    }
}

fn uses_expected_fallback(target: &str) -> bool {
    target.contains("windows") || target.starts_with("wasm32-")
}
