//! Native adapters describe supported actions and schemas. Installed CLI
//! versions and historical acceptance records are not runtime permissions.
use crate::models::*;

pub(crate) struct NativeMechanismSpec<'a> {
    pub(crate) key: &'a str,
    pub(crate) agent: AgentCliKind,
    pub(crate) category: AgentAssetCategory,
    pub(crate) schema: u32,
    pub(crate) source_schema: Option<&'a str>,
    pub(crate) argv: &'a [&'a str],
    pub(crate) scopes: &'a [AgentAssetScope],
    pub(crate) reload: &'a str,
}

pub(crate) fn native_records(spec: NativeMechanismSpec<'_>) -> Vec<AgentAssetMechanismRecord> {
    [AgentAssetActionKind::Enable, AgentAssetActionKind::Disable]
        .into_iter()
        .map(|action| AgentAssetMechanismRecord {
            id: AgentMechanismId(format!(
                "{}:{}",
                spec.key,
                if action == AgentAssetActionKind::Enable {
                    "enable"
                } else {
                    "disable"
                }
            )),
            agent_kind: spec.agent,
            category: spec.category,
            action,
            // File mutations require the guarded Unix atomic writer. CLI plans
            // also retain file guards and may compose atomic state changes.
            platforms: vec![AgentHostPlatform::Macos, AgentHostPlatform::Linux],
            scopes: spec.scopes.to_vec(),
            adapter_schema_version: spec.schema,
            source_schema: spec.source_schema.map(str::to_owned),
            executable_argv: spec.argv.iter().map(|arg| (*arg).to_owned()).collect(),
            inspection: "执行前核对配置与权限，执行后重新读取并确认状态".into(),
            idempotent: true,
            commit_point: if spec.argv.is_empty() {
                "同目录原子替换开始"
            } else {
                "原生 CLI 创建子进程前最后可取消点"
            }
            .into(),
            reload_effect: Some(spec.reload.to_owned()),
            redaction_rules: vec!["计划显示状态和影响范围；原始配置与 CLI 输出不进入日志".into()],
        })
        .collect()
}

pub(crate) fn native_removal_record(spec: NativeMechanismSpec<'_>) -> AgentAssetMechanismRecord {
    AgentAssetMechanismRecord {
        id: AgentMechanismId(format!("{}:remove", spec.key)),
        agent_kind: spec.agent,
        category: spec.category,
        action: AgentAssetActionKind::Remove,
        platforms: vec![AgentHostPlatform::Macos, AgentHostPlatform::Linux],
        scopes: spec.scopes.to_vec(),
        adapter_schema_version: spec.schema,
        source_schema: spec.source_schema.map(str::to_owned),
        executable_argv: spec.argv.iter().map(|arg| (*arg).to_owned()).collect(),
        inspection: "执行前核对精确原生身份、配置范围与安装来源，执行后重新盘点确认资产已卸载"
            .into(),
        idempotent: false,
        commit_point: "原生 CLI 创建卸载子进程前最后可取消点".into(),
        reload_effect: Some(spec.reload.to_owned()),
        redaction_rules: vec!["只显示命令结构和影响范围；原始配置与 CLI 输出不进入日志".into()],
    }
}

pub(crate) fn unavailable_error(
    reason: AgentAssetActionUnavailableReason,
) -> AgentAssetMutationError {
    AgentAssetMutationError::unavailable(reason)
}

pub(crate) fn schema_error() -> AgentAssetMutationError {
    unavailable_error(AgentAssetActionUnavailableReason::UnsupportedSchema)
}

pub(crate) fn safe_native_id(id: &str) -> Result<&str, AgentAssetMutationError> {
    if id.is_empty() || id.len() > 512 || id.starts_with('-') || id.chars().any(char::is_control) {
        return Err(unavailable_error(
            AgentAssetActionUnavailableReason::ScopeAmbiguous,
        ));
    }
    Ok(id)
}
