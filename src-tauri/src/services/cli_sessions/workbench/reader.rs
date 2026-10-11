use super::{scope, state, with_read_budget, with_source_root, AgentSessionActor};
use crate::{
    models::{
        AgentSessionDetail, AgentSessionDetailRequest, AgentSessionMessageContent,
        AgentSessionMessageRequest,
    },
    services::agent_cli::contracts::{SessionContentSearchRequest, SessionReadBudget},
    state::AppState,
};
use tauri::Manager;

fn with_session<T>(
    actor: AgentSessionActor<'_>,
    revision: &str,
    session_ref: &str,
    budget: &SessionReadBudget,
    read: impl FnOnce(&state::ReferenceState, &state::SourceResources, bool) -> Result<T, String>,
) -> Result<T, String> {
    with_read_budget(budget, || {
        let target = super::resolve_session_ref(actor, revision, session_ref)?;
        let reference = state::registry()
            .references
            .get(&(actor.key(), revision.into(), session_ref.into()))
            .cloned()
            .ok_or("会话引用已失效，请刷新")?;
        let scope = scope::validated_scope(actor, revision)?;
        let source = scope
            .public
            .sources
            .iter()
            .find(|source| source.id == reference.row.source_id)
            .ok_or("原生会话来源已变化")?;
        let settings = actor
            .app
            .state::<AppState>()
            .data
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .settings
            .clone();
        let config = super::super::index_config(actor.app, &settings)?;
        let resources = state::source_resources(actor, source, &config);
        let result = with_source_root(&target.config_root, || {
            let progress = resources.index.search(
                &reference.row.workspace_id,
                &reference.record,
                scope::history_adapter(target.cli_kind)?,
                &SessionContentSearchRequest { terms: Vec::new() },
                budget,
            )?;
            read(&reference, &resources, !progress.complete)
        })?;
        budget.check()?;
        super::resolve_session_ref(actor, revision, session_ref)?;
        Ok(result)
    })
}

pub(crate) fn detail(
    actor: AgentSessionActor<'_>,
    request: &AgentSessionDetailRequest,
    budget: &SessionReadBudget,
) -> Result<AgentSessionDetail, String> {
    with_session(
        actor,
        &request.scope_revision,
        &request.session_ref,
        budget,
        |reference, resources, indexing| {
            let mut row = reference.row.clone();
            super::query::apply_activity(std::slice::from_mut(&mut row));
            let detail = resources.index.read_page(
                &row.workspace_id,
                &reference.record.record_key,
                row.session.clone(),
                request,
                indexing,
                budget,
            )?;
            Ok(AgentSessionDetail { row, detail })
        },
    )
}

pub(crate) fn message_content(
    actor: AgentSessionActor<'_>,
    request: &AgentSessionMessageRequest,
    budget: &SessionReadBudget,
) -> Result<AgentSessionMessageContent, String> {
    with_session(
        actor,
        &request.scope_revision,
        &request.session_ref,
        budget,
        |reference, resources, _| {
            resources.index.read_content(
                &reference.row.workspace_id,
                &reference.record.record_key,
                request,
                budget,
            )
        },
    )
}
