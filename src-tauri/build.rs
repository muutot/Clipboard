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

    // macOS only: force the two frameworks onto the link line. The
    // `#[link]` attributes on the extern blocks under `platform/macos/` did not
    // put CoreFoundation there on the current toolchain/runner combination
    // (`ld: symbol(s) not found: "_CFLocaleCopyCurrentLocale"` on arm64), and
    // only build-script libs are guaranteed to land after the dependency
    // rlibs. ApplicationServices is emitted alongside it because the
    // CoreGraphics declarations share the same extern-block fate.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
}
