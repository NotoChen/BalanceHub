use super::super::journal;
use crate::models::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf, sync::Mutex};

#[derive(Clone, Serialize, Deserialize)]
struct Attempt {
    operation_id: String,
    at: String,
    settled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct Policy {
    agent_kind: AgentCliKind,
    channel_label: String,
    directory: String,
    enabled: bool,
    enabled_at: i64,
    generation: u64,
    attempt: Option<Attempt>,
    paused: Option<String>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct SavedPolicies {
    policies: BTreeMap<String, Policy>,
}

pub(in super::super) struct AutomaticUpdates {
    path: Option<PathBuf>,
    saved: Mutex<Result<SavedPolicies, String>>,
    observations: Mutex<BTreeMap<String, (AgentAutomaticUpdateState, String)>>,
    blocked: Mutex<Option<String>>,
}

impl AutomaticUpdates {
    pub(in super::super) fn new(path: Option<PathBuf>) -> Self {
        let saved = match &path {
            Some(path) => (|| {
                match fs::metadata(path) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return Ok(SavedPolicies::default())
                    }
                    Err(error) => return Err(error.to_string()),
                    Ok(metadata) if metadata.len() > 256 * 1024 => {
                        return Err("自动更新设置超过读取范围".into())
                    }
                    Ok(_) => {}
                }
                let saved: SavedPolicies =
                    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                if saved.policies.len() > 128 {
                    return Err("自动更新设置数量异常".into());
                }
                Ok(saved)
            })(),
            None if cfg!(test) => Ok(SavedPolicies::default()),
            None => Err("自动更新设置目录未初始化".into()),
        };
        Self {
            path,
            saved: Mutex::new(saved),
            observations: Mutex::default(),
            blocked: Mutex::default(),
        }
    }

    fn change(
        &self,
        update: impl FnOnce(&mut SavedPolicies) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut saved = self.saved.lock().map_err(|_| "自动更新设置不可用")?;
        let mut next = saved
            .as_ref()
            .map_err(|error| format!("自动更新设置读取失败：{error}"))?
            .clone();
        update(&mut next)?;
        if let Some(path) = &self.path {
            if let Err(error) = journal::write_json(path, &next) {
                let message = format!(
                    "保存自动更新设置失败：{error}；本次运行已停止自动更新，请修复后重启应用"
                );
                self.block(message.clone());
                return Err(message);
            }
        }
        *saved = Ok(next);
        Ok(())
    }

    pub(in super::super) fn set(
        &self,
        key: &str,
        enabled: bool,
        target: Option<&AgentLifecycleTarget>,
    ) -> Result<(), String> {
        if enabled {
            if let Some(message) = self
                .blocked
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                return Err(message.clone());
            }
        }
        self.change(|saved| {
            if enabled {
                let target = target.ok_or("未找到对应安装，请重新检测")?;
                if target.update_key.as_deref() != Some(key) {
                    return Err("安装渠道或位置已变化，请重新确认".into());
                }
                if saved.policies.len() >= 128 && !saved.policies.contains_key(key) {
                    return Err("自动更新设置数量已达上限".into());
                }
                let generation = saved
                    .policies
                    .get(key)
                    .map_or(1, |policy| policy.generation.saturating_add(1));
                saved.policies.insert(
                    key.into(),
                    Policy {
                        agent_kind: target.agent_kind,
                        channel_label: target.channel_label.clone(),
                        directory: target.directory.clone().ok_or("安装目录未确认")?,
                        enabled: true,
                        enabled_at: chrono::Utc::now().timestamp_millis(),
                        generation,
                        attempt: None,
                        paused: None,
                    },
                );
            } else if let Some(policy) = saved.policies.get_mut(key) {
                policy.enabled = false;
                policy.generation = policy.generation.saturating_add(1);
            }
            Ok(())
        })?;
        self.observations
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
        Ok(())
    }

    pub(in super::super) fn generation(&self, key: &str) -> Option<u64> {
        if self.blocked.lock().ok()?.is_some() {
            return None;
        }
        let saved = self.saved.lock().ok()?;
        let policy = saved.as_ref().ok()?.policies.get(key)?;
        (policy.enabled && policy.paused.is_none()).then_some(policy.generation)
    }

    pub(in super::super) fn may_run(&self, key: &str, operation_id: &str) -> bool {
        if self
            .blocked
            .lock()
            .map_or(true, |blocked| blocked.is_some())
        {
            return false;
        }
        self.saved.lock().ok().is_some_and(|saved| {
            saved.as_ref().ok().is_some_and(|saved| {
                saved.policies.get(key).is_some_and(|policy| {
                    policy.enabled
                        && policy.paused.is_none()
                        && policy.attempt.as_ref().is_some_and(|attempt| {
                            attempt.operation_id == operation_id && !attempt.settled
                        })
                })
            })
        })
    }

    pub(in super::super) fn begin(
        &self,
        key: &str,
        generation: u64,
        operation: &AgentLifecycleOperation,
    ) -> Result<(), String> {
        self.change(|saved| {
            let policy = saved.policies.get_mut(key).ok_or("自动更新已关闭")?;
            if !policy.enabled || policy.generation != generation || policy.paused.is_some() {
                return Err("自动更新设置已变化，本次任务未执行".into());
            }
            policy.attempt = Some(Attempt {
                operation_id: operation.id.clone(),
                at: operation.created_at.clone(),
                settled: false,
            });
            Ok(())
        })
    }

    pub(in super::super) fn recover_and_finish(
        &self,
        operations: &[AgentLifecycleOperation],
    ) -> Result<bool, String> {
        let mut changes = Vec::new();
        {
            let saved = self.saved.lock().map_err(|_| "自动更新设置不可用")?;
            let saved = saved.as_ref().map_err(Clone::clone)?;
            for (key, policy) in &saved.policies {
                // The newest operation also covers a crash between journaling and begin().
                let Some(operation) = operations
                    .iter()
                    .filter(|operation| {
                        operation.origin == AgentLifecycleOrigin::Automatic
                            && operation.update_key.as_ref() == Some(key)
                            && chrono::DateTime::parse_from_rfc3339(&operation.created_at)
                                .is_ok_and(|time| time.timestamp_millis() >= policy.enabled_at)
                    })
                    .max_by_key(|operation| &operation.created_at)
                else {
                    continue;
                };
                let same = policy
                    .attempt
                    .as_ref()
                    .is_some_and(|attempt| attempt.operation_id == operation.id);
                if same
                    && (policy
                        .attempt
                        .as_ref()
                        .is_some_and(|attempt| attempt.settled)
                        || operation.outcome.is_none())
                {
                    continue;
                }
                changes.push((key.clone(), operation.clone()));
            }
        }
        if changes.is_empty() {
            return Ok(false);
        }
        self.change(|saved| {
            for (key, operation) in &changes {
                let Some(policy) = saved.policies.get_mut(key) else {
                    continue;
                };
                if chrono::DateTime::parse_from_rfc3339(&operation.created_at)
                    .is_ok_and(|time| time.timestamp_millis() < policy.enabled_at)
                {
                    continue;
                }
                policy.attempt = Some(Attempt {
                    operation_id: operation.id.clone(),
                    at: operation.created_at.clone(),
                    settled: operation.outcome.is_some(),
                });
                if operation.outcome.is_some() {
                    let upgraded = operation.outcome
                        == Some(AgentAssetOperationOutcome::AppliedVerified)
                        && super::super::super::environment::version_state(
                            operation.from_version.as_deref(),
                            operation.observed_version.as_deref(),
                        ) == AgentLifecycleVersionState::UpdateAvailable;
                    policy.paused = (!upgraded).then(|| {
                        operation.message.clone().unwrap_or_else(|| {
                            "上次更新结果需要核对，请查看任务记录后恢复自动更新".into()
                        })
                    });
                }
            }
            Ok(())
        })?;
        let mut observations = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        for (key, operation) in &changes {
            if operation.outcome.is_some() {
                observations.remove(key);
            }
        }
        Ok(true)
    }

    pub(in super::super) fn observe(
        &self,
        key: &str,
        state: AgentAutomaticUpdateState,
        message: impl Into<String>,
    ) -> bool {
        let value = (state, message.into());
        let mut observations = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        if observations.get(key) == Some(&value) {
            return false;
        }
        observations.insert(key.into(), value);
        true
    }

    pub(in super::super) fn block(&self, message: String) {
        *self.blocked.lock().unwrap_or_else(|e| e.into_inner()) = Some(message);
    }

    pub(in super::super) fn pause(
        &self,
        key: &str,
        generation: u64,
        message: String,
    ) -> Result<(), String> {
        self.change(|saved| {
            if let Some(policy) = saved
                .policies
                .get_mut(key)
                .filter(|policy| policy.generation == generation)
            {
                policy.paused = Some(message);
            }
            Ok(())
        })
    }

    pub(in super::super) fn snapshot(
        &self,
        targets: &[AgentLifecycleTarget],
    ) -> Vec<AgentAutomaticUpdate> {
        let saved = self.saved.lock().unwrap_or_else(|e| e.into_inner());
        let observations = self.observations.lock().unwrap_or_else(|e| e.into_inner());
        let blocked = self.blocked.lock().unwrap_or_else(|e| e.into_inner());
        let mut result = Vec::new();
        for target in targets {
            let key = target.update_key.as_deref();
            let policy = saved
                .as_ref()
                .ok()
                .and_then(|saved| key.and_then(|key| saved.policies.get(key)));
            let unique = key.is_some_and(|key| {
                targets
                    .iter()
                    .filter(|target| target.update_key.as_deref() == Some(key))
                    .count()
                    == 1
            });
            let available = unique && saved.is_ok() && blocked.is_none();
            let enabled = policy.is_some_and(|policy| policy.enabled);
            let (state, message) = if let Some(error) = blocked.as_ref() {
                (AgentAutomaticUpdateState::Unavailable, error.clone())
            } else if let Err(error) = saved.as_ref() {
                (
                    AgentAutomaticUpdateState::Unavailable,
                    format!("自动更新设置不可用：{error}"),
                )
            } else if !available {
                (
                    AgentAutomaticUpdateState::Unavailable,
                    if key.is_some() {
                        "此渠道对应多个安装入口，请先核对安装".into()
                    } else {
                        target
                            .actions
                            .iter()
                            .find_map(|action| action.reason_message.clone())
                            .unwrap_or_else(|| "此安装尚无可验证的自动更新方式".into())
                    },
                )
            } else if !enabled {
                (
                    AgentAutomaticUpdateState::Disabled,
                    "每 6 小时检查，结束使用后通过原渠道更新".into(),
                )
            } else if let Some(message) = policy.and_then(|policy| policy.paused.as_ref()) {
                (AgentAutomaticUpdateState::Paused, message.clone())
            } else if let Some(observation) = key.and_then(|key| observations.get(key)) {
                observation.clone()
            } else {
                (
                    AgentAutomaticUpdateState::Scheduled,
                    target
                        .version
                        .message
                        .clone()
                        .unwrap_or_else(|| "已开启；到期检查，结束使用后更新".into()),
                )
            };
            result.push(AgentAutomaticUpdate {
                key: target.update_key.clone(),
                target_id: Some(target.id.clone()),
                agent_kind: target.agent_kind,
                channel_label: target.channel_label.clone(),
                directory: target.directory.clone(),
                enabled,
                can_enable: available,
                can_resume: available && enabled && state == AgentAutomaticUpdateState::Paused,
                state,
                message,
                operation_id: policy
                    .and_then(|policy| policy.attempt.as_ref())
                    .map(|attempt| attempt.operation_id.clone()),
                last_attempt_at: policy
                    .and_then(|policy| policy.attempt.as_ref())
                    .map(|attempt| attempt.at.clone()),
                next_check_at: target.version.next_check_at.clone(),
            });
        }
        if let Ok(saved) = saved.as_ref() {
            for (key, policy) in &saved.policies {
                if !policy.enabled
                    || targets
                        .iter()
                        .any(|target| target.update_key.as_ref() == Some(key))
                {
                    continue;
                }
                result.push(AgentAutomaticUpdate {
                    key: Some(key.clone()),
                    target_id: None,
                    agent_kind: policy.agent_kind,
                    channel_label: policy.channel_label.clone(),
                    directory: Some(policy.directory.clone()),
                    enabled: true,
                    can_enable: false,
                    can_resume: false,
                    state: AgentAutomaticUpdateState::Unavailable,
                    message:
                        "原安装已移除、渠道已变化或暂时不可读取；自动更新已停止执行，可关闭此设置"
                            .into(),
                    operation_id: policy
                        .attempt
                        .as_ref()
                        .map(|attempt| attempt.operation_id.clone()),
                    last_attempt_at: policy.attempt.as_ref().map(|attempt| attempt.at.clone()),
                    next_check_at: None,
                });
            }
        }
        result
    }
}
