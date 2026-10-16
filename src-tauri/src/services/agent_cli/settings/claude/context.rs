use super::*;

pub(super) struct ModelWindow {
    pub capacity: i64,
    pub compact: i64,
    pub approximate: bool,
}

/// Documented Claude Code windows for identifiable model versions. Bare
/// account-dependent aliases remain unresolved unless the user pinned them.
pub(super) fn window(snapshot: &NativeSnapshot, model: &str) -> Option<ModelWindow> {
    let mut model = model.to_ascii_lowercase();
    let (alias, extended) = model
        .strip_suffix("[1m]")
        .map_or((model.as_str(), false), |alias| (alias, true));
    if ["opus", "sonnet", "haiku", "fable"].contains(&alias) {
        let key = format!("ANTHROPIC_DEFAULT_{}_MODEL", alias.to_ascii_uppercase());
        let mapped = snapshot.env(&key, Some(&format!("/env/{key}")))?;
        model = mapped.text().to_ascii_lowercase();
        if extended && !model.ends_with("[1m]") {
            model.push_str("[1m]");
        }
    }
    let native_large = [
        "opus-4-7",
        "opus-4.7",
        "opus-4-8",
        "opus-4.8",
        "opus-5",
        "sonnet-5",
        "haiku-5-5",
        "haiku-5.5",
        "fable-5",
    ]
    .iter()
    .any(|name| model.contains(name));
    let native_small = ["opus-4-6", "opus-4.6", "sonnet-4-6", "sonnet-4.6"]
        .iter()
        .any(|name| model.contains(name));
    if !native_large && !native_small && !model.contains("[1m]") {
        return None;
    }
    let disabled_large = snapshot
        .env(
            "CLAUDE_CODE_DISABLE_1M_CONTEXT",
            Some("/env/CLAUDE_CODE_DISABLE_1M_CONTEXT"),
        )
        .is_some_and(|value| matches!(value.text().as_str(), "1" | "true"));
    let large = (native_large || model.contains("[1m]")) && !disabled_large;
    Some(ModelWindow {
        capacity: if large { 1_000_000 } else { 200_000 },
        compact: if native_large && large {
            967_000
        } else if large {
            1_000_000
        } else {
            200_000
        },
        approximate: native_large && large,
    })
}
