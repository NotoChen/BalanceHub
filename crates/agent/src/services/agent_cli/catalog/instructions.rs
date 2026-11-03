//! Shared instruction schemes live in the existing transactional library.
use super::{digest, opaque_id, CatalogService};
use crate::models::{AgentInstructionSaveRequest, AgentInstructionSet};

pub(super) const MAX_SETS: usize = 128;

pub(super) fn validate(set: &AgentInstructionSet) -> Result<(), String> {
    if set.id.is_empty()
        || set.id.len() > 128
        || !set
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("指令方案标识无效".to_owned());
    }
    if set.name.trim().is_empty() || set.name.chars().count() > 100 {
        return Err("请输入 1 至 100 字的方案名称".to_owned());
    }
    let texts = std::iter::once(&set.common).chain(set.additions.values());
    let mut size = 0usize;
    for text in texts {
        size = size.saturating_add(text.len());
        if text.contains('\0') || text.contains("<!-- balancehub:instructions") {
            return Err("正文包含无效字符或共享指令管理标记".to_owned());
        }
    }
    if size > 512 * 1024 {
        return Err("指令方案超过 512 KiB，请精简正文".to_owned());
    }
    Ok(())
}

pub(super) fn revision(set: &AgentInstructionSet) -> String {
    digest(&serde_json::to_vec(&(&set.name, &set.common, &set.additions)).unwrap_or_default())
}

impl CatalogService {
    pub(crate) fn instruction_sets(&self) -> Result<Vec<AgentInstructionSet>, String> {
        self.repository
            .read_view(|library| Ok(library.instructions.values().cloned().collect()))
    }

    pub(crate) fn instruction_set(
        &self,
        id: &str,
        expected: &str,
    ) -> Result<AgentInstructionSet, String> {
        self.repository.read_view(|library| {
            let value = library
                .instructions
                .get(id)
                .ok_or("指令方案已删除，请刷新共享库")?;
            if value.revision != expected {
                return Err("指令方案已更新，请刷新后重新预览".to_owned());
            }
            Ok(value.clone())
        })
    }

    pub(crate) fn save_instruction_set(
        &self,
        request: AgentInstructionSaveRequest,
    ) -> Result<AgentInstructionSet, String> {
        let id = request.id.clone().map(Ok).unwrap_or_else(opaque_id)?;
        let mut desired = AgentInstructionSet {
            id: id.clone(),
            revision: String::new(),
            name: request.name.trim().to_owned(),
            common: request.common,
            additions: request.additions,
        };
        validate(&desired)?;
        desired.revision = revision(&desired);
        self.repository.transact(|library| {
            match library.instructions.get(&id) {
                Some(current)
                    if request.expected_revision.as_deref() == Some(current.revision.as_str()) => {}
                None if request.id.is_none() && library.instructions.len() < MAX_SETS => {}
                _ => return Err("指令方案已变化或数量达到上限，请刷新共享库后重试".to_owned()),
            }
            library.instructions.insert(id, desired.clone());
            Ok(desired)
        })
    }

    pub(crate) fn delete_instruction_set(&self, id: &str, expected: &str) -> Result<(), String> {
        self.repository.transact(|library| {
            if library
                .instructions
                .get(id)
                .is_none_or(|set| set.revision != expected)
            {
                return Err("指令方案已变化，请刷新后重试".to_owned());
            }
            library.instructions.remove(id);
            Ok(())
        })
    }
}
