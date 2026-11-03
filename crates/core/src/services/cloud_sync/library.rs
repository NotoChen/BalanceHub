//! 共享库（Agent 资产的 Skill / MCP / Hook 定义）在同步中的扩展点。
//!
//! 资产库只存在于桌面：服务端没有本机 Agent 目录，同步时只处理配置文档。
//! 核心定义接口，桌面在 `desktop_host.rs` 里接上真正的资产库。

use super::format::SyncDocuments;

/// 一次提交可以把共享库和配置文档放在同一个事务里：`commit` 由同步引擎
/// 提供，负责写配置并返回警告；实现方在写入共享库的同一临界区调用它。
pub type LibraryCommit<'a> = &'a mut dyn FnMut(
    Option<&[u8]>,
    Option<&[u8]>,
    &mut dyn FnMut() -> Result<(), String>,
) -> Result<Option<String>, String>;

pub trait CloudLibrary: Send + Sync {
    fn validate(&self, documents: &SyncDocuments) -> Result<(), String>;

    fn snapshot(&self) -> Result<SyncDocuments, String>;

    fn apply(
        &self,
        expected: &SyncDocuments,
        desired: &SyncDocuments,
        commit: LibraryCommit<'_>,
    ) -> Result<Option<String>, String>;
}
