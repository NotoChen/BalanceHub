//! One published removal capability for lists, Agent panels and plan preparation.
use super::source_index::definition_source;
use crate::{models::*, services::agent_cli::environment::mutation::MutationInventory};
use std::path::Path;

pub(super) fn source<'a>(
    snapshot: &'a MutationInventory,
    asset: &AgentAssetRecord,
) -> Result<&'a AgentAssetSource, String> {
    if !cfg!(unix) {
        return Err("当前平台尚未提供文件移除执行器".to_owned());
    }
    if !matches!(
        asset.category,
        AgentAssetCategory::Skill | AgentAssetCategory::Mcp
    ) {
        return Err(match asset.category {
            AgentAssetCategory::Plugin | AgentAssetCategory::Extension => {
                "该 Agent 尚未提供可安全调用的原生插件卸载机制，请使用 Agent 的原生卸载入口"
            }
            _ => "此配置的移除尚未接入，请在 Agent 的原生配置中管理",
        }
        .to_owned());
    }
    if asset.relationships.provided_by.is_some() || asset.relationships.action_owner.is_some() {
        return Err("此资源由插件提供，不能单独删除插件内的文件；请管理所属插件".to_owned());
    }
    if !matches!(
        asset.scope,
        AgentAssetScope::User | AgentAssetScope::Workspace | AgentAssetScope::Local
    ) {
        return Err("系统或组织管理的配置不能从此处移除".to_owned());
    }
    let source = definition_source(snapshot, asset).ok_or("未找到唯一的原生定义来源")?;
    if source.revision.is_missing {
        return Err("来源文件已缺失，请刷新后核对".to_owned());
    }
    if matches!(
        source.origin,
        AgentAssetInstallationOrigin::Bundled | AgentAssetInstallationOrigin::NativePackage
    ) {
        return Err("此资源由安装包管理，请使用对应的卸载入口".to_owned());
    }
    if asset.category == AgentAssetCategory::Skill {
        let file = Path::new(&source.path);
        if file.file_name().is_none_or(|name| name != "SKILL.md") {
            return Err("Skill 来源不是可确认的 SKILL.md 文件".to_owned());
        }
        let parent = file
            .parent()
            .and_then(Path::parent)
            .ok_or("Skill 目录无效")?;
        // Remove only a direct member of a discovered writable Skill root.
        // This excludes bundled .system packages and arbitrary parent folders.
        if !snapshot.inventory.sources.iter().any(|root| {
            root.context_id == source.context_id
                && root.source_kind == AgentAssetSourceKind::Directory
                && root.categories == [AgentAssetCategory::Skill]
                && root.writable
                && Path::new(&root.path) == parent
                && matches!(
                    root.origin,
                    AgentAssetInstallationOrigin::LocalFiles
                        | AgentAssetInstallationOrigin::SharedFiles
                )
        }) {
            return Err("Skill 所在目录不可写或不属于当前原生 Skill 目录".to_owned());
        }
    } else if !source.writable {
        return Err("MCP 定义文件不可写".to_owned());
    }
    Ok(source)
}

pub(super) fn action(snapshot: &MutationInventory, asset: &AgentAssetRecord) -> AgentAssetAction {
    let result = source(snapshot, asset);
    AgentAssetAction {
        action: AgentAssetActionKind::Remove,
        available: result.is_ok(),
        reason: result
            .as_ref()
            .err()
            .map(|_| AgentAssetActionUnavailableReason::SourceUnavailable),
        mechanism_id: Some(format!("catalog-remove:{}", asset.agent_kind.key())),
        confirmation_required: true,
        reload_effect: Some(
            result
                .err()
                .unwrap_or_else(|| "从原生配置移除；共享来源的影响范围将在预览中列出".to_owned()),
        ),
        trust_effect: None,
        selected_installation_id: None,
        risks: Vec::new(),
    }
}
