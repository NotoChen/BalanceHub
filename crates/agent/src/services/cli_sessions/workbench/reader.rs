use super::{scope, state, with_read_budget, with_source_root, AgentSessionActor};
use crate::{
    models::{
        AgentSessionDetail, AgentSessionDetailRequest, AgentSessionMessageContent,
        AgentSessionMessageRequest,
    },
    services::{
        agent_cli::contracts::SessionReadBudget,
        cli_sessions::{
            record_stream::WindowRequest,
            transcript::{
                same_transcript_cursor_revision, window_request_for_cursor,
                window_request_for_revision, Transcript,
            },
        },
    },
};
use std::sync::Arc;

fn with_session<T>(
    actor: AgentSessionActor<'_>,
    revision: &str,
    session_ref: &str,
    anchor: Option<&str>,
    budget: &SessionReadBudget,
    select: impl FnOnce(
        &state::SourceResources,
    ) -> Result<(Option<Arc<Transcript>>, WindowRequest), String>,
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
            let (cached, window) = select(&resources)?;
            let transcript = match cached {
                Some(transcript) => {
                    budget.check()?;
                    transcript
                }
                None => resources.transcripts.load(
                    &reference.row.workspace_id,
                    &reference.record,
                    adapter,
                    window,
                    anchor,
                    budget,
                )?,
            };
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
    // A cursor already selects the page; only a bare search anchor may need a
    // different window than the newest one.
    let anchor = match request.cursor {
        None => request.anchor.as_deref(),
        Some(_) => None,
    };
    with_session(
        actor,
        &request.scope_revision,
        &request.session_ref,
        anchor,
        budget,
        |resources| {
            let cursor = request.cursor.as_deref();
            // Same-transcript pages name the exact revision already decoded
            // for this session; reuse it instead of decoding the same range
            // again.
            let cached = same_transcript_cursor_revision(cursor)
                .and_then(|revision| resources.transcripts.find_by_revision(revision));
            Ok((cached, window_request_for_cursor(cursor)?))
        },
        |reference, transcript| {
            let mut row = reference.row.clone();
            super::query::apply_activity(std::slice::from_mut(&mut row));
            // A windowed transcript is a stable bounded slice, not a partial
            // decode waiting for more records; only a still-growing source is
            // indexed lazily.
            let indexing =
                !transcript.complete && !transcript.truncated && transcript.window.is_none();
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
        None,
        budget,
        |resources| {
            // Expanding a message should read the window the visible page
            // came from, not whichever window is newest.
            Ok((
                resources.transcripts.find_by_revision(&request.revision),
                window_request_for_revision(&request.revision)?,
            ))
        },
        |_, transcript| transcript.read_content(request, budget),
    )
}
