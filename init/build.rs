use std::path::PathBuf;
fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let linker_ld = manifest.join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", linker_ld.display());
    println!("cargo:rerun-if-changed=linker.ld");
}
