//! WSL 环境命令。

use crate::{api::blocking, models::WslInventory, services::wsl};

pub(crate) async fn get_wsl_inventory(force: bool) -> Result<WslInventory, String> {
    blocking("WSL 环境检测", move || Ok(wsl::refresh(force))).await
}

pub(crate) async fn start_wsl_distribution(distro_id: String) -> Result<WslInventory, String> {
    blocking("启动 WSL", move || wsl::start(&distro_id)).await
}
