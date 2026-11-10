//! 测试断言的路径归一化。
//!
//! 测试里的路径按 `/` 书写，便于所有平台读同一份断言；Windows 的
//! `Path::join` 会生成 `\`，比较前统一换成 `/`。仅测试构建使用。

pub(crate) fn slashes(value: &str) -> String {
    value.replace('\\', "/")
}

pub(crate) fn slash_args(args: &[String]) -> Vec<String> {
    args.iter().map(|arg| slashes(arg)).collect()
}

pub(crate) fn ends_with_path(value: &str, suffix: &str) -> bool {
    slashes(value).ends_with(suffix)
}

pub(crate) fn same_path(value: &str, expected: &str) -> bool {
    slashes(value) == expected
}
