fn main() {
    println!("cargo:rustc-check-cfg=cfg(libunibreak_available)");

    if pkg_config::Config::new().probe("libunibreak").is_ok() {
        println!("cargo:rustc-cfg=libunibreak_available");
    } else {
        println!("cargo:warning=libunibreak not found via pkg-config; rassa-unibreak will use fallback logic");
    }
}