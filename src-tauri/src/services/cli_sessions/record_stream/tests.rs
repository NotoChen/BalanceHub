use super::*;
use crate::{
    models::CliSessionMessageRole,
    services::agent_cli::contracts::{SessionIndexMessage, SessionReadBudget},
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "balancehub-record-stream-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn source(&self, name: &str, format: SessionIndexFormat) -> SessionIndexSource {
        SessionIndexSource {
            path: self.0.join(name),
            parser_version: 1,
            format,
            decode,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn decode(
    _path: &Path,
    _sequence: u64,
    bytes: &[u8],
    _state: &mut Value,
) -> Vec<SessionIndexMutation> {
    let value: Value = serde_json::from_slice(bytes).unwrap();
    let Some(id) = value.get("id").and_then(Value::as_str) else {
        return vec![];
    };
    vec![SessionIndexMutation::Put {
        message: SessionIndexMessage {
            id: id.into(),
            role: CliSessionMessageRole::User,
            content: value["text"].as_str().unwrap_or_default().into(),
        },
        priority: 1,
    }]
}
fn contents(batch: &Batch) -> Vec<&str> {
    batch
        .mutations
        .iter()
        .filter_map(|(_, mutation)| match mutation {
            SessionIndexMutation::Put { message, .. } => Some(message.content.as_str()),
            _ => None,
        })
        .collect()
}
fn read_budget() -> SessionReadBudget {
    SessionReadBudget::new(
        Instant::now() + Duration::from_secs(30),
        Arc::new(AtomicBool::new(false)),
        u64::MAX,
    )
}

#[test]
fn batches_survive_checkpoint_serialization_and_only_read_appended_bytes() {
    let fixture = Fixture::new();
    let source = fixture.source("append.jsonl", SessionIndexFormat::JsonLines);
    let text: String = (0..2000)
        .map(|index| format!("{{\"id\":\"{index}\",\"text\":\"{}\"}}\n", "x".repeat(512)))
        .collect();
    fs::write(&source.path, &text).unwrap();
    let first = read_batch_with_limit(&source, None, &|| true, 64 * 1024).unwrap();
    assert!(first.reset && !first.checkpoint.complete);
    assert!(first.checkpoint.offset > 0 && first.checkpoint.offset < text.len() as u64);
    let mut count = contents(&first).len();
    let mut checkpoint: Checkpoint =
        serde_json::from_str(&serde_json::to_string(&first.checkpoint).unwrap()).unwrap();
    for pass in 0..100 {
        assert!(pass < 99);
        let batch = read_batch_with_limit(&source, Some(&checkpoint), &|| true, 64 * 1024).unwrap();
        assert!(!batch.reset);
        assert!(batch.checkpoint.offset >= checkpoint.offset);
        count += contents(&batch).len();
        checkpoint = batch.checkpoint;
        if checkpoint.complete {
            break;
        }
    }
    assert_eq!(count, 2000);
    let unchanged = read_budget();
    let hit = crate::services::cli_sessions::workbench::with_read_budget(&unchanged, || {
        read_batch(&source, Some(&checkpoint), &|| true)
    })
    .unwrap();
    assert!(hit.mutations.is_empty());
    assert_eq!(unchanged.bytes.load(Ordering::Relaxed), 0);
    OpenOptions::new()
        .append(true)
        .open(&source.path)
        .unwrap()
        .write_all(b"{\"id\":\"new\",\"text\":\"appended needle\"}\n")
        .unwrap();
    let budget = read_budget();
    let appended = crate::services::cli_sessions::workbench::with_read_budget(&budget, || {
        read_batch(&source, Some(&checkpoint), &|| true)
    })
    .unwrap();
    assert!(!appended.reset);
    assert!(appended.checkpoint.complete);
    assert_eq!(contents(&appended), ["appended needle"]);
    assert!(
        budget.bytes.load(Ordering::Relaxed) < 32 * 1024,
        "an append must not rescan the megabyte prefix"
    );
}

#[test]
fn an_unfinished_trailing_line_is_retried_when_it_grows() {
    let fixture = Fixture::new();
    let source = fixture.source("partial.jsonl", SessionIndexFormat::JsonLines);
    let first = "{\"id\":\"one\",\"text\":\"first\"}\n";
    fs::write(
        &source.path,
        format!("{first}{{\"id\":\"two\",\"text\":\"part"),
    )
    .unwrap();
    let batch = read_batch(&source, None, &|| true).unwrap();
    assert!(batch.checkpoint.complete);
    assert_eq!(batch.checkpoint.offset, first.len() as u64);
    assert_eq!(contents(&batch), ["first"]);
    OpenOptions::new()
        .append(true)
        .open(&source.path)
        .unwrap()
        .write_all(b"ial\"}\n")
        .unwrap();
    let next = read_batch(&source, Some(&batch.checkpoint), &|| true).unwrap();
    assert!(!next.reset);
    assert_eq!(contents(&next), ["partial"]);
    assert_eq!(next.checkpoint.offset, next.checkpoint.source_bytes);
}

#[test]
fn rewriting_truncating_replacing_or_changing_parser_version_resets_only_the_source() {
    let fixture = Fixture::new();
    let mut source = fixture.source("rewrite.jsonl", SessionIndexFormat::JsonLines);
    fs::write(&source.path, "{\"id\":\"one\",\"text\":\"original\"}\n").unwrap();
    let mut previous = read_batch(&source, None, &|| true).unwrap().checkpoint;
    for text in [
        "{\"id\":\"two\",\"text\":\"rewritten and larger\"}\n",
        "{\"id\":\"tiny\",\"text\":\"x\"}\n",
    ] {
        fs::write(&source.path, text).unwrap();
        let batch = read_batch(&source, Some(&previous), &|| true).unwrap();
        assert!(batch.reset);
        assert_eq!(batch.checkpoint.sequence, 1);
        previous = batch.checkpoint;
    }
    let replacement = fixture.0.join("replacement");
    fs::write(
        &replacement,
        "{\"id\":\"other\",\"text\":\"replacement\"}\n",
    )
    .unwrap();
    fs::remove_file(&source.path).unwrap();
    fs::rename(replacement, &source.path).unwrap();
    let batch = read_batch(&source, Some(&previous), &|| true).unwrap();
    assert!(batch.reset);
    assert_eq!(contents(&batch), ["replacement"]);
    source.parser_version += 1;
    assert!(
        read_batch(&source, Some(&batch.checkpoint), &|| true)
            .unwrap()
            .reset
    );
}

#[test]
fn sources_above_512_mib_start_normally_and_can_be_cancelled_between_batches() {
    let fixture = Fixture::new();
    let source = fixture.source("large.jsonl", SessionIndexFormat::JsonLines);
    let mut file = File::create(&source.path).unwrap();
    file.write_all(b"{\"id\":\"first\",\"text\":\"readable\"}\n")
        .unwrap();
    file.set_len(513 * 1024 * 1024).unwrap();
    drop(file);
    let batch = read_batch_with_limit(&source, None, &|| true, 1).unwrap();
    assert_eq!(contents(&batch), ["readable"]);
    assert!(batch.checkpoint.source_bytes > 512 * 1024 * 1024);
    assert!(!batch.checkpoint.complete);
    assert!(read_batch(&source, Some(&batch.checkpoint), &|| false)
        .err()
        .unwrap()
        .contains("已取消"));
}

#[test]
fn json_messages_stream_across_checkpoints_with_escaped_strings_and_nested_values() {
    let fixture = Fixture::new();
    let source = fixture.source("session.json", SessionIndexFormat::JsonMessages);
    let value = serde_json::json!({"sessionId":"native", "messages":[
        {"id":"one", "text":"引号 \" 和反斜线 \\ 及括号 [ }", "nested":{"a":[1, {"b":"\\\""}]}},
        {"id":"two", "text":"second"}
    ], "summary":"metadata after messages", "numbers":42});
    fs::write(&source.path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    let mut previous = None;
    let mut texts = Vec::new();
    for pass in 0..100 {
        assert!(pass < 99);
        let batch = read_batch_with_limit(&source, previous.as_ref(), &|| true, 16).unwrap();
        texts.extend(contents(&batch).into_iter().map(str::to_owned));
        let complete = batch.checkpoint.complete;
        previous =
            Some(serde_json::from_str(&serde_json::to_string(&batch.checkpoint).unwrap()).unwrap());
        if complete {
            break;
        }
    }
    assert_eq!(
        texts,
        [value["messages"][0]["text"].as_str().unwrap(), "second"]
    );
    let mut metadata = Vec::new();
    assert!(
        !read_json_messages_limited(&source.path, usize::MAX, &|| true, |value| metadata
            .push(value))
        .unwrap()
    );
    assert!(metadata
        .iter()
        .any(|value| value["summary"] == "metadata after messages"));
    assert!(metadata.iter().any(|value| value["sessionId"] == "native"));
    assert!(read_json_messages_limited(&source.path, 80, &|| true, |_| {}).unwrap());
}

#[test]
fn oversized_jsonl_and_json_records_yield_and_preserve_the_next_message() {
    let fixture = Fixture::new();
    for format in [
        SessionIndexFormat::JsonLines,
        SessionIndexFormat::JsonMessages,
    ] {
        let source = fixture.source(
            if format == SessionIndexFormat::JsonLines {
                "oversized.jsonl"
            } else {
                "oversized.json"
            },
            format,
        );
        let mut file = File::create(&source.path).unwrap();
        if format == SessionIndexFormat::JsonMessages {
            file.write_all(b"{\"messages\":[").unwrap();
        }
        file.write_all(b"{\"id\":\"huge\",\"text\":\"").unwrap();
        let block = vec![b'x'; 1024 * 1024];
        for _ in 0..33 {
            file.write_all(&block).unwrap();
        }
        file.write_all(b"\"}").unwrap();
        file.write_all(if format == SessionIndexFormat::JsonLines {
            b"\n"
        } else {
            b","
        })
        .unwrap();
        file.write_all(b"{\"id\":\"tail\",\"text\":\"searchable tail\"}")
            .unwrap();
        if format == SessionIndexFormat::JsonMessages {
            file.write_all(b"]}").unwrap();
        }
        drop(file);
        let mut previous = None;
        let mut texts = Vec::new();
        let mut passes = 0;
        loop {
            passes += 1;
            assert!(passes < 100);
            let batch =
                read_batch_with_limit(&source, previous.as_ref(), &|| true, 64 * 1024).unwrap();
            texts.extend(contents(&batch).into_iter().map(str::to_owned));
            let complete = batch.checkpoint.complete;
            previous = Some(batch.checkpoint);
            if complete {
                break;
            }
        }
        assert!(passes > 1);
        assert_eq!(texts, ["searchable tail"]);
        assert_eq!(previous.unwrap().skipped_records, 1);
    }
}
