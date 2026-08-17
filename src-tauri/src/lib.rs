mod adapters;
mod agent_cli_catalog;
mod app_events;
mod commands;
mod contracts;
mod desktop;
mod limits;
mod models;
mod network;
mod platform;
mod provider_protocol_catalog;
mod services;
mod state;
mod storage;
mod terminal_catalog;
mod tray;
mod util;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    desktop::run();
}
