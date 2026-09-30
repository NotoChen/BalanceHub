use serde::{Deserialize, Serialize};

use super::{
    AgentCliKind, AgentRuntimeScope, TemporaryCliInstance, TemporaryCliPreference,
    TemporaryCliTerminalKind, Workspace,
};

/// Backend-issued native identity retained before a terminal or Hook can report
/// a running session. The source identity includes the native data namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionLaunchIdentity {
    pub session_ref: String,
    pub source_identity: String,
    pub native_session_id: String,
    pub runtime_scope: AgentRuntimeScope,
}

impl AgentSessionLaunchIdentity {
    pub(crate) fn matches_source_session(
        &self,
        scope: &AgentRuntimeScope,
        source_identity: &str,
        native_session_id: &str,
    ) -> bool {
        self.runtime_scope == *scope
            && self.source_identity == source_identity
            && self.native_session_id == native_session_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AgentSessionResumeIntent {
    Native {},
    #[serde(rename_all = "camelCase")]
    Provider {
        provider_id: String,
        #[serde(default)]
        api_key_local_id: Option<String>,
    },
}

/// Paths and native IDs are resolved from sessionRef on the backend. Only the
/// executable selection follows the existing exact-path CLI validation flow.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSessionResumeRequest {
    pub request_id: String,
    pub session_ref: String,
    pub scope_revision: String,
    pub cli_path: String,
    pub terminal_kind: TemporaryCliTerminalKind,
    pub intent: AgentSessionResumeIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentSessionResumeState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    /// A terminal may already have received the command. Keep its identity
    /// reservation and reconcile evidence instead of replaying the launch.
    Uncertain,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionResumeResult {
    pub instance: TemporaryCliInstance,
    pub workspaces: Vec<Workspace>,
    pub workspace_error: Option<String>,
    pub preference: Option<TemporaryCliPreference>,
    pub reused: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionResumeOperation {
    pub id: String,
    pub revision: u64,
    pub request_id: String,
    pub session_ref: String,
    pub cli_kind: Option<AgentCliKind>,
    pub state: AgentSessionResumeState,
    pub message: String,
    pub created_at: String,
    pub updated_at: String,
    pub can_cancel: bool,
    /// Only exact source/native-ID matches enter this list. It can contain
    /// several already running instances of the same native session.
    pub runtime_ids: Vec<String>,
    pub result: Option<AgentSessionResumeResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_resume_does_not_accept_provider_credentials_or_native_paths() {
        assert_eq!(
            serde_json::to_value(AgentSessionResumeIntent::Native {}).unwrap(),
            serde_json::json!({ "kind": "native" })
        );
        let request = serde_json::json!({
            "requestId": "request-1",
            "sessionRef": "opaque-session-1",
            "scopeRevision": "scope-1",
            "cliPath": "/fixture/codex",
            "terminalKind": "terminal",
            "intent": { "kind": "native" }
        });
        assert!(serde_json::from_value::<AgentSessionResumeRequest>(request.clone()).is_ok());
        for field in ["workdir", "resumeId", "nativeSessionId", "apiKey", "model"] {
            let mut invalid = request.clone();
            invalid[field] = serde_json::json!("client-controlled");
            assert!(serde_json::from_value::<AgentSessionResumeRequest>(invalid).is_err());
        }
        let mut invalid = request;
        invalid["intent"]["providerId"] = serde_json::json!("provider-1");
        assert!(serde_json::from_value::<AgentSessionResumeRequest>(invalid).is_err());
    }

    #[test]
    fn provider_resume_accepts_only_local_references() {
        let intent = serde_json::json!({
            "kind": "provider", "providerId": "provider-1", "apiKeyLocalId": "key-1"
        });
        assert_eq!(
            serde_json::from_value::<AgentSessionResumeIntent>(intent.clone()).unwrap(),
            AgentSessionResumeIntent::Provider {
                provider_id: "provider-1".to_owned(),
                api_key_local_id: Some("key-1".to_owned()),
            }
        );
        let mut invalid = intent;
        invalid["apiKey"] = serde_json::json!("secret");
        assert!(serde_json::from_value::<AgentSessionResumeIntent>(invalid).is_err());
    }
}
