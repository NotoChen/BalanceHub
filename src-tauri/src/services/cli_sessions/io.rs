use crate::services::agent_cli::contracts::SessionContentSearchRequest;
use serde_json::Value;
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use super::truncate_text;

const MAX_SESSION_RECORD_BYTES: usize = 32 * 1024 * 1024;
const MAX_RETAINED_LINE_BUFFER_BYTES: usize = 1024 * 1024;
const BACKGROUND_SCAN_PAUSE_BYTES: usize = 2 * 1024 * 1024;
const BACKGROUND_SCAN_PAUSE: Duration = Duration::from_millis(1);

pub(crate) fn session_index_source_fingerprint(
    path: &Path,
    parser_version: u32,
) -> Result<(String, u64), String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("读取会话文件元数据失败({}): {error}", path.display()))?;
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let source_bytes = metadata.len();
    Ok((
        format!("v{parser_version}:{source_bytes}:{modified_nanos}"),
        source_bytes,
    ))
}

pub(crate) fn read_json_lines_limited(
    path: &Path,
    max_bytes: usize,
    label: &str,
    mut observe: impl FnMut(usize, Value),
) -> Result<bool, String> {
    let mut file =
        File::open(path).map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
    let file_bytes = file
        .metadata()
        .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?
        .len();
    let max_bytes = max_bytes.max(1);
    let mut sequence = 0usize;
    if file_bytes <= max_bytes as u64 {
        read_json_segment(
            BufReader::new(file),
            max_bytes,
            false,
            path,
            label,
            &mut sequence,
            &mut observe,
        )?;
        return Ok(false);
    }

    // Large transcripts are append-only in all supported CLIs. Keep both the
    // beginning (title/initial request) and the tail (latest conversation)
    // instead of spending the complete byte budget on stale early turns.
    let head_bytes = max_bytes / 2;
    let tail_bytes = max_bytes.saturating_sub(head_bytes);
    if head_bytes > 0 {
        read_json_segment(
            BufReader::new(
                file.try_clone()
                    .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?,
            ),
            head_bytes,
            false,
            path,
            label,
            &mut sequence,
            &mut observe,
        )?;
    }

    let tail_start = file_bytes.saturating_sub(tail_bytes as u64);
    let skip_partial_line = if tail_start == 0 {
        false
    } else {
        file.seek(SeekFrom::Start(tail_start - 1))
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        let mut previous = [0u8; 1];
        file.read_exact(&mut previous)
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        previous[0] != b'\n'
    };
    file.seek(SeekFrom::Start(tail_start))
        .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
    read_json_segment(
        BufReader::new(file),
        tail_bytes,
        skip_partial_line,
        path,
        label,
        &mut sequence,
        &mut observe,
    )?;
    Ok(true)
}

/// 顺序扫描完整 JSONL 会话文件，但任何时刻只保留一条记录。文件总大小不设
/// 窗口限制，从而可以命中超大会话中段；单条异常记录仍有上限，避免损坏或
/// 恶意状态文件迫使桌面进程分配无界内存。
pub(crate) fn scan_json_lines_matching(
    path: &Path,
    label: &str,
    request: &SessionContentSearchRequest,
    is_current: &dyn Fn() -> bool,
    mut observe: impl FnMut(usize, Value) -> bool,
) -> Result<(), String> {
    scan_json_records(path, label, is_current, |sequence, line| {
        if !json_record_may_match(line, request) {
            return false;
        }
        serde_json::from_slice::<Value>(line)
            .ok()
            .is_some_and(|value| observe(sequence, value))
    })
}

pub(crate) fn scan_json_records(
    path: &Path,
    label: &str,
    is_current: &dyn Fn() -> bool,
    observe: impl FnMut(usize, &[u8]) -> bool,
) -> Result<(), String> {
    scan_json_records_with_pacing(path, label, is_current, false, observe)
}

/// 后台索引使用轻量 I/O 节流，避免连续读取超大会话时长时间占满一个 CPU
/// 核心或磁盘带宽。前台直接搜索继续使用 `scan_json_records`，不引入等待。
pub(crate) fn scan_json_records_background(
    path: &Path,
    label: &str,
    is_current: &dyn Fn() -> bool,
    observe: impl FnMut(usize, &[u8]) -> bool,
) -> Result<(), String> {
    scan_json_records_with_pacing(path, label, is_current, true, observe)
}

fn scan_json_records_with_pacing(
    path: &Path,
    label: &str,
    is_current: &dyn Fn() -> bool,
    background: bool,
    mut observe: impl FnMut(usize, &[u8]) -> bool,
) -> Result<(), String> {
    let file = File::open(path).map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut pacer = ScanPacer::new(background);
    let mut sequence = 0usize;
    loop {
        if !is_current() {
            return Err("会话检索已被新的搜索替换".to_string());
        }
        line.clear();
        let bytes = reader
            .by_ref()
            .take((MAX_SESSION_RECORD_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        if bytes == 0 {
            break;
        }
        pacer.record(bytes, is_current)?;
        if line.len() > MAX_SESSION_RECORD_BYTES {
            if !line.ends_with(b"\n") {
                discard_until_newline(&mut reader, path, label, is_current, &mut pacer)?;
            }
            release_large_byte_buffer(&mut line);
            sequence = sequence.saturating_add(1);
            continue;
        }
        let should_stop = observe(sequence, &line);
        release_large_byte_buffer(&mut line);
        if should_stop {
            break;
        }
        sequence = sequence.saturating_add(1);
    }
    Ok(())
}

/// 丢弃超出单条记录上限的剩余字节，但不把异常超长记录继续累积到内存。
/// `BufRead::read_until` 即便传入临时 Vec，也会随着输入增长；这里按缓冲区
/// 消费，并在每个块之间检查取消状态，保证损坏的 JSONL 不会拖垮搜索任务。
fn discard_until_newline(
    reader: &mut BufReader<File>,
    path: &Path,
    label: &str,
    is_current: &dyn Fn() -> bool,
    pacer: &mut ScanPacer,
) -> Result<(), String> {
    loop {
        if !is_current() {
            return Err("会话检索已被新的搜索替换".to_string());
        }
        let (consumed, reached_newline, eof) = {
            let buffer = reader
                .fill_buf()
                .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
            if buffer.is_empty() {
                (0, false, true)
            } else if let Some(index) = buffer.iter().position(|byte| *byte == b'\n') {
                (index + 1, true, false)
            } else {
                (buffer.len(), false, false)
            }
        };
        if consumed > 0 {
            reader.consume(consumed);
            pacer.record(consumed, is_current)?;
        }
        if reached_newline || eof {
            return Ok(());
        }
    }
}

struct ScanPacer {
    background: bool,
    bytes_since_pause: usize,
}

impl ScanPacer {
    fn new(background: bool) -> Self {
        Self {
            background,
            bytes_since_pause: 0,
        }
    }

    fn record(&mut self, bytes: usize, is_current: &dyn Fn() -> bool) -> Result<(), String> {
        if !self.background {
            return Ok(());
        }
        self.bytes_since_pause = self.bytes_since_pause.saturating_add(bytes);
        let pause_count = self.bytes_since_pause / BACKGROUND_SCAN_PAUSE_BYTES;
        if pause_count == 0 {
            return Ok(());
        }
        if !is_current() {
            return Err("会话检索已被新的搜索替换".to_string());
        }
        self.bytes_since_pause %= BACKGROUND_SCAN_PAUSE_BYTES;
        std::thread::sleep(BACKGROUND_SCAN_PAUSE.saturating_mul(pause_count as u32));
        Ok(())
    }
}

fn release_large_byte_buffer(buffer: &mut Vec<u8>) {
    if buffer.capacity() > MAX_RETAINED_LINE_BUFFER_BYTES {
        *buffer = Vec::new();
    } else {
        buffer.clear();
    }
}

fn release_large_string_buffer(buffer: &mut String) {
    if buffer.capacity() > MAX_RETAINED_LINE_BUFFER_BYTES {
        *buffer = String::new();
    } else {
        buffer.clear();
    }
}

pub(crate) fn json_record_may_match(line: &[u8], request: &SessionContentSearchRequest) -> bool {
    if request.terms.is_empty() {
        return true;
    }
    request.terms.iter().any(|term| {
        let needle = term.value.as_bytes();
        if needle.is_empty() {
            return true;
        }
        if !term.value.is_ascii() && line.windows(2).any(|window| window == br"\u") {
            return true;
        }
        line.windows(needle.len()).any(|window| {
            if term.value.is_ascii() {
                window.eq_ignore_ascii_case(needle)
            } else {
                window == needle
            }
        })
    })
}

fn read_json_segment(
    reader: BufReader<File>,
    max_bytes: usize,
    skip_partial_line: bool,
    path: &Path,
    label: &str,
    sequence: &mut usize,
    observe: &mut impl FnMut(usize, Value),
) -> Result<(), String> {
    let mut reader = reader.take(max_bytes as u64);
    let mut line = String::new();
    if skip_partial_line {
        reader
            .read_line(&mut line)
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        release_large_string_buffer(&mut line);
    }
    loop {
        line.clear();
        let bytes = reader
            .read_line(&mut line)
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        if bytes == 0 {
            break;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line.trim_end()) {
            observe(*sequence, value);
            *sequence = (*sequence).saturating_add(1);
        }
        release_large_string_buffer(&mut line);
    }
    Ok(())
}

pub(crate) fn compact_json(value: &Value, limit: usize) -> String {
    truncate_text(&serde_json::to_string(value).unwrap_or_default(), limit).0
}

/// 将 JSON 完整序列化给搜索路径使用。展示路径应使用 `compact_json`，搜索
/// 路径则不能先截断，否则关键词落在工具输入/输出尾部时会被漏掉。
pub(crate) fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_line_buffers_release_abnormally_large_allocations() {
        let mut bytes = Vec::with_capacity(MAX_RETAINED_LINE_BUFFER_BYTES + 1);
        bytes.extend_from_slice(b"record");
        release_large_byte_buffer(&mut bytes);
        assert_eq!(bytes.capacity(), 0);

        let mut text = String::with_capacity(MAX_RETAINED_LINE_BUFFER_BYTES + 1);
        text.push_str("record");
        release_large_string_buffer(&mut text);
        assert_eq!(text.capacity(), 0);
    }

    #[test]
    fn scan_line_buffers_keep_small_allocations_for_reuse() {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(b"record");
        release_large_byte_buffer(&mut bytes);
        assert!(bytes.is_empty());
        assert!(bytes.capacity() >= 256);

        let mut text = String::with_capacity(256);
        text.push_str("record");
        release_large_string_buffer(&mut text);
        assert!(text.is_empty());
        assert!(text.capacity() >= 256);
    }
}
