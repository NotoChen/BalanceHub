use crate::services::agent_cli::environment::mutation::GuardedFile;

#[cfg(any(unix, windows))]
pub(super) fn save(file: &GuardedFile, bytes: &[u8]) -> Result<(), String> {
    use crate::services::agent_cli::environment::mutation::atomic;
    match atomic::replace(file, bytes, || Ok(())) {
        Ok(atomic::AtomicWriteResult::Unchanged | atomic::AtomicWriteResult::Replaced) => Ok(()),
        #[cfg(unix)]
        Ok(atomic::AtomicWriteResult::ReplacedNotSynced) => {
            Err("共享库已替换，但目录同步失败；请重新读取当前版本，勿重复提交".to_owned())
        }
        Err(_) => Err("共享库原子保存失败，已保留当前文件".to_owned()),
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) fn save(_file: &GuardedFile, _bytes: &[u8]) -> Result<(), String> {
    Err("此平台不支持共享库持久化".to_owned())
}
