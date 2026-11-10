//! Registry-backed, user-scope managed Hook adapters.
//!
//! Command handlers call this registry once; Agent-specific schema and
//! ownership behavior stays inside the selected adapter.

mod catalog_link;
mod codex;
mod common;
mod generic;
mod kimi;
mod locking;

use crate::models::{AgentCliKind, AgentHookInspection, AgentHookMutation, AgentHookPlan};
pub(crate) use catalog_link::{
    configuration_ownership_guard, prepare_catalog_change, prepare_catalog_policy_change,
};
pub use codex::CodexHookService;
use generic::{GenericAgent, GenericHookService};
use kimi::KimiHookService;

pub(crate) enum ManagedHookService {
    Codex(CodexHookService),
    Generic(GenericHookService),
    Kimi(KimiHookService),
}

pub fn inspect(service: &ManagedHookService) -> AgentHookInspection {
    match service {
        ManagedHookService::Codex(service) => service.inspect(),
        ManagedHookService::Generic(service) => service.inspect(),
        ManagedHookService::Kimi(service) => service.inspect(),
    }
}

pub fn plan(service: &ManagedHookService, mutation: AgentHookMutation) -> AgentHookPlan {
    match service {
        ManagedHookService::Codex(service) => service.plan(mutation),
        ManagedHookService::Generic(service) => service.plan(mutation),
        ManagedHookService::Kimi(service) => service.plan(mutation),
    }
}

pub fn apply(
    service: &ManagedHookService,
    plan: AgentHookPlan,
) -> Result<AgentHookInspection, String> {
    match service {
        ManagedHookService::Codex(service) => service.apply(plan),
        ManagedHookService::Generic(service) => service.apply(plan),
        ManagedHookService::Kimi(service) => service.apply(plan),
    }
}

pub fn helper_from_process_args() -> Option<i32> {
    codex::helper_from_process_args()
}

pub fn service(agent_kind: AgentCliKind) -> Result<ManagedHookService, String> {
    match agent_kind {
        AgentCliKind::Codex => Ok(ManagedHookService::Codex(CodexHookService::from_host()?)),
        AgentCliKind::ClaudeCode => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::ClaudeCode,
        )?)),
        AgentCliKind::Gemini => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::Gemini,
        )?)),
        AgentCliKind::Grok => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::Grok,
        )?)),
        AgentCliKind::CommandCode => Ok(ManagedHookService::Generic(
            GenericHookService::from_host(GenericAgent::CommandCode)?,
        )),
        AgentCliKind::QwenCode => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::QwenCode,
        )?)),
        AgentCliKind::CodeBuddy => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::CodeBuddy,
        )?)),
        AgentCliKind::Qoder => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::Qoder,
        )?)),
        AgentCliKind::KimiCode => Ok(ManagedHookService::Kimi(KimiHookService::from_host()?)),
        AgentCliKind::Droid => Ok(ManagedHookService::Generic(GenericHookService::from_host(
            GenericAgent::Droid,
        )?)),
        AgentCliKind::OpenCode
        | AgentCliKind::Pi
        | AgentCliKind::PrimeAgent
        | AgentCliKind::Crush
        | AgentCliKind::Copilot
        | AgentCliKind::OpenClaw
        | AgentCliKind::Jcode
        | AgentCliKind::MothX
        | AgentCliKind::StepCode
        | AgentCliKind::DevEco
        | AgentCliKind::MiMoCode
        | AgentCliKind::AtomCode
        | AgentCliKind::Cline
        | AgentCliKind::Penguin
        | AgentCliKind::Goose
        | AgentCliKind::Hermes
        | AgentCliKind::Dsh
        | AgentCliKind::Omp
        | AgentCliKind::Morph
        | AgentCliKind::MiniMaxCode
        | AgentCliKind::Raycast
        | AgentCliKind::Omo => Err("当前 Agent 未提供会话 Hook 接入能力".into()),
    }
}
