mod adapters;
mod commands;
mod contracts;
mod desktop;
mod limits;
mod models;
mod network;
mod platform;
mod services;
mod state;
mod storage;
mod tray;
mod util;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    desktop::run();
}
