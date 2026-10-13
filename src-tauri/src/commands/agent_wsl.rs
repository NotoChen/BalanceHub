use crate::{models::WslInventory, services::wsl};

#[tauri::command]
pub(crate) async fn get_wsl_inventory(
    app: tauri::AppHandle,
    force: bool,
) -> Result<WslInventory, String> {
    super::run_blocking("WSL 环境检测", move || Ok(wsl::refresh(&app, force))).await
}

#[tauri::command]
pub(crate) async fn start_wsl_distribution(
    app: tauri::AppHandle,
    distro_id: String,
) -> Result<WslInventory, String> {
    super::run_blocking("启动 WSL", move || wsl::start(&app, &distro_id)).await
}
