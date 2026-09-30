//! Unlink the declared Skill reference, never its resolved shared target.
use super::*;

pub(crate) fn remove_skill_reference(
    anchor: &VerifiedPathAnchor,
    before_commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    #[cfg(unix)]
    {
        let link = anchor
            .readonly_skill_link
            .as_ref()
            .ok_or("所选来源不是已验证的 Skill 链接")?;
        // A previously removed sibling changes directory timestamps. Preserve
        // identity and exact link evidence while allowing our own batch edits.
        let parent = reopen_verified_directory_identity(&link.manifest)
            .map_err(|_| "Skill 链接目录已变化")?;
        if parent
            .platform
            .read_directory_link(link.entry_name.as_ref())
            .map_err(|_| "Skill 链接不可读取")?
            != link.evidence
        {
            return Err("Skill 链接已变化，请重新预览".to_owned());
        }
        before_commit()?;
        parent
            .revalidate_identity()
            .map_err(|_| "Skill 链接父目录已变化")?;
        if parent
            .platform
            .read_directory_link(link.entry_name.as_ref())
            .map_err(|_| "Skill 链接不可读取")?
            != link.evidence
        {
            return Err("Skill 链接已变化，请重新预览".to_owned());
        }
        rustix::fs::unlinkat(
            parent.source_handle(),
            link.entry_name.as_str(),
            rustix::fs::AtFlags::empty(),
        )
        .map_err(|_| "移除 Skill 链接失败")?;
        parent
            .source_handle()
            .sync_all()
            .map_err(|_| "链接已移除，但目录同步失败，请重新检查")?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (anchor, before_commit);
        Err("当前平台尚未提供链接移除执行器".to_owned())
    }
}
