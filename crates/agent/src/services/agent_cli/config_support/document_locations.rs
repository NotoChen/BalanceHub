//! Structural locations for visual/raw configuration navigation.
//! Reuses the JSON scanner and TOML deserializer, including quoted/dotted keys.
use crate::models::AgentConfigurationFormat;
use serde::{
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, ops::Range};

#[derive(Clone)]
pub(crate) struct KeyRange {
    pub key: Range<usize>,
    pub value: Range<usize>,
}
pub(crate) type Ranges = BTreeMap<Vec<String>, KeyRange>;

pub(crate) fn read(
    text: &str,
    format: AgentConfigurationFormat,
    root: &Value,
    paths: &[Vec<String>],
) -> Result<Ranges, String> {
    if super::text_document::body(text).trim().is_empty() {
        return Ok(Ranges::new());
    }
    if format != AgentConfigurationFormat::Toml {
        return super::json_source::json_key_ranges(
            text,
            format == AgentConfigurationFormat::Jsonc,
        );
    }
    let body = super::text_document::body(text);
    let mut ranges = Ranges::new();
    let parser = body
        .parse::<toml_edit::de::Deserializer>()
        .map_err(|_| "TOML 配置位置无法解析")?;
    Locate {
        path: Vec::new(),
        key: 0..0,
        value: root,
        paths,
        ranges: &mut ranges,
    }
    .deserialize(parser)
    .map_err(|_| "TOML 配置位置无法解析")?;
    let offset = text.len() - body.len();
    for range in ranges.values_mut() {
        range.key.start += offset;
        range.key.end += offset;
        range.value.start += offset;
        range.value.end += offset;
    }
    Ok(ranges)
}

struct Locate<'a> {
    path: Vec<String>,
    key: Range<usize>,
    value: &'a Value,
    paths: &'a [Vec<String>],
    ranges: &'a mut Ranges,
}
impl Locate<'_> {
    fn relevant(&self, path: &[String]) -> bool {
        self.paths
            .iter()
            .any(|target| target.starts_with(path) || path.starts_with(target))
    }
    fn record(self, mut value: Range<usize>) -> Range<usize> {
        let key = if self.key.is_empty() {
            value.start..value.start
        } else {
            self.key
        };
        value.end = value.end.max(key.end);
        self.ranges.insert(
            self.path,
            KeyRange {
                key,
                value: value.clone(),
            },
        );
        value
    }
}
impl<'de> DeserializeSeed<'de> for Locate<'_> {
    type Value = Range<usize>;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        // Implicit TOML tables have no value span. Their actual key and child
        // spans locate them without relying on parser internals.
        if self.value.is_object() {
            return deserializer.deserialize_map(self);
        }
        if self.value.is_array()
            && self
                .paths
                .iter()
                .any(|path| path.starts_with(&self.path) && path.len() > self.path.len())
        {
            return deserializer.deserialize_seq(self);
        }
        // Skip scalar payloads, including a TOML datetime's synthetic serde map.
        let value = toml::Spanned::<IgnoredAny>::deserialize(deserializer)?.span();
        Ok(self.record(value))
    }
}
impl<'de> Visitor<'de> for Locate<'_> {
    type Value = Range<usize>;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a TOML configuration value")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut range = self.key.clone();
        while let Some(key) = map.next_key::<toml::Spanned<String>>()? {
            let mut path = self.path.clone();
            path.push(key.get_ref().clone());
            if !self.relevant(&path) {
                map.next_value::<IgnoredAny>()?;
                continue;
            }
            let value = map.next_value_seed(Locate {
                path,
                key: key.span(),
                value: self.value.get(key.get_ref()).unwrap_or(&Value::Null),
                paths: self.paths,
                ranges: self.ranges,
            })?;
            if range.is_empty() {
                range.start = key.span().start;
            }
            range.end = range.end.max(value.end);
        }
        Ok(self.record(range))
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
        let mut range = self.key.clone();
        for index in 0.. {
            let mut path = self.path.clone();
            path.push(index.to_string());
            if self.relevant(&path) {
                let Some(value) = seq.next_element_seed(Locate {
                    path,
                    key: 0..0,
                    value: self.value.get(index).unwrap_or(&Value::Null),
                    paths: self.paths,
                    ranges: self.ranges,
                })?
                else {
                    break;
                };
                if range.is_empty() {
                    range.start = value.start;
                }
                range.end = range.end.max(value.end);
            } else if seq.next_element::<IgnoredAny>()?.is_none() {
                break;
            }
        }
        Ok(self.record(range))
    }
}
