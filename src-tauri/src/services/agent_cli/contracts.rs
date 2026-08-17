use crate::models::{
    AgentCliKind, CliConfigFile, CliConfigPreview, CliConfigSnapshot, CliSessionSummary, Provider,
    TemporaryCliSessionMode,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default)]
pub(crate) struct EnvironmentPatch {
    set: BTreeMap<String, String>,
    remove: BTreeSet<String>,
}

impl EnvironmentPatch {
    pub(crate) fn set(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        self.remove.remove(&name);
        self.set.insert(name, value.into());
    }

    pub(crate) fn remove(&mut self, name: impl Into<String>) {
        let name = name.into();
        self.set.remove(&name);
        self.remove.insert(name);
    }

    pub(crate) fn set_values(&self) -> impl Iterator<Item = (&str, &str)> {
        self.set
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub(crate) fn removed_names(&self) -> impl Iterator<Item = &str> {
        self.remove.iter().map(String::as_str)
    }
}

pub(crate) struct TemporaryLaunchRequest<'a> {
    pub provider_name: &'a str,
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub model: &'a str,
    pub session_name: &'a str,
    pub resume_id: &'a str,
    pub session_mode: TemporaryCliSessionMode,
    pub auxiliary_file_path: Option<&'a Path>,
}

#[derive(Debug, Clone)]
pub(crate) struct TemporaryLaunchPlan {
    pub args: Vec<String>,
    pub environment: EnvironmentPatch,
    pub auxiliary_file_content: Option<String>,
}

type TemporaryLaunchBuilder =
    for<'a> fn(TemporaryLaunchRequest<'a>) -> Result<TemporaryLaunchPlan, String>;

#[derive(Clone, Copy)]
pub(crate) struct TemporaryLaunchFeatures {
    pub model_selection: bool,
    pub session_resume: bool,
    pub session_name: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct TemporaryLaunchAdapter {
    features: TemporaryLaunchFeatures,
    auxiliary_file_name: Option<&'static str>,
    build_plan: TemporaryLaunchBuilder,
}

impl TemporaryLaunchAdapter {
    pub(crate) const fn new(
        features: TemporaryLaunchFeatures,
        auxiliary_file_name: Option<&'static str>,
        build_plan: TemporaryLaunchBuilder,
    ) -> Self {
        Self {
            features,
            auxiliary_file_name,
            build_plan,
        }
    }

    pub(crate) const fn supports_model_selection(&self) -> bool {
        self.features.model_selection
    }

    pub(crate) const fn supports_session_resume(&self) -> bool {
        self.features.session_resume
    }

    pub(crate) const fn supports_session_name(&self) -> bool {
        self.features.session_name
    }

    pub(crate) const fn auxiliary_file_name(&self) -> Option<&'static str> {
        self.auxiliary_file_name
    }

    pub(crate) fn build_plan(
        &self,
        request: TemporaryLaunchRequest<'_>,
    ) -> Result<TemporaryLaunchPlan, String> {
        (self.build_plan)(request)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentFilePlan {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone)]
pub(crate) enum LivenessResponseSource {
    Stdout,
    File(PathBuf),
}

pub(crate) struct LivenessRequest<'a> {
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub model: &'a str,
    pub prompt: &'a str,
    pub timeout_seconds: u64,
    pub isolated_home: &'a Path,
    pub output_path: &'a Path,
}

#[derive(Debug, Clone)]
pub(crate) struct LivenessPlan {
    pub args: Vec<String>,
    pub environment: EnvironmentPatch,
    pub files: Vec<AgentFilePlan>,
    pub response_source: LivenessResponseSource,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedTokenUsage {
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ParsedLivenessOutput {
    pub response: String,
    pub error: Option<String>,
    pub usage: ParsedTokenUsage,
}

type LivenessPlanBuilder = for<'a> fn(LivenessRequest<'a>) -> Result<LivenessPlan, String>;
type LivenessOutputParser = fn(&str, &str) -> ParsedLivenessOutput;

#[derive(Clone, Copy)]
pub(crate) struct LivenessAdapter {
    build_plan: LivenessPlanBuilder,
    parse_output: LivenessOutputParser,
}

impl LivenessAdapter {
    pub(crate) const fn new(
        build_plan: LivenessPlanBuilder,
        parse_output: LivenessOutputParser,
    ) -> Self {
        Self {
            build_plan,
            parse_output,
        }
    }

    pub(crate) fn build_plan(&self, request: LivenessRequest<'_>) -> Result<LivenessPlan, String> {
        (self.build_plan)(request)
    }

    pub(crate) fn parse_output(&self, response_output: &str, stdout: &str) -> ParsedLivenessOutput {
        (self.parse_output)(response_output, stdout)
    }
}

type SessionLister = fn(AgentCliKind, &Path) -> Result<Vec<CliSessionSummary>, String>;

#[derive(Clone, Copy)]
pub(crate) struct SessionAdapter {
    list: SessionLister,
}

impl SessionAdapter {
    pub(crate) const fn new(list: SessionLister) -> Self {
        Self { list }
    }

    pub(crate) fn list(
        &self,
        cli_kind: AgentCliKind,
        workdir: &Path,
    ) -> Result<Vec<CliSessionSummary>, String> {
        (self.list)(cli_kind, workdir)
    }
}

type ConfigSnapshotReader = fn(AgentCliKind, &[Provider]) -> CliConfigSnapshot;
type ConfigPreviewBuilder = fn(AgentCliKind, &Provider) -> Result<CliConfigPreview, String>;
type ConfigSwitcher =
    fn(AgentCliKind, &Provider, Option<&str>, &[CliConfigFile]) -> Result<(), String>;

#[derive(Clone, Copy)]
pub(crate) struct DefaultConfigAdapter {
    snapshot: ConfigSnapshotReader,
    preview: ConfigPreviewBuilder,
    switch: ConfigSwitcher,
}

impl DefaultConfigAdapter {
    pub(crate) const fn new(
        snapshot: ConfigSnapshotReader,
        preview: ConfigPreviewBuilder,
        switch: ConfigSwitcher,
    ) -> Self {
        Self {
            snapshot,
            preview,
            switch,
        }
    }

    pub(crate) fn snapshot(
        &self,
        cli_kind: AgentCliKind,
        providers: &[Provider],
    ) -> CliConfigSnapshot {
        (self.snapshot)(cli_kind, providers)
    }

    pub(crate) fn preview(
        &self,
        cli_kind: AgentCliKind,
        provider: &Provider,
    ) -> Result<CliConfigPreview, String> {
        (self.preview)(cli_kind, provider)
    }

    pub(crate) fn switch(
        &self,
        cli_kind: AgentCliKind,
        provider: &Provider,
        expected_revision: Option<&str>,
        files: &[CliConfigFile],
    ) -> Result<(), String> {
        (self.switch)(cli_kind, provider, expected_revision, files)
    }
}

type EndpointNormalizer = fn(&str) -> String;

#[derive(Clone, Copy)]
pub(crate) struct EndpointAdapter {
    normalize_base_url: EndpointNormalizer,
}

impl EndpointAdapter {
    pub(crate) const fn new(normalize_base_url: EndpointNormalizer) -> Self {
        Self { normalize_base_url }
    }

    pub(crate) fn normalize_base_url(&self, base_url: &str) -> String {
        (self.normalize_base_url)(base_url)
    }
}
