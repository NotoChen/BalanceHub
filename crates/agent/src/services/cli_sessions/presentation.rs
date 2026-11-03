//! Native tool facts are decoded once in Rust and reused by all reader surfaces.
use crate::models::*;
use serde_json::Value;

#[cfg(test)]
mod tests;

pub(crate) fn tool_call(
    name: &str,
    input: &Value,
    call_id: Option<String>,
) -> SessionMessageMetadata {
    let lower = name.to_lowercase();
    let kind = if lower.starts_with("mcp__") || lower.starts_with("mcp.") {
        SessionToolKind::Mcp
    } else if lower.contains("patch") || lower.contains("edit") {
        SessionToolKind::Edit
    } else if lower.contains("write") {
        SessionToolKind::Write
    } else if lower.contains("bash")
        || lower.contains("shell")
        || lower.contains("exec")
        || lower == "run"
    {
        SessionToolKind::Shell
    } else if lower.contains("search") || lower.contains("grep") || lower.contains("glob") {
        SessionToolKind::Search
    } else if lower.contains("read") || lower.contains("view") {
        SessionToolKind::Read
    } else if lower.contains("agent") || lower == "task" {
        SessionToolKind::Agent
    } else {
        SessionToolKind::Other
    };
    let parsed = input
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let data = parsed.as_ref().unwrap_or(input);
    let title = [
        "command",
        "cmd",
        "file_path",
        "path",
        "pattern",
        "query",
        "description",
        "prompt",
    ]
    .iter()
    .find_map(|key| data.get(key).and_then(Value::as_str))
    .or_else(|| data.as_str())
    .unwrap_or(name);
    SessionMessageMetadata {
        kind: SessionMessageKind::ToolCall,
        tool_name: Some(name.into()),
        tool_kind: Some(kind),
        call_id,
        title: super::clean_text(title, 180),
        ..Default::default()
    }
}

pub(crate) fn tool_result(value: &Value, call_id: Option<String>) -> SessionMessageMetadata {
    let (exit_code, status) = result_facts(value, 0);
    SessionMessageMetadata {
        kind: SessionMessageKind::ToolResult,
        call_id,
        status,
        exit_code,
        ..Default::default()
    }
}

fn result_facts(value: &Value, depth: usize) -> (Option<i32>, Option<SessionToolStatus>) {
    if depth >= 5 {
        return (None, None);
    }
    let parsed = value
        .as_str()
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let value = parsed.as_ref().unwrap_or(value);
    let nested = ["result", "response", "rawOutput", "output", "Ok", "content"]
        .iter()
        .find_map(|key| value.get(key))
        .map(|value| result_facts(value, depth + 1))
        .unwrap_or_default();
    let code = value
        .get("exit_code")
        .or_else(|| value.get("exitCode"))
        .or_else(|| {
            value
                .get("metadata")
                .and_then(|metadata| metadata.get("exit_code"))
        })
        .and_then(Value::as_i64)
        .and_then(|code| i32::try_from(code).ok())
        .or_else(|| {
            value.as_str().and_then(|text| {
                text.lines().find_map(|line| {
                    line.strip_prefix("Process exited with code ")
                        .or_else(|| line.strip_prefix("Process exit code: "))
                        .and_then(|code| code.trim().parse().ok())
                })
            })
        })
        .or(nested.0);
    let explicit = value.get("status").and_then(Value::as_str);
    let error = value.get("error").or_else(|| value.get("Err"));
    let failed = value
        .get("is_error")
        .or_else(|| value.get("isError"))
        .and_then(Value::as_bool)
        == Some(true)
        || error.is_some_and(|error| !error.is_null() && error != false && error != "")
        || value.get("success").and_then(Value::as_bool) == Some(false)
        || code.is_some_and(|code| code != 0)
        || matches!(explicit, Some("failed" | "error"))
        || nested.1 == Some(SessionToolStatus::Failed);
    let status = if failed {
        Some(SessionToolStatus::Failed)
    } else if matches!(explicit, Some("cancelled" | "canceled")) {
        Some(SessionToolStatus::Cancelled)
    } else if matches!(explicit, Some("running" | "in_progress" | "pending")) {
        Some(SessionToolStatus::Running)
    } else if code == Some(0)
        || value.get("success").and_then(Value::as_bool) == Some(true)
        || matches!(explicit, Some("completed" | "success" | "succeeded"))
    {
        Some(SessionToolStatus::Succeeded)
    } else {
        nested.1
    };
    (code, status)
}

pub(crate) fn file_changes(
    metadata: &SessionMessageMetadata,
    content: &str,
) -> Vec<SessionFileChange> {
    if !matches!(
        metadata.kind,
        SessionMessageKind::ToolCall | SessionMessageKind::ToolResult
    ) {
        return Vec::new();
    }
    if !matches!(
        metadata.tool_kind,
        Some(SessionToolKind::Edit | SessionToolKind::Write)
    ) {
        return Vec::new();
    }
    let value = serde_json::from_str::<Value>(content).ok();
    if let Some(changes) = value
        .as_ref()
        .and_then(|value| value.get("changes"))
        .and_then(Value::as_object)
    {
        return changes
            .iter()
            .filter_map(|(path, change)| {
                if let Some(patch) = change
                    .get("unified_diff")
                    .or_else(|| change.get("diff"))
                    .and_then(Value::as_str)
                {
                    return Some(SessionFileChange {
                        path: path.clone(),
                        before: String::new(),
                        after: String::new(),
                        patch: Some(patch.into()),
                        before_known: true,
                    });
                }
                let content = change.get("content").and_then(Value::as_str)?;
                let kind = change.get("type").and_then(Value::as_str)?;
                match kind {
                    "add" | "delete" => Some(SessionFileChange {
                        path: path.clone(),
                        before: if kind == "delete" {
                            content.into()
                        } else {
                            String::new()
                        },
                        after: if kind == "add" {
                            content.into()
                        } else {
                            String::new()
                        },
                        patch: None,
                        before_known: true,
                    }),
                    _ => None,
                }
            })
            .collect();
    }
    let patch = value
        .as_ref()
        .and_then(|value| value.get("patch").or_else(|| value.get("input")))
        .and_then(Value::as_str)
        .unwrap_or(content);
    if patch.contains("*** Begin Patch")
        || patch.starts_with("diff --git ")
        || patch.starts_with("--- ")
    {
        return patch_changes(patch);
    }
    let Some(value) = value else {
        return Vec::new();
    };
    let path = value
        .get("file_path")
        .or_else(|| value.get("path"))
        .or_else(|| value.get("filePath"))
        .and_then(Value::as_str)
        .unwrap_or("文件");
    let edits = value.get("edits").and_then(Value::as_array);
    let values = edits
        .map(|edits| edits.iter().collect::<Vec<_>>())
        .unwrap_or_else(|| vec![&value]);
    values
        .into_iter()
        .filter_map(|value| {
            let before = value
                .get("old_string")
                .or_else(|| value.get("oldText"))
                .or_else(|| value.get("old_text"))
                .and_then(Value::as_str);
            let after = value
                .get("new_string")
                .or_else(|| value.get("newText"))
                .or_else(|| value.get("new_text"))
                .or_else(|| value.get("content"))
                .and_then(Value::as_str)?;
            Some(SessionFileChange {
                path: path.into(),
                before: before.unwrap_or_default().into(),
                after: after.into(),
                patch: None,
                before_known: before.is_some(),
            })
        })
        .collect()
}

fn patch_changes(patch: &str) -> Vec<SessionFileChange> {
    let mut changes = Vec::<SessionFileChange>::new();
    for line in patch.lines() {
        let path = ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix))
            .or_else(|| {
                line.strip_prefix("diff --git ")
                    .and_then(|line| line.split_once(" b/").map(|(_, path)| path))
            });
        if let Some(path) = path {
            changes.push(SessionFileChange {
                path: path.into(),
                before: String::new(),
                after: String::new(),
                patch: Some(String::new()),
                before_known: true,
            });
        }
        if let Some(change) = changes.last_mut() {
            if line == "*** End Patch" || line.starts_with("*** Move to:") {
                continue;
            }
            if let Some(patch) = &mut change.patch {
                patch.push_str(line);
                patch.push('\n');
            }
        }
    }
    if changes.is_empty() && patch.starts_with("--- ") {
        changes.push(SessionFileChange {
            path: patch
                .lines()
                .find_map(|line| line.strip_prefix("+++ "))
                .unwrap_or("文件改动")
                .into(),
            before: String::new(),
            after: String::new(),
            patch: Some(patch.into()),
            before_known: true,
        });
    }
    changes
}
