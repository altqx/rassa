fn main() {
    println!("cargo:rustc-check-cfg=cfg(libunibreak_available)");

    let target = std::env::var("TARGET").unwrap_or_default();
    if uses_expected_fallback(&target) {
        return;
    }

    if pkg_config::Config::new().probe("libunibreak").is_ok() {
        println!("cargo:rustc-cfg=libunibreak_available");
    } else {
        println!(
            "cargo:warning=libunibreak not found via pkg-config; rassa-unibreak will use fallback logic"
        );
    }
}

fn uses_expected_fallback(target: &str) -> bool {
    target.contains("windows") || target.starts_with("wasm32-")
}
