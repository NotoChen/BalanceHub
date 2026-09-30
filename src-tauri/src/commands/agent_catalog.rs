//! Opaque, window-bound global asset commands. Paths and native mutations are
//! resolved from the same backend inventory used for catalog publication.
use crate::{
    models::*,
    services::{
        agent_cli::{
            catalog::CatalogService,
            environment::{
                self,
                mutation::{MutationInspector, MutationInventory},
            },
        },
        cli_paths::user_home,
    },
    state::AppState,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tauri::{ipc::Channel, AppHandle, Manager, Window};

#[tauri::command]
pub(crate) async fn get_agent_asset_catalog(
    app: AppHandle,
    window: Window,
    workspace: Option<String>,
    progress: Channel<AgentAssetCatalog>,
) -> Result<AgentAssetCatalog, String> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| "无法创建资产访问会话")?;
    let service = state.agent_catalog(&app)?;
    let registry = Arc::clone(&state.agent_asset_access);
    let settings = state
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let normalized =
        environment::normalize_optional_workspace(workspace.as_deref().map(Path::new))?;
    let permit = tokio::time::timeout(
        Duration::from_secs(25),
        Arc::clone(&service.publication_slots).acquire_owned(),
    )
    .await
    .map_err(|_| "等待资产目录刷新超时")?
    .map_err(|_| "资产目录服务已关闭")?;
    let ticket = registry
        .begin_publish(&actor, normalized.as_deref())
        .map_err(|error| error.to_string())?;
    let sequence = ticket.sequence();
    super::run_blocking("读取全局资产目录", move || {
        // The worker owns the permit even if the frontend goes away during HMR.
        let _permit = permit;
        let started = Instant::now();
        let previous = service
            .published_catalog(&actor, normalized.as_deref().and_then(Path::to_str))
            .ok();
        let restored = if previous.is_none() {
            service.restore_display(normalized.as_deref())
        } else {
            None
        };
        if let Some(snapshot) = &restored {
            let _ = progress.send(snapshot.catalog.clone());
        }
        let (mut catalog, inputs, build, mode) = if let Some(previous) = previous
            .as_ref()
            .filter(|previous| previous.inputs.is_current(&settings))
        {
            (
                previous.catalog.clone(),
                previous.inputs.clone(),
                previous.inventory_build(),
                "内存复用",
            )
        } else if let Some(restored) = restored.and_then(|mut snapshot| {
            service
                .resume_display(&mut snapshot, &settings)
                .then_some(snapshot)
        }) {
            let build = environment::InventoryBuild {
                inventory: restored.catalog.inventory.clone(),
                access_evidence: Vec::new(),
            };
            (restored.catalog, restored.inputs, build, "磁盘恢复")
        } else {
            let environment_before =
                crate::services::agent_cli::catalog::inputs::environment_signature(&settings);
            let build = service.refresh_inventory(
                previous.as_deref(),
                &settings,
                normalized.as_deref(),
                None,
            )?;
            let snapshot = MutationInventory {
                inventory: build.inventory.clone(),
                source_anchors: build
                    .access_evidence
                    .iter()
                    .map(|evidence| (evidence.source_id.clone(), evidence.anchor.clone()))
                    .collect(),
            };
            let (catalog, inputs) =
                service.catalog_for_publication(&snapshot, &settings, &environment_before)?;
            (catalog, inputs, build, "来源更新")
        };
        registry
            .publish_ticket(
                ticket,
                &mut catalog.inventory,
                build.access_evidence.clone(),
            )
            .map_err(|error| error.to_string())?;
        bind_published_access(&mut catalog);
        service.publish_read_snapshot(&actor, sequence, catalog.clone(), build, inputs.clone())?;
        if mode == "来源更新" {
            service.persist_display(&catalog, &inputs);
        }
        if cfg!(debug_assertions) {
            eprintln!(
                "Agent 资源目录：方式={}，耗时={}ms，来源={}，原生条目={}，资源={}",
                mode,
                started.elapsed().as_millis(),
                catalog.inventory.sources.len(),
                catalog.inventory.assets.len(),
                catalog.assets.len()
            );
        }
        Ok(catalog)
    })
    .await
}

fn bind_published_access(catalog: &mut AgentAssetCatalog) {
    let natives = catalog
        .inventory
        .assets
        .iter()
        .map(|native| (native.stable_id.as_str(), native))
        .collect::<std::collections::BTreeMap<_, _>>();
    for binding in catalog
        .assets
        .iter_mut()
        .flat_map(|asset| &mut asset.bindings)
    {
        if let Some(native) = natives.get(binding.id.as_str()) {
            binding.native = (*native).clone();
        }
    }
}

#[tauri::command]
pub(crate) async fn get_agent_catalog_revision(
    app: AppHandle,
    window: Window,
    workspace: Option<String>,
) -> Result<Option<String>, String> {
    let (service, actor) = session(&app, &window)?;
    // Only checks this window's publication. No source scan or CLI process.
    super::run_blocking("确认资源目录就绪", move || {
        service.publication_revision(&actor, workspace.as_deref())
    })
    .await
}

#[tauri::command]
pub(crate) async fn has_agent_catalog_changes(
    app: AppHandle,
    window: Window,
    workspace: Option<String>,
    revision: String,
) -> Result<bool, String> {
    let (service, actor) = session(&app, &window)?;
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    super::run_blocking("核对资源来源变化", move || {
        let Ok(publication) = service.published_catalog(&actor, workspace.as_deref()) else {
            return Ok(true);
        };
        Ok(publication.catalog.revision != revision || !publication.inputs.is_current(&settings))
    })
    .await
}

#[tauri::command]
pub(crate) async fn read_agent_catalog_content(
    app: AppHandle,
    window: Window,
    request: AgentCatalogContentRequest,
    progress: Channel<AgentCatalogContent>,
) -> Result<AgentCatalogContent, String> {
    let (service, actor) = session(&app, &window)?;
    let publication = service.published_catalog(&actor, request.workspace.as_deref())?;
    let inspector = inspector(&app, request.workspace)?;
    let worker_service = Arc::clone(&service);
    service
        .run_read(&actor, "content", &request.request_id, move |control| {
            let started = Instant::now();
            let result = crate::services::agent_cli::configuration::read_catalog_content(
                &worker_service,
                &request.asset_id,
                &publication,
                &control,
                |kind| refresh_read_inventory(inspector.as_ref(), kind),
                |content| {
                    control.check()?;
                    progress
                        .send(content.clone())
                        .map_err(|_| "资源读取窗口已关闭".to_owned())
                },
            );
            if cfg!(debug_assertions) {
                eprintln!(
                    "Agent 资源正文：耗时={}ms，成功={}",
                    started.elapsed().as_millis(),
                    result.is_ok()
                );
            }
            result
        })
        .await
}

#[tauri::command]
pub(crate) fn cancel_agent_catalog_read(
    app: AppHandle,
    window: Window,
    request_id: String,
) -> Result<(), String> {
    let (service, actor) = session(&app, &window)?;
    service.cancel_read(&actor, &request_id);
    Ok(())
}

pub(super) fn refresh_read_inventory(
    inspector: &dyn MutationInspector,
    kind: AgentCliKind,
) -> Result<MutationInventory, String> {
    let build = environment::display_inventory(
        inspector.settings(),
        inspector.workspace(),
        Some(&std::collections::BTreeSet::from([kind])),
        None,
    )?;
    Ok(MutationInventory {
        inventory: build.inventory,
        source_anchors: build
            .access_evidence
            .into_iter()
            .map(|evidence| (evidence.source_id, evidence.anchor))
            .collect(),
    })
}

#[tauri::command]
pub(crate) async fn get_agent_catalog_definition(
    app: AppHandle,
    asset_id: String,
) -> Result<AgentCatalogDefinition, String> {
    let service = app.state::<AppState>().agent_catalog(&app)?;
    super::run_blocking("读取共享定义", move || service.definition(&asset_id)).await
}

#[tauri::command]
pub(crate) async fn save_agent_catalog_definition(
    app: AppHandle,
    request: AgentCatalogSaveRequest,
) -> Result<AgentCatalogDefinition, String> {
    let service = app.state::<AppState>().agent_catalog(&app)?;
    super::run_blocking("保存共享定义", move || service.save(request)).await
}

#[tauri::command]
pub(crate) async fn delete_agent_catalog_definition(
    app: AppHandle,
    request: AgentCatalogDeleteRequest,
) -> Result<(), String> {
    let service = app.state::<AppState>().agent_catalog(&app)?;
    let inspector = inspector(&app, request.workspace.clone())?;
    super::run_blocking("删除未应用的共享定义", move || {
        service.delete_definition(request, inspector)
    })
    .await
}

#[tauri::command]
pub(crate) async fn adopt_agent_catalog_asset(
    app: AppHandle,
    request: AgentCatalogAdoptRequest,
) -> Result<AgentCatalogDefinition, String> {
    let service = app.state::<AppState>().agent_catalog(&app)?;
    let inspector = inspector(&app, request.workspace.clone())?;
    super::run_blocking("收录原生资产", move || {
        service.adopt(request, inspector)
    })
    .await
}

#[tauri::command]
pub(crate) async fn get_agent_catalog_agent_panel(
    app: AppHandle,
    window: Window,
    request: AgentCatalogAgentPanelRequest,
    request_id: String,
) -> Result<AgentCatalogAgentPanel, String> {
    let (service, actor) = session(&app, &window)?;
    let publication = service.published_catalog(&actor, request.workspace.as_deref())?;
    let inspector = inspector(&app, request.workspace.clone())?;
    let worker_service = Arc::clone(&service);
    service
        .run_read(&actor, "panel", &request_id, move |control| {
            control.check()?;
            let scoped = request.agent_kind.map(|kind| {
                publication.snapshot_for_kinds(&std::collections::BTreeSet::from([kind]))
            });
            let snapshot = scoped.as_ref().unwrap_or(&publication.snapshot);
            worker_service.agent_panel(request, inspector, &publication.catalog, snapshot, &control)
        })
        .await
}

#[tauri::command]
pub(crate) async fn preview_agent_catalog_relation(
    app: AppHandle,
    window: Window,
    request: AgentCatalogRelationPreviewRequest,
) -> Result<AgentCatalogRelationPreview, String> {
    let (service, actor) = session(&app, &window)?;
    let inspector = inspector(&app, request.workspace.clone())?;
    super::run_blocking("比较资源对应关系", move || {
        service.preview_relation(&actor, request, inspector)
    })
    .await
}

#[tauri::command]
pub(crate) async fn commit_agent_catalog_relation(
    app: AppHandle,
    window: Window,
    request: AgentCatalogRelationCommitRequest,
) -> Result<AgentCatalogRelationCommitResult, String> {
    let (service, actor) = session(&app, &window)?;
    super::run_blocking("保存资源对应关系", move || {
        service.commit_relation(&actor, request)
    })
    .await
}

#[tauri::command]
pub(crate) async fn plan_agent_catalog(
    app: AppHandle,
    window: Window,
    request: AgentCatalogPlanRequest,
    request_id: String,
) -> Result<AgentCatalogPlan, String> {
    let (service, actor) = session(&app, &window)?;
    let publication = service.published_catalog(&actor, request.workspace.as_deref())?;
    let inspector = inspector(&app, request.workspace.clone())?;
    let worker = Arc::clone(&service);
    let owner = actor.clone();
    service
        .run_read(&actor, "plan", &request_id, move |control| {
            control.check()?;
            if request.target_ids.is_empty() {
                worker.plan_options(&request, &publication, &control)
            } else {
                let kinds = crate::services::agent_cli::catalog::plan_scope::agents(
                    &publication,
                    &request,
                )?;
                let inspector = crate::services::agent_cli::environment::mutation::scoped_inspector(
                    inspector, kinds,
                );
                let result = worker.plan_read(&owner, request, inspector, &publication, &control);
                control.check()?;
                result
            }
        })
        .await
}

#[tauri::command]
pub(crate) fn apply_agent_catalog(
    app: AppHandle,
    window: Window,
    request: AgentCatalogApplyRequest,
) -> Result<AgentCatalogOperation, String> {
    let (service, actor) = session(&app, &window)?;
    let operation = service.start(&actor, &request)?;
    let id = operation.id.clone();
    tauri::async_runtime::spawn_blocking(move || service.run_operation(&id));
    Ok(operation)
}

#[tauri::command]
pub(crate) fn get_agent_catalog_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentCatalogOperation, String> {
    let (service, actor) = session(&app, &window)?;
    service.operation(&actor, &operation_id)
}
#[tauri::command]
pub(crate) fn list_agent_catalog_operations(
    app: AppHandle,
    window: Window,
) -> Result<Vec<AgentCatalogOperation>, String> {
    let (service, actor) = session(&app, &window)?;
    Ok(service.operations(&actor))
}
#[tauri::command]
pub(crate) fn cancel_agent_catalog_operation(
    app: AppHandle,
    window: Window,
    operation_id: String,
) -> Result<AgentCatalogOperation, String> {
    let (service, actor) = session(&app, &window)?;
    service.cancel(&actor, &operation_id)
}

fn session(app: &AppHandle, window: &Window) -> Result<(Arc<CatalogService>, String), String> {
    let state = app.state::<AppState>();
    let actor = state
        .asset_actor(window.label())
        .map_err(|_| "无法创建资产操作会话")?;
    Ok((state.agent_catalog(app)?, actor))
}

pub(super) fn inspector(
    app: &AppHandle,
    workspace: Option<String>,
) -> Result<Arc<dyn MutationInspector>, String> {
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    let home = user_home().ok_or("无法定位原生用户目录")?;
    if workspace
        .as_deref()
        .is_some_and(|path| !Path::new(path).is_absolute())
    {
        return Err("请选择绝对路径工作目录".to_owned());
    }
    Ok(Arc::new(CatalogInspector {
        settings,
        home,
        workspace: workspace.map(PathBuf::from),
    }))
}

struct CatalogInspector {
    settings: AppSettings,
    home: PathBuf,
    workspace: Option<PathBuf>,
}
impl MutationInspector for CatalogInspector {
    fn inspect(&self) -> Result<MutationInventory, AgentAssetMutationError> {
        let build = environment::inventory_with_access(&self.settings, self.workspace.as_deref())
            .map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed)
        })?;
        Ok(MutationInventory {
            inventory: build.inventory,
            source_anchors: build
                .access_evidence
                .into_iter()
                .map(|evidence| (evidence.source_id, evidence.anchor))
                .collect(),
        })
    }
    fn inspect_for_read(
        &self,
        canceled: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<MutationInventory, AgentAssetMutationError> {
        let kinds = crate::services::agent_cli::definitions()
            .iter()
            .map(|definition| definition.kind)
            .collect();
        let build = environment::inventory_for_agents_cancellable(
            &self.settings,
            self.workspace.as_deref(),
            &kinds,
            Some(canceled),
        )
        .map_err(|_| {
            AgentAssetMutationError::new(AgentAssetMutationErrorKind::PreparationFailed)
        })?;
        Ok(MutationInventory {
            inventory: build.inventory,
            source_anchors: build
                .access_evidence
                .into_iter()
                .map(|evidence| (evidence.source_id, evidence.anchor))
                .collect(),
        })
    }
    fn home(&self) -> &Path {
        &self.home
    }
    fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }
    fn settings(&self) -> &AppSettings {
        &self.settings
    }
}

/// Pure draft conversion: no filesystem, catalog refresh, or MCP process access.
#[tauri::command]
pub(crate) fn read_agent_mcp_form(
    text: String,
    format: AgentConfigurationFormat,
    agent_kind: Option<AgentCliKind>,
) -> Result<AgentMcpFormRead, String> {
    crate::services::agent_cli::catalog::mcp_editor::read(&text, format, agent_kind)
}

#[tauri::command]
pub(crate) fn render_agent_mcp_form(
    input: AgentCatalogMcpInput,
    original_text: String,
    format: AgentConfigurationFormat,
    agent_kind: Option<AgentCliKind>,
) -> Result<String, String> {
    crate::services::agent_cli::catalog::mcp_editor::render(
        input,
        &original_text,
        format,
        agent_kind,
    )
}
