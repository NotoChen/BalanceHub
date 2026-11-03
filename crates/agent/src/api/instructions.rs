//! 共享指令集命令。

use crate::{
    api::{blocking, configuration::run_configuration},
    models::*,
};
use std::time::Duration;

async fn run<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::time::timeout(Duration::from_secs(15), blocking("共享指令", task))
        .await
        .map_err(|_| "共享指令操作超时，请重新读取状态后再操作".to_owned())?
}

pub(crate) async fn list_agent_instruction_sets() -> Result<Vec<AgentInstructionSet>, String> {
    let catalog = crate::state::services().catalog()?;
    run(move || catalog.instruction_sets()).await
}

pub(crate) async fn save_agent_instruction_set(
    request: AgentInstructionSaveRequest,
) -> Result<AgentInstructionSet, String> {
    let catalog = crate::state::services().catalog()?;
    run(move || catalog.save_instruction_set(request)).await
}

pub(crate) async fn delete_agent_instruction_set(
    id: String,
    expected_revision: String,
) -> Result<(), String> {
    let catalog = crate::state::services().catalog()?;
    run(move || catalog.delete_instruction_set(&id, &expected_revision)).await
}

pub(crate) async fn list_agent_instruction_targets(
    actor: &str,
    request: AgentConfigurationListRequest,
) -> Result<Vec<AgentConfigurationSource>, AgentConfigurationError> {
    let (actor, service) = super::configuration::actor_and_service(actor)?;
    let settings = crate::state::app_data().settings;
    run_configuration(move || service.instruction_targets(&actor, request, &settings)).await
}

pub(crate) async fn begin_agent_instruction_edit(
    actor: &str,
    request: AgentInstructionEditRequest,
) -> Result<AgentConfigurationEdit, AgentConfigurationError> {
    let (actor, service) = super::configuration::actor_and_service(actor)?;
    let catalog = crate::state::services().catalog().map_err(error)?;
    run_configuration(move || {
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
