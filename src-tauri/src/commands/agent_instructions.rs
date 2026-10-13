use crate::{models::*, state::AppState};
use std::time::Duration;
use tauri::{AppHandle, Manager, Window};

async fn run<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::time::timeout(
        Duration::from_secs(15),
        super::run_blocking("共享指令", task),
    )
    .await
    .map_err(|_| "共享指令操作超时，请重新读取状态后再操作".to_owned())?
}

#[tauri::command]
pub(crate) async fn list_agent_instruction_sets(
    app: AppHandle,
) -> Result<Vec<AgentInstructionSet>, String> {
    let catalog = app.state::<AppState>().agent_catalog(&app)?;
    run(move || catalog.instruction_sets()).await
}

#[tauri::command]
pub(crate) async fn save_agent_instruction_set(
    app: AppHandle,
    request: AgentInstructionSaveRequest,
) -> Result<AgentInstructionSet, String> {
    let catalog = app.state::<AppState>().agent_catalog(&app)?;
    run(move || catalog.save_instruction_set(request)).await
}

#[tauri::command]
pub(crate) async fn delete_agent_instruction_set(
    app: AppHandle,
    id: String,
    expected_revision: String,
) -> Result<(), String> {
    let catalog = app.state::<AppState>().agent_catalog(&app)?;
    run(move || catalog.delete_instruction_set(&id, &expected_revision)).await
}

#[tauri::command]
pub(crate) async fn list_agent_instruction_targets(
    app: AppHandle,
    window: Window,
    request: AgentConfigurationListRequest,
) -> Result<Vec<AgentConfigurationSource>, AgentConfigurationError> {
    let (actor, service) = super::agent_configuration::actor_and_service(&app, &window)?;
    let settings = app
        .state::<AppState>()
        .data
        .read()
        .unwrap_or_else(|error| error.into_inner())
        .settings
        .clone();
    super::agent_configuration::run_configuration(move || {
        service.instruction_targets(&actor, request, &settings)
    })
    .await
}

#[tauri::command]
pub(crate) async fn begin_agent_instruction_edit(
    app: AppHandle,
    window: Window,
    request: AgentInstructionEditRequest,
) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
    let (actor, service) = super::agent_configuration::actor_and_service(&app, &window)?;
    let catalog = app.state::<AppState>().agent_catalog(&app).map_err(error)?;
    super::agent_configuration::run_configuration(move || {
        let scheme = catalog
            .instruction_set(&request.id, &request.expected_revision)
            .map_err(error)?;
        service.begin_instruction_edit(&actor, request, &scheme)
    })
    .await
}

fn error(message: String) -> AgentConfigurationError {
    AgentConfigurationError {
        kind: AgentConfigurationErrorKind::SourceChanged,
        message,
        diagnostics: Vec::new(),
    }
}
