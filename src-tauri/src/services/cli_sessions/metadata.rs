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
    let file = super::workbench::open_session_file(path)
        .map_err(|error| format!("读取会话身份失败：{error}"))?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    if length == 0 {
        return Ok(None);
    }
    // Keep the limit below buffering, and stop as soon as the native identity
    // is available, even if the remainder is huge, missing or malformed.
    let mut reader = BufReader::with_capacity(4096, file.take(MAX_HEADER_BYTES));
    loop {
        super::workbench::check_read_budget()?;
        let available = reader.fill_buf().map_err(|error| error.to_string())?;
        if available.is_empty() {
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

struct Header<'a, F> {
    fields: &'a [&'a str],
    ready: &'a mut F,
    found: &'a mut Option<Map<String, Value>>,
}

impl<'de, F: FnMut(&Map<String, Value>, bool) -> bool> DeserializeSeed<'de> for Header<'_, F> {
    type Value = Map<String, Value>;

    fn deserialize<D: serde::Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        decoder.deserialize_map(self)
    }
}

impl<'de, F: FnMut(&Map<String, Value>, bool) -> bool> Visitor<'de> for Header<'_, F> {
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
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(metadata)
    }
}
