fn main() {
    tauri_build::build();

    // Linux only: link the glibc/libstdc++ compatibility shim that satisfies
    // symbols referenced by pyke's prebuilt ONNX Runtime (see glibc_compat.c
    // for the full rationale). Native libs emitted by this build script are
    // linked after dependency rlibs, so the shim resolves the ort_sys
    // references. No-op on glibc >= 2.38 systems.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        cc::Build::new().file("glibc_compat.c").compile("glibc_compat");
    }
}
