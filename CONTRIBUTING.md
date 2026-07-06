# 反馈与二次开发

BalanceHub 当前只接受 Issue，不接受 Pull Request。仓库保留开发说明，是为了让维护者和需要二次开发的人能理解项目结构、启动流程和检查方式；它不是外部合并代码的流程说明。

如果你只是想反馈问题或建议，请直接使用 [Issues](https://github.com/NotoChen/BalanceHub/issues)。

## 协作边界

- 不接受 Pull Request。
- 问题反馈、功能建议和兼容性信息请通过 Issue 提交。
- 维护者会根据项目方向自行实现、测试和合并改动。
- 允许非商业二次开发，但分发修改版或派生作品时必须遵守 `LICENSE`。

仓库中的 `.github/pull_request_template.md` 和自动关闭 PR 的 workflow 只用于说明协作规则，避免误开 PR 后没有反馈。

## 项目结构

```text
.
├── .github/                    # Issue 模板、CI、Release 和 PR 自动关闭 workflow
├── docs/                       # GitHub Pages 文档站
│   └── assets/screenshots/     # README 和文档页使用的真实 App 截图
├── src/                        # Vue 前端
│   ├── api/                    # Tauri invoke 封装
│   ├── assets/                 # 前端静态资源
│   ├── components/             # 页面、抽屉、弹窗和业务组件
│   ├── composables/            # 前端业务状态和交互逻辑
│   ├── stores/                 # Pinia store、类型和默认值
│   ├── styles/                 # 全局样式和模块样式
│   └── utils/                  # 展示格式化、拖拽、测活等纯工具逻辑
├── src-tauri/                  # Tauri / Rust 后端
│   ├── capabilities/           # Tauri 权限能力配置
│   ├── icons/                  # 应用图标
│   ├── src/
│   │   ├── adapters/           # 外部命令或系统能力适配
│   │   ├── models/             # Rust 数据模型和序列化结构
│   │   ├── providers/          # NewAPI 兼容中转站接口实现
│   │   ├── services/           # 调度、通知、测活、中转站服务
│   │   ├── lib.rs              # Tauri command 注册和应用初始化
│   │   ├── storage.rs          # 本地配置读写、迁移和恢复
│   │   └── tray.rs             # 系统托盘 / 菜单栏相关逻辑
│   ├── tauri.conf.json         # 开发构建配置
│   └── tauri.release.conf.json # Release / updater 相关配置
├── package.json                # 前端依赖和 npm 脚本
├── src-tauri/Cargo.toml        # Rust crate、Tauri 和后端依赖
└── README.md                   # 项目入口说明
```

## 开发环境

建议使用以下版本或更新版本：

- Node.js 20 LTS+
- npm 10+
- Rust stable
- Tauri CLI 2.x，对应 `@tauri-apps/cli`
- 操作系统对应的 Tauri 2 构建依赖

平台依赖：

- macOS：Xcode Command Line Tools。
- Windows：Microsoft C++ Build Tools、WebView2 Runtime。
- Linux：WebKitGTK、GTK、OpenSSL、AppIndicator 等 Tauri 2 所需系统库，具体包名按发行版不同会有差异。

## 本地开发

安装依赖：

```bash
npm install
```

启动桌面开发环境：

```bash
npm run tauri dev
```

只启动前端 Vite：

```bash
npm run dev
```

前端生产构建：

```bash
npm run build
```

本地打包桌面应用：

```bash
npm run tauri build
```

## 质量检查

提交前至少运行：

```bash
npm run build
```

Rust 检查：

```bash
cd src-tauri
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

文档或配置改动也应检查：

```bash
git diff --check
```

## 本地数据

BalanceHub 的真实账号配置保存在系统应用配置目录，不在仓库内。不要提交：

- 本地应用数据。
- 导出的中转站配置。
- Cookie、访问令牌、API Key。
- Tauri updater 私钥。
- 打包产物、缓存和依赖目录。

## 二次开发建议

二次开发时建议从以下顺序理解代码：

1. `src/App.vue`：应用入口和主要组件组合。
2. `src/composables/useAppController.ts`：前端主要状态编排。
3. `src/stores/provider-types.ts`：前端中转站、设置、日志、用量等类型。
4. `src-tauri/src/lib.rs`：Tauri command 注册。
5. `src-tauri/src/models/`：后端序列化模型。
6. `src-tauri/src/providers/`：NewAPI 兼容接口实现。
7. `src-tauri/src/services/`：调度、通知、测活和跨模块业务。

保持改动边界清晰。UI、前端状态、后端 command、存储模型和 provider 接口尽量分开修改；涉及数据结构变更时，需要同步前后端类型和本地配置迁移逻辑。

## 维护者发布

发布新版本时，下面几个位置必须保持一致：

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- Git tag，例如 `v0.2.0`

用户可感知的功能变化需要更新 `CHANGELOG.md`。安装、更新、配置、发布流程变化需要同步更新 README 或 `docs/`。

自动更新读取 GitHub Release 的 `latest.json`，并使用 `.sig` 文件校验更新包。`.sig` 不是手动安装入口。

## Issue 规则

- 问题反馈请提供复现路径、系统信息和 BalanceHub 版本。
- 功能建议请描述真实使用场景，不只描述界面形式。
- 涉及中转站兼容问题时，请说明中转站类型和可公开的接口行为。
- 不要在 Issue 中粘贴账号 Cookie、访问令牌、API Key 或其他敏感配置。
