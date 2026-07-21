# 功能与架构参考

本文承接 README 中不适合展开的细节，集中说明 BalanceHub 的功能边界、技术框架、架构分层和目录结构。

## 功能清单

| 功能 | 定义 | 实现位置 | 说明 |
| --- | --- | --- | --- |
| 中转站账号管理 | 把多个 NewAPI 兼容站点集中到一个桌面面板中管理。 | `src/components/ProviderBoard.vue`、`src/components/ProviderCard.vue`、`src/components/ProviderEditorDrawer.vue`、`src-tauri/src/services/provider_service/` | 当前 UI 只展示 NewAPI 类型；AnyRouter 按 NewAPI 方言兼容处理，不作为独立类型展示。 |
| 认证方式管理 | 按站点保存 Cookie、访问令牌、API Key 等认证信息。 | `src/stores/provider-types.ts`、`src-tauri/src/models/`、`src-tauri/src/providers/newapi_http.rs` | 默认优先级是 Cookie > 访问令牌 > API Key。 |
| 站点探测 | 从中转站读取名称、图标、额度单位和货币符号。 | `src-tauri/src/providers/newapi_site.rs` | 用于减少手动填写，并保证余额、日志、签到记录单位显示一致。 |
| 余额刷新 | 查询账号或 API Key 当前额度、已用额度和可用额度。 | `src-tauri/src/providers/newapi_quota.rs`、`src-tauri/src/services/provider_service/quota.rs` | API Key 查询明确按 Key 维度展示；无限额度按无限状态处理。 |
| 自动刷新 | 按用户配置周期刷新中转站状态。 | `src-tauri/src/services/scheduler.rs` | 适合多站点长期挂后台观察余额和异常状态。 |
| 签到 | 对支持签到的 NewAPI 兼容站点执行每日签到。 | `src-tauri/src/providers/newapi_checkin.rs`、`src-tauri/src/providers/anyrouter.rs`、`src-tauri/src/services/provider_service/check_in.rs` | 签到结果会结合接口返回和余额变化展示，避免把无余额变化误判为有效收益。 |
| 签到记录 | 展示每日签到结果和余额增量。 | `src/components/CheckInCalendarModal.vue`、`src-tauri/src/providers/newapi_checkin/records.rs` | 兼容站点返回的显示额度和 NewAPI 原始额度单位转换。 |
| 用量趋势 | 查看周期内请求量和额度消耗趋势。 | `src/components/UsageTrendModal.vue`、`src/composables/useUsageTrendChart.ts`、`src-tauri/src/providers/newapi_usage.rs` | 用于判断站点消耗变化、请求峰值和账户使用节奏。 |
| 请求日志 | 查看模型请求记录、状态、Token 和消耗。 | `src/components/RequestLogsModal.vue`、`src-tauri/src/providers/newapi_logs.rs` | 消耗金额沿用站点元数据中的额度单位、货币符号和换算规则。 |
| API Key 管理 | 查看、创建、删除中转站 API Key，并读取 Key 额度。 | `src/components/ApiKeyManagerModal.vue`、`src-tauri/src/providers/newapi_keys.rs` | 适合从桌面端快速生成 Codex / Claude Code 使用的 Key。 |
| 修改密码 | 在支持的 NewAPI 站点上发起密码修改流程。 | `src/components/PasswordChangeModal.vue`、`src-tauri/src/providers/newapi_account.rs` | 仅在站点能力和认证信息满足要求时展示操作入口。 |
| 可用模型 | 读取中转站可用模型清单。 | `src/components/AvailableModelsModal.vue`、`src/composables/useAvailableModels.ts` | 用于确认当前站点是否支持目标模型。 |
| CLI 测活 | 使用 Codex / Claude Code CLI 对中转站执行真实请求验证。 | `src-tauri/src/services/liveness/command.rs`、`src-tauri/src/services/liveness/process.rs` | 测活会消耗真实额度，首次开启自动测活前会要求确认。 |
| CLI 候选扫描 | 扫描本机 Codex / Claude Code 可执行文件。 | `src-tauri/src/services/liveness/cli.rs`、`src/components/settings/SettingsCliManager.vue` | 扫描 PATH、常见安装目录和 Node 包管理器路径；不扫描 Codex Desktop App 内置二进制。 |
| 临时 CLI 启动 | 使用当前中转站临时启动 Codex / Claude Code CLI。 | `src-tauri/src/services/temporary_cli.rs`、`src/components/ProviderCard.vue`、`src/composables/useProviderActions.ts` | 仅覆盖 API Key、Base URL 和模型，工作目录由用户选择，其他 CLI 配置继续沿用默认配置。 |
| CC Switch 导入 | 将当前中转站配置通过深链交给 CC Switch。 | `src/utils/ccswitch-deeplink.ts`、`src-tauri/src/lib.rs` | 支持 Codex、Claude Code、OpenCode、OpenClaw、Hermes 目标；不声明 Gemini 支持。 |
| 测活时间线 | 保存并展示每个中转站最近的测活结果。 | `src/components/ProviderLivenessTimeline.vue`、`src/utils/provider-liveness.ts` | 用于区分余额正常但 CLI 不可用、模型不可用或网络异常。 |
| 系统通知 | 对自动刷新、自动签到等结果发出系统通知。 | `src-tauri/src/services/notifications/adapters/system.rs` | 系统通知使用纯文本内容，避免显示 Markdown 语法。 |
| Webhook 通知 | 通过钉钉、企业微信、飞书、Slack 或通用 Webhook 推送消息。 | `src-tauri/src/services/notifications/adapters/` | 不同渠道按各自消息格式发送，并处理签名和返回校验。 |
| 配置导入导出 | 将本地中转站配置导出备份或迁移到另一台设备。 | `src/composables/useAppDataTransfer.ts`、`src-tauri/src/storage.rs` | 导出的配置可能包含敏感认证信息，应自行妥善保管。 |
| 本地存储恢复 | 读写本地配置，并从临时文件恢复异常写入。 | `src-tauri/src/storage.rs` | 避免写入中断导致配置文件损坏。 |
| 系统托盘 / 菜单栏 | 将桌面 App 保持在后台并提供快速入口。 | `src-tauri/src/tray.rs` | 支持显示窗口、刷新和退出，适合长期后台运行。 |
| 自动更新 | 从 GitHub Releases 读取更新元数据并安装新版本。 | `src-tauri/tauri.conf.json`、`src-tauri/tauri.release.conf.json`、`.github/workflows/release.yml` | `.sig` 是 Tauri updater 自动更新签名文件，不是用户手动安装入口。 |
| 主题和响应式布局 | 提供明暗主题和不同窗口宽度下的可用布局。 | `src/styles/modules/` | 优先保证桌面工具场景的信息密度和扫描效率。 |

## 技术框架

| 层级 | 技术 | 用途 |
| --- | --- | --- |
| 桌面容器 | Tauri 2 | 打包跨平台桌面应用，提供窗口、托盘、权限、通知、更新和系统能力。 |
| 后端语言 | Rust 2021 | 实现 NewAPI 请求、调度、存储、通知、测活和 Tauri command。 |
| 前端框架 | Vue 3 | 构建设置、卡片、弹窗、抽屉和状态交互。 |
| 前端状态 | Pinia | 管理中转站、设置、运行状态和 UI 派生数据。 |
| UI 组件 | Arco Design Vue | 提供表单、弹窗、按钮、抽屉、消息提示等基础组件。 |
| 构建工具 | Vite 6 | 前端开发服务器和生产构建。 |
| 类型系统 | TypeScript | 约束前端状态、API 返回和组件数据。 |
| HTTP 客户端 | reqwest | 后端访问 NewAPI、Webhook 和更新相关接口。 |
| 异步运行 | tokio | 支撑定时任务、网络请求和外部进程等待。 |
| 序列化 | serde / serde_json | 读写本地配置、解析 NewAPI 响应、构造 Webhook 请求。 |
| 加密签名 | hmac / sha2 / base64 | 生成钉钉、飞书等 Webhook 签名。 |
| 时间处理 | chrono | 处理签到日期、调度时间和本地时间判断。 |
| Tauri 插件 | opener / dialog / notification / autostart / updater / process | 打开链接、文件选择、系统通知、开机启动、自动更新和进程能力。 |

## 架构说明

BalanceHub 按“前端交互、Tauri command、Rust 服务、站点接口、本地存储”分层：

```text
Vue 3 UI
  -> composables / Pinia store
    -> Tauri invoke API
      -> src-tauri/src/lib.rs command
        -> services/provider_service 调度业务
          -> providers/newapi_* 访问中转站
          -> services/liveness 执行 CLI 测活
          -> services/notifications 发送通知
          -> storage.rs 读写本地配置
```

前端负责操作体验和状态呈现；Rust 负责带认证的站点请求、调度、持久化、通知和外部 CLI 调用。Cookie、Token、API Key 不需要交给远端服务，也不依赖浏览器页面直接访问中转站。

## 目录说明

```text
.
├── README.md                         # 项目入口、能力摘要、截图和文档导航
├── CHANGELOG.md                      # 用户可感知的版本更新记录
├── CONTRIBUTING.md                   # 本地开发、二次开发和仓库协作边界
├── docs/                             # GitHub Pages 文档站和详细说明
│   ├── index.html                    # 项目主页
│   ├── getting-started.md            # 快速开始
│   ├── provider-config.md            # 中转站配置说明
│   ├── liveness.md                   # Codex / Claude Code 测活说明
│   ├── release.md                    # 发布包和自动更新说明
│   ├── faq.md                        # 常见问题
│   ├── reference.md                  # 功能与架构参考
│   └── assets/screenshots/           # README 和 Pages 共用截图
├── .github/
│   ├── workflows/ci.yml              # 多平台质量检查
│   ├── workflows/release.yml         # tag 触发的正式打包发布
│   ├── workflows/pages.yml           # GitHub Pages 文档站部署
│   ├── workflows/close-pull-requests.yml
│   ├── scripts/release-notes.mjs     # 从 CHANGELOG 生成 Release 正文
│   └── ISSUE_TEMPLATE/               # Issue 模板
├── src/                              # Vue 前端
│   ├── App.vue                       # 应用根组件
│   ├── main.ts                       # 前端入口
│   ├── api/                          # Tauri invoke 封装
│   ├── components/                   # 页面、抽屉、弹窗、卡片和设置组件
│   ├── composables/                  # 前端状态编排和业务动作
│   ├── stores/                       # Pinia store、类型和默认值
│   ├── styles/                       # 全局样式和业务模块样式
│   └── utils/                        # 展示格式化、拖拽、测活、趋势等工具函数
└── src-tauri/                        # Tauri / Rust 后端
    ├── tauri.conf.json               # 开发配置、窗口、安全策略和插件配置
    ├── tauri.release.conf.json       # Release / updater 构建配置
    ├── capabilities/                 # Tauri 权限能力配置
    ├── icons/                        # 应用图标
    └── src/
        ├── lib.rs                    # Tauri command 注册和应用初始化
        ├── main.rs                   # 桌面进程入口
        ├── state.rs                  # 全局运行状态
        ├── storage.rs                # 本地配置读写、版本检查和恢复
        ├── tray.rs                   # 系统托盘 / 菜单栏
        ├── network.rs                # 系统代理和网络辅助逻辑
        ├── adapters/                 # 站点适配器入口
        ├── models/                   # Rust 数据模型
        ├── providers/                # NewAPI 兼容站点接口实现
        └── services/                 # 业务服务、测活、通知和调度
```

## 关键边界

- BalanceHub 只考虑桌面 App，不提供 Web 自部署版本。
- 当前仅支持 NewAPI 兼容中转站；AnyRouter 作为 NewAPI 方言兼容，不在 UI 上作为独立类型展示。
- sub2api 尚未接入，不在文档中声明支持。
- 仓库只接受 Issue，不接受 Pull Request。
- 不提交本地配置、导出的中转站配置、Cookie、访问令牌、API Key、updater 私钥或真实账号数据。
