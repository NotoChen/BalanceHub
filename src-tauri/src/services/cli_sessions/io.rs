use crate::services::agent_cli::contracts::SessionContentSearchRequest;
use serde_json::Value;
use std::{
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
    let mut file = super::workbench::open_session_file(path)
        .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
    let file_bytes = file
        .metadata()
        .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?
        .len();
    let max_bytes = max_bytes.max(1);
    let mut sequence = 0usize;
    if file_bytes <= max_bytes as u64 {
        read_json_segment(
            file,
            JsonSegmentWindow {
                max_bytes,
                skip_partial_line: false,
                allow_incomplete_window_end: false,
            },
            path,
            label,
            &mut sequence,
            &mut observe,
        )?;
        super::workbench::check_read_budget()?;
        return Ok(false);
    }

    // Large transcripts are append-only in all supported CLIs. Keep both the
    // beginning (title/initial request) and the tail (latest conversation)
    // instead of spending the complete byte budget on stale early turns.
    let head_bytes = max_bytes / 2;
    let tail_bytes = max_bytes.saturating_sub(head_bytes);
    if head_bytes > 0 {
        read_json_segment(
            file.try_clone()
                .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?,
            JsonSegmentWindow {
                max_bytes: head_bytes,
                skip_partial_line: false,
                allow_incomplete_window_end: true,
            },
            path,
            label,
            &mut sequence,
            &mut observe,
        )?;
    }

    super::workbench::check_read_budget()?;
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
        file,
        JsonSegmentWindow {
            max_bytes: tail_bytes,
            skip_partial_line,
            allow_incomplete_window_end: true,
        },
        path,
        label,
        &mut sequence,
        &mut observe,
    )?;
    super::workbench::check_read_budget()?;
    Ok(true)
}

pub(crate) fn read_json_lines_prefix(
    path: &Path,
    max_bytes: usize,
    label: &str,
    mut observe: impl FnMut(Value),
) -> Result<(), String> {
    let file = super::workbench::open_session_file(path)
        .map_err(|error| format!("{label}失败：{error}"))?;
    // Limit below BufReader so a small prefix cannot prefetch the whole pass.
    // A final incomplete or split UTF-8 record is ignored without losing the
    // complete native events that preceded it.
    let mut reader = BufReader::new(file.take(max_bytes as u64));
    let mut line = Vec::new();
    loop {
        super::workbench::check_read_budget()?;
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .map_err(|error| error.to_string())?
            == 0
        {
            break;
        }
        if let Ok(value) = serde_json::from_slice(&line) {
            observe(value);
        }
    }
    Ok(())
}

/// 顺序扫描完整 JSONL 会话文件，但任何时刻只保留一条记录。超过请求固定
/// 单文件上限时明确跳过完整扫描；单条异常记录另有上限，避免损坏或恶意
/// 状态文件迫使桌面进程分配无界内存。
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
    if let Some(limit) = super::workbench::session_file_read_limit(path)? {
        return Err(format!("{label}：{}", limit.reason));
    }
    let file = super::workbench::open_session_file(path)
        .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
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
    reader: &mut BufReader<impl Read>,
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

struct JsonSegmentWindow {
    max_bytes: usize,
    skip_partial_line: bool,
    allow_incomplete_window_end: bool,
}

fn read_json_segment(
    reader: impl Read,
    window: JsonSegmentWindow,
    path: &Path,
    label: &str,
    sequence: &mut usize,
    observe: &mut impl FnMut(usize, Value),
) -> Result<(), String> {
    // Take must wrap the raw reader so BufReader cannot prefetch beyond a window.
    let mut reader = BufReader::new(reader.take(window.max_bytes as u64));
    let mut line = Vec::new();
    if window.skip_partial_line {
        super::workbench::check_read_budget()?;
        reader
            .read_until(b'\n', &mut line)
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        super::workbench::check_read_budget()?;
        release_large_byte_buffer(&mut line);
    }
    loop {
        super::workbench::check_read_budget()?;
        line.clear();
        let bytes = reader
            .read_until(b'\n', &mut line)
            .map_err(|err| format!("{label}失败：{}：{err}", path.display()))?;
        super::workbench::check_read_budget()?;
        if bytes == 0 {
            break;
        }
        let text = match std::str::from_utf8(&line) {
            Ok(text) => text,
            Err(error)
                if window.allow_incomplete_window_end
                    && reader.get_ref().limit() == 0
                    && !line.ends_with(b"\n")
                    && error.error_len().is_none() =>
            {
                break;
            }
            Err(_) => {
                return Err(format!(
                    "{label}失败：{}：stream did not contain valid UTF-8",
                    path.display()
                ));
            }
        };
        if let Ok(value) = serde_json::from_str::<Value>(text.trim_end()) {
            super::workbench::check_read_budget()?;
            observe(*sequence, value);
            *sequence = (*sequence).saturating_add(1);
        }
        release_large_byte_buffer(&mut line);
    }
    super::workbench::check_read_budget()?;
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
    use crate::services::{
        agent_cli::contracts::SessionReadBudget, cli_sessions::workbench::with_read_budget,
    };
    use std::{
        cell::Cell,
        fs,
        path::PathBuf,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Instant,
    };

    const TEST_LABEL: &str = "读取合成 JSONL";
    const SEGMENT_PATH: &str = "synthetic-segment.jsonl";

    struct JsonFile(PathBuf);

    impl JsonFile {
        fn new(name: &str, bytes: &[u8]) -> Self {
            let path = std::env::temp_dir().join(format!(
                "balancehub-session-segment-{name}-{}.jsonl",
                std::process::id()
            ));
            fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }

    impl Drop for JsonFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn segment_records(
        reader: impl Read,
        window: JsonSegmentWindow,
    ) -> (Result<(), String>, Vec<(usize, Value)>) {
        let mut records = Vec::new();
        let mut sequence = 0;
        let result = read_json_segment(
            reader,
            window,
            Path::new(SEGMENT_PATH),
            TEST_LABEL,
            &mut sequence,
            &mut |sequence, value| records.push((sequence, value)),
        );
        (result, records)
    }

    fn assert_utf8_error(error: &str, path: &Path) {
        assert!(error.contains(TEST_LABEL), "{error}");
        assert!(error.contains(path.to_string_lossy().as_ref()), "{error}");
        assert!(
            error.contains("stream did not contain valid UTF-8"),
            "{error}"
        );
    }

    fn read_budget(max_bytes: u64) -> SessionReadBudget {
        SessionReadBudget::new(
            Instant::now() + Duration::from_secs(5),
            Arc::new(AtomicBool::new(false)),
            max_bytes,
        )
    }

    #[test]
    fn scan_line_buffers_release_abnormally_large_allocations() {
        let mut bytes = Vec::with_capacity(MAX_RETAINED_LINE_BUFFER_BYTES + 1);
        bytes.extend_from_slice(b"record");
        release_large_byte_buffer(&mut bytes);
        assert_eq!(bytes.capacity(), 0);
    }

    #[test]
    fn scan_line_buffers_keep_small_allocations_for_reuse() {
        let mut bytes = Vec::with_capacity(256);
        bytes.extend_from_slice(b"record");
        release_large_byte_buffer(&mut bytes);
        assert!(bytes.is_empty());
        assert!(bytes.capacity() >= 256);
    }

    #[test]
    fn limited_json_windows_keep_complete_records_when_multibyte_boundaries_are_cut() {
        const WINDOW_BYTES: usize = 32 * 1024;
        for (character, retained_bytes) in [("中", 1), ("中", 2), ("😀", 1), ("😀", 2), ("😀", 3)]
        {
            let mut bytes = b"{\"id\":\"head\"}\n{\"id\":\"middle\",\"text\":\"".to_vec();
            bytes.resize(WINDOW_BYTES - retained_bytes, b'a');
            bytes.extend_from_slice(character.as_bytes());
            bytes.extend_from_slice(b"a synthetic gap between both sample boundaries");
            let tail_character_start = bytes.len();
            bytes.extend_from_slice(character.as_bytes());
            bytes.extend_from_slice(b"\"}\n");
            let tail_prefix = b"{\"id\":\"tail\",\"padding\":\"";
            let tail_suffix = b"\"}\n";
            let tail_record_bytes = WINDOW_BYTES - (character.len() - 1) - 3;
            bytes.extend_from_slice(tail_prefix);
            bytes.resize(
                bytes.len() + tail_record_bytes - tail_prefix.len() - tail_suffix.len(),
                b'z',
            );
            bytes.extend_from_slice(tail_suffix);

            let text = std::str::from_utf8(&bytes).unwrap();
            assert!(text
                .lines()
                .all(|line| serde_json::from_str::<Value>(line).is_ok()));
            assert!(bytes.len() > WINDOW_BYTES * 2);
            assert_eq!(
                &bytes[WINDOW_BYTES - retained_bytes..WINDOW_BYTES],
                &character.as_bytes()[..retained_bytes]
            );
            assert!(std::str::from_utf8(&bytes[..WINDOW_BYTES])
                .unwrap_err()
                .error_len()
                .is_none());
            let tail_start = bytes.len() - WINDOW_BYTES;
            assert_eq!(tail_start, tail_character_start + 1);
            assert_eq!(bytes[tail_start] & 0xc0, 0x80);

            let file = JsonFile::new("utf8-boundaries", &bytes);
            let mut records = Vec::new();
            let truncated = read_json_lines_limited(
                &file.0,
                WINDOW_BYTES * 2,
                TEST_LABEL,
                |sequence, value| records.push((sequence, value)),
            )
            .unwrap();
            assert!(truncated);
            assert_eq!(
                records
                    .iter()
                    .map(|(sequence, value)| (*sequence, value["id"].as_str().unwrap()))
                    .collect::<Vec<_>>(),
                vec![(0, "head"), (1, "tail")]
            );
        }
    }

    #[test]
    fn segment_only_ignores_incomplete_utf8_at_an_exhausted_window_end() {
        for fragment in [
            b"{\"text\":\"\xe4\xb8".as_slice(),
            b"{\"text\":\"\xf0\x9f".as_slice(),
        ] {
            let mut bytes = b"{\"id\":\"before\"}\n".to_vec();
            bytes.extend_from_slice(fragment);
            for (allow_incomplete_window_end, extra_capacity) in [(true, 0), (false, 0), (true, 1)]
            {
                let (result, records) = segment_records(
                    bytes.as_slice(),
                    JsonSegmentWindow {
                        max_bytes: bytes.len() + extra_capacity,
                        skip_partial_line: false,
                        allow_incomplete_window_end,
                    },
                );
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].0, 0);
                assert_eq!(records[0].1["id"], "before");
                if allow_incomplete_window_end && extra_capacity == 0 {
                    result.unwrap();
                } else {
                    assert_utf8_error(&result.unwrap_err(), Path::new(SEGMENT_PATH));
                }
            }
        }
    }

    #[test]
    fn limited_json_rejects_real_utf8_errors_in_complete_files_and_sampled_lines() {
        let invalid = [
            b"{\"text\":\"\xff\"}\n".as_slice(),
            b"{\"text\":\"\xe4\xb8\n".as_slice(),
            b"{\"text\":\"\xff".as_slice(),
            b"{\"text\":\"\xe4\xb8".as_slice(),
            b"{\"text\":\"\xf0\x9f".as_slice(),
        ];
        for bytes in invalid {
            let file = JsonFile::new("bad-full-utf8", bytes);
            for cap in [bytes.len(), bytes.len() + 1] {
                let error =
                    read_json_lines_limited(&file.0, cap, TEST_LABEL, |_, _| {}).unwrap_err();
                assert_utf8_error(&error, &file.0);
            }
        }
        for invalid_line in &invalid[..3] {
            let cap = invalid_line.len();
            let mut head = invalid_line.to_vec();
            head.resize(cap * 4, b' ');
            let mut tail = vec![b'a'; cap * 3];
            tail.push(b'\n');
            tail.extend_from_slice(invalid_line);
            for bytes in [head, tail] {
                let file = JsonFile::new("bad-sampled-utf8", &bytes);
                let error =
                    read_json_lines_limited(&file.0, cap * 2, TEST_LABEL, |_, _| {}).unwrap_err();
                assert_utf8_error(&error, &file.0);
            }
        }
    }

    #[test]
    fn limited_json_keeps_valid_unterminated_json_and_skips_json_syntax_errors() {
        let mut bytes = b"{\"id\":\"head\"}\nnot json\n{\"id\":\"middle\",\"padding\":\"".to_vec();
        bytes.resize(400, b'a');
        bytes.extend_from_slice(b"\"}\n{\"id\":\"tail\"}");
        let file = JsonFile::new("unterminated-json", &bytes);
        for (cap, expected) in [
            (bytes.len(), vec![(0, "head"), (1, "middle"), (2, "tail")]),
            (64, vec![(0, "head"), (1, "tail")]),
        ] {
            let mut records = Vec::new();
            let truncated = read_json_lines_limited(&file.0, cap, TEST_LABEL, |sequence, value| {
                records.push((sequence, value));
            })
            .unwrap();
            assert_eq!(truncated, cap < bytes.len());
            assert_eq!(
                records
                    .iter()
                    .map(|(sequence, value)| (*sequence, value["id"].as_str().unwrap()))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    struct FailingReader<'a> {
        prefix: &'a [u8],
    }

    impl Read for FailingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.prefix.is_empty() {
                return Err(std::io::Error::other("injected segment read failure"));
            }
            self.prefix.read(buffer)
        }
    }

    #[test]
    fn segment_propagates_read_errors_during_records_and_partial_line_discard() {
        for (skip_partial_line, prefix, expected_records) in [
            (false, b"{\"id\":\"before\"}\n".as_slice(), 1),
            (true, b"\x80partial without newline".as_slice(), 0),
        ] {
            let (result, records) = segment_records(
                FailingReader { prefix },
                JsonSegmentWindow {
                    max_bytes: prefix.len() + 32,
                    skip_partial_line,
                    allow_incomplete_window_end: true,
                },
            );
            let error = result.unwrap_err();
            assert!(error.contains(TEST_LABEL));
            assert!(error.contains(SEGMENT_PATH));
            assert!(error.contains("injected segment read failure"));
            assert_eq!(records.len(), expected_records);
        }
    }

    struct CountedReader<'a> {
        bytes: &'a [u8],
        count: &'a Cell<usize>,
    }

    impl Read for CountedReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let bytes = self.bytes.read(buffer)?;
            self.count.set(self.count.get() + bytes);
            Ok(bytes)
        }
    }

    #[test]
    fn segment_raw_reads_do_not_prefetch_past_the_window() {
        let bytes = b"{\"id\":\"item\"}\n".repeat(256);
        for cap in [1, 31, 257] {
            let count = Cell::new(0);
            let (result, _) = segment_records(
                CountedReader {
                    bytes: &bytes,
                    count: &count,
                },
                JsonSegmentWindow {
                    max_bytes: cap,
                    skip_partial_line: false,
                    allow_incomplete_window_end: true,
                },
            );
            result.unwrap();
            assert_eq!(count.get(), cap);
        }
    }

    #[test]
    fn limited_json_windows_charge_only_their_caps_and_the_boundary_probe() {
        let bytes = b"{\"id\":\"item\"}\n".repeat(256);
        let file = JsonFile::new("shared-byte-budget", &bytes);
        let budget = read_budget(129);
        let result = with_read_budget(&budget, || {
            read_json_lines_limited(&file.0, 128, TEST_LABEL, |_, _| {})
        });
        assert!(result.unwrap());
        assert_eq!(budget.bytes.load(Ordering::Relaxed), 129);
        let budget = read_budget(128);
        let error = with_read_budget(&budget, || {
            read_json_lines_limited(&file.0, 128, TEST_LABEL, |_, _| {})
        })
        .unwrap_err();
        assert!(error.contains("字节预算"));
    }

    #[test]
    fn limited_json_budget_guards_stop_expired_cancelled_and_buffered_reads() {
        let bytes = b"{\"id\":\"first\"}\n{\"id\":\"second\"}\n";
        let file = JsonFile::new("request-guards", bytes);
        let cancelled = read_budget(4096);
        cancelled.cancelled.store(true, Ordering::Release);
        let mut expired = read_budget(4096);
        expired.deadline = Instant::now();
        for (budget, expected) in [(cancelled, "已取消"), (expired, "时间预算")] {
            let mut observed = 0;
            let error = with_read_budget(&budget, || {
                read_json_lines_limited(&file.0, bytes.len(), TEST_LABEL, |_, _| observed += 1)
            })
            .unwrap_err();
            assert!(error.contains(expected));
            assert_eq!(observed, 0);
        }
        for bytes in [b"{\"id\":\"first\"}\n".as_slice(), bytes.as_slice()] {
            let file = JsonFile::new("buffered-cancellation", bytes);
            let budget = read_budget(4096);
            let mut observed = Vec::new();
            let error = with_read_budget(&budget, || {
                read_json_lines_limited(&file.0, bytes.len(), TEST_LABEL, |sequence, value| {
                    assert_eq!(budget.bytes.load(Ordering::Relaxed), bytes.len() as u64);
                    observed.push((sequence, value));
                    budget.cancelled.store(true, Ordering::Release);
                })
            })
            .unwrap_err();
            assert!(error.contains("已取消"));
            assert_eq!(observed.len(), 1);
            assert_eq!(observed[0].0, 0);
            assert_eq!(observed[0].1["id"], "first");
        }
    }
}
