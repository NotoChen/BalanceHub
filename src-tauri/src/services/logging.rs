//! 后台任务的失败线索落盘。
//!
//! 托盘常驻的后台进程出问题时，用户能拿出来的只有截图；这里把调度、通知、
//! 索引维护等后台路径的告警写进按天滚动的日志文件，出问题可以直接把文件发出来。
//!
//! 级别通过设置即时切换，不需要重启；文件按天滚动，并按天数据保留期清理旧文件。

use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

use log::{Log, Metadata, Record};

static LOGGER: OnceLock<FileLogger> = OnceLock::new();
static RETENTION_DAYS: AtomicU64 = AtomicU64::new(7);

struct FileLogger {
    directory: PathBuf,
    current: Mutex<Option<DailyFile>>,
}

struct DailyFile {
    /// 本地日期 `YYYY-MM-DD`，与文件名同源；跨天后换文件。
    date: String,
    file: File,
}

impl FileLogger {
    fn write_line(&self, level: log::Level, message: &str) {
        let now = local_now();
        let mut guard = self.current.lock().unwrap_or_else(|err| err.into_inner());
        let rotate = guard.as_ref().is_none_or(|daily| daily.date != now.date);
        if rotate {
            match open_daily_file(&self.directory, &now.date) {
                Ok(file) => {
                    *guard = Some(DailyFile {
                        date: now.date.clone(),
                        file,
                    });
                }
                Err(_) => return,
            }
            prune_old_logs(&self.directory, RETENTION_DAYS.load(Ordering::Relaxed));
        }
        if let Some(daily) = guard.as_mut() {
            let _ = writeln!(
                daily.file,
                "{} {} {}",
                now.stamp,
                level_label(level),
                message
            );
            let _ = daily.file.flush();
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        // 开发期在终端里同时可见，安装版只写文件。
        #[cfg(debug_assertions)]
        eprintln!("[{}] {message}", level_label(record.level()));
        self.write_line(record.level(), &message);
    }

    fn flush(&self) {}
}

/// 在 Tauri 启动流程里安装一次。目录不可用时保持静默：
/// 日志是辅助能力，不能因为它没装好而阻塞应用启动。
///
/// 目录由调用方给出，不使用 Tauri 的 `app_log_dir()`：开发版必须和安装版
/// 分开落盘，路径规则和配置目录保持一致。
pub fn init(directory: &Path, settings: &crate::models::AppSettings) {
    RETENTION_DAYS.store(settings.log_retention_days, Ordering::Relaxed);
    log::set_max_level(settings.log_level.as_filter());
    if LOGGER.get().is_some() {
        return;
    }
    if std::fs::create_dir_all(directory).is_err() {
        return;
    }
    prune_old_logs(directory, settings.log_retention_days);
    let logger = FileLogger {
        directory: directory.to_path_buf(),
        current: Mutex::new(None),
    };
    if LOGGER.set(logger).is_ok() {
        if let Some(logger) = LOGGER.get() {
            let _ = log::set_logger(logger);
        }
    }
}

/// 设置保存后立即生效，无需重启。
pub fn apply_settings(settings: &crate::models::AppSettings) {
    RETENTION_DAYS.store(settings.log_retention_days, Ordering::Relaxed);
    log::set_max_level(settings.log_level.as_filter());
}

fn level_label(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "ERROR",
        log::Level::Warn => "WARN",
        log::Level::Info => "INFO",
        log::Level::Debug => "DEBUG",
        log::Level::Trace => "TRACE",
    }
}

struct LocalNow {
    date: String,
    stamp: String,
}

fn local_now() -> LocalNow {
    let now = chrono::Local::now();
    LocalNow {
        date: now.format("%Y-%m-%d").to_string(),
        stamp: now.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
    }
}

fn open_daily_file(directory: &Path, date: &str) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join(format!("balancehub-{date}.log")))
}

/// 只清理本模块自己生成的 `balancehub-YYYY-MM-DD.log`，目录里的其他文件不碰。
///
/// 判据用文件名里的日期而不是 mtime：备份、同步工具会改写 mtime，日期才是
/// 这份日志真正属于哪一天。保留期按自然日计算，`retention_days = 1` 只留今天。
fn prune_old_logs(directory: &Path, retention_days: u64) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let cutoff = chrono::Local::now().date_naive()
        - chrono::Duration::days(retention_days.saturating_sub(1) as i64);
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(date) = name
            .strip_prefix("balancehub-")
            .and_then(|name| name.strip_suffix(".log"))
            .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        else {
            continue;
        };
        if date < cutoff {
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::prune_old_logs;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn pruning_removes_only_old_balancehub_logs() {
        let directory = std::env::temp_dir().join(format!(
            "balancehub-log-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&directory).expect("temp dir");
        let old_log = directory.join("balancehub-2000-01-01.log");
        let foreign = directory.join("other-app.log");
        let today_log = directory.join(format!(
            "balancehub-{}.log",
            chrono::Local::now().format("%Y-%m-%d")
        ));
        File::create(&old_log)
            .and_then(|mut file| file.write_all(b"old"))
            .expect("old log");
        File::create(&foreign)
            .and_then(|mut file| file.write_all(b"keep"))
            .expect("foreign log");
        File::create(&today_log)
            .and_then(|mut file| file.write_all(b"today"))
            .expect("today log");

        prune_old_logs(&directory, 1);

        assert!(!old_log.exists());
        assert!(today_log.exists());
        assert!(foreign.exists());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
