use crate::{
    models::AgentAssetSourceKind,
    services::agent_cli::{
        contracts::{
            SessionProjectRelocationAdapter, SessionProjectRelocationPaths,
            SessionRelocationFileKind,
        },
        environment::{
            mutation::GuardedFile,
            verified_path::{inspect_verified_path, VerifiedPathAnchor},
        },
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{File, FileTimes},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;
const MAX_NODES: usize = 32_768;
pub(super) struct Member {
    pub relative: PathBuf,
    pub source: GuardedFile,
    pub digest: String,
    pub output_digest: String,
    pub output_bytes: u64,
    pub modified: SystemTime,
}
pub(super) struct Snapshot {
    pub files: Vec<Member>,
    pub directories: Vec<(PathBuf, VerifiedPathAnchor)>,
    pub bytes: u64,
    pub output_bytes: u64,
    pub sessions: usize,
}

pub(super) fn scan(
    root: &Path,
    paths: &SessionProjectRelocationPaths,
    adapter: SessionProjectRelocationAdapter,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Snapshot, String> {
    let mut snapshot = Snapshot {
        files: Vec::new(),
        directories: Vec::new(),
        bytes: 0,
        output_bytes: 0,
        sessions: 0,
    };
    let mut stack = vec![paths.source.clone()];
    while let Some(directory) = stack.pop() {
        check()?;
        let guard =
            inspect_verified_path(&[root], root, &directory, AgentAssetSourceKind::Directory)
                .map_err(|_| "项目目录不可安全读取")?;
        let metadata = guard.metadata().map_err(|_| "项目目录信息不可读取")?;
        require_idle(metadata.modified().map_err(|_| "无法核对目录更新时间")?)?;
        let mut digest = Sha256::new();
        guard.update_revision(&mut digest);
        let revision = crate::models::AgentAssetRevision {
            identity: format!("{:x}", digest.finalize()),
            is_directory: true,
            ..Default::default()
        };
        snapshot.directories.push((
            directory
                .strip_prefix(&paths.source)
                .map_err(|_| "项目目录越界")?
                .to_path_buf(),
            guard.anchor(&revision, None),
        ));
        for entry in guard.read_entries().map_err(|_| "项目目录无法枚举")? {
            check()?;
            if snapshot.files.len() + snapshot.directories.len() + stack.len() >= MAX_NODES {
                return Err("项目文件数超过本次迁移的 32768 项预算，请先整理该项目".to_owned());
            }
            let entry = entry.map_err(|_| "项目文件无法读取")?;
            if entry.is_symlink {
                return Err("项目历史包含目录链接，请先确认链接内容再迁移".to_owned());
            }
            let path = directory.join(entry.name);
            if entry.source_kind == AgentAssetSourceKind::Directory {
                stack.push(path);
                continue;
            }
            if entry.source_kind != AgentAssetSourceKind::File {
                return Err("项目历史包含特殊文件，未进行迁移".to_owned());
            }
            let source =
                GuardedFile::capture_metadata_path(root, &path).map_err(|error| error.message)?;
            let file = source.reopen_metadata().map_err(|error| error.message)?;
            let metadata = file.metadata().map_err(|_| "无法读取会话文件信息")?;
            let modified = metadata.modified().map_err(|_| "无法读取会话更新时间")?;
            require_idle(modified)?;
            let relative = path
                .strip_prefix(&paths.source)
                .map_err(|_| "会话文件越界")?
                .to_path_buf();
            let result = transform(
                &source,
                &relative,
                paths,
                adapter,
                &mut std::io::sink(),
                check,
            )?;
            snapshot.bytes = snapshot
                .bytes
                .checked_add(metadata.len())
                .ok_or("项目大小无效")?;
            snapshot.output_bytes = snapshot
                .output_bytes
                .checked_add(result.2)
                .ok_or("项目大小无效")?;
            if (adapter.file_kind)(&relative) == SessionRelocationFileKind::MainTranscript {
                snapshot.sessions += 1;
            }
            snapshot.files.push(Member {
                relative,
                source,
                digest: result.0,
                output_digest: result.1,
                output_bytes: result.2,
                modified,
            });
        }
        guard.revalidate().map_err(|_| "项目目录在读取时发生变化")?;
    }
    snapshot.files.sort_by(|a, b| a.relative.cmp(&b.relative));
    snapshot
        .directories
        .sort_by_key(|(path, _)| path.components().count());
    if snapshot.sessions == 0 {
        return Err("该项目没有可迁移的主会话".to_owned());
    }
    Ok(snapshot)
}

fn require_idle(modified: SystemTime) -> Result<(), String> {
    if !SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age >= Duration::from_secs(60))
    {
        return Err("项目历史在最近一分钟内仍有写入；请关闭该项目的 Agent 后再迁移".to_owned());
    }
    Ok(())
}

fn transform(
    source: &GuardedFile,
    relative: &Path,
    paths: &SessionProjectRelocationPaths,
    adapter: SessionProjectRelocationAdapter,
    output: &mut dyn Write,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(String, String, u64), String> {
    let guard = source.reopen_metadata().map_err(|error| error.message)?;
    let mut reader = BufReader::new(guard.source_handle());
    let kind = (adapter.file_kind)(relative);
    let mut before = Sha256::new();
    let mut after = Sha256::new();
    let mut bytes = 0u64;
    let mut saw_origin = false;
    let mut chunk = Vec::new();
    loop {
        check()?;
        chunk.clear();
        let size = match kind {
            SessionRelocationFileKind::MainTranscript | SessionRelocationFileKind::Transcript => {
                // A bound applies to one JSONL record, not the full transcript.
                reader
                    .by_ref()
                    .take(MAX_RECORD_BYTES as u64 + 1)
                    .read_until(b'\n', &mut chunk)
                    .map_err(|_| "会话记录读取失败")?
            }
            SessionRelocationFileKind::Document => reader
                .by_ref()
                .take(MAX_RECORD_BYTES as u64 + 1)
                .read_to_end(&mut chunk)
                .map_err(|_| "会话索引读取失败")?,
            SessionRelocationFileKind::Copy => {
                chunk.resize(256 * 1024, 0);
                let size = reader.read(&mut chunk).map_err(|_| "项目资源读取失败")?;
                chunk.truncate(size);
                size
            }
        };
        if size == 0 {
            break;
        }
        if size > MAX_RECORD_BYTES {
            return Err("单条会话记录或索引超过 64 MiB，请先检查文件是否异常".to_owned());
        }
        before.update(&chunk);
        let (changed, matched) = (adapter.transform)(relative, &chunk, paths, saw_origin)?;
        saw_origin |= matched;
        after.update(&changed);
        bytes = bytes
            .checked_add(changed.len() as u64)
            .ok_or("迁移文件大小无效")?;
        output
            .write_all(&changed)
            .map_err(|_| "迁移文件写入失败，请检查可用空间")?;
    }
    if kind == SessionRelocationFileKind::MainTranscript && !saw_origin {
        return Err(format!("{} 未记录可确认的项目目录", relative.display()));
    }
    guard
        .revalidate()
        .map_err(|_| "会话在读取期间仍有写入，请关闭客户端后重试")?;
    Ok((
        format!("{:x}", before.finalize()),
        format!("{:x}", after.finalize()),
        bytes,
    ))
}

pub(super) fn stage(
    snapshot: &Snapshot,
    root: &Path,
    directory: &Path,
    paths: &SessionProjectRelocationPaths,
    adapter: SessionProjectRelocationAdapter,
    context: &crate::services::agent_cli::maintenance::TaskContext,
) -> Result<(), String> {
    use crate::services::agent_cli::config_support::{directories, file_layout};
    for (relative, _) in &snapshot.directories {
        directories::ensure_directory_before(root, &directory.join(relative), || context.check())?;
    }
    for (index, member) in snapshot.files.iter().enumerate() {
        context.check()?;
        context.progress(format!(
            "准备迁移文件 {}/{}",
            index + 1,
            snapshot.files.len()
        ));
        let path = directory.join(&member.relative);
        let parent = path.parent().ok_or("迁移目标没有父目录")?;
        let guard = inspect_verified_path(&[root], root, parent, AgentAssetSourceKind::Directory)
            .map_err(|_| "暂存目录不可安全访问")?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| "无法创建迁移暂存文件")?;
        let result = transform(
            &member.source,
            &member.relative,
            paths,
            adapter,
            &mut output,
            &|| context.check(),
        )?;
        if result.0 != member.digest
            || result.1 != member.output_digest
            || result.2 != member.output_bytes
        {
            return Err("会话内容在预览后发生变化，请重新预览".to_owned());
        }
        let source = member
            .source
            .reopen_metadata()
            .map_err(|error| error.message)?;
        file_layout::copy_permissions(source.source_handle(), &output)
            .map_err(|_| "无法保留会话文件权限")?;
        output
            .set_times(FileTimes::new().set_modified(member.modified))
            .map_err(|_| "无法保留会话更新时间")?;
        output.sync_all().map_err(|_| "暂存文件同步失败")?;
        drop(output);
        let verify = inspect_verified_path(&[root], root, &path, AgentAssetSourceKind::File)
            .map_err(|_| "暂存文件不可核对")?;
        if checksum(verify.source_handle(), &|| context.check())? != member.output_digest {
            return Err("暂存文件内容核对失败".to_owned());
        }
        verify
            .revalidate()
            .map_err(|_| "暂存文件在核对期间发生变化")?;
        guard
            .revalidate_identity()
            .map_err(|_| "暂存父目录在写入时变化")?;
    }
    Ok(())
}

pub(super) fn verify(
    snapshot: &Snapshot,
    paths: &SessionProjectRelocationPaths,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<(), String> {
    let mut expected = snapshot
        .files
        .iter()
        .map(|member| member.relative.clone())
        .collect::<BTreeSet<_>>();
    expected.extend(
        snapshot
            .directories
            .iter()
            .filter(|(path, _)| !path.as_os_str().is_empty())
            .map(|(path, _)| path.clone()),
    );
    let mut seen = BTreeSet::new();
    for (relative, anchor) in &snapshot.directories {
        check()?;
        let guard =
            crate::services::agent_cli::environment::verified_path::reopen_verified_path(anchor)
                .map_err(|_| "项目目录在预览后发生变化")?;
        for entry in guard.read_entries().map_err(|_| "无法重新核对项目目录")? {
            let entry = entry.map_err(|_| "无法重新读取项目目录")?;
            seen.insert(relative.join(entry.name));
        }
    }
    if seen != expected {
        return Err("项目文件集合在预览后发生变化，请重新预览".to_owned());
    }
    for file in &snapshot.files {
        check()?;
        let guard = file
            .source
            .reopen_metadata()
            .map_err(|error| error.message)?;
        if checksum(guard.source_handle(), check)? != file.digest {
            return Err("会话内容在暂存期间发生变化，尚未移动原文件".to_owned());
        }
        guard.revalidate().map_err(|_| "会话仍被其他客户端写入")?;
    }
    if paths.source == paths.destination {
        return Err("源目录与目标目录不能相同".to_owned());
    }
    Ok(())
}
fn checksum(mut file: &File, check: &dyn Fn() -> Result<(), String>) -> Result<String, String> {
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        check()?;
        let size = file.read(&mut buffer).map_err(|_| "读取文件校验值失败")?;
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
