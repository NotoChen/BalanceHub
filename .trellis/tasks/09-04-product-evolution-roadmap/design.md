# 技术设计

## Architecture

六项能力以现有 Provider、Agent、Terminal、网络和存储边界为基础，新增领域服务负责投影或编排，不复制协议实现。

```text
Protocol adapters + Provider state + Agent/Terminal registries
                        |
                        +-> Availability facts/projection -> Decision workspace
                        +-> Runtime evidence/reducer       -> Agent runtime projection
                        +-> Session adapters/index         -> Session workspace
                        +-> Read-only probes               -> Diagnostics center
                        +-> Provider save planner          -> Onboarding wizard
                        +-> Typed observations             -> Automation evaluator
```

### Shared source-of-truth boundaries

- Provider 类型、认证和动作能力：Rust 协议目录与 `ProviderService`。
- Agent 能力、会话和配置：动态 Agent registry 与 adapter。
- Terminal 能力：Terminal registry 和平台模块。
- 网络语义：`src-tauri/src/network/`。
- 持久化：`ProviderService` 事务与 schema migration。
- UI 只消费结构化 IPC，不重新判断协议、重复账号、Key 可用性或 Agent 能力。

## Foundation Changes

共享基础只在出现首个消费者时按需完成，避免先做无边界重构：

1. 将后台任务字符串 kind/status 收敛为 Rust 类型化契约；任务有稳定 ID、单一终态、有界历史和明确取消语义。
2. 将 `contracts.rs` 和 `provider-types.ts` 按 feature 拆分，但保持公开 IPC 行为不变。
3. 在会话工作台前，将全局搜索 generation 改为请求作用域取消，并将全局索引锁改为按数据库路径隔离。
4. 在自动化规则前，将 scheduler 拆成 runner 与独立 job；规则引擎只消费类型化 observation。
5. 每次拆分都先补足行为测试，再移动职责，不把结构迁移和产品行为混成不可审查的大提交。

## Availability Contract

新增只读 `AvailabilityService`。输入为带 revision 的 Provider 快照、Agent registry/config snapshot 和已持久化观察；服务不得调用网络、协议 adapter、CLI 或存储写入。

核心结果：

- `AvailabilitySnapshot`: `generated_at_ms`、`source_revision`、模型目录、选择条件、分页候选。
- `AvailabilityCandidate`: Provider ID、Key local ID、脱敏标签、模型证据、额度证据、测活证据、Agent 绑定、动作能力、排序层级与原因。
- 每项证据携带来源、观测时间和 `fresh/refreshing/stale/unknown`；新鲜度不是可用性。
- 新增 `quota_synced_at_ms`、`models_synced_at_ms`、`models_api_key_local_id`、`api_keys_synced_at_ms`，并为新测活记录增加可选 `api_key_local_id`。
- 历史无归属记录保持 unknown，不回填为当前 Key。

排序从强到弱：精确 Key+模型实测成功、Key 明确模型能力、当前 Key 的 Provider 级模型观察、元数据可用但模型未知、明确禁用/过期/耗尽/缺失。未知不得落入不可用层级。

## Feature Boundaries

### Agent runtime and Hook management

先建立 `AgentEnvironmentDescriptor`、`AgentInstallation`、`AgentAssetRecord` 与 capability-driven adapter，只读盘点配置、Skills、Plugin/Extension、MCP、Hook、Status UI 及 installed/latest stable 版本。不同 Agent 的来源、scope、优先级、信任与启停语义由 adapter 返回，公共 UI 不假设统一开关。配置入口仅解析 Rust allow-list 中的 opaque file ID，提供路径、打开和有界只读预览。

现有临时 CLI 状态文件不被 Hook 替换，而是作为统一运行时的强证据来源。`cli_runtime` 按职责拆为 launch instance repository、Hook event repository、runtime reducer、runtime projection 和 session enrichment；统一投影稳定后删除 `TemporaryCliInstance` 作为公开 IPC/UI 真源及其长期 4 秒全量轮询。

```text
BalanceHub launch registration/status file ----+
Official Agent Hook -> durable event spool -----+-> Runtime reducer
Session adapter -> title/model enrichment ------+-> AgentRuntimeSession
Optional exact process/terminal evidence -------+
```

- `AgentRuntimeSession` 分离 `runtime_id`、可选 Agent `session_id`、`origin`、`runtime_scope`、运行活动状态、进程证据、终端证据、Provider 来源和可执行动作。
- BalanceHub 启动脚本注入 `BALANCEHUB_CLI_INSTANCE_ID`；Hook helper 只用该稳定 ID 合并两类证据，不进行 cwd/时间窗口猜测。
- 启动登记、精确 PID 消失和 status-file 退出码可形成强终态；Hook stop/after-agent 只表示本轮 idle；无精确 PID 的外部证据过期只能进入 unknown。
- Hook helper 采用有上限的 stdin、Agent-owned decoder、元数据 allow-list、单事件临时文件加原子 rename；不以必须在线的 localhost/Unix Socket 作为唯一传输。
- Hook 管理是 capability-driven 资源操作：`inspect -> plan -> user confirmation -> revision check -> apply -> reread verify`。安装、删除和启停复用 ownership manifest、revision、结构化配置编辑与原子替换；`repair` 只生成修复 plan，不直接写入。
- Agent 管理界面按 Agent 与 runtime scope 展示受管 Hook 资源；手动添加、删除、启停和修复都先生成可审阅 plan，再由同一 apply 管线执行，应用启动本身不静默篡改 Hook 配置。
- 健康扫描只有读取和报告权限，不触发自愈；owned 资源缺失或 fingerprint 漂移时进入 `conflict`/`helper_missing` 等状态。只有用户主动发起修复且新 plan 仍无冲突时才允许 apply，不能安全修改时保持原状。
- 第一阶段只写 user scope 的 BalanceHub-owned Hook 资源，不修改 Agent trust store、系统/managed/project 配置、Shell profile 或其他资产；不申请管理员权限。只读盘点可以展示其他 scope，但能力返回 unsupported/read-only。
- 首个 Hook adapter 使用 Codex CLI，验证官方 Hook、trust 状态、App 离线 spool 和非接管边界；其余 Agent 在同一合同上逐个接入。PID/TTY 补证只使用无需新增权限的只读能力，取不到就保持字段缺失。
- 版本比较按安装来源和 channel 缓存，默认只比较 latest stable；使用统一网络代理/证书语义，失败保留上次成功事实并显示检查时间，不自动更新。
- 健康结果分别表达 installed、enabled、trusted、helper、spool、last event 和 conflict；文件存在不等于健康，真实新会话事件才完成验证。
- App 启动、恢复前台和后台增量消费使用批次与幂等 event ID；前端改为事件推送并在窗口恢复时做快照校准。

### Session workbench

- 独立 feature/composable/view，复用现有会话 summary、detail、index 和 resume command。
- 查询按调用方 request ID/Agent/工作目录隔离；不同 Agent 数据库锁互不阻塞。
- 搜索返回会话列表，详情按需加载消息；工具数据不进入全文索引。
- 运行实例列表只消费统一 `AgentRuntimeSession`；Provider 卡片仅统计明确带有 `provider_ref` 的 BalanceHub 启动会话。

### Diagnostics

- Rust runner 编排独立 probe，支持并发上限、单项超时、整体取消和增量事件。
- 检查输出包含 `check_id/category/status/summary/evidence/duration/remediation/platform`。
- proxy、Agent、Terminal、配置、协议和 updater probe 必须只读；updater 检查不得改 pending update 状态。
- 脱敏在 Rust IPC 前统一完成，导出内容与预览完全一致。

### Provider onboarding

- 新增只读 `plan_provider_save`，在最终确认前返回规范化地址、协议、认证要求、重复结果、可选动作与预期写入。
- 草稿由前端短期维护；最终写入仍走 Provider 事务。
- 检查步骤复用诊断结果词汇；可能刷新凭据或创建远程 Key 的动作独立标识和确认。
- 新流程替换原有新增路径，编辑现有 Provider 保持原职责。

### Automation

- 用户规则持久化于 schema 迁移后的顶层数据；运行期冷却/边沿/去重状态放入有界 cache repository。
- evaluator 消费 Rust 类型化 facts/observations，不解析中文文案、toast 或 UI 状态。
- 仅在 false -> true 边沿触发，冷却与 re-arm 可确定复现；未知/过期事实不触发。
- 首版 action 只有通知，调度失败不得阻塞刷新、签到或测活 job。

## Performance And Concurrency

- 可用性查询在读锁内筛选并只克隆结果字段，不生成模型 x Key 笛卡尔全量镜像，不新增未经证明必要的数据库。
- 支持 200 Provider、每个 2000 模型、每个 100 Key 的分页夹具；输入筛选需防抖并支持取消/过期拒写。
- 后台任务和规则历史有数量与驻留上限；同类周期任务单实例运行。
- 所有前端异步状态使用 request ID、revision 或稳定主键，不依赖 Vue 对象身份。

## Compatibility And Migration

- 持久化变更必须同步 Rust 模型、TypeScript 接收结构、默认值、导入导出和逐版本迁移。
- 自动化规则预期需要 schema `11 -> 12`；实施前以当时仓库 schema 为准重新确认，不能在规划阶段提前修改。
- 平台能力返回 supported/skipped 及原因；不得伪装三端一致。
- 回滚以子任务为单位；纯投影和 UI 可直接回退，持久化任务需保留向后恢复策略并验证高版本拒绝。

## Navigation Decision

现有 `AppTopbar.vue` 已承载搜索、添加、刷新、签到、任务、临时 CLI、公告、更新、GitHub 和设置，不适合继续追加无关联全局图标；决策中心也不是适合大型弹窗的短事务。

已确认采用“搜索区上下文入口 -> 独立主体工作区 -> 返回并恢复原面板上下文”：面板模式保留中转站搜索，并在搜索组合控件尾部提供单一“模型决策”入口；进入后同一位置变为“返回 + 动态模型搜索”，主体由 Provider 卡片替换为候选比较。面板 query、滚动位置和焦点来源必须恢复，决策 query 与面板 query 独立。

这不是永久并列 Tab。只有未来至少三个独立工作区成为高频长期任务时，才另行设计统一 workspace switcher。正式实现不能把按钮直接嵌入现有 `<label>`，需重构为语义正确的 search wrapper；也不能使用简单 DOM `hidden` 作为状态模型。

## Validation Matrix

- 通用：`npm run build`、`npm test`、`cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`npm run doctor:platform`。
- 异步：取消、超时、窗口关闭、旧 request ID、单一终态和 busy 状态释放。
- 性能：大规模 Provider/模型/Key fixture、分页、内存复制边界、索引并发。
- 隐私：IPC、任务事件、诊断预览/导出均断言无明文秘密。
- 平台：macOS、Linux、Windows 分别验证 Agent/Terminal/网络/证书/进程清理和降级原因。
