use super::{scope, state, with_read_budget, with_source_root, AgentSessionActor};
use crate::{
    models::{
        AgentSessionDetail, AgentSessionDetailRequest, AgentSessionMessageContent,
        AgentSessionMessageRequest, AgentSessionSearchHit, AgentSessionSearchRequest,
        AgentSessionSearchResult,
    },
    services::{
        agent_cli::contracts::{SessionReadBudget, SessionTranscriptFormat},
        cli_sessions::{
            record_stream::WindowRequest,
            search::{SearchAccumulator, SearchQuery},
            source::compressed,
            transcript::{
                same_transcript_cursor_revision, session_read_window, window_request_for_cursor,
                window_request_for_revision, Transcript,
            },
        },
    },
};
use std::{collections::HashSet, sync::Arc};

/// Two 32 MiB windows per call keep every IPC request bounded while still
/// making progress through a large session. The caller resumes with the
/// returned cursor until `complete` is true.
const SEARCH_WINDOWS_PER_CALL: u64 = 2;

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

/// Searches from the newest fragment towards the beginning of the file. The
/// caller can resume with `next_cursor`; hits carry the revision of their
/// fragment so selecting one can open that exact window.
pub(crate) fn search(
    actor: AgentSessionActor<'_>,
    request: &AgentSessionSearchRequest,
    budget: &SessionReadBudget,
) -> Result<AgentSessionSearchResult, String> {
    with_read_budget(budget, || {
        let target =
            super::resolve_session_ref(actor, &request.scope_revision, &request.session_ref)?;
        let reference = state::registry()
            .references
            .get(&(
                actor.key(),
                request.scope_revision.clone(),
                request.session_ref.clone(),
            ))
            .cloned()
            .ok_or("会话引用已失效，请刷新")?;
        let scope = scope::validated_scope(actor, &request.scope_revision)?;
        let source = scope
            .public
            .sources
            .iter()
            .find(|source| source.id == reference.row.source_id)
            .ok_or("原生会话来源已变化")?;
        let resources = state::source_resources(actor, source);
        let adapter = scope::history_adapter(target.cli_kind)?;
        let query = SearchQuery::new(&request.query)?;
        if query.is_empty() {
            return Err("请输入搜索关键字".into());
        }
        let terms = SearchAccumulator::new(&query).content_request();

        with_source_root(&target.config_root, || {
            let sources = (adapter.transcript)(&reference.record)?;
            // A whole-session search walks record-aligned byte windows. A
            // multi-file or stateful format has no window model; its reader
            // already searches the transcript it can decode in one pass.
            if sources.len() != 1
                || !sources[0].window_safe
                || sources[0].format != SessionTranscriptFormat::Lines
                || compressed(&sources[0].path)
            {
                return Err("该会话格式不支持按片段搜索".into());
            }

            search_windows(request.cursor.clone(), &terms, budget, |window| {
                resources.transcripts.load(
                    &reference.row.workspace_id,
                    &reference.record,
                    adapter,
                    window,
                    None,
                    budget,
                )
            })
        })
    })
}

/// Walks byte windows towards the beginning of the file. Kept separate from
/// session resolution so the cursor and deduplication rules can be tested
/// with a small transcript loader.
fn search_windows(
    mut cursor: Option<String>,
    terms: &crate::services::agent_cli::contracts::SessionContentSearchRequest,
    budget: &SessionReadBudget,
    mut load: impl FnMut(WindowRequest) -> Result<Arc<Transcript>, String>,
) -> Result<AgentSessionSearchResult, String> {
    let mut hits = Vec::new();
    let mut scanned_windows = 0u64;
    let mut total_windows = 0u64;
    let mut complete = false;
    let mut seen_starts = HashSet::new();
    while scanned_windows < SEARCH_WINDOWS_PER_CALL {
        budget.check()?;
        let window_request = window_request_for_cursor(cursor.as_deref())?;
        let transcript = load(window_request)?;
        scanned_windows = scanned_windows.saturating_add(1);
        let window = transcript.window.map(session_read_window);
        total_windows = total_windows.max(window.as_ref().map_or(1, |value| value.total));
        let revision = transcript.revision().to_owned();
        hits.extend(
            transcript
                .search_hits(terms)
                .into_iter()
                .map(|hit| AgentSessionSearchHit {
                    hit,
                    cursor: revision.clone(),
                    window: window.clone(),
                }),
        );
        let Some(window) = window else {
            // The whole transcript fit in one decode.
            complete = true;
            break;
        };
        if !window.has_earlier {
            complete = true;
            break;
        }
        if !seen_starts.insert(window.start) {
            return Err("会话片段位置重复，已停止搜索".into());
        }
        cursor = Some(format!("{revision}:earlier:{}", window.start));
    }
    budget.check()?;
    Ok(AgentSessionSearchResult {
        hits,
        next_cursor: (!complete).then_some(cursor).flatten(),
        scanned_windows,
        total_windows,
        complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_cli::contracts::{
        SessionTranscriptMessage, SessionTranscriptMutation, SessionTranscriptSource,
    };
    use serde_json::Value;
    use std::{
        fs,
        io::{Seek, SeekFrom, Write},
        path::Path,
        sync::{atomic::AtomicBool, Arc},
        time::{Duration, Instant},
    };

    fn decode(_: &Path, _: u64, bytes: &[u8], _: &mut Value) -> Vec<SessionTranscriptMutation> {
        let value: Value = serde_json::from_slice(bytes).unwrap_or(Value::Null);
        vec![SessionTranscriptMutation::Put {
            message: SessionTranscriptMessage {
                id: value["id"].as_str().unwrap_or_default().to_string(),
                role: crate::models::CliSessionMessageRole::User,
                content: value["content"].as_str().unwrap_or_default().to_string(),
                metadata: Default::default(),
            },
            priority: 0,
        }]
    }

    fn source(path: &Path) -> SessionTranscriptSource {
        SessionTranscriptSource {
            path: path.to_path_buf(),
            parser_version: 1,
            format: SessionTranscriptFormat::Lines,
            window_safe: true,
            sqlite: None,
            decode,
        }
    }

    fn budget() -> SessionReadBudget {
        SessionReadBudget::new(
            Instant::now() + Duration::from_secs(30),
            Arc::new(AtomicBool::new(false)),
            u64::MAX,
        )
    }

    #[test]
    fn whole_session_search_walks_backwards_until_the_file_head() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("search.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(b"{\"id\":\"head\",\"content\":\"needle head\"}\n")
            .unwrap();
        file.set_len(70 * 1024 * 1024).unwrap();
        file.seek(SeekFrom::End(0)).unwrap();
        file.write_all(b"\n{\"id\":\"tail\",\"content\":\"needle tail\"}\n")
            .unwrap();
        drop(file);

        let query = SearchQuery::new("needle").unwrap();
        let terms = SearchAccumulator::new(&query).content_request();
        let budget = budget();
        let mut load = |request| {
            Ok(Arc::new(Transcript::from_sources_with_window(
                "workspace",
                "session",
                &[source(&path)],
                request,
                &budget,
            )?))
        };

        let first = search_windows(None, &terms, &budget, &mut load).unwrap();
        assert_eq!(first.scanned_windows, 2);
        assert!(!first.complete);
        assert!(first.next_cursor.is_some());
        assert_eq!(first.total_windows, 3);
        assert!(first
            .hits
            .iter()
            .any(|hit| hit.hit.snippet.contains("tail")));

        let second = search_windows(first.next_cursor, &terms, &budget, &mut load).unwrap();
        assert!(second.complete);
        assert_eq!(second.next_cursor, None);
        assert!(second
            .hits
            .iter()
            .any(|hit| hit.hit.snippet.contains("head")));
        assert!(second.hits.iter().all(|hit| hit.window.is_some()));
    }
}
