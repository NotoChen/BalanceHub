//! Unambiguous native configuration decoding shared by preview and mutation.

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigDocumentFormat {
    Json,
    Jsonc,
    Toml,
}

pub(crate) fn parse(bytes: &[u8], format: ConfigDocumentFormat) -> Option<Value> {
    let text = std::str::from_utf8(bytes).ok()?;
    let value = match format {
        ConfigDocumentFormat::Json => {
            let mut decoder = serde_json::Deserializer::from_str(text);
            let value = UniqueValue::deserialize(&mut decoder).ok()?.0;
            decoder.end().ok()?;
            value
        }
        ConfigDocumentFormat::Jsonc => {
            crate::services::agent_cli::config_support::parse_jsonc_document(text).ok()?
        }
        ConfigDocumentFormat::Toml => {
            let value = text.parse::<toml::Value>().ok()?;
            serde_json::to_value(value).ok()?
        }
    };
    value.is_object().then_some(value)
}

pub(crate) fn serialize(value: &Value, format: ConfigDocumentFormat) -> Option<String> {
    match format {
        ConfigDocumentFormat::Json | ConfigDocumentFormat::Jsonc => {
            serde_json::to_string_pretty(value).ok()
        }
        ConfigDocumentFormat::Toml => toml::to_string_pretty(value).ok(),
    }
}

/// serde_json::Value normally accepts the last duplicate field. Preview and
/// mutation must agree on an unambiguous native configuration document.
struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

struct UniqueVisitor;

impl<'de> Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an unambiguous structured configuration")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Bool(value)))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue(value.into()))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue(value.into()))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(value)
            .map(|value| UniqueValue(Value::Number(value)))
            .ok_or_else(|| E::custom("invalid numeric configuration"))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value.to_string())))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::String(value)))
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<UniqueValue>()? {
            values.push(value.0);
        }
        Ok(UniqueValue(Value::Array(values)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate configuration field"));
            }
            values.insert(key, map.next_value::<UniqueValue>()?.0);
        }
        Ok(UniqueValue(Value::Object(values)))
    }
}
