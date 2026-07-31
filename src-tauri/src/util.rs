use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// 自 Unix 纪元以来的毫秒数。
pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

/// 自 Unix 纪元以来的秒数。
pub fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

/// 当前本地月份，格式 `YYYY-MM`。
pub fn current_month() -> String {
    chrono::Local::now().format("%Y-%m").to_string()
}

/// NewAPI 额度单位（quota_per_unit）的默认值。
pub const DEFAULT_QUOTA_PER_UNIT: f64 = 500_000.0;

/// 有上限地读取 UTF-8 文本文件，避免损坏或恶意文件把整个进程内存吃满。
pub fn read_text_file_limited(
    path: &Path,
    max_bytes: usize,
    context: &str,
) -> Result<String, String> {
    let file =
        File::open(path).map_err(|err| format!("{context}失败({}): {err}", path.display()))?;
    let mut bytes = Vec::with_capacity(max_bytes.min(64 * 1024));
    file.take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("{context}失败({}): {err}", path.display()))?;
    if bytes.len() > max_bytes {
        return Err(format!(
            "{context}失败({})：文件超过 {} 上限",
            path.display(),
            byte_limit_label(max_bytes)
        ));
    }
    String::from_utf8(bytes).map_err(|err| {
        format!(
            "{context}失败({})：文件不是有效 UTF-8：{err}",
            path.display()
        )
    })
}

fn byte_limit_label(bytes: usize) -> String {
    if bytes >= 1024 * 1024 {
        format!("{} MiB", bytes / 1024 / 1024)
    } else {
        format!("{} KiB", bytes / 1024)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, time::SystemTime};

    fn temp_file(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "balancehub-util-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn limited_reader_rejects_oversized_files() {
        let path = temp_file("oversized");
        fs::write(&path, b"12345").unwrap();
        let result = read_text_file_limited(&path, 4, "读取测试文件");
        let _ = fs::remove_file(path);
        assert!(result.unwrap_err().contains("上限"));
    }

    #[test]
    fn limited_reader_returns_utf8_text() {
        let path = temp_file("utf8");
        fs::write(&path, "BalanceHub").unwrap();
        let result = read_text_file_limited(&path, 64, "读取测试文件").unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(result, "BalanceHub");
    }
}
