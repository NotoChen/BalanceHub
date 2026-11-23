fn main() {
    let attributes = tauri_build::Attributes::new();
    #[cfg(windows)]
    let attributes = {
        embed_windows_app_manifest();
        attributes.windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest())
    };

    configure_windows_stack_reserve();
    tauri_build::try_build(attributes).expect("failed to run Tauri build script");
}

/// Windows 主线程沿用 PE 默认的 1 MiB 栈；WebView2、Tauri IPC 调度与命令
/// future 叠加后可能触发启动栈溢出（issue #15）。这里把主线程预留调高到
/// 8 MiB，异步工作线程仍由 `desktop.rs` 单独配置为 16 MiB。
fn configure_windows_stack_reserve() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg=/STACK:8388608");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--stack,8388608");
    }
}

#[cfg(windows)]
fn embed_windows_app_manifest() {
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-app-manifest.xml");

    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
}
