//! 极简日志：服务端没有桌面端的文件日志设置，直接把 `log` 门面写到标准输出。
//!
//! 级别由 `BALANCEHUB_LOG`（`error` / `warn` / `info` / `debug` / `trace`）控制，
//! 默认 `info`。

use log::{LevelFilter, Log, Metadata, Record};

struct StdoutLogger;

impl Log for StdoutLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        println!(
            "{} {} {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            record.level(),
            record.args()
        );
    }

    fn flush(&self) {}
}

pub fn init() {
    let level = std::env::var("BALANCEHUB_LOG")
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "error" => Some(LevelFilter::Error),
            "warn" => Some(LevelFilter::Warn),
            "info" => Some(LevelFilter::Info),
            "debug" => Some(LevelFilter::Debug),
            "trace" => Some(LevelFilter::Trace),
            _ => None,
        })
        .unwrap_or(LevelFilter::Info);
    let _ = log::set_logger(&StdoutLogger).map(|()| log::set_max_level(level));
}
