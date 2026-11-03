//! Agent 命令的传输无关分发。
//!
//! 桌面把每个命令注册成 Tauri command，无界面后端经 HTTP RPC 调用同一份实现。
//! 命令参数沿用前端当前的 IPC 结构；错误可以是纯文本，也可以是带 `kind` 的
//! 结构化对象（资产访问、配置编辑等），由宿主原样回传。

mod assets;
mod catalog;
mod cli;
mod configuration;
mod instructions;
mod lifecycle;
mod maintenance;
mod mcp_check;
mod overview;
mod session_resume;
mod sessions;
mod temporary_cli_settings;
mod usage;
mod wsl;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// 命令可能产生的错误：纯文本或带 `kind` 的结构化对象。
pub type ApiError = Value;
pub type ApiResult = Result<Value, ApiError>;

/// 流式结果（资产目录与资源正文）的接收方。
pub trait ProgressSink: Send + Sync + 'static {
    fn send(&self, event: Value) -> Result<(), String>;
}

struct NullSink;

impl ProgressSink for NullSink {
    fn send(&self, _event: Value) -> Result<(), String> {
        Ok(())
    }
}

pub const COMMANDS: &[&str] = &[
    // 配置
    "list_agent_configuration_sources",
    "read_agent_configuration_source",
    "begin_agent_configuration_edit",
    "read_agent_configuration_form",
    "begin_agent_resource_edit",
    "plan_agent_configuration_save",
    "apply_agent_configuration_plan",
    "discard_agent_configuration_edit",
    "get_agent_configuration_operation",
    "list_agent_configuration_operations",
    "cancel_agent_configuration_operation",
    "open_agent_configuration_source",
    // 共享指令
    "list_agent_instruction_sets",
    "save_agent_instruction_set",
    "delete_agent_instruction_set",
    "list_agent_instruction_targets",
    "begin_agent_instruction_edit",
    // CLI 生命周期
    "set_agent_automatic_update",
    "get_agent_lifecycle_catalog",
    "plan_agent_lifecycle",
    "apply_agent_lifecycle",
    "get_agent_lifecycle_operation",
    "list_agent_lifecycle_operations",
    "cancel_agent_lifecycle_operation",
    // 维护
    "get_agent_maintenance_change",
    "discard_agent_maintenance_plan",
    "list_agent_history",
    "plan_agent_history_restore",
    "list_agent_shared_versions",
    "plan_agent_shared_version",
    "get_agent_skill_origin",
    "save_agent_skill_origin",
    "check_agent_skill_update",
    "plan_agent_skill_update",
    "get_agent_batch_options",
    "plan_agent_batch",
    "apply_agent_maintenance",
    "get_agent_mcp_tool_rules",
    "plan_agent_mcp_tools",
    "plan_agent_session_relocation",
    "get_agent_maintenance_operation",
    "list_agent_maintenance_operations",
    "cancel_agent_maintenance_operation",
    // MCP 检测
    "start_agent_mcp_check",
    "get_agent_mcp_check",
    "cancel_agent_mcp_check",
    // 摘要
    "get_cached_agent_overview",
    "refresh_agent_overview",
    // 会话
    "count_agent_sessions",
    "get_agent_session_scope",
    "query_agent_sessions",
    "get_agent_session_detail",
    "get_agent_session_message_content",
    "cancel_agent_session_query",
    "resume_agent_session",
    "get_agent_session_resume_operation",
    "list_agent_session_resume_operations",
    "cancel_agent_session_resume_operation",
    // WSL
    "get_wsl_inventory",
    "start_wsl_distribution",
    // 临时 CLI 设置
    "get_agent_model_catalog",
    "get_temporary_cli_settings",
    "get_temporary_cli_assets",
    "list_temporary_cli_profiles",
    "save_temporary_cli_profile",
    "delete_temporary_cli_profile",
    // 资产
    "get_agent_environment_inventory",
    "read_agent_environment_asset",
    "read_agent_environment_source",
    "open_agent_environment_asset",
    "open_agent_environment_source",
    "plan_agent_asset",
    "apply_agent_asset",
    "get_agent_asset_operation",
    "list_agent_asset_operations",
    "cancel_agent_asset_operation",
    "verify_agent_asset_operation",
    // 全局资产目录
    "get_agent_asset_catalog",
    "get_agent_catalog_revision",
    "has_agent_catalog_changes",
    "read_agent_catalog_content",
    "cancel_agent_catalog_read",
    "get_agent_catalog_definition",
    "save_agent_catalog_definition",
    "delete_agent_catalog_definition",
    "adopt_agent_catalog_asset",
    "get_agent_catalog_agent_panel",
    "preview_agent_catalog_relation",
    "commit_agent_catalog_relation",
    "plan_agent_catalog",
    "apply_agent_catalog",
    "get_agent_catalog_operation",
    "list_agent_catalog_operations",
    "cancel_agent_catalog_operation",
    "read_agent_mcp_form",
    "render_agent_mcp_form",
    // 会话用量
    "estimate_session_usage",
    // CLI 与环境
    "launch_temporary_cli",
    "preview_temporary_cli_launch",
    "get_cli_session_index_status",
    "clear_cli_session_index",
    "get_cli_runtime_snapshot",
    "get_agent_runtime_snapshot",
    "activate_agent_runtime",
    "get_temporary_cli_instance",
    "forget_workspace",
    "browse_workspace_directories",
    "preview_cli_config",
    "get_cached_cli_tools",
    "probe_cli_tools",
    "probe_terminals",
    "inspect_agent_hook",
    "plan_agent_hook",
    "apply_agent_hook",
    "health_agent_hook",
    "repair_agent_hook",
    "verify_agent_hook",
];

pub async fn dispatch(
    command: &str,
    args: Value,
    client: &str,
    progress: Option<Arc<dyn ProgressSink>>,
) -> ApiResult {
    dispatch_inner(command, &args, client, progress).await
}

async fn dispatch_inner(
    command: &str,
    args: &Value,
    client: &str,
    progress: Option<Arc<dyn ProgressSink>>,
) -> ApiResult {
    match command {
        // ── 配置 ────────────────────────────────────────────────
        "list_agent_configuration_sources" => obj(configuration::list_agent_configuration_sources(
            client,
            arg(args, "request")?,
        )
        .await),
        "read_agent_configuration_source" => {
            obj(configuration::read_agent_configuration_source(client, arg(args, "request")?).await)
        }
        "begin_agent_configuration_edit" => {
            obj(configuration::begin_agent_configuration_edit(client, arg(args, "request")?).await)
        }
        "read_agent_configuration_form" => {
            obj(configuration::read_agent_configuration_form(client, arg(args, "request")?).await)
        }
        "begin_agent_resource_edit" => obj(configuration::begin_agent_resource_edit(
            client,
            arg(args, "request")?,
            arg(args, "requestId")?,
        )
        .await),
        "plan_agent_configuration_save" => {
            obj(configuration::plan_agent_configuration_save(client, arg(args, "request")?).await)
        }
        "apply_agent_configuration_plan" => obj(configuration::apply_agent_configuration_plan(
            client,
            arg(args, "request")?,
        )),
        "discard_agent_configuration_edit" => obj(configuration::discard_agent_configuration_edit(
            client,
            arg(args, "editId")?,
        )),
        "get_agent_configuration_operation" => obj(
            configuration::get_agent_configuration_operation(client, arg(args, "operationId")?)
                .await,
        ),
        "list_agent_configuration_operations" => {
            obj(configuration::list_agent_configuration_operations(client).await)
        }
        "cancel_agent_configuration_operation" => obj(
            configuration::cancel_agent_configuration_operation(client, arg(args, "operationId")?),
        ),
        "open_agent_configuration_source" => {
            obj(configuration::open_agent_configuration_source(client, arg(args, "request")?).await)
        }

        // ── 共享指令 ────────────────────────────────────────────
        "list_agent_instruction_sets" => text(instructions::list_agent_instruction_sets().await),
        "save_agent_instruction_set" => {
            text(instructions::save_agent_instruction_set(arg(args, "request")?).await)
        }
        "delete_agent_instruction_set" => text(
            instructions::delete_agent_instruction_set(
                arg(args, "id")?,
                arg(args, "expectedRevision")?,
            )
            .await,
        ),
        "list_agent_instruction_targets" => {
            obj(instructions::list_agent_instruction_targets(client, arg(args, "request")?).await)
        }
        "begin_agent_instruction_edit" => {
            obj(instructions::begin_agent_instruction_edit(client, arg(args, "request")?).await)
        }

        // ── CLI 生命周期 ────────────────────────────────────────
        "set_agent_automatic_update" => {
            obj(lifecycle::set_agent_automatic_update(arg(args, "request")?).await)
        }
        "get_agent_lifecycle_catalog" => {
            obj(lifecycle::get_agent_lifecycle_catalog(arg(args, "request")?).await)
        }
        "plan_agent_lifecycle" => {
            obj(lifecycle::plan_agent_lifecycle(client, arg(args, "request")?).await)
        }
        "apply_agent_lifecycle" => {
            obj(lifecycle::apply_agent_lifecycle(client, arg(args, "request")?).await)
        }
        "get_agent_lifecycle_operation" => {
            obj(lifecycle::get_agent_lifecycle_operation(client, arg(args, "operationId")?).await)
        }
        "list_agent_lifecycle_operations" => {
            obj(lifecycle::list_agent_lifecycle_operations(client).await)
        }
        "cancel_agent_lifecycle_operation" => obj(lifecycle::cancel_agent_lifecycle_operation(
            client,
            arg(args, "operationId")?,
        )
        .await),

        // ── 维护 ────────────────────────────────────────────────
        "get_agent_maintenance_change" => text(
            maintenance::get_agent_maintenance_change(
                client,
                arg(args, "planToken")?,
                arg(args, "index")?,
            )
            .await,
        ),
        "discard_agent_maintenance_plan" => text(maintenance::discard_agent_maintenance_plan(
            client,
            arg(args, "planToken")?,
        )),
        "list_agent_history" => {
            text(maintenance::list_agent_history(arg(args, "offset")?, arg(args, "limit")?).await)
        }
        "plan_agent_history_restore" => text(
            maintenance::plan_agent_history_restore(
                client,
                arg(args, "historyId")?,
                optional(args, "selectedPaths")?,
            )
            .await,
        ),
        "list_agent_shared_versions" => {
            text(maintenance::list_agent_shared_versions(arg(args, "assetId")?).await)
        }
        "plan_agent_shared_version" => text(
            maintenance::plan_agent_shared_version(
                client,
                arg(args, "assetId")?,
                arg(args, "version")?,
            )
            .await,
        ),
        "get_agent_skill_origin" => text(
            maintenance::get_agent_skill_origin(
                client,
                arg(args, "assetId")?,
                optional(args, "workspace")?,
            )
            .await,
        ),
        "save_agent_skill_origin" => text(
            maintenance::save_agent_skill_origin(
                arg(args, "assetId")?,
                arg(args, "version")?,
                optional(args, "origin")?,
            )
            .await,
        ),
        "check_agent_skill_update" => text(
            maintenance::check_agent_skill_update(
                client,
                arg(args, "assetId")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "plan_agent_skill_update" => text(
            maintenance::plan_agent_skill_update(
                client,
                arg(args, "assetId")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "get_agent_batch_options" => text(
            maintenance::get_agent_batch_options(
                client,
                arg(args, "assetIds")?,
                arg(args, "action")?,
                optional(args, "workspace")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "plan_agent_batch" => text(
            maintenance::plan_agent_batch(
                client,
                arg(args, "requests")?,
                optional(args, "workspace")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "apply_agent_maintenance" => text(maintenance::apply_agent_maintenance(
            client,
            arg(args, "request")?,
        )),
        "get_agent_mcp_tool_rules" => text(
            maintenance::get_agent_mcp_tool_rules(
                client,
                arg(args, "request")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "plan_agent_mcp_tools" => text(
            maintenance::plan_agent_mcp_tools(
                client,
                arg(args, "request")?,
                arg(args, "disabled")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "plan_agent_session_relocation" => {
            text(maintenance::plan_agent_session_relocation(client, arg(args, "request")?).await)
        }
        "get_agent_maintenance_operation" => text(maintenance::get_agent_maintenance_operation(
            client,
            arg(args, "operationId")?,
        )),
        "list_agent_maintenance_operations" => {
            text(maintenance::list_agent_maintenance_operations(client))
        }
        "cancel_agent_maintenance_operation" => text(
            maintenance::cancel_agent_maintenance_operation(client, arg(args, "operationId")?),
        ),

        // ── MCP 检测 ────────────────────────────────────────────
        "start_agent_mcp_check" => text(mcp_check::start_agent_mcp_check(
            client,
            arg(args, "connection")?,
        )),
        "get_agent_mcp_check" => text(mcp_check::get_agent_mcp_check(client, arg(args, "id")?)),
        "cancel_agent_mcp_check" => {
            text(mcp_check::cancel_agent_mcp_check(client, arg(args, "id")?))
        }

        // ── 摘要 ────────────────────────────────────────────────
        "get_cached_agent_overview" => {
            text(overview::get_cached_agent_overview(optional(args, "workspace")?).await)
        }
        "refresh_agent_overview" => text(
            overview::refresh_agent_overview(
                arg(args, "agentKind")?,
                optional(args, "workspace")?,
                arg(args, "force")?,
            )
            .await,
        ),

        // ── 会话 ────────────────────────────────────────────────
        "count_agent_sessions" => {
            text(sessions::count_agent_sessions(client.to_owned(), arg(args, "request")?).await)
        }
        "get_agent_session_scope" => text(
            sessions::get_agent_session_scope(
                client.to_owned(),
                optional(args, "explicitWorkdir")?,
            )
            .await,
        ),
        "query_agent_sessions" => {
            text(sessions::query_agent_sessions(client.to_owned(), arg(args, "request")?).await)
        }
        "get_agent_session_detail" => {
            text(sessions::get_agent_session_detail(client.to_owned(), arg(args, "request")?).await)
        }
        "get_agent_session_message_content" => text(
            sessions::get_agent_session_message_content(client.to_owned(), arg(args, "request")?)
                .await,
        ),
        "cancel_agent_session_query" => {
            sessions::cancel_agent_session_query(client, arg(args, "request")?);
            Ok(Value::Null)
        }
        "resume_agent_session" => {
            text(session_resume::resume_agent_session(client, arg(args, "request")?).await)
        }
        "get_agent_session_resume_operation" => text(
            session_resume::get_agent_session_resume_operation(client, arg(args, "operationId")?)
                .await,
        ),
        "list_agent_session_resume_operations" => {
            text(session_resume::list_agent_session_resume_operations(client).await)
        }
        "cancel_agent_session_resume_operation" => text(
            session_resume::cancel_agent_session_resume_operation(
                client,
                arg(args, "operationId")?,
            )
            .await,
        ),

        // ── WSL ────────────────────────────────────────────────
        "get_wsl_inventory" => text(wsl::get_wsl_inventory(arg(args, "force")?).await),
        "start_wsl_distribution" => text(wsl::start_wsl_distribution(arg(args, "distroId")?).await),

        // ── 临时 CLI 设置 ──────────────────────────────────────
        "get_agent_model_catalog" => text(
            temporary_cli_settings::get_agent_model_catalog(
                arg(args, "context")?,
                arg(args, "refresh")?,
            )
            .await,
        ),
        "get_temporary_cli_settings" => {
            text(temporary_cli_settings::get_temporary_cli_settings(arg(args, "request")?).await)
        }
        "get_temporary_cli_assets" => text(
            temporary_cli_settings::get_temporary_cli_assets(
                arg(args, "request")?,
                arg(args, "refresh")?,
            )
            .await,
        ),
        "list_temporary_cli_profiles" => {
            to_json(temporary_cli_settings::list_temporary_cli_profiles())
        }
        "save_temporary_cli_profile" => {
            text(temporary_cli_settings::save_temporary_cli_profile(arg(args, "profile")?).await)
        }
        "delete_temporary_cli_profile" => {
            text(temporary_cli_settings::delete_temporary_cli_profile(arg(args, "id")?).await)
        }

        // ── 资产 ────────────────────────────────────────────────
        "get_agent_environment_inventory" => text(
            assets::get_agent_environment_inventory(client, optional(args, "workspace")?).await,
        ),
        "read_agent_environment_asset" => {
            obj(assets::read_agent_environment_asset(client, arg(args, "request")?).await)
        }
        "read_agent_environment_source" => {
            obj(assets::read_agent_environment_source(client, arg(args, "request")?).await)
        }
        "open_agent_environment_asset" => {
            obj(assets::open_agent_environment_asset(client, arg(args, "request")?).await)
        }
        "open_agent_environment_source" => {
            obj(assets::open_agent_environment_source(client, arg(args, "request")?).await)
        }
        "plan_agent_asset" => obj(assets::plan_agent_asset(
            client,
            arg(args, "request")?,
            arg(args, "requestId")?,
            arg(args, "agentKind")?,
        )
        .await),
        "apply_agent_asset" => obj(assets::apply_agent_asset(client, arg(args, "request")?)),
        "get_agent_asset_operation" => obj(assets::get_agent_asset_operation(
            client,
            arg(args, "operationId")?,
        )),
        "list_agent_asset_operations" => obj(assets::list_agent_asset_operations(client)),
        "cancel_agent_asset_operation" => obj(assets::cancel_agent_asset_operation(
            client,
            arg(args, "operationId")?,
        )),
        "verify_agent_asset_operation" => {
            obj(assets::verify_agent_asset_operation(client, arg(args, "operationId")?).await)
        }

        // ── 全局资产目录 ────────────────────────────────────────
        "get_agent_asset_catalog" => text(
            catalog::get_agent_asset_catalog(
                client,
                optional(args, "workspace")?,
                progress.unwrap_or_else(|| Arc::new(NullSink)),
            )
            .await,
        ),
        "get_agent_catalog_revision" => {
            text(catalog::get_agent_catalog_revision(client, optional(args, "workspace")?).await)
        }
        "has_agent_catalog_changes" => text(
            catalog::has_agent_catalog_changes(
                client,
                optional(args, "workspace")?,
                arg(args, "revision")?,
            )
            .await,
        ),
        "read_agent_catalog_content" => text(
            catalog::read_agent_catalog_content(
                client,
                arg(args, "request")?,
                progress.unwrap_or_else(|| Arc::new(NullSink)),
            )
            .await,
        ),
        "cancel_agent_catalog_read" => text(catalog::cancel_agent_catalog_read(
            client,
            arg(args, "requestId")?,
        )),
        "get_agent_catalog_definition" => {
            text(catalog::get_agent_catalog_definition(arg(args, "assetId")?).await)
        }
        "save_agent_catalog_definition" => {
            text(catalog::save_agent_catalog_definition(arg(args, "request")?).await)
        }
        "delete_agent_catalog_definition" => {
            text(catalog::delete_agent_catalog_definition(arg(args, "request")?).await)
        }
        "adopt_agent_catalog_asset" => {
            text(catalog::adopt_agent_catalog_asset(arg(args, "request")?).await)
        }
        "get_agent_catalog_agent_panel" => text(
            catalog::get_agent_catalog_agent_panel(
                client,
                arg(args, "request")?,
                arg(args, "requestId")?,
            )
            .await,
        ),
        "preview_agent_catalog_relation" => {
            text(catalog::preview_agent_catalog_relation(client, arg(args, "request")?).await)
        }
        "commit_agent_catalog_relation" => {
            text(catalog::commit_agent_catalog_relation(client, arg(args, "request")?).await)
        }
        "plan_agent_catalog" => text(
            catalog::plan_agent_catalog(client, arg(args, "request")?, arg(args, "requestId")?)
                .await,
        ),
        "apply_agent_catalog" => text(catalog::apply_agent_catalog(client, arg(args, "request")?)),
        "get_agent_catalog_operation" => text(catalog::get_agent_catalog_operation(
            client,
            arg(args, "operationId")?,
        )),
        "list_agent_catalog_operations" => text(catalog::list_agent_catalog_operations(client)),
        "cancel_agent_catalog_operation" => text(catalog::cancel_agent_catalog_operation(
            client,
            arg(args, "operationId")?,
        )),
        "read_agent_mcp_form" => text(catalog::read_agent_mcp_form(
            arg(args, "text")?,
            arg(args, "format")?,
            optional(args, "agentKind")?,
        )),
        "render_agent_mcp_form" => text(catalog::render_agent_mcp_form(
            arg(args, "input")?,
            arg(args, "originalText")?,
            arg(args, "format")?,
            optional(args, "agentKind")?,
        )),

        "estimate_session_usage" => text(usage::estimate_session_usage(arg(args, "usage")?).await),

        // ── CLI 与环境 ─────────────────────────────────────────
        "launch_temporary_cli" => text(cli::launch_temporary_cli(arg(args, "input")?).await),
        "preview_temporary_cli_launch" => {
            text(cli::preview_temporary_cli_launch(arg(args, "input")?).await)
        }
        "get_cli_session_index_status" => text(cli::get_cli_session_index_status().await),
        "clear_cli_session_index" => text(cli::clear_cli_session_index().await),
        "get_cli_runtime_snapshot" => text(cli::get_cli_runtime_snapshot().await),
        "get_agent_runtime_snapshot" => text(cli::get_agent_runtime_snapshot().await),
        "activate_agent_runtime" => {
            text(cli::activate_agent_runtime(arg(args, "runtimeId")?).await)
        }
        "get_temporary_cli_instance" => {
            text(cli::get_temporary_cli_instance(arg(args, "instanceId")?).await)
        }
        "forget_workspace" => text(cli::forget_workspace(arg(args, "path")?).await),
        "browse_workspace_directories" => {
            text(cli::browse_workspace_directories(optional(args, "path")?).await)
        }
        "preview_cli_config" => obj(cli::preview_cli_config(
            client,
            arg(args, "id")?,
            arg(args, "cliKind")?,
            arg(args, "apiKeyLocalId")?,
        )
        .await),
        "get_cached_cli_tools" => text(cli::get_cached_cli_tools().await),
        "probe_cli_tools" => text(cli::probe_cli_tools(arg(args, "deep")?).await),
        "probe_terminals" => text(cli::probe_terminals().await),
        "inspect_agent_hook" => text(cli::inspect_agent_hook(arg(args, "agentKind")?).await),
        "plan_agent_hook" => {
            text(cli::plan_agent_hook(arg(args, "agentKind")?, arg(args, "mutation")?).await)
        }
        "apply_agent_hook" => {
            text(cli::apply_agent_hook(arg(args, "agentKind")?, arg(args, "plan")?).await)
        }
        "health_agent_hook" => text(cli::health_agent_hook(arg(args, "agentKind")?).await),
        "repair_agent_hook" => text(cli::repair_agent_hook(arg(args, "agentKind")?).await),
        "verify_agent_hook" => text(cli::verify_agent_hook(arg(args, "agentKind")?).await),
        _ => Err(Value::String(format!("当前宿主不支持命令：{command}"))),
    }
}

// ── 参数与结果 ──────────────────────────────────────────────────────

pub(crate) fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    let value = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(value).map_err(|error| format!("参数 {name} 无效：{error}"))
}

pub(crate) fn optional<T: DeserializeOwned>(args: &Value, name: &str) -> Result<Option<T>, String> {
    match args.get(name) {
        Some(Value::Null) | None => Ok(None),
        Some(value) => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("参数 {name} 无效：{error}")),
    }
}

fn to_json<T: Serialize>(input: T) -> ApiResult {
    serde_json::to_value(input).map_err(|error| Value::String(format!("结果序列化失败：{error}")))
}

/// 纯文本错误。
fn text<T: Serialize>(result: Result<T, String>) -> ApiResult {
    match result {
        Ok(value) => to_json(value),
        Err(message) => Err(Value::String(message)),
    }
}

/// 带 `kind` 的结构化错误原样回传，前端继续按错误类型展示。
fn obj<T: Serialize, E: Serialize>(result: Result<T, E>) -> ApiResult {
    match result {
        Ok(value) => to_json(value),
        Err(error) => Err(to_json(error)?),
    }
}

/// 读取模型资料：与桌面共用同一份核心实现与缓存。
pub(crate) async fn fetch_model_metadata(ids: Vec<String>) -> Result<Value, String> {
    balancehub_core::api::dispatch(
        "get_model_metadata",
        serde_json::json!({ "ids": ids, "refresh": false, "providerId": null }),
    )
    .await
}

/// 在阻塞线程池执行同步任务。
pub(crate) async fn blocking<T, F>(label: &'static str, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    balancehub_core::runtime::spawn_blocking(task)
        .await
        .map_err(|error| format!("{label}任务异常: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for command in COMMANDS {
            assert!(seen.insert(command), "命令 {command} 重复登记");
        }
    }
}
