# 更新记录

BalanceHub 的重要变更会记录在这里。

版本规则：

- `MAJOR`：存储结构、配置结构或核心工作流出现不兼容变化。
- `MINOR`：新增兼容功能。
- `PATCH`：问题修复和小幅兼容优化。

## 0.1.0 - 2026-06-25

首次公开版本。

### 新增

- NewAPI 兼容中转站账号管理。
- 余额刷新、签到、用量趋势、请求日志、API Key 管理和修改密码流程。
- Codex / Claude Code 测活，支持中转站单独覆盖全局配置。
- 配置导入导出，支持跨设备迁移。
- 系统通知和 Webhook 通知。
- 基于 GitHub Releases 和 Tauri Updater 的自动更新。
- macOS、Windows、Linux 的 x64 和 ARM64 发布包。

### 说明

- Windows 发布包使用 NSIS `setup.exe`。
- Linux 发布包包含 AppImage、deb、rpm。
- `.sig` 文件是自动更新签名文件，不是手动安装入口。
