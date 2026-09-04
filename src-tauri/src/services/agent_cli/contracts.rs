use crate::models::{
    AgentAssetCategory, AgentAssetScope, AgentCliKind, CliConfigFile, CliConfigPreview,
    CliConfigSnapshot, CliSessionDetail, CliSessionMessageRole, CliSessionSummary, Provider,
    TemporaryCliSessionMode,
};

#[derive(Debug, Clone)]
pub(crate) struct AgentAssetDeclaration {
    pub category: AgentAssetCategory,
    pub native_id: &'static str,
    pub label: &'static str,
    pub path: PathBuf,
    pub scope: AgentAssetScope,
    pub precedence: u32,
    pub writable: bool,
    pub sensitive: bool,
    pub is_directory: bool,
}

pub(crate) type AgentAssetDiscovery = fn(&Path, Option<&Path>) -> Vec<AgentAssetDeclaration>;

#[derive(Clone, Copy)]
pub(crate) struct EnvironmentAdapter {
    discover: AgentAssetDiscovery,
    package_name: &'static str,
}

impl EnvironmentAdapter {
    pub(crate) const fn new(discover: AgentAssetDiscovery, package_name: &'static str) -> Self {
        Self {
            discover,
            package_name,
        }
    }

    pub(crate) fn discover(
        &self,
        home: &Path,
        workspace: Option<&Path>,
    ) -> Vec<AgentAssetDeclaration> {
        (self.discover)(home, workspace)
    }

    pub(crate) const fn package_name(&self) -> &'static str {
        self.package_name
    }
}
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
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
pub(crate) type SessionMetadataLookup =
    for<'a> fn(
        SessionMetadataLookupRequest<'a>,
    ) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError>;
type SessionSearcher = fn(
    AgentCliKind,
    &Path,
    &str,
    &SessionContentSearchRequest,
    &dyn Fn() -> bool,
) -> Result<SessionContentSearchResult, String>;
type SessionDetailReader =
    fn(AgentCliKind, &Path, &str, SessionReadLimits) -> Result<CliSessionDetail, String>;
type SessionIndexReader = fn(
    AgentCliKind,
    &Path,
    &str,
    Option<&str>,
    &dyn Fn() -> bool,
) -> Result<SessionIndexLoadResult, String>;

#[derive(Debug, Clone)]
pub(crate) struct SessionMetadataLookupRequest<'a> {
    pub cli_kind: AgentCliKind,
    pub session_id: &'a str,
    pub workdir: Option<&'a Path>,
    pub transcript_path_hint: Option<&'a Path>,
    pub previous: Option<&'a SessionMetadataCursor>,
    pub budget: SessionMetadataLookupBudget,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionMetadataLookupBudget {
    pub max_bytes: usize,
    pub deadline: Instant,
    pub cancelled: Arc<AtomicBool>,
}

impl SessionMetadataLookupBudget {
    pub(crate) fn check(&self, bytes: usize) -> Result<(), SessionMetadataLookupError> {
        if self.cancelled.load(Ordering::Acquire) {
            return Err(SessionMetadataLookupError::Cancelled);
        }
        if Instant::now() >= self.deadline || bytes > self.max_bytes {
            return Err(SessionMetadataLookupError::TimedOut);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMetadataSnapshot {
    pub title: Option<String>,
    pub model: Option<String>,
    pub workdir: Option<String>,
    pub last_activity_at: Option<i64>,
    pub source_revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMetadataCursor {
    pub source_identity: String,
    pub source_len: u64,
    pub next_offset: u64,
    pub parser_version: u32,
    pub opaque_state: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionMetadataLookupResult {
    Ready {
        snapshot: SessionMetadataSnapshot,
        cursor: Option<SessionMetadataCursor>,
    },
    Pending {
        partial: Option<SessionMetadataSnapshot>,
        cursor: SessionMetadataCursor,
    },
    NotReady,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionMetadataLookupError {
    Unsupported(String),
    Cancelled,
    TimedOut,
    InvalidSource,
    Io(String),
    Parse(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionSearchTerm {
    pub index: usize,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionContentSearchRequest {
    pub terms: Vec<SessionSearchTerm>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SessionContentSearchResult {
    pub matched_term_indexes: Vec<usize>,
    pub has_content: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionReadLimits {
    pub max_file_bytes: usize,
    pub max_messages: usize,
    pub max_total_chars: usize,
    pub max_message_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionIndexMessage {
    pub id: String,
    pub role: CliSessionMessageRole,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SessionIndexLoadResult {
    Unchanged {
        fingerprint: String,
        source_bytes: u64,
    },
    Updated {
        fingerprint: String,
        source_bytes: u64,
        messages: Vec<SessionIndexMessage>,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct SessionAdapter {
    list: SessionLister,
    search: Option<SessionSearcher>,
    detail: Option<SessionDetailReader>,
    index: Option<SessionIndexReader>,
    metadata: Option<SessionMetadataLookup>,
}

impl SessionAdapter {
    pub(crate) const fn new(
        list: SessionLister,
        search: Option<SessionSearcher>,
        detail: Option<SessionDetailReader>,
        index: Option<SessionIndexReader>,
        metadata: Option<SessionMetadataLookup>,
    ) -> Self {
        Self {
            list,
            search,
            detail,
            index,
            metadata,
        }
    }

    pub(crate) fn list(
        &self,
        cli_kind: AgentCliKind,
        workdir: &Path,
    ) -> Result<Vec<CliSessionSummary>, String> {
        (self.list)(cli_kind, workdir)
    }

    pub(crate) const fn supports_detail(&self) -> bool {
        self.detail.is_some()
    }

    pub(crate) const fn supports_search(&self) -> bool {
        self.search.is_some()
    }

    pub(crate) const fn supports_index(&self) -> bool {
        self.index.is_some()
    }

    pub(crate) const fn supports_metadata(&self) -> bool {
        self.metadata.is_some()
    }

    pub(crate) fn lookup_metadata(
        &self,
        request: SessionMetadataLookupRequest<'_>,
    ) -> Result<SessionMetadataLookupResult, SessionMetadataLookupError> {
        let lookup = self.metadata.ok_or_else(|| {
            SessionMetadataLookupError::Unsupported(
                "当前 Agent CLI 不支持定向会话元数据读取".into(),
            )
        })?;
        lookup(request)
    }

    pub(crate) fn search(
        &self,
        cli_kind: AgentCliKind,
        workdir: &Path,
        session_id: &str,
        request: &SessionContentSearchRequest,
        is_current: &dyn Fn() -> bool,
    ) -> Result<SessionContentSearchResult, String> {
        let searcher = self
            .search
            .ok_or_else(|| "当前 Agent CLI 不支持检索会话正文".to_string())?;
        searcher(cli_kind, workdir, session_id, request, is_current)
    }

    pub(crate) fn detail(
        &self,
        cli_kind: AgentCliKind,
        workdir: &Path,
        session_id: &str,
        limits: SessionReadLimits,
    ) -> Result<CliSessionDetail, String> {
        let reader = self
            .detail
            .ok_or_else(|| "当前 Agent CLI 不支持读取会话详情".to_string())?;
        reader(cli_kind, workdir, session_id, limits)
    }

    pub(crate) fn index(
        &self,
        cli_kind: AgentCliKind,
        workdir: &Path,
        session_id: &str,
        known_fingerprint: Option<&str>,
        is_current: &dyn Fn() -> bool,
    ) -> Result<SessionIndexLoadResult, String> {
        let reader = self
            .index
            .ok_or_else(|| "当前 Agent CLI 不支持建立会话索引".to_string())?;
        reader(cli_kind, workdir, session_id, known_fingerprint, is_current)
    }
}

type ConfigSnapshotReader = fn(AgentCliKind, &[Provider]) -> CliConfigSnapshot;
type ConfigPreviewBuilder = fn(AgentCliKind, &Provider, &str) -> Result<CliConfigPreview, String>;
type ConfigSwitcher =
    fn(AgentCliKind, &Provider, &str, Option<&str>, &[CliConfigFile]) -> Result<(), String>;

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
        api_key_local_id: &str,
    ) -> Result<CliConfigPreview, String> {
        (self.preview)(cli_kind, provider, api_key_local_id)
    }

    pub(crate) fn switch(
        &self,
        cli_kind: AgentCliKind,
        provider: &Provider,
        api_key_local_id: &str,
        expected_revision: Option<&str>,
        files: &[CliConfigFile],
    ) -> Result<(), String> {
        (self.switch)(
            cli_kind,
            provider,
            api_key_local_id,
            expected_revision,
            files,
        )
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
