fn main() {
    println!("cargo:rustc-check-cfg=cfg(fribidi_available)");

    if pkg_config::Config::new().probe("fribidi").is_ok() {
        println!("cargo:rustc-cfg=fribidi_available");
    } else {
        println!(
            "cargo:warning=fribidi not found via pkg-config; rassa-unicode will use bidi fallback logic"
        );
    }
}
