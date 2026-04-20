//! Build script for `umbrix-bsp-qemu-virt`.
//!
//! Passes the linker script at the crate root to the linker with an absolute
//! path so resolution does not depend on the linker's working directory.
//! See `docs/decisions/0012-boot-flow-qemu-virt.md` for the memory layout the
//! linker script encodes.

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set by Cargo when running build scripts");

    println!("cargo:rustc-link-arg=-T{manifest_dir}/linker.ld");
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/boot.s");
}
