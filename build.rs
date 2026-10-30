fn main() {
    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        let config = slint_build::CompilerConfiguration::new()
            .with_style("cupertino".into())
            .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
        slint_build::compile_with_config("ui/main.slint", config)
            .expect("compile MK user interface");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/mk.rc");
        println!("cargo:rerun-if-changed=assets/mk.ico");
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
        let gnu = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
        let resource = out.join(if gnu { "mk-res.o" } else { "mk.res" });
        let status = if gnu {
            std::process::Command::new("x86_64-w64-mingw32-windres")
                .args(["-i", "assets/mk.rc", "-O", "coff", "-o"])
                .arg(&resource)
                .status()
        } else {
            std::process::Command::new("rc.exe")
                .args(["/nologo", "/fo"])
                .arg(&resource)
                .arg("assets/mk.rc")
                .status()
        }
        .expect("Windows resource compiler (windres/rc.exe)");
        assert!(
            status.success(),
            "compile MK Windows icon/version resources"
        );
        println!("cargo:rustc-link-arg={}", resource.display());
    }
}
