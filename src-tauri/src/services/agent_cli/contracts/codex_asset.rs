//! Private native evidence. The Codex adapter owns decoding and resolution.

use std::{fmt, path::PathBuf};

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum CodexSkillSelector {
    Name(String),
    Path(PathBuf),
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CodexSkillRule {
    pub(crate) selector: CodexSkillSelector,
    pub(crate) enabled: bool,
}

#[derive(Clone)]
pub(crate) enum CodexAssetPayload {
    SkillDefinition {
        canonical_path: PathBuf,
        plugin_id: Option<String>,
    },
    SkillRules {
        rules: Vec<CodexSkillRule>,
        available: bool,
    },
    PluginConfig {
        table: toml::value::Table,
    },
    PluginConfigRoot {
        entry_count: usize,
        valid: bool,
    },
    PluginVersions {
        versions: Vec<String>,
        complete: bool,
    },
    PluginManifest {
        namespace: String,
        valid: bool,
        skill_roots: Vec<PathBuf>,
        mcp_path: Option<PathBuf>,
    },
    PluginMcp {
        table: toml::value::Table,
    },
}

impl fmt::Debug for CodexSkillSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Name(_) => "Name(<private>)",
            Self::Path(_) => "Path(<private>)",
        })
    }
}

impl fmt::Debug for CodexSkillRule {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexSkillRule")
            .field("selector", &self.selector)
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl fmt::Debug for CodexAssetPayload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut output = formatter.debug_struct("CodexAssetPayload");
        match self {
            Self::SkillDefinition { plugin_id, .. } => output
                .field("variant", &"SkillDefinition")
                .field("plugin_provided", &plugin_id.is_some()),
            Self::SkillRules { rules, available } => output
                .field("variant", &"SkillRules")
                .field("rule_count", &rules.len())
                .field("available", available),
            Self::PluginConfig { table } => output
                .field("variant", &"PluginConfig")
                .field("field_count", &table.len()),
            Self::PluginConfigRoot { entry_count, valid } => output
                .field("variant", &"PluginConfigRoot")
                .field("entry_count", entry_count)
                .field("valid", valid),
            Self::PluginVersions { versions, complete } => output
                .field("variant", &"PluginVersions")
                .field("version_count", &versions.len())
                .field("complete", complete),
            Self::PluginManifest {
                valid,
                skill_roots,
                mcp_path,
                ..
            } => output
                .field("variant", &"PluginManifest")
                .field("valid", valid)
                .field("skill_root_count", &skill_roots.len())
                .field("mcp_configured", &mcp_path.is_some()),
            Self::PluginMcp { table } => output
                .field("variant", &"PluginMcp")
                .field("field_count", &table.len()),
        };
        output.finish()
    }
}
