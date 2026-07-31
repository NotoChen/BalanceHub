# 发布与更新

BalanceHub 通过 GitHub Releases 分发桌面安装包。

## 发布包

当前发布流程会构建：

- macOS Apple Silicon 和 Intel 的 `.dmg`。
- Windows x64 和 ARM64 的 NSIS `setup.exe`。
- Linux x64 和 ARM64 的 AppImage、deb、rpm。

## 发布前检查

打 tag 前先在本地确认：

- `git status --short` 没有未提交改动。
- `npm run build` 通过。
- `cd src-tauri && cargo fmt --check` 通过。
- `cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings` 通过。
- `cd src-tauri && cargo test` 通过。
- 当前系统至少完成一次真实安装包构建和启动验证。
- GitHub Secrets 已配置 `TAURI_SIGNING_PRIVATE_KEY` 和 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。

## 自动更新

Release 会生成 Tauri updater 需要的 `latest.json` 和 `.sig` 文件。应用内检查更新时会读取 `latest.json`，选择当前平台匹配的安装包，并用 `.sig` 校验。

客户端在启动 30 秒后静默检查，之后每 6 小时检查；发现新版本只提示，不会自动下载。用户确认后才开始下载，下载阶段支持取消，并受停滞超时、总时长和 256 MiB 包大小上限保护。进入签名校验和系统安装阶段后不允许取消。

`.sig` 只服务于应用内自动更新，不是用户需要手动打开的文件。

## 版本说明

用户可感知的变化会记录在 [CHANGELOG.md](https://github.com/NotoChen/BalanceHub/blob/main/CHANGELOG.md)。正式发布时，GitHub Release 页面也会展示对应版本说明。
