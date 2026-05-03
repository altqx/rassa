fn main() {
    println!("cargo:rustc-link-arg-cdylib=-Wl,-soname,libass.so");
}
