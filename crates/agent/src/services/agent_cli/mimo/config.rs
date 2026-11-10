use super::super::config_support::openai_base_url;
use super::super::configuration::contracts::ProviderConfigurationCandidate;
use super::super::opencode_family::{self, FamilySpec};
use crate::{
    models::{AgentCliKind, CliConfigSnapshot, Provider},
    services::agent_cli::integration::xdg_config_home,
    services::cli_paths::configured_path,
};
use std::path::{Path, PathBuf};

pub(super) const SPEC: FamilySpec = FamilySpec {
    label: "MiMo Code",
    dir_for_home: config_dir_for_home,
    file_in_dir,
    configuration_label: "MiMo Code 用户配置",
    configuration_note: "全局 mimocode.jsonc、mimocode.json 或 config.json 中实际存在的第一个；MIMOCODE_HOME 可覆盖目录",
};

pub(super) fn config_dir_for_home(home: &Path) -> PathBuf {
    if let Some(dir) = configured_path("MIMOCODE_HOME").filter(|dir| dir.is_absolute()) {
        return dir.join("config");
    }
    xdg_config_home(home).join("mimocode")
}

fn file_in_dir(root: &Path) -> PathBuf {
    for name in ["mimocode.jsonc", "mimocode.json", "config.json"] {
        let path = root.join(name);
        if path.is_file() {
            return path;
        }
    }
    root.join("mimocode.json")
}

pub(super) fn config_path_for_config_root(root: &Path) -> PathBuf {
    file_in_dir(root)
}

pub(super) fn temporary_config(
    model: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, String> {
    opencode_family::merge_config(
        &SPEC,
        "",
        "BalanceHub",
        model,
        &openai_base_url(base_url),
        api_key,
    )
}

pub(super) fn snapshot(cli_kind: AgentCliKind, providers: &[Provider]) -> CliConfigSnapshot {
    opencode_family::snapshot(&SPEC, cli_kind, providers)
}

pub(super) fn candidates(
    cli_kind: AgentCliKind,
    provider: &Provider,
    api_key_local_id: &str,
) -> Result<Vec<ProviderConfigurationCandidate>, String> {
    opencode_family::candidates(&SPEC, cli_kind, provider, api_key_local_id)
}
