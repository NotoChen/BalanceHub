//! Single compile-time catalog for built-in Agent CLIs.
//!
//! The consumer macro receives the Rust enum variant, serialized key and module name. Keeping
//! those three identities together prevents the model enum and service registry from drifting.

#[macro_export]
macro_rules! for_each_agent_cli {
    ($consumer:ident) => {
        $consumer! {
            Codex => { key: "codex", module: codex },
            ClaudeCode => { key: "claudeCode", module: claude },
            Gemini => { key: "gemini", module: gemini },
            Grok => { key: "grok", module: grok },
            QwenCode => { key: "qwenCode", module: qwen },
            OpenCode => { key: "opencode", module: opencode },
            Pi => { key: "pi", module: pi },
            Crush => { key: "crush", module: crush },
            Copilot => { key: "copilot", module: copilot },
            Droid => { key: "droid", module: droid },
            Qoder => { key: "qoder", module: qoder },
            CodeBuddy => { key: "codebuddy", module: codebuddy },
            OpenClaw => { key: "openclaw", module: openclaw },
            Jcode => { key: "jcode", module: jcode },
            MothX => { key: "mothx", module: mothx },
            KimiCode => { key: "kimiCode", module: kimi },
            StepCode => { key: "stepCode", module: stepcode },
            DevEco => { key: "deveco", module: deveco },
            MiMoCode => { key: "mimoCode", module: mimo },
            AtomCode => { key: "atomcode", module: atomcode },
            Cline => { key: "cline", module: cline },
            PrimeAgent => { key: "primeAgent", module: primeagent },
            Penguin => { key: "penguin", module: penguin },
            Goose => { key: "goose", module: goose },
            Hermes => { key: "hermes", module: hermes },
            Dsh => { key: "dsh", module: dsh },
            Omp => { key: "omp", module: omp },
            Morph => { key: "morph", module: morph },
            MiniMaxCode => { key: "minimaxCode", module: minimax },
            Raycast => { key: "raycast", module: raycast },
            Omo => { key: "omo", module: omo },
            CommandCode => { key: "commandCode", module: commandcode },
        }
    };
}

pub use for_each_agent_cli;
