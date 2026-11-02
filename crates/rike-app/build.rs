//! Finds the built TSX bundle (`ui/.nsh/out/bundle.js`, made by `scripts/build-release.sh`)
//! for `include_str!`. Without it the build still works: a placeholder that reports the
//! missing bundle stands in, so `cargo check` does not need the UI to be built first.

use std::env;
use std::fs;
use std::path::PathBuf;

const PLACEHOLDER: &str = "throw new Error(\"the Rike UI bundle is missing: run scripts/build-release.sh and rebuild\");\n";

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let built = manifest.join("../../ui/.nsh/out/bundle.js");
    println!("cargo:rerun-if-changed={}", built.display());
    println!("cargo:rerun-if-changed=build.rs");

    let (path, present) = if built.is_file() {
        (built, "1")
    } else {
        let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("missing-bundle.js");
        if fs::read_to_string(&out).ok().as_deref() != Some(PLACEHOLDER) {
            fs::write(&out, PLACEHOLDER).expect("write the placeholder bundle");
        }
        (out, "0")
    };
    println!("cargo:rustc-env=RIKE_UI_BUNDLE={}", path.display());
    println!("cargo:rustc-env=RIKE_UI_BUNDLE_PRESENT={present}");
}
