use super::{scope, state, with_read_budget, with_source_root, AgentSessionActor};
use crate::{
    models::{
        AgentSessionDetail, AgentSessionDetailRequest, AgentSessionMessageContent,
        AgentSessionMessageRequest,
    },
    services::{agent_cli::contracts::SessionReadBudget, cli_sessions::transcript::Transcript},
};
use std::sync::Arc;

fn with_session<T>(
    actor: AgentSessionActor<'_>,
    revision: &str,
    session_ref: &str,
    budget: &SessionReadBudget,
    read: impl FnOnce(&state::ReferenceState, Arc<Transcript>) -> Result<T, String>,
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
        let resources = state::source_resources(actor, source);
        let adapter = scope::history_adapter(target.cli_kind)?;
        let result = with_source_root(&target.config_root, || {
            let transcript = resources.transcripts.load(
                &reference.row.workspace_id,
                &reference.record,
                adapter,
                budget,
            )?;
            read(&reference, transcript)
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
        |reference, transcript| {
            let mut row = reference.row.clone();
            super::query::apply_activity(std::slice::from_mut(&mut row));
            let indexing = !transcript.complete || transcript.oversized;
            let detail = transcript.read_page(row.session.clone(), request, indexing, budget)?;
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
        |_, transcript| transcript.read_content(request, budget),
    )
}
