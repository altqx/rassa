fn main() {
    let target_family = std::env::var("CARGO_CFG_TARGET_FAMILY").unwrap_or_default();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // Set ELF SONAME only; wasm-ld, link.exe, and Darwin linkers reject this flag.
    if target_family == "unix" && !matches!(target_os.as_str(), "macos" | "ios") {
        println!("cargo:rustc-link-arg-cdylib=-Wl,-soname,libass.so.9");
        create_elf_soname_link();
    }
}

#[cfg(unix)]
fn create_elf_soname_link() {
    use std::{fs, os::unix::fs::symlink, path::PathBuf};

    let Some(profile_dir) = std::env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .and_then(|path| path.ancestors().nth(3).map(PathBuf::from))
    else {
        println!("cargo:warning=unable to locate Cargo profile directory for libass.so.9");
        return;
    };
    let link = profile_dir.join("libass.so.9");
    if fs::symlink_metadata(&link).is_ok() {
        if fs::read_link(&link).ok().as_deref() == Some(std::path::Path::new("libass.so")) {
            return;
        }
        println!(
            "cargo:warning=not replacing existing {} while creating the libass SONAME link",
            link.display()
        );
        return;
    }
    if let Err(error) = symlink("libass.so", &link) {
        println!("cargo:warning=failed to create {}: {error}", link.display());
    }
}

#[cfg(not(unix))]
fn create_elf_soname_link() {}
