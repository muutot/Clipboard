fn main() {
    tauri_build::build();

    // Linux only: link the glibc/libstdc++ compatibility shim that satisfies
    // symbols referenced by pyke's prebuilt ONNX Runtime (see glibc_compat.c
    // for the full rationale). Native libs emitted by this build script are
    // linked after dependency rlibs, so the shim resolves the ort_sys
    // references. No-op on glibc >= 2.38 systems.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        cc::Build::new()
            .file("glibc_compat.c")
            .compile("glibc_compat");
    }

    // macOS only: force the frameworks onto the link line. The `#[link]`
    // attributes on the extern blocks under `platform/macos/`, and the
    // `rustc-link-lib=framework=` form, did not put CoreFoundation on the final
    // link line for the lib-test target on the current toolchain/runner
    // combination (`ld: symbol(s) not found: "_CFLocaleCopyCurrentLocale"` on
    // arm64). `rustc-link-arg` passes the raw arguments to cc after the object
    // files, which even a single-pass linker resolves.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-framework");
        println!("cargo:rustc-link-arg=CoreFoundation");
    }
}
