//! 环境自检：把已缓存的 Agent 摘要归并成“可用 / 部分可用 / 不可用”。
//!
//! 这里只读摘要，不重新探测 CLI，也不解析配置文件；发起自检的一方负责先
//! 刷新摘要，避免打开面板时再对每个 Agent 做一轮昂贵的系统调用。

use super::super::{definitions, AgentCliDefinition};
use crate::models::*;
use std::collections::BTreeMap;

pub(crate) fn self_check_report(
    snapshots: Vec<AgentOverviewSnapshot>,
    session_scope: Option<&AgentSessionScope>,
    session_scope_error: &str,
    hooks: &BTreeMap<AgentCliKind, AgentHookInspection>,
) -> AgentSelfCheckReport {
    let mut snapshots = snapshots
        .into_iter()
        .map(|snapshot| (snapshot.agent_kind, snapshot))
        .collect::<BTreeMap<_, _>>();
    let entries = definitions()
        .iter()
        .map(|definition| {
            check_entry(
                definition,
                snapshots.remove(&definition.kind),
                session_scope,
                session_scope_error,
                hooks.get(&definition.kind),
            )
        })
        .collect::<Vec<_>>();
    let count = |status| {
        entries
            .iter()
            .filter(|entry| entry.status == status)
            .count()
    };
    let ready = count(AgentSelfCheckStatus::Ready);
    let degraded = count(AgentSelfCheckStatus::Degraded);
    let unavailable = count(AgentSelfCheckStatus::Unavailable);
    let unknown = count(AgentSelfCheckStatus::Unknown);
    let status = if unknown == entries.len() {
        AgentSelfCheckStatus::Unknown
    } else if ready + degraded == 0 {
        AgentSelfCheckStatus::Unavailable
    } else if degraded + unavailable + unknown == 0 {
        AgentSelfCheckStatus::Ready
    } else {
        AgentSelfCheckStatus::Degraded
    };
    AgentSelfCheckReport {
        checked_at: chrono::Utc::now().to_rfc3339(),
        status,
        ready,
        degraded,
        unavailable,
        unknown,
        entries,
    }
}

fn check_entry(
    definition: &AgentCliDefinition,
    snapshot: Option<AgentOverviewSnapshot>,
    session_scope: Option<&AgentSessionScope>,
    session_scope_error: &str,
    hook: Option<&AgentHookInspection>,
) -> AgentSelfCheckEntry {
    let capabilities = definition.capabilities();
    let label = definition.label.to_string();
    let Some(snapshot) = snapshot else {
        return AgentSelfCheckEntry {
            agent_kind: definition.kind,
            label,
            status: AgentSelfCheckStatus::Unknown,
            executable: String::new(),
            version: String::new(),
            configuration_sources: 0,
            session_sources_total: 0,
            session_sources_available: 0,
            session_history: capabilities.session_history,
            hook_inventory: capabilities.hook_inventory,
            hook: hook.map(hook_summary),
            issues: vec![issue(
                AgentSelfCheckArea::Configuration,
                AgentConfigurationDiagnosticSeverity::Info,
                "尚未读取 Agent 摘要",
                "打开 Agent 总览并等待摘要读取完成后，再运行环境自检。",
            )],
        };
    };

    let mut issues = Vec::new();
    if !snapshot.probe.available {
        let detail = if snapshot.probe.message.trim().is_empty() {
            "未在设置的路径或 PATH 中找到可执行文件。".to_owned()
        } else {
            snapshot.probe.message.clone()
        };
        issues.push(issue(
            AgentSelfCheckArea::Executable,
            AgentConfigurationDiagnosticSeverity::Error,
            "CLI 不可用",
            detail,
        ));
    }
    if !snapshot.configuration_error.is_empty() {
        issues.push(issue(
            AgentSelfCheckArea::Configuration,
            AgentConfigurationDiagnosticSeverity::Error,
            "配置来源读取失败",
            snapshot.configuration_error.clone(),
        ));
    }
    if let Some(configuration) = snapshot.configuration.as_ref() {
        let mut diagnostics = configuration
            .diagnostics
            .iter()
            .chain(
                configuration
                    .sources
                    .iter()
                    .flat_map(|source| source.diagnostics.iter()),
            )
            // A missing native config is normal before the Agent is configured;
            // only the diagnostics that need user action belong in a self-check.
            .filter(|diagnostic| {
                diagnostic.severity != AgentConfigurationDiagnosticSeverity::Info
                    && diagnostic.code != "configurationSourceUnavailable"
            })
            .collect::<Vec<_>>();
        diagnostics.sort_by_key(|diagnostic| {
            diagnostic.severity != AgentConfigurationDiagnosticSeverity::Error
        });
        if let Some(first) = diagnostics.first() {
            let mut detail = diagnostic_detail(configuration, first);
            if diagnostics.len() > 1 {
                detail.push_str(&format!("；另有 {} 条诊断。", diagnostics.len() - 1));
            }
            issues.push(issue(
                AgentSelfCheckArea::Configuration,
                first.severity,
                if first.severity == AgentConfigurationDiagnosticSeverity::Error {
                    "配置存在错误"
                } else {
                    "配置需要处理"
                },
                detail,
            ));
        }
    }

    if let Some(configuration) = snapshot.configuration.as_ref() {
        if !configuration.sources.is_empty()
            && !configuration.sources.iter().any(|source| {
                source.actions.iter().any(|action| {
                    action.available
                        && matches!(
                            action.action,
                            AgentConfigurationActionKind::Edit
                                | AgentConfigurationActionKind::Create
                        )
                })
            })
        {
            issues.push(issue(
                AgentSelfCheckArea::Configuration,
                AgentConfigurationDiagnosticSeverity::Warning,
                "配置只能读取",
                "已找到配置来源，但没有可写的编辑或创建目标；请检查文件权限或系统策略。",
            ));
        }
    }

    let mut session_sources_total = 0;
    let mut session_sources_available = 0;
    if capabilities.session_history {
        if !session_scope_error.is_empty() {
            issues.push(issue(
                AgentSelfCheckArea::SessionHistory,
                AgentConfigurationDiagnosticSeverity::Warning,
                "会话来源读取失败",
                session_scope_error,
            ));
        } else if let Some(scope) = session_scope {
            let sources = scope
                .sources
                .iter()
                .filter(|source| source.agent_kind == definition.kind)
                .collect::<Vec<_>>();
            session_sources_total = sources.len();
            session_sources_available = sources.iter().filter(|source| source.available).count();
            if sources.is_empty() {
                issues.push(issue(
                    AgentSelfCheckArea::SessionHistory,
                    AgentConfigurationDiagnosticSeverity::Warning,
                    "未注册可读的会话来源",
                    "该 Agent 支持历史会话，但没有可用的会话适配器来源。",
                ));
            } else if session_sources_available == 0 && snapshot.probe.available {
                let roots = sources
                    .iter()
                    .map(|source| source.config_root.as_str())
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("、");
                issues.push(issue(
                    AgentSelfCheckArea::SessionHistory,
                    AgentConfigurationDiagnosticSeverity::Warning,
                    "会话目录不可读",
                    format!("已安装 CLI，但未找到可读的会话目录：{roots}"),
                ));
            }
        } else {
            issues.push(issue(
                AgentSelfCheckArea::SessionHistory,
                AgentConfigurationDiagnosticSeverity::Warning,
                "会话来源尚未读取",
                "本次自检未读取会话目录，无法确认历史会话是否可用。",
            ));
        }
    }

    if let Some(hook) = hook {
        if let Some(issue) = hook_issue(hook) {
            issues.push(issue);
        }
    }

    let has_executable = snapshot.probe.available;
    let has_configuration =
        snapshot.configuration.is_some() && snapshot.configuration_error.is_empty();
    let status = match (has_executable, has_configuration) {
        (true, true) if issues.is_empty() => AgentSelfCheckStatus::Ready,
        (false, false) => AgentSelfCheckStatus::Unavailable,
        _ => AgentSelfCheckStatus::Degraded,
    };
    AgentSelfCheckEntry {
        agent_kind: snapshot.agent_kind,
        label,
        status,
        executable: snapshot.probe.path,
        version: snapshot.probe.version,
        configuration_sources: snapshot
            .configuration
            .as_ref()
            .map_or(0, |configuration| configuration.sources.len()),
        session_sources_total,
        session_sources_available,
        session_history: capabilities.session_history,
        hook_inventory: capabilities.hook_inventory,
        hook: hook.map(hook_summary),
        issues,
    }
}

fn hook_summary(inspection: &AgentHookInspection) -> AgentSelfCheckHook {
    AgentSelfCheckHook {
        state: inspection.state,
        installed: inspection.installed,
        enabled: inspection.enabled,
        config_path: inspection.config_path.clone(),
        detail: inspection.diagnostics.join("；"),
    }
}

/// 未安装/停用是用户可选状态，不算故障；冲突、助手缺失或数据目录不可用
/// 会让活跃会话观测悄悄失效，必须出现在自检结果里。
fn hook_issue(inspection: &AgentHookInspection) -> Option<AgentSelfCheckIssue> {
    let (message, severity) = match inspection.state {
        AgentHookHealthState::Conflict => (
            "Hook 配置冲突",
            AgentConfigurationDiagnosticSeverity::Warning,
        ),
        AgentHookHealthState::HelperMissing => (
            "Hook 助手缺失",
            AgentConfigurationDiagnosticSeverity::Warning,
        ),
        AgentHookHealthState::SpoolBlocked => (
            "Hook 数据目录不可用",
            AgentConfigurationDiagnosticSeverity::Warning,
        ),
        AgentHookHealthState::InstalledUntrusted => {
            ("Hook 未信任", AgentConfigurationDiagnosticSeverity::Warning)
        }
        AgentHookHealthState::InstalledUnverified => {
            ("Hook 待校验", AgentConfigurationDiagnosticSeverity::Warning)
        }
        _ => return None,
    };
    let detail = if inspection.diagnostics.is_empty() {
        format!(
            "Hook 状态：{}。可在 Agent 卡片的 Hook 面板中检查或修复。",
            hook_state_label(inspection.state)
        )
    } else {
        inspection.diagnostics.join("；")
    };
    Some(issue(AgentSelfCheckArea::Hook, severity, message, detail))
}

fn hook_state_label(state: AgentHookHealthState) -> &'static str {
    match state {
        AgentHookHealthState::NotInstalled => "未安装",
        AgentHookHealthState::InstalledUntrusted => "未信任",
        AgentHookHealthState::InstalledUnverified => "待校验",
        AgentHookHealthState::Healthy => "正常",
        AgentHookHealthState::Disabled => "已停用",
        AgentHookHealthState::Conflict => "冲突",
        AgentHookHealthState::HelperMissing => "助手缺失",
        AgentHookHealthState::SpoolBlocked => "数据目录不可用",
        AgentHookHealthState::Unsupported => "不支持",
    }
}

fn diagnostic_detail(
    configuration: &AgentConfigurationSnapshot,
    diagnostic: &AgentConfigurationDiagnostic,
) -> String {
    let location = match (diagnostic.line, diagnostic.column) {
        (Some(line), Some(column)) => format!("（第 {line} 行第 {column} 列）"),
        (Some(line), None) => format!("（第 {line} 行）"),
        _ => String::new(),
    };
    let source = diagnostic
        .source_id
        .as_deref()
        .and_then(|source_id| {
            configuration
                .sources
                .iter()
                .find(|source| source.source_id == source_id)
        })
        .map(|source| format!("来源：{}。", source.label))
        .unwrap_or_default();
    format!("{source}{}{location}", diagnostic.message)
}

fn issue(
    area: AgentSelfCheckArea,
    severity: AgentConfigurationDiagnosticSeverity,
    message: impl Into<String>,
    detail: impl Into<String>,
) -> AgentSelfCheckIssue {
    AgentSelfCheckIssue {
        area,
        severity,
        message: message.into(),
        detail: detail.into(),
    }
}
