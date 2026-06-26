# 发布与更新

BalanceHub 通过 GitHub Releases 分发桌面安装包。

## 发布包

当前发布流程会构建：

- macOS Apple Silicon 和 Intel 的 `.dmg`。
- Windows x64 和 ARM64 的 NSIS `setup.exe`。
- Linux x64 和 ARM64 的 AppImage、deb、rpm。

## 自动更新

Release 会生成 Tauri updater 需要的 `latest.json` 和 `.sig` 文件。应用内检查更新时会读取 `latest.json`，选择当前平台匹配的安装包，并用 `.sig` 校验。

`.sig` 只服务于应用内自动更新，不是用户需要手动打开的文件。

## 版本说明

用户可感知的变化会记录在 [CHANGELOG.md](https://github.com/NotoChen/BalanceHub/blob/main/CHANGELOG.md)。正式发布时，GitHub Release 页面也会展示对应版本说明。
