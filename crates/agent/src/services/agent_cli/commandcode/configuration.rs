use super::config;
use crate::{
    models::*,
    services::agent_cli::{
        configuration::{contracts::*, native_support as support},
        contracts::AgentSourceDiscoveryRequest,
        environment::SourceInput,
    },
};
use std::path::Path;

pub(super) const fn adapter() -> AgentConfigurationAdapter {
    AgentConfigurationAdapter {
        discover,
        validate: support::validate,
    }
}

fn discover(
    request: AgentSourceDiscoveryRequest<'_>,
    output: &mut dyn AgentConfigurationSourceOutput,
) {
    let root = config::config_dir_for_home(request.home);
    let files = [
        File {
            path: config::user_config_path(&root),
            key: "config",
            label: "Command Code 用户配置",
            rule: "全局 config.json；model 以 balancehub/<模型 ID> 选择模型",
        },
        File {
            path: config::settings_path(&root),
            label: "Command Code 设置",
            key: "settings",
            rule: "全局 settings.json；同名设置覆盖 config.json 中的值",
        },
        File {
            path: config::providers_path(&root),
            label: "Command Code Provider 配置",
            key: "providers",
            rule: "providers.json 声明 BYOK Provider；balancehub 条目指向中转站",
        },
        File {
            path: config::credentials_path(&root),
            label: "Command Code 凭据",
            key: "credentials",
            rule: "credentials.json 保存各 Provider 的 API Key；原生 /connect 也写入这里",
        },
        File {
            path: config::mcp_path(&root),
            label: "MCP 配置",
            key: "mcp",
            rule: "mcp.json 声明用户级 MCP 服务；项目级配置位于项目目录 .mcp.json",
        },
    ];
    for file in files {
        let allowed_root = file
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| request.home.to_path_buf());
        if output
            .emit(support::document(
                SourceInput {
                    origin: AgentAssetInstallationOrigin::ConfigEntry,
                    native_source_key: file.key,
                    label: file.label,
                    path: file.path,
                    allowed_root: &allowed_root,
                    scope: AgentAssetScope::User,
                    precedence: 10,
                    sensitive: true,
                    source_kind: AgentAssetSourceKind::File,
                    categories: &[],
                },
                AgentConfigurationFormat::Jsonc,
                true,
                file.rule,
            ))
            .is_break()
        {
            return;
        }
    }
}

struct File {
    path: std::path::PathBuf,
    key: &'static str,
    label: &'static str,
    rule: &'static str,
}
