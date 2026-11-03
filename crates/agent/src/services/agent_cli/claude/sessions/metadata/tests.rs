use super::*;
use crate::{models::AgentCliKind, services::agent_cli::contracts::SessionMetadataLookupBudget};
use serde_json::json;
use std::{
    sync::{atomic::AtomicBool, Arc, Barrier},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    cwd: PathBuf,
    prefix: String,
}
impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "balancehub-claude-metadata-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("projects")).unwrap();
        let (raw, encoded) = if cfg!(target_os = "windows") {
            ("C:\\fixture\\", "C--fixture-")
        } else {
            ("/fixture/", "-fixture-")
        };
        Self {
            root,
            cwd: PathBuf::from(format!("{raw}{}", "a".repeat(240 - raw.len()))),
            prefix: format!("{encoded}{}-", "a".repeat(200 - encoded.len())),
        }
    }
    fn write(&self, suffix: &str, file: &str, value: serde_json::Value) -> PathBuf {
        let path = self
            .root
            .join("projects")
            .join(format!("{}{suffix}", self.prefix))
            .join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, value.to_string()).unwrap();
        path
    }
    fn request<'a>(
        &'a self,
        id: &'a str,
        hint: Option<&'a Path>,
        max_bytes: usize,
    ) -> SessionMetadataLookupRequest<'a> {
        SessionMetadataLookupRequest {
            cli_kind: AgentCliKind::ClaudeCode,
            session_id: id,
            workdir: Some(&self.cwd),
            transcript_path_hint: hint,
            previous: None,
            budget: SessionMetadataLookupBudget {
                max_bytes,
                deadline: Instant::now() + Duration::from_secs(5),
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        }
    }
    fn value(&self, id: &str) -> serde_json::Value {
        json!({"sessionId":id,"cwd":self.cwd,"type":"user","message":{"content":"metadata native body"}})
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_missing_project_hint_lifecycle(fixture: &Fixture, project_name: &str) {
    let projects = fixture.root.join("projects");
    let hint = projects.join(project_name).join("session.jsonl");
    let canonical_hint = fixture
        .root
        .canonicalize()
        .unwrap()
        .join("projects")
        .join(project_name)
        .join("session.jsonl");
    let wrong_prefix = projects.join("wrong-prefix/session.jsonl");
    let outside = fixture
        .root
        .join("outside")
        .join(project_name)
        .join("session.jsonl");
    fs::remove_dir(&projects).unwrap();
    for projects_exist in [false, true] {
        if projects_exist {
            fs::create_dir(&projects).unwrap();
        }
        assert!(!hint.parent().unwrap().exists());
        for hint in [&hint, &canonical_hint] {
            assert_eq!(
                lookup(&fixture.root, fixture.request("session", Some(hint), 4096)).unwrap(),
                SessionMetadataLookupResult::NotReady
            );
        }
        for invalid in [&wrong_prefix, &outside] {
            assert_eq!(
                lookup(
                    &fixture.root,
                    fixture.request("session", Some(invalid), 4096)
                ),
                Err(SessionMetadataLookupError::InvalidSource)
            );
        }
    }
    fs::create_dir(hint.parent().unwrap()).unwrap();
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", Some(&hint), 4096)).unwrap(),
        SessionMetadataLookupResult::NotReady
    );
    fs::write(&hint, fixture.value("session").to_string()).unwrap();
    for hint in [&hint, &canonical_hint] {
        let result = lookup(&fixture.root, fixture.request("session", Some(hint), 4096)).unwrap();
        assert!(matches!(result, SessionMetadataLookupResult::Ready { .. }));
    }
}

#[test]
fn short_metadata_missing_project_hints_become_ready_without_losing_source_checks() {
    let mut fixture = Fixture::new("short-missing-hint");
    let (cwd, project_name) = if cfg!(target_os = "windows") {
        ("C:\\fixture\\short_missing", "C--fixture-short-missing")
    } else {
        ("/fixture/short_missing", "-fixture-short-missing")
    };
    fixture.cwd = PathBuf::from(cwd);
    assert_missing_project_hint_lifecycle(&fixture, project_name);
}

#[test]
fn long_metadata_missing_project_hints_become_ready_without_losing_source_checks() {
    let fixture = Fixture::new("long-missing-hint");
    assert_missing_project_hint_lifecycle(&fixture, &format!("{}bun-native", fixture.prefix));
}

#[test]
fn long_metadata_hint_and_no_hint_share_native_origin_and_cursor() {
    let fixture = Fixture::new("ready");
    let file = fixture.write("bun-native", "session.jsonl", fixture.value("session"));
    for hint in [None, Some(file.as_path())] {
        let SessionMetadataLookupResult::Ready { snapshot, cursor } =
            lookup(&fixture.root, fixture.request("session", hint, 4096)).unwrap()
        else {
            panic!("expected verified native session")
        };
        assert_eq!(snapshot.title.as_deref(), Some("metadata native body"));
        assert_eq!(
            snapshot.workdir.as_deref(),
            Some(fixture.cwd.to_string_lossy().as_ref())
        );
        let cursor = cursor.unwrap();
        assert_eq!(cursor.source_len, file.metadata().unwrap().len());
        assert_eq!(cursor.next_offset, cursor.source_len);
    }
}

#[test]
fn metadata_rejects_wrong_id_prefix_and_source_hints_and_keeps_unknown_origin_not_ready() {
    let fixture = Fixture::new("invalid");
    let file = fixture.write("sdk-native", "session.jsonl", fixture.value("other-id"));
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", Some(&file), 4096)),
        Err(SessionMetadataLookupError::InvalidSource)
    );
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", None, 4096)).unwrap(),
        SessionMetadataLookupResult::NotReady
    );
    for path in [
        fixture.root.join("projects/wrong-prefix/session.jsonl"),
        fixture.root.join("outside/session.jsonl"),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, fixture.value("session").to_string()).unwrap();
        assert_eq!(
            lookup(&fixture.root, fixture.request("session", Some(&path), 4096)),
            Err(SessionMetadataLookupError::InvalidSource)
        );
    }
    let mut wrong_origin = fixture.value("session");
    wrong_origin["cwd"] = json!(format!("{}B", fixture.cwd.to_string_lossy()));
    fs::write(&file, wrong_origin.to_string()).unwrap();
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", Some(&file), 4096)),
        Err(SessionMetadataLookupError::InvalidSource)
    );
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", None, 4096)).unwrap(),
        SessionMetadataLookupResult::NotReady
    );
    let unknown =
        json!({"sessionId":"session","type":"user","message":{"content":"unknown origin"}});
    fs::write(&file, unknown.to_string()).unwrap();
    for hint in [None, Some(file.as_path())] {
        assert_eq!(
            lookup(&fixture.root, fixture.request("session", hint, 4096)).unwrap(),
            SessionMetadataLookupResult::NotReady
        );
    }
}

#[test]
fn metadata_requires_unique_proven_candidate_and_rejects_path_session_ids() {
    let fixture = Fixture::new("ambiguous");
    fixture.write("bun-a", "session.jsonl", fixture.value("session"));
    fixture.write("sdk-b", "session.jsonl", fixture.value("session"));
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", None, 4096)),
        Err(SessionMetadataLookupError::InvalidSource)
    );
    for id in ["../session", "parent\\session"] {
        assert_eq!(
            lookup(&fixture.root, fixture.request(id, None, 4096)),
            Err(SessionMetadataLookupError::InvalidSource)
        );
    }
}

#[test]
fn metadata_candidates_share_cumulative_bytes_and_preserve_single_file_pending() {
    let fixture = Fixture::new("bytes");
    for suffix in ["a-first", "b-second"] {
        let file = fixture.write(suffix, "session.jsonl", fixture.value("session"));
        let mut text = fs::read_to_string(&file).unwrap();
        assert!(text.len() < 700);
        text.push_str(&" ".repeat(700 - text.len()));
        fs::write(&file, text).unwrap();
    }
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", None, 1000)),
        Err(SessionMetadataLookupError::TimedOut)
    );
    let result = lookup(&fixture.root, fixture.request("session", None, 100)).unwrap();
    let SessionMetadataLookupResult::Pending { partial, cursor } = result else {
        panic!("one oversized file remains pending")
    };
    assert!(partial.is_none());
    assert_eq!(cursor.source_len, 700);
    assert_eq!(cursor.next_offset, 0);
}

#[test]
fn metadata_cancelled_or_expired_requests_never_publish_ready() {
    let fixture = Fixture::new("guards");
    fixture.write("bun-native", "session.jsonl", fixture.value("session"));
    let request = fixture.request("session", None, 4096);
    request.budget.cancelled.store(true, Ordering::Release);
    assert_eq!(
        lookup(&fixture.root, request),
        Err(SessionMetadataLookupError::Cancelled)
    );
    let mut request = fixture.request("session", None, 4096);
    request.budget.deadline = Instant::now();
    assert_eq!(
        lookup(&fixture.root, request),
        Err(SessionMetadataLookupError::TimedOut)
    );
}

#[test]
fn metadata_cancellation_during_project_enumeration_keeps_the_cancelled_error() {
    use super::super::projects::with_next_project_entry;
    let fixture = Fixture::new("cancel-enumeration");
    fixture.write("bun-native", "session.jsonl", fixture.value("session"));
    let request = fixture.request("session", None, 4096);
    let cancelled = request.budget.cancelled.clone();
    let visited = Arc::new(AtomicBool::new(false));
    let observed = visited.clone();
    let result = with_next_project_entry(
        move || {
            observed.store(true, Ordering::Release);
            cancelled.store(true, Ordering::Release);
        },
        || lookup(&fixture.root, request),
    );
    assert!(visited.load(Ordering::Acquire));
    assert_eq!(result, Err(SessionMetadataLookupError::Cancelled));
    assert!(matches!(
        lookup(&fixture.root, fixture.request("session", None, 4096)),
        Ok(SessionMetadataLookupResult::Ready { .. })
    ));
}

#[test]
fn metadata_cancellation_during_file_reads_keeps_the_cancelled_error() {
    let fixture = Fixture::new("cancel-read");
    let mut value = fixture.value("session");
    value["padding"] = json!("x".repeat(32 * 1024 * 1024));
    fixture.write("bun-native", "session.jsonl", value);
    let request = fixture.request("session", None, 64 * 1024 * 1024);
    let budget = SessionReadBudget::new(
        request.budget.deadline,
        request.budget.cancelled.clone(),
        request.budget.max_bytes as u64,
    );
    let barrier = Arc::new(Barrier::new(2));
    let finished = AtomicBool::new(false);
    let result = std::thread::scope(|threads| {
        let barrier_worker = barrier.clone();
        let bytes = budget.bytes.clone();
        let cancelled = budget.cancelled.clone();
        let finished_ref = &finished;
        let worker = threads.spawn(move || {
            barrier_worker.wait();
            while bytes.load(Ordering::Acquire) == 0 && !finished_ref.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            cancelled.store(true, Ordering::Release);
        });
        barrier.wait();
        let result = with_read_budget(&budget, || {
            with_source_root(&fixture.root, || {
                lookup_with_budget(&fixture.root, request, &budget)
            })
        });
        finished.store(true, Ordering::Release);
        worker.join().unwrap();
        result
    });
    assert!(budget.bytes.load(Ordering::Relaxed) > 0);
    assert_eq!(result, Err(SessionMetadataLookupError::Cancelled));
}

#[cfg(unix)]
#[test]
fn metadata_never_follows_symlink_transcripts_or_project_hints() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("links");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let outside_file = outside.join("session.jsonl");
    fs::write(&outside_file, fixture.value("session").to_string()).unwrap();
    let project = fixture
        .root
        .join("projects")
        .join(format!("{}bun-linked", fixture.prefix));
    symlink(&outside, &project).unwrap();
    assert_eq!(
        lookup(
            &fixture.root,
            fixture.request("session", Some(&project.join("session.jsonl")), 4096)
        ),
        Err(SessionMetadataLookupError::InvalidSource)
    );
    assert_eq!(
        lookup(&fixture.root, fixture.request("session", None, 4096)).unwrap(),
        SessionMetadataLookupResult::NotReady
    );
    let ordinary = fixture
        .root
        .join("projects")
        .join(format!("{}sdk-ordinary", fixture.prefix));
    fs::create_dir_all(&ordinary).unwrap();
    let linked_file = ordinary.join("session.jsonl");
    symlink(&outside_file, &linked_file).unwrap();
    assert_eq!(
        lookup(
            &fixture.root,
            fixture.request("session", Some(&linked_file), 4096)
        ),
        Err(SessionMetadataLookupError::InvalidSource)
    );

    let linked_root = Fixture::new("linked-project-root");
    fs::remove_dir(linked_root.root.join("projects")).unwrap();
    symlink(
        fixture.root.join("projects"),
        linked_root.root.join("projects"),
    )
    .unwrap();
    assert_eq!(
        lookup(
            &linked_root.root,
            linked_root.request("session", Some(&linked_file), 4096)
        ),
        Err(SessionMetadataLookupError::InvalidSource)
    );
}
