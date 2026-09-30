use super::{AgentSessionResumeService, Entry};
use crate::{
    models::{
        AgentSessionResumeOperation, AgentSessionResumeResult, AgentSessionResumeState,
        TemporaryCliInstance, TemporaryCliInstanceStatus,
    },
    services::temporary_cli::ResumeCompletion,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FinalizationState {
    Pending,
    Recording,
    Complete,
}

pub(super) struct FinalizationClaim {
    pub completion: ResumeCompletion,
    pub instance: TemporaryCliInstance,
    pub launched: bool,
}

impl AgentSessionResumeService {
    pub(super) fn claim_finalization(&self, id: &str) -> Option<FinalizationClaim> {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let entry = registry.operations.get_mut(id)?;
        if !entry.dispatch_finished || entry.finalization != FinalizationState::Pending {
            return None;
        }
        let instance = entry.instance.clone()?;
        if !entry.launch_confirmed && instance.status != TemporaryCliInstanceStatus::Exited {
            return None;
        }
        let completion = entry.completion.clone()?;
        entry.finalization = FinalizationState::Recording;
        Some(FinalizationClaim {
            completion,
            instance,
            launched: entry.launch_confirmed,
        })
    }

    pub(super) fn complete_finalization(
        &self,
        id: &str,
        mut result: AgentSessionResumeResult,
    ) -> Result<AgentSessionResumeOperation, String> {
        self.update(id, |entry| {
            if entry.finalization != FinalizationState::Recording
                || !entry.matches_instance(&result.instance)
            {
                return Err("继续任务的登记结果已失效".to_owned());
            }
            if let Some(instance) = &entry.instance {
                result.instance = instance.clone();
            }
            // The final result and state become visible in one transaction.
            // Another poll can see Recording, but never a partial success.
            entry.operation.result = Some(result);
            entry.finalization = FinalizationState::Complete;
            entry.publish_progress();
            Ok(())
        })
    }
}

impl Entry {
    pub(super) fn publish_progress(&mut self) {
        let Some(instance) = &self.instance else {
            return;
        };
        if matches!(
            self.operation.state,
            AgentSessionResumeState::Failed | AgentSessionResumeState::Cancelled
        ) {
            return;
        }
        let exited = instance.status == TemporaryCliInstanceStatus::Exited;
        if self.finalization == FinalizationState::Complete
            && self.operation.result.is_some()
            && self.dispatch_finished
        {
            if self.launch_confirmed {
                self.operation.state = AgentSessionResumeState::Succeeded;
                self.operation.message = if exited {
                    match instance.exit_code {
                        Some(code) => format!("继续命令已结束，退出码 {code}"),
                        None => "继续命令已结束".to_owned(),
                    }
                } else {
                    "继续命令已在终端启动".to_owned()
                };
            } else if exited {
                self.operation.state = AgentSessionResumeState::Failed;
                self.operation.message = match instance.exit_code {
                    Some(code) => format!("继续命令已退出，退出码 {code}"),
                    None => "启动进程已退出，未取得成功继续的证据".to_owned(),
                };
            }
        } else if self.operation.state == AgentSessionResumeState::Running {
            if self.launch_confirmed {
                self.operation.message = if self.dispatch_finished {
                    "继续命令已启动，正在登记目录使用信息".to_owned()
                } else {
                    "已取得进程证据，正在等待终端启动结果".to_owned()
                };
            } else if exited {
                self.operation.message = "启动进程已结束，正在核对本次启动结果".to_owned();
            }
        }
    }
}

pub(super) fn has_launch_evidence(instance: &TemporaryCliInstance) -> bool {
    (instance.status == TemporaryCliInstanceStatus::Running && instance.pid.is_some())
        || (instance.status == TemporaryCliInstanceStatus::Exited && instance.exit_code == Some(0))
}
