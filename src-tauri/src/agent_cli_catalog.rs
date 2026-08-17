//! Single compile-time catalog for built-in Agent CLIs.
//!
//! The consumer macro receives the Rust enum variant, serialized key and module name. Keeping
//! those three identities together prevents the model enum and service registry from drifting.

macro_rules! for_each_agent_cli {
    ($consumer:ident) => {
        $consumer! {
            Codex => { key: "codex", module: codex },
            ClaudeCode => { key: "claudeCode", module: claude },
            Gemini => { key: "gemini", module: gemini },
            Grok => { key: "grok", module: grok },
        }
    };
}

pub(crate) use for_each_agent_cli;
