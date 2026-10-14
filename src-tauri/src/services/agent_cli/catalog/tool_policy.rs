mod receipts;
use super::{
    native::{tool_rules::ToolRuleFormat, NativeCatalogAdapter},
    CatalogService, PublishedCatalog, ReadControl,
};
use crate::{
    models::*,
    services::agent_cli::{
        config_support::native_history,
        definition,
        environment::{
            config_document::{self, ConfigDocumentFormat},
            mutation::{locking::shared_locks, GuardedFile, MutationInspector, MutationInventory},
        },
        maintenance::{
            self,
            files::{self, FileWrite},
            MaintenanceService,
        },
    },
};
pub(super) use receipts::Store;
use receipts::{Intent, Receipt};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path, sync::Arc};

struct Target {
    adapter: &'static NativeCatalogAdapter,
    format: ToolRuleFormat,
    server: String,
    source: GuardedFile,
    guards: Vec<GuardedFile>,
    document: Value,
    rules: Vec<String>,
    external: Vec<String>,
    key: String,
}

pub(crate) struct McpToolSelection {
    pub request: AgentMcpToolsRequest,
    pub disabled: Vec<String>,
}

pub(crate) fn selected_agent(
    publication: &PublishedCatalog,
    request: &AgentMcpToolsRequest,
) -> Result<AgentCliKind, String> {
    Ok(binding(publication, request)?.agent_kind)
}
fn binding<'a>(
    publication: &'a PublishedCatalog,
    request: &AgentMcpToolsRequest,
) -> Result<&'a AgentAssetRecord, String> {
    publication
        .catalog
        .assets
        .iter()
        .find(|asset| asset.id == request.asset_id && asset.category == AgentAssetCategory::Mcp)
        .and_then(|asset| {
            asset
                .bindings
                .iter()
                .find(|binding| binding.id == request.binding_id)
        })
        .map(|binding| &binding.native)
        .ok_or_else(|| "所选 MCP 配置已不存在，请刷新".to_owned())
}
fn tool_names(names: &[String]) -> Result<(), String> {
    if names.is_empty()
        || names.len() > crate::services::agent_cli::mcp_check::MAX_TOOLS
        || names.iter().collect::<BTreeSet<_>>().len() != names.len()
        || names
            .iter()
            .any(|name| name.is_empty() || name.len() > 256 || name.chars().any(char::is_control))
    {
        return Err("工具名称无效或数量过多".to_owned());
    }
    Ok(())
}
fn target(snapshot: &MutationInventory, original: &AgentAssetRecord) -> Result<Target, String> {
    let native = snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == original.stable_id)
        .ok_or("原生 MCP 已变化")?;
    let source = super::source_index::definition_source(snapshot, native)
        .filter(|source| !source.revision.is_missing)
        .ok_or("未找到唯一的 MCP 定义来源")?;
    let context = snapshot
        .inventory
        .contexts
        .iter()
        .find(|context| context.id == native.context_id)
        .ok_or("Agent 上下文不存在")?;
    let adapter = definition(native.agent_kind)
        .environment
        .catalog_adapter()
        .ok_or("此 Agent 没有 MCP 配置适配器")?;
    let format = adapter.tool_rule_format()?;
    let definition =
        super::source_index::definition_anchor(snapshot, native).ok_or("MCP 定义来源不明确")?;
    let mcp_file = GuardedFile::capture_for_read(
        source,
        &snapshot.source_anchors,
        super::package::MAX_FILE_BYTES,
    )
    .map_err(|error| error.message)?;
    let document = adapter
        .parse_document(mcp_file.bytes().ok_or("MCP 配置已缺失")?)
        .ok_or("MCP 配置无法解析")?;
    adapter.declaration_value(&document, definition, context)?;
    let mut guards = Vec::new();
    let mut external = Vec::new();
    let (file, document) = if let Some(path) = format.settings_path(context, native.scope)? {
        let settings = snapshot
            .inventory
            .sources
            .iter()
            .find(|source| {
                source.context_id == context.id
                    && Path::new(&source.path) == path
                    && source.writable
            })
            .ok_or("此作用域的 Claude 权限文件不可写")?;
        guards.push(mcp_file);
        for candidate in snapshot.inventory.sources.iter().filter(|candidate| {
            candidate.context_id == context.id
                && candidate.path != settings.path
                && Path::new(&candidate.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        matches!(
                            name,
                            "settings.json" | "settings.local.json" | "managed-settings.json"
                        )
                    })
        }) {
            let guard = GuardedFile::capture(candidate, &snapshot.source_anchors)
                .map_err(|error| error.message)?;
            let root = json(&guard)?;
            external.extend(format.current(&root, adapter.table, &native.native_id)?);
            guards.push(guard);
        }
        let file = GuardedFile::capture(settings, &snapshot.source_anchors)
            .map_err(|error| error.message)?;
        let root = json(&file)?;
        (file, root)
    } else {
        // Policies stored in the declaration itself need a writable standalone
        // definition. Claude's separate settings file only requires read access
        // to the connection, including a plugin-provided connection.
        super::removal::source(snapshot, native)?;
        (mcp_file, document)
    };
    file.replacement_restriction()?;
    let rules = format.current(&document, adapter.table, &native.native_id)?;
    let key = super::digest(
        format!(
            "{}\0{}\0{}",
            file.pathname_domain(),
            adapter.key,
            native.native_id
        )
        .as_bytes(),
    );
    Ok(Target {
        adapter,
        format,
        server: native.native_id.clone(),
        source: file,
        guards,
        document,
        rules,
        external,
        key,
    })
}
fn json(file: &GuardedFile) -> Result<Value, String> {
    file.bytes()
        .map(|bytes| {
            config_document::parse(bytes, ConfigDocumentFormat::Json)
                .ok_or_else(|| "权限配置无效，请先修复原文".to_owned())
        })
        .unwrap_or_else(|| Ok(serde_json::json!({})))
}

impl CatalogService {
    pub(crate) fn mcp_tool_rules(
        &self,
        request: &AgentMcpToolsRequest,
        publication: &PublishedCatalog,
        inspector: &dyn MutationInspector,
        control: &ReadControl,
    ) -> Result<AgentMcpToolRules, String> {
        tool_names(&request.tools)?;
        control.check()?;
        let snapshot = inspector
            .inspect_for_read(control.cancellation_flag())
            .map_err(|error| error.message)?;
        let target = target(&snapshot, binding(publication, request)?)?;
        let stored = self.tool_rules.read(&target.key)?;
        let hash = target.source.bytes().map(super::digest);
        let owned = stored.reconciled(hash.as_deref()).owned;
        let rules = request
            .tools
            .iter()
            .map(|tool| {
                control.check()?;
                let rule = target.format.rule(&target.server, tool);
                let mut restricted_by = target
                    .rules
                    .iter()
                    .filter(|current| !owned.contains(current))
                    .chain(&target.external)
                    .filter(|rule| target.format.matches(rule, &target.server, tool))
                    .cloned()
                    .collect::<Vec<_>>();
                if target.format.excluded_by_include(
                    &target.document,
                    target.adapter.table,
                    &target.server,
                    tool,
                )? {
                    restricted_by.push("未列入原配置的工具白名单".to_owned());
                }
                restricted_by.sort();
                restricted_by.dedup();
                Ok(AgentMcpToolRule {
                    name: tool.clone(),
                    disabled_here: owned.contains(&rule) && target.rules.contains(&rule),
                    restricted_by,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        control.check()?;
        target.source.revalidate().map_err(|error| error.message)?;
        for guard in &target.guards {
            guard.revalidate().map_err(|error| error.message)?;
        }
        Ok(AgentMcpToolRules {
            binding_id: request.binding_id.clone(),
            rules,
            path: crate::platform::paths::compatible_path(target.source.path())
                .to_string_lossy()
                .into_owned(),
            notes: vec![
                "更改会在 Agent 下次加载 MCP 时生效。原有权限与工具白名单保持原样。".to_owned(),
            ],
        })
    }

    pub(crate) fn plan_mcp_tools(
        self: &Arc<Self>,
        actor: &str,
        maintenance: &MaintenanceService,
        selection: McpToolSelection,
        publication: &PublishedCatalog,
        inspector: Arc<dyn MutationInspector>,
        control: &ReadControl,
    ) -> Result<AgentMaintenancePlan, String> {
        let McpToolSelection { request, disabled } = selection;
        tool_names(&request.tools)?;
        if disabled.iter().any(|name| !request.tools.contains(name)) {
            return Err("停用列表包含未选中的工具".to_owned());
        }
        control.check()?;
        let snapshot = inspector
            .inspect_for_read(control.cancellation_flag())
            .map_err(|error| error.message)?;
        let binding = binding(publication, &request)?;
        let mut target = target(&snapshot, binding)?;
        let expected = self.tool_rules.read(&target.key)?;
        let hash = target.source.bytes().map(super::digest);
        let current = expected.reconciled(hash.as_deref());
        let selected = request
            .tools
            .iter()
            .map(|tool| target.format.rule(&target.server, tool))
            .collect::<BTreeSet<_>>();
        if selected.len() != request.tools.len() {
            return Err("工具名称在 Agent 中归一化后重名，无法单独管理".to_owned());
        }
        let wanted = disabled
            .iter()
            .map(|tool| target.format.rule(&target.server, tool))
            .collect::<BTreeSet<_>>();
        let mut rules = target
            .rules
            .iter()
            .filter(|rule| {
                !(current.owned.contains(rule)
                    && selected.contains(*rule)
                    && !wanted.contains(*rule))
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut owned = current
            .owned
            .iter()
            .filter(|rule| rules.contains(rule))
            .cloned()
            .collect::<Vec<_>>();
        for rule in wanted {
            if !rules.contains(&rule) {
                rules.push(rule.clone());
                owned.push(rule);
            }
        }
        owned.sort();
        owned.dedup();
        if rules == target.rules {
            return Err("没有需要修改的工具规则；原有规则不会被撤销".to_owned());
        }
        target.format.replace(
            &mut target.document,
            target.adapter.table,
            &target.server,
            &rules,
        )?;
        let after = target.adapter.rewrite_tool_rules(
            target.source.bytes(),
            &target.document,
            &target.server,
            &rules,
        )?;
        let pending = Receipt {
            owned: current.owned,
            pending: Some(Intent {
                before: hash,
                after: super::digest(&after),
                owned: owned.clone(),
            }),
        };
        let write = FileWrite {
            id: target.source.pathname_domain(),
            source: target.source,
            after: Some(after),
            executable: None,
            mode: None,
        };
        let mut display = maintenance::preview(
            AgentMaintenanceKind::McpTools,
            format!("调整 {} 的工具", target.server),
            vec![write.change()],
            vec![maintenance::item(&write.id, "保存工具规则")],
        );
        display.refresh.agents.push(binding.agent_kind);
        display.notes.push(
            "启用只撤回 BalanceHub 添加的停用项；已有的权限规则和白名单继续生效。".to_owned(),
        );
        let bytes = target.guards.iter().fold(write.bytes(), |total, guard| {
            total.saturating_add(guard.bytes().map_or(0, <[u8]>::len))
        });
        let catalog = Arc::clone(self);
        control.check()?;
        maintenance.plan(actor, display, bytes, move |context| {
            let locks = shared_locks();
            let domains = write
                .source
                .lock_domains()
                .into_iter()
                .chain(target.guards.iter().flat_map(GuardedFile::lock_domains))
                .collect::<Vec<_>>();
            let _locked = locks
                .acquire(&domains, context.canceled(), context.deadline())
                .map_err(|_| "等待工具配置写入超时或已取消")?;
            for guard in &target.guards {
                guard.revalidate().map_err(|error| error.message)?;
            }
            write.source.revalidate().map_err(|error| error.message)?;
            context.check()?;
            catalog
                .tool_rules
                .replace(&target.key, &expected, &pending)?;
            let mut writes = [write];
            let applied = files::apply_locked(&mut writes, context);
            let observed = GuardedFile::capture_path(
                writes[0].source.allowed_root(),
                writes[0].source.path(),
                super::package::MAX_FILE_BYTES,
            )
            .map_err(|error| error.message)?;
            let hash = observed.bytes().map(native_history::hash);
            catalog.tool_rules.replace(
                &target.key,
                &pending,
                &pending.reconciled(hash.as_deref()),
            )?;
            applied
        })
    }
}
