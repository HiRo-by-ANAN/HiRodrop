fn main() {
    println!("cargo:rerun-if-changed=src/macos_service.m");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    cc::Build::new()
        .file("src/macos_service.m")
        .flag("-fobjc-arc")
        .warnings(true)
        .compile("hirodrop_macos_service");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=CoreServices");
}
