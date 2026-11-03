//! Read native identity fields without deserializing message bodies. This is a
//! bounded header read, not validation of the remaining transcript document.
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use serde_json::{Map, Value};
use std::{
    fmt,
    io::{BufRead, BufReader, Read},
    path::Path,
};

const MAX_HEADER_BYTES: u64 = 1024 * 1024;

pub(crate) fn read_session_header(
    path: &Path,
    fields: &[&str],
    mut ready: impl FnMut(&Map<String, Value>, bool) -> bool,
) -> Result<Option<Map<String, Value>>, String> {
    let file = super::source::reader(path).map_err(|error| format!("读取会话身份失败：{error}"))?;
    // Keep the limit below buffering, and stop as soon as the native identity
    // is available, even if the remainder is huge, missing or malformed.
    let mut reader = BufReader::with_capacity(4096, file.take(MAX_HEADER_BYTES));
    let mut saw_record = false;
    loop {
        super::workbench::check_read_budget()?;
        let available = reader.fill_buf().map_err(|error| error.to_string())?;
        if available.is_empty() {
            if !saw_record {
                return Ok(None);
            }
            return Err("会话元数据未包含可确认的原生身份".into());
        }
        let whitespace = available
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        if whitespace > 0 {
            reader.consume(whitespace);
            continue;
        }
        let mut decoder = serde_json::Deserializer::from_reader(&mut reader);
        saw_record = true;
        let mut found = None;
        let result = Header {
            fields,
            ready: &mut ready,
            found: &mut found,
        }
        .deserialize(&mut decoder);
        if let Some(metadata) = found {
            return Ok(Some(metadata));
        }
        let metadata = result.map_err(|error| format!("读取会话身份元数据失败：{error}"))?;
        if ready(&metadata, true) {
            return Ok(Some(metadata));
        }
    }
}

struct Header<'a> {
    fields: &'a [&'a str],
    ready: &'a mut dyn FnMut(&Map<String, Value>, bool) -> bool,
    found: &'a mut Option<Map<String, Value>>,
}

impl<'de> DeserializeSeed<'de> for Header<'_> {
    type Value = Map<String, Value>;

    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Header<'_> {
    type Value = Map<String, Value>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("原生会话元数据对象")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut metadata = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if self.fields.contains(&key.as_str()) {
                metadata.insert(key, map.next_value::<Value>()?);
                if (self.ready)(&metadata, false) {
                    *self.found = Some(metadata);
                    // Returning Ok would make serde_json require the closing
                    // brace and continue into the body. The owned result above
                    // is the only successful early-exit signal to our caller.
                    return Err(serde::de::Error::custom("native identity collected"));
                }
            } else {
                // A dotted selector reads only the needed native payload
                // fields, without deserializing embedded instructions/bodies.
                let prefix = format!("{key}.");
                let nested = self
                    .fields
                    .iter()
                    .filter_map(|field| field.strip_prefix(&prefix))
                    .collect::<Vec<_>>();
                if nested.is_empty() {
                    map.next_value::<IgnoredAny>()?;
                    continue;
                }
                let mut nested_found = None;
                let result = {
                    let mut ready = |fields: &Map<String, Value>, _complete| {
                        let mut candidate = metadata.clone();
                        candidate.insert(key.clone(), Value::Object(fields.clone()));
                        (self.ready)(&candidate, false)
                    };
                    map.next_value_seed(Header {
                        fields: &nested,
                        ready: &mut ready,
                        found: &mut nested_found,
                    })
                };
                if let Some(fields) = nested_found {
                    metadata.insert(key, Value::Object(fields));
                    *self.found = Some(metadata);
                    return Err(serde::de::Error::custom("native identity collected"));
                }
                metadata.insert(key, Value::Object(result?));
                if (self.ready)(&metadata, false) {
                    *self.found = Some(metadata);
                    return Err(serde::de::Error::custom("native identity collected"));
                }
            }
        }
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent_cli::contracts::SessionReadBudget;
    use crate::services::cli_sessions::workbench::with_read_budget;
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };

    #[test]
    fn native_payload_identity_can_stop_before_an_oversized_or_unfinished_body() {
        let path = std::env::temp_dir().join(format!(
            "balancehub-nested-session-header-{}.jsonl",
            std::process::id()
        ));
        let record = format!(
            r#"{{"type":"session_meta","payload":{{"id":"native","cwd":"/synthetic/project","source":"cli","base_instructions":"{}"#,
            "irrelevant native instructions ".repeat(100_000)
        );
        std::fs::write(&path, record).unwrap();
        let budget = SessionReadBudget::new(
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            32 * 1024,
        );
        let metadata = with_read_budget(&budget, || {
            read_session_header(
                &path,
                &["type", "payload.id", "payload.cwd", "payload.source"],
                |fields, _| {
                    fields.get("payload").is_some_and(|payload| {
                        payload.get("id").is_some()
                            && payload.get("cwd").is_some()
                            && payload.get("source").is_some()
                    })
                },
            )
        })
        .unwrap()
        .unwrap();
        assert_eq!(metadata["payload"]["id"], "native");
        assert_eq!(metadata["payload"]["cwd"], "/synthetic/project");
        assert!(metadata["payload"].get("base_instructions").is_none());
        assert!(budget.bytes.load(Ordering::Relaxed) < 16 * 1024);
        std::fs::remove_file(path).unwrap();
    }
}
