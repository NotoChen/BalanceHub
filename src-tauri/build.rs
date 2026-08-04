fn main() {
    let attributes = tauri_build::Attributes::new();
    #[cfg(windows)]
    let attributes = {
        embed_windows_app_manifest();
        attributes.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
    };

    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}

#[cfg(windows)]
fn embed_windows_app_manifest() {
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");

    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
