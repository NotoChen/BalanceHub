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
│   │   └── provider-card/      # 中转站卡片头部、主体、操作与菜单
│   ├── composables/            # 前端业务状态和交互逻辑
│   ├── stores/                 # Pinia store、类型和默认值
│   ├── styles/                 # 全局样式和模块样式
│   └── utils/                  # 展示格式化、拖拽、测活等纯工具逻辑
├── src-tauri/                  # Tauri / Rust 后端
│   ├── capabilities/           # Tauri 权限能力配置
│   ├── icons/                  # 应用图标
│   ├── src/
│   │   ├── adapters/           # NewAPI、Sub2API、通用 API 与协议探测适配
│   │   ├── commands/           # Tauri command 实现与注册清单
│   │   ├── contracts.rs        # Rust 计算并输出给前端的 IPC View
│   │   ├── desktop.rs          # 桌面应用初始化、插件和单实例编排
│   │   ├── models/             # Rust 数据模型和序列化结构
│   │   ├── network/            # HTTP 客户端、代理解析和平台系统代理读取
│   │   ├── platform/           # 深链、后台进程等桌面平台差异封装
│   │   ├── services/           # 调度、通知、测活、中转站服务
│   │   ├── lib.rs              # Rust crate 入口
│   │   ├── storage.rs          # 本地配置模块入口
│   │   ├── storage/            # 配置读写、迁移、恢复与测试
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

带安装版配置副本启动（调试真实数据）：

```bash
npm run dev:session
```

`dev:session` 先把安装版配置只读复制到 `~/Library/Caches/BalanceHub/dev-session`，再在仓库 `target/dev-session` 下建立隔离的 `BALANCEHUB_DEV_ROOT` 并启动 `tauri dev`；安装版目录本身不会被写入。脚本同时注入 `BALANCEHUB_DEV_SAMPLE=1`，开发版第一次加载配置时把各类卡片形态的演示数据并入：演示卡片以「演示 ·」开头、按 ID 合并，真实站点原样保留，并关闭自动刷新、签到与测活，重复启动不会重复追加。设置页的「追加演示数据」在演示卡片被删掉后可以再补一次。

只启动前端 Vite：

```bash
npm run dev
```

前端生产构建：

```bash
npm run build
npm test
```

推荐使用统一自检入口：

```bash
npm run doctor:platform  # 只读检查平台脚本变量和构建空间
npm run doctor           # 任务收尾时按需执行完整质量检查
```

生成给平台 shell 执行的脚本时，内部控制变量必须避开宿主解释器的自动变量和保留字，并使用项目命名空间：Unix shell 使用 `bh_` 前缀，Windows 批处理使用 `BH_` 前缀。CLI 协议要求的外部环境变量（例如 `OPENAI_API_KEY`、`TERM`）按其约定名称保留，不属于内部控制变量。重点规避 zsh 的 `status`、`pipestatus`、`argv`、`commands`、`funcstack`、`history`、`options`、`signals`、`words`，POSIX shell 的 `PWD`、`OLDPWD`、`PPID`，PowerShell 的 `$PID`、`$HOME`、`$PWD`、`$?`、`$LASTEXITCODE`，以及 cmd 的 `ERRORLEVEL`、`CD`、`DATE`、`TIME`、`RANDOM`。平台脚本改动后必须运行 `npm run doctor:platform`。

本地 Cargo、Tauri 和 doctor 共用 `scripts/build-cache.mjs` 解析的构建环境。macOS 默认 target 为 `~/Library/Caches/BalanceHub/cargo-target`；Windows 使用 `%LOCALAPPDATA%/BalanceHub/cargo-target`；Linux 使用 XDG 缓存目录。doctor 复用同一目录，不再单独创建 `doctor` target。通过 `CARGO_TARGET_DIR` 可以显式覆盖，相对路径统一以仓库根目录为基准；CI 可以使用自己的工作区目录。

每次命令启动都会注入相同环境，默认 `CARGO_BUILD_JOBS=2`、`CARGO_INCREMENTAL=0`；显式环境设置会被保留。关闭增量编译可以减少磁盘占用，代价是部分重新编译耗时增加。不要依赖上次终端的 `export`，也不要直接执行未配置环境的 Cargo 命令重新生成仓库内 target。

本机可额外在 Git 本地排除列表中登记 `.cargo/config.toml`，使用绝对路径设置相同的 `[build] target-dir`、`jobs=2`、`incremental=false`，为仓库内的原生 Cargo 和 IDE 提供默认值。该文件仅在本机使用，不提交或复制到其他机器；默认目录变更时需同步。原生 Cargo 不执行下面的空间预检，日常任务仍应通过 npm 入口运行。

重型 Rust/Tauri 构建前后都会检查 target 所在磁盘，默认可用空间不足 20 GiB 时阻止新构建。只读信息、格式检查和清理仍可执行。target 超过 8 GiB 时提示复查历史产物，不会自动删除；检查也会提示遗留的仓库或 doctor 目录。阈值分别由 `BALANCEHUB_MIN_FREE_GIB` 和 `BALANCEHUB_TARGET_WARN_GIB` 配置，`npm run doctor -- --strict-cache` 可将缓存超限作为失败。预检或前序步骤失败后，自检停止后续构建。

只有确认没有构建进程或运行中的开发程序占用目标产物时，才执行 `npm run cargo -- clean`。需要清理遗留目录时，可以显式使用 `npm run cargo -- clean --target-dir /absolute/old/target`。不要为等待构建锁另建 target；临时干净构建验证结束后应回收临时目录。诊断结果只输出到终端，不生成报告文件。

### 异步 UI 与并发状态约束

- 会调用 Tauri IPC、外部进程、终端自动化或网络的操作必须有明确的超时、取消机制或后端任务状态；成功、失败和超时都必须释放前端忙碌状态。
- 启动终端、Agent CLI 等不可预测时长的外部动作，确认后应立即关闭发起弹窗并转入后台任务中心；不得让启动 Promise、进程轮询或外部窗口状态持续禁用整个弹窗、页面或主面板。
- 普通异步操作不得用 `closable`、`mask-closable`、`esc-to-close` 把模态窗口锁死。只有签名校验、更新安装等无法安全中断的关键事务可以例外，并在模板中添加 `balancehub-critical-modal-lock:` 注释说明取消边界。
- 异步结果防过期使用递增 request ID、revision、稳定标量主键或显式取消标记；不要把对象放入 Vue 深层 `ref` 后再用 `===` / `!==` 与原始对象比较，响应式代理会改变对象身份。
- 修改异步弹窗、IPC 启动链路或后台任务状态时，补充回归测试覆盖界面及时关闭、后台 Promise 未完成时主界面不锁定、失败/超时释放和过期结果不回写。

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
npm run cargo -- fmt --check
npm run cargo -- clippy --locked --all-targets --all-features -- -D warnings
npm run cargo -- test --locked
```

按改动范围选择验证：文档改动只执行差异和相关文档检查；前端改动执行相关前端检查；构建脚本改动执行 `npm run doctor:platform` 和相关脚本测试；Rust 迭代先指定相关测试。任务收尾按需运行一次完整 doctor，不重复运行输入未变化、已经通过的全量构建。同一工作区的重型 Rust 检查串行执行。

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
4. `src-tauri/src/desktop.rs`：Tauri 应用编排、插件和 command 注册。
5. `src-tauri/src/commands/`：按应用、CLI 和中转站职责拆分的 command 实现。
6. `src-tauri/src/contracts.rs`：Rust 派生的前端操作能力和 IPC 返回 View。
7. `src-tauri/src/models/`：后端持久化模型和业务数据结构。
8. `src-tauri/src/adapters/`：协议探测、分发及 NewAPI、Sub2API、通用 API 实现。
9. `src-tauri/src/services/`：调度、通知、测活和跨模块业务。
10. `src-tauri/src/network/`：业务请求、Webhook、updater 和 CLI 共用的代理语义。

保持改动边界清晰。UI、前端状态、后端 command、存储模型和协议接口尽量分开修改；涉及持久化数据结构时，需要同步前后端类型和本地配置迁移逻辑。账号管理、签到、密钥管理、邀请等操作能力由 Rust 计算，经 `contracts.rs` 返回；前端 `provider-actions.ts` 只读取结果，不复制业务判断。

### 新增 Agent CLI

Agent CLI 使用内置静态注册表和能力 Adapter，不依赖插件系统或代码生成。新增 Agent 时：

1. 在 `src-tauri/src/agent_cli_catalog.rs` 增加一条身份、序列化 key 和模块名声明。
2. 新增 `src-tauri/src/services/agent_cli/<agent>/`，在 `mod.rs` 返回定义，并按实际能力提供 `launch.rs`、`liveness.rs`、`sessions.rs`、`config.rs`。不支持的能力保持为 `None`，不要添加空壳实现。
3. 在 `src/agent-cli/visuals.ts` 增加原生图标和卡片轨道颜色；名称、可用状态和能力仍以 Rust 探测结果为准。
4. 不修改 `temporary_cli.rs`、`liveness.rs`、`cli_sessions/mod.rs`、`cli_runtime/config.rs` 等通用编排，也不在前端增加具体 Agent 分支。需要修改这些文件通常意味着 Adapter 契约仍不完整。

`BALANCEHUB_<AGENT>_CLI_PATH` 会从 catalog key 自动生成；Agent 定义中的 `additional_env_keys` 只登记 CLI 自身或历史兼容变量。设置中的 CLI 路径、站点 Agent Base URL、会话来源和能力列表都使用动态结构，增加内置 Agent 不需要新增一套持久化字段或前端布尔规则。

完成后至少运行 `npm run build`、`npm test`、`npm run doctor:platform`、Rust fmt / clippy / test 和 `git diff --check`。注册完整性、通用编排分支和 Agent 身份硬编码都有回归测试，不要通过忽略告警绕过。

### 新增中转站协议

中转站协议使用编译期 catalog、能力契约和描述注册表，不使用代码生成。Rust 负责协议身份、认证 Schema、能力和操作语义，前端只接收 IPC 描述并渲染：

1. 在 `src-tauri/src/provider_protocol_catalog.rs` 登记 Rust 枚举名、序列化 key 和描述模块名；`ProviderProtocol` 与运行时注册表会同时生成，不能只改其中一处。
2. 在 `src-tauri/src/adapters/<protocol>/` 实现真实站点请求，并在该模块的 `protocol.rs` 实现 `adapters/protocol/contracts.rs` 中确实支持的能力。未支持的能力保持未注册，不添加返回固定错误的空壳 Adapter。
3. 新增 `src-tauri/src/adapters/protocol/registry/<protocol>.rs`，声明认证方式、字段 Schema、探测角色、操作说明、凭据助手策略和能力对象。`adapters/protocol.rs` 只负责运行时分发，不放协议字段或站点分支。
4. 协议探测复用注册表中的 `ConnectionCapability::probe_site` 和 `ProtocolDetectionRole`。通用 fallback 只能在对应认证边界内启用，不能因为请求失败就把已知账号协议降级成通用 API。
5. 同步 `src/stores/provider-types.ts` 的 IPC 字面量类型；只有图标或卡片视觉确实不同才增加前端映射，不在 Vue、Pinia 或 composable 中复制认证和能力判断。
6. 如果新增了持久化字段或改变数据结构，必须同步 Rust 模型、默认值、存储迁移和前端接收类型。单纯新增协议枚举不应伪造旧配置兼容分支。

注册表测试会检查协议身份唯一性、默认认证 Schema、必填字段、能力与操作说明、探测边界和 IPC 序列化。新增协议后应先补齐这些契约，再编写具体 UI。

### 新增终端

终端身份、平台探测和启动策略彼此分离：

1. 在 `src-tauri/src/terminal_catalog.rs` 登记枚举名、序列化 key 和 fallback 名称；Rust 模型的 `TemporaryCliTerminalKind` 由 catalog 生成。
2. 在 `src-tauri/src/services/temporary_cli/terminal/<platform>.rs` 或对应平台子目录实现探测与启动，并加入该平台 `TerminalDefinition` 注册表。某个平台不支持时不要注册，也不要提供假成功实现。
3. 精确激活窗口是可选能力，只有终端能返回稳定 locator 时才登记 activator；否则保持普通未跟踪启动。
4. 同步 `src/stores/provider-types.ts` 的终端字面量类型；需要原生图标时更新 `TerminalBrandIcon.vue`，没有图标时沿用统一终端 fallback，不复制探测名称。
5. Unix、PowerShell、cmd 脚本继续复用 `temporary_cli/shell_runtime/`，内部变量遵守 `bh_`、`BH_` 和 `BALANCEHUB_` 命名规则。不要在单个终端策略中重新实现一套环境变量或代理拼接。

平台注册表完整性已有单元测试；脚本或启动参数变化后必须额外运行 `npm run doctor:platform`。

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
