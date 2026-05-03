fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=csrc/rassa_libass_raster.c");
    println!("cargo:rerun-if-changed=csrc/config.h");
    println!("cargo:rerun-if-changed=csrc/ass_compat.h");
    println!("cargo:rerun-if-changed=csrc/libass");

    let freetype = pkg_config::Config::new()
        .probe("freetype2")
        .expect("freetype2 must be available via pkg-config");

    let libass_dir = std::path::PathBuf::from("csrc/libass");
    let mut build = cc::Build::new();
    build
        .warnings(false)
        .flag_if_supported("-std=c99")
        .include("csrc")
        .include(&libass_dir)
        .include(libass_dir.join("c"))
        .file("csrc/rassa_libass_raster.c")
        .file(libass_dir.join("ass_outline.c"))
        .file(libass_dir.join("ass_rasterizer.c"))
        .file(libass_dir.join("ass_utils.c"))
        .file(libass_dir.join("c/c_rasterizer.c"));

    for path in freetype.include_paths {
        build.include(path);
    }

    build.compile("rassa_libass_raster");
}
