//! Unlink a verified directory reference, never its resolved shared target.
use super::*;

pub(crate) struct DirectoryReference {
    parent: VerifiedPathAnchor,
    name: String,
    evidence: DirectoryLinkEvidence,
}

impl DirectoryReference {
    pub(crate) fn capture(
        root: &Path,
        path: &Path,
        target: &Path,
        junction: bool,
    ) -> Result<Self, String> {
        let parent_path = path.parent().ok_or("链接父目录无效")?;
        let parent =
            inspect_verified_path(&[root], root, parent_path, AgentAssetSourceKind::Directory)
                .map_err(|_| "链接父目录不可访问")?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("链接文件名无效")?
            .to_owned();
        let evidence = parent
            .platform
            .read_directory_link(name.as_ref())
            .map_err(|_| "目录链接不可读取")?;
        if evidence.target_text != target || evidence.junction != junction {
            return Err("目录链接已变化，请重新预览".to_owned());
        }
        let mut hash = Sha256::new();
        parent.update_revision(&mut hash);
        let revision = AgentAssetRevision {
            identity: format!("{:x}", hash.finalize()),
            is_directory: true,
            ..Default::default()
        };
        Ok(Self {
            parent: parent.anchor(&revision, None),
            name,
            evidence,
        })
    }
    pub(crate) fn domain(&self) -> String {
        crate::services::agent_cli::environment::mutation::pathname_domain(
            &self.parent.display_path().join(&self.name),
        )
    }
    pub(crate) fn revalidate(&self) -> Result<(), String> {
        self.reopen().map(|_| ())
    }
    fn reopen(&self) -> Result<VerifiedPathGuard, String> {
        let parent =
            reopen_verified_directory_identity(&self.parent).map_err(|_| "链接父目录已变化")?;
        if parent
            .platform
            .read_directory_link(self.name.as_ref())
            .map_err(|_| "链接不可读取")?
            != self.evidence
        {
            return Err("目录链接已变化，请重新预览".to_owned());
        }
        Ok(parent)
    }
    pub(crate) fn remove(
        &self,
        before_commit: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        let parent = self.reopen()?;
        crate::services::agent_cli::config_support::native_history::record_link(
            self.parent.allowed_root(),
            &self.parent.display_path().join(&self.name),
            &self.evidence.target_text,
            self.evidence.junction,
        )?;
        #[cfg(unix)]
        {
            before_commit()?;
            parent
                .revalidate_identity()
                .map_err(|_| "链接父目录已变化")?;
            if parent
                .platform
                .read_directory_link(self.name.as_ref())
                .map_err(|_| "目录链接不可读取")?
                != self.evidence
            {
                return Err("目录链接在提交前已变化".to_owned());
            }
            rustix::fs::unlinkat(
                parent.source_handle(),
                self.name.as_str(),
                rustix::fs::AtFlags::empty(),
            )
            .map_err(|_| "移除目录链接失败")?;
            parent
                .source_handle()
                .sync_all()
                .map_err(|_| "链接已移除，但目录同步失败，请刷新核对".to_owned())
        }
        #[cfg(windows)]
        {
            parent
                .platform
                .remove_link(self.name.as_ref(), &self.evidence, before_commit)
        }
    }
}

pub(crate) fn remove_skill_reference(
    anchor: &VerifiedPathAnchor,
    before_commit: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let link = anchor
        .readonly_skill_link
        .as_ref()
        .ok_or("所选来源不是已验证的 Skill 链接")?;
    DirectoryReference {
        parent: (*link.manifest).clone(),
        name: link.entry_name.clone(),
        evidence: link.evidence.clone(),
    }
    .remove(before_commit)
}
