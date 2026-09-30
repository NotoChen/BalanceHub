//! Registry-backed, user-scope managed Hook adapters.
//!
//! Command handlers call this registry once; Agent-specific schema and
//! ownership behavior stays inside the selected adapter.

mod catalog_link;
mod codex;
mod common;
mod generic;
mod locking;

use crate::models::{AgentCliKind, AgentHookInspection, AgentHookMutation, AgentHookPlan};
pub(crate) use catalog_link::{
    configuration_ownership_guard, prepare_catalog_change, prepare_catalog_policy_change,
};
pub use codex::CodexHookService;
use generic::{GenericAgent, GenericHookService};

pub(crate) enum ManagedHookService {
    Codex(CodexHookService),
    Generic(GenericHookService),
}

pub fn inspect(service: &ManagedHookService) -> AgentHookInspection {
    match service {
        ManagedHookService::Codex(service) => service.inspect(),
        ManagedHookService::Generic(service) => service.inspect(),
    }
}

pub fn plan(service: &ManagedHookService, mutation: AgentHookMutation) -> AgentHookPlan {
    match service {
        ManagedHookService::Codex(service) => service.plan(mutation),
        ManagedHookService::Generic(service) => service.plan(mutation),
    }
}

pub fn apply(
    service: &ManagedHookService,
    plan: AgentHookPlan,
) -> Result<AgentHookInspection, String> {
    match service {
        ManagedHookService::Codex(service) => service.apply(plan),
        ManagedHookService::Generic(service) => service.apply(plan),
    }
}

pub fn helper_from_process_args() -> Option<i32> {
    codex::helper_from_process_args()
}

pub fn app_service(
    app: &tauri::AppHandle,
    agent_kind: AgentCliKind,
) -> Result<ManagedHookService, String> {
    match agent_kind {
        AgentCliKind::Codex => Ok(ManagedHookService::Codex(CodexHookService::from_app(app)?)),
        AgentCliKind::ClaudeCode => Ok(ManagedHookService::Generic(GenericHookService::from_app(
            app,
            GenericAgent::ClaudeCode,
        )?)),
        AgentCliKind::Gemini => Ok(ManagedHookService::Generic(GenericHookService::from_app(
            app,
            GenericAgent::Gemini,
        )?)),
        AgentCliKind::Grok => Ok(ManagedHookService::Generic(GenericHookService::from_app(
            app,
            GenericAgent::Grok,
        )?)),
    }
}
