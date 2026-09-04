# Research: 产品演进五项能力的架构边界与风险审计

- Query: 在现有 BalanceHub 架构上实现可用性决策中心、Agent 会话工作台、自动化规则中心、一键诊断中心和中转站接入向导时，哪些现有抽象应复用，哪些核心文件必须先拆分，跨层、性能、存储、隐私和三端兼容风险如何控制？
- Scope: internal
- Date: 2026-09-04

## Findings

### 1. 总体结论

五项能力都可以建立在当前代码上，但不应直接继续扩充现有入口、大型 service 或前端总控 composable。当前项目已经有 Provider 协议目录、Agent CLI 注册表、Terminal 目录、Provider 事务边界、代理解析、受控进程执行和会话适配器，这些应继续作为唯一真源；新功能应当是对这些能力的编排和只读投影，而不是另建一套识别、调度或权限判断。

实施顺序不能按五个产品页面并行铺开，也不应先做一轮没有直接消费者的全局重构。共享边界在各自首个消费者开始前按需收敛：决策中心先处理事实归属和紧凑投影；会话工作台先处理作用域取消与按库锁；诊断中心接入前收敛类型化后台任务生命周期；自动化规则接入前拆分 scheduler；接入向导在诊断结果词汇稳定后实现。只有自动化规则对可用性事实投影构成硬依赖。

### 2. 应保留为唯一真源的现有抽象

- Provider 类型目录已经集中定义可见协议及其能力入口，应继续由它决定 NewAPI、Sub2API 和通用 API Key 的分发，不能在向导或前端新增协议枚举分支：`src-tauri/src/provider_protocol_catalog.rs:7-17`。
- 协议定义包含标识、显示名和能力描述，协议能力的判定应由 Rust 返回，前端只消费结果：`src-tauri/src/adapters/protocol/definition.rs:15-22`、`src-tauri/src/adapters/protocol/definition.rs:92-110`、`src-tauri/src/adapters/protocol/definition.rs:147-156`。
- Agent CLI 注册表已经承载 Agent 定义与能力，不应在会话工作台、决策中心或诊断页写死 Claude Code、Codex、Gemini CLI、Grok Build：`src-tauri/src/services/agent_cli.rs:21-59`、`src-tauri/src/services/agent_cli.rs:85-135`。
- Terminal 目录和平台注册模块已经提供终端发现边界，新功能只应请求“可用终端及能力”，不能在 Vue 或通用 service 中按操作系统重新枚举：`src-tauri/src/terminal_catalog.rs:7-25`、`src-tauri/src/services/temporary_cli/terminal/mod.rs`。
- Provider 请求上下文包含 revision/CAS 语义，异步探测、刷新和向导保存必须保留该并发保护：`src-tauri/src/services/provider_service.rs:25-83`。
- Provider 持久化已经有原子事务边界，所有最终写入仍应经过该边界，不允许向导或规则引擎直接修改共享状态：`src-tauri/src/services/provider_service/transaction.rs:101-160`。

### 3. 必须优先拆分的过重模块

以下文件已经同时承担多个职责，再加入新能力会放大维护和并发风险：

- `src-tauri/src/services/cli_sessions/index.rs` 约 1791 行，同时包含队列、取消、SQLite schema、搜索、容量治理、维护和任务事件。应拆为 `config.rs`、`schema.rs`、`repository.rs`、`search.rs`、`build_queue.rs`、`capacity.rs`、`events.rs`，保持对外行为不变。
- `src-tauri/src/services/scheduler.rs` 约 710 行，把 runner、刷新、签到、测活及事件发布放在一起。应拆成 runner 和独立 job，再让自动化规则只消费结构化结果，不进入 scheduler 主循环。
- `src/stores/provider-types.ts` 约 798 行，继续加入五项功能类型会形成前端镜像总仓。应按 feature 拆出类型，同时保持 Rust IPC 契约为事实来源。
- `src/composables/useBackgroundTaskCenter.ts` 约 445 行，既接收后端事件又从多个前端 ref 合成任务，不适合作为规则、诊断和索引任务继续扩张的载体。
- `src-tauri/src/contracts.rs` 约 451 行，应按 Provider、可用性、自动化、诊断、向导领域拆分，避免形成新的总合同文件。

拆分阶段只移动职责并补回归测试，不混入产品行为变化。这样后续每个功能的 diff 和回归范围才可审计。

### 4. 可用性决策中心

现有数据能支撑第一版，但数据粒度并不一致：

- Provider 级可用模型来自 `ProviderCapabilities.available_models`：`src-tauri/src/models/state.rs:158-180`。
- 自动化状态目前只有通用的 `last_synced_at`，没有模型数据自己的刷新时间：`src-tauri/src/models/state.rs:185-196`。
- 测活记录包含 Agent、模型和 Base URL，但没有 API Key 本地 ID，不能据此宣称任意 Key/模型组合已验证：`src-tauri/src/models/liveness.rs:7-34`。
- Key 维度的额度和模型限制已有结构：`src-tauri/src/models/provider_results.rs:70-100`。
- CLI 当前绑定可从配置快照取得：`src-tauri/src/models/provider_results.rs:659-668`。

第一阶段应实现只读 Rust 投影服务，返回“事实 + 来源 + 新鲜度 + 置信度”，并支持查询和分页。不要在前端把 Provider、Key、模型、测活和 CLI 快照临时 join 成决策；也不要先落一份重复数据库。对没有 Key 级实测证据的组合，应明确标记为“协议/模型声明可用”而不是“已验证可用”。

性能上必须在读锁内完成筛选和投影，只克隆结果所需字段。`AppData` 同时持有全部 Provider、设置、工作空间和偏好：`src-tauri/src/models.rs:43-59`；Provider 快照当前会克隆完整 `AppData`，代码也注明模型和历史数据可能较大：`src-tauri/src/services/provider_service/transaction.rs:31-49`。上限允许 200 个 Provider、每个 2000 个模型、每个 100 个 Key：`src-tauri/src/limits.rs:15-19`。因此不能在每次输入或排序时复制全量模型及敏感凭据到前端。

建议边界：

- Rust：`services/availability/` 负责投影、排序、来源解释和分页；`commands/availability.rs` 只做参数校验与 IPC。
- Vue：`features/availability/` 负责过滤状态、分页和操作反馈；直接动作仍调用已有刷新、测活、CLI 切换和临时 CLI command。
- 返回结构不含 API Key、Token、Cookie、密码；只返回本地稳定 ID、脱敏显示值和必要统计。

### 5. Agent 会话工作台

已有共享会话适配器和会话元数据合同可以复用：`src-tauri/src/services/agent_cli/contracts.rs:188-265`、`src-tauri/src/services/agent_cli/contracts.rs:274-333`。搜索结果本来就以会话元数据为单位，而不是逐条消息结果：`src-tauri/src/models/cli_sessions.rs:5-24`、`src-tauri/src/models/cli_sessions.rs:56-77`，这与“搜索后仍展示独立 Agent 会话列表”的产品要求一致。

上线工作台前需要解决两个并发瓶颈：

- 搜索取消目前使用进程级 generation，不同 Agent、不同窗口或不同搜索域会互相取消：`src-tauri/src/commands/cli.rs:44-99`。应改为调用方生成的 request ID 或以窗口/Agent/工作目录为键的取消域。
- 索引虽然按 Agent 使用不同数据库，但文件锁是进程全局的：`src-tauri/src/services/cli_sessions/index.rs:127-137`、`src-tauri/src/services/cli_sessions/index.rs:1494`。应改为按数据库路径维护锁注册表，使不同 Agent 的构建与读取不互相阻塞。

摘要缓存已经有 Agent/工作目录键和容量约束，可继续复用：`src-tauri/src/services/cli_sessions/mod.rs:37-49`、`src-tauri/src/services/cli_sessions/mod.rs:167-261`。

工作台应有独立路由/视图和 composable，复用现有搜索、详情、索引和临时 CLI resume command。不要把当前临时 CLI 的工作空间选择 composable 扩展成永久工作台控制器。详情页按消息角色、文本正文和必要元数据渲染；工具调用和工具输出可以在详情中按需折叠，但不进入全文检索索引。

### 6. 自动化规则中心

当前 scheduler 一次执行完整 tick 后固定休眠 30 秒：`src-tauri/src/services/scheduler.rs:122-131`；刷新、签到和测活在同一个 tick 中顺序运行：`src-tauri/src/services/scheduler.rs:147-362`。单项内部虽已有并发上限，例如刷新为 6：`src-tauri/src/services/provider_service/refresh.rs:181-259`，签到和测活为 3：`src-tauri/src/services/scheduler.rs:71-74`、`src-tauri/src/services/scheduler.rs:280-285`、`src-tauri/src/services/scheduler.rs:539-552`，但慢任务仍会延迟后续 job。

在引入规则前，应将 scheduler 拆为：

- `runner.rs`：只管理生命周期、周期和取消。
- `refresh_job.rs`、`check_in_job.rs`、`liveness_job.rs`：每类任务独立调度、单实例运行、有限并发。
- `automation/evaluator.rs`：消费结构化 operation observation，判断边沿、阈值、冷却期和动作；不直接解析 UI 状态或错误文案。
- `automation/runtime_state.rs`：保存有界的最近触发状态，防止重启后重复触发。

持久化方面，只把用户编辑的规则定义新增为顶层 `AppData.automation_rules`。最近观察值、冷却时间和去重状态应放入有界 app-cache 文件或专用小型 runtime repository，不能塞进每个 Provider，也不能每次评估都重写 `data.json`。

当前 schema 为 11：`src-tauri/src/models.rs:43`；迁移按版本逐步执行并拒绝更高版本：`src-tauri/src/storage/migration.rs:21-75`、`src-tauri/src/storage/migration.rs:78-335`。新增持久规则时必须一次完成 `11 -> 12` 迁移、默认值、规范化和上限、导入导出、Rust/TypeScript 合同以及回归测试，不能只给结构加可选字段绕过迁移。

规则输入必须是 Rust 定义的类型化事件，例如 `RefreshCompleted`、`CheckInFailed`、`QuotaBelowThreshold`，不能解析 `runtime.error_message` 或中文提示。动作必须有去重键、冷却时间、最大并发和失败退避，并服从已有 Provider revision/CAS。

### 7. 一键诊断中心

诊断应被定义为只读、可取消、可导出的结构化探测，不是“自动修复”入口。现有能力可以复用：

- 受控子进程和进程树终止：`src-tauri/src/platform/process.rs:25-64`、`src-tauri/src/platform/process.rs:66-105`、`src-tauri/src/platform/process.rs:163-178`。
- Agent 发现、版本检测和超时：`src-tauri/src/services/agent_cli/discovery.rs:22`、`src-tauri/src/services/agent_cli/discovery.rs:184-202`、`src-tauri/src/services/agent_cli/discovery.rs:269-321`。
- Terminal 探测应继续经过 terminal registry。
- 代理解析应复用 `src-tauri/src/network/proxy.rs:90-108`、`src-tauri/src/network/proxy.rs:202-243`。
- HTTP 栈已经启用 native roots 与 SOCKS：`src-tauri/Cargo.toml:23-26`。

建议新增 `services/diagnostics/`，通用 runner 之下按 `macos.rs`、`linux.rs`、`windows.rs` 隔离平台探测，输出 `check_id`、severity、status、summary、evidence、remediation。所有命令都要有超时、取消和进程树清理；诊断结束、失败或取消都必须释放后台任务和前端忙碌状态。

诊断不得启动终端、切换默认 CLI 配置、保存 Provider、执行应用更新、触发签到，或用真实付费模型请求做测活。脱敏必须发生在 Rust IPC 之前，不能先把秘密返回 Vue 再隐藏。导出中禁止包含代理凭据、API Key、Token、Cookie、密码、完整环境变量和完整配置文件内容。

### 8. 中转站接入向导

协议探测已经并行探测注册协议：`src-tauri/src/adapters/detector.rs:15-43`，并对冲突、模糊和 fallback 有明确结果：`src-tauri/src/adapters/detector.rs:46-124`。重复检测及合并、独立新增、覆盖等冲突决策已经由 Rust 实现：`src-tauri/src/services/provider_service/persistence.rs:18-145`、`src-tauri/src/services/provider_service/persistence.rs:262-307`。

接入向导应当只是对“地址规范化 -> 协议探测 -> 能力/认证方式选择 -> 凭据补全 -> 连接测试 -> 重复冲突决策 -> 最终保存”的编排。草稿在前端短期保存，只有最后确认才调用既有事务边界写入 `AppData`。每一步由 Rust 返回带状态和可恢复建议的结构化结果，Vue 不复制协议探测、认证能力或重复判断。

现有凭据助手已经约 588 行且在前端管理状态机，不能继续把完整向导塞进同一 composable。应新增 `features/onboarding/` 和 `services/provider_onboarding/`，把步骤合同显式化，同时复用 detector、协议 capability、凭据补全、连接测试和保存冲突合同。

向导必须允许用户在探测模糊或失败时显式选择已注册协议，但不能把 AnyRouter 作为独立 UI 类型。通用 API Key 只暴露 API Key；NewAPI/Sub2API 默认账号密码，并按 Rust 返回能力展示 Cookie、访问令牌或 API Key。

### 9. 后台任务契约按消费者收敛

当前 Rust 任务事件以字符串 kind/status 表达：`src-tauri/src/app_events.rs:9-21`。前端同时维护自己的 union，并从多个无关 ref 合成任务：`src/composables/useBackgroundTaskCenter.ts:14-77`、`src/composables/useBackgroundTaskCenter.ts:97-247`。若直接加入索引、诊断、规则或向导任务，会产生更多前后端手工镜像和状态不一致。该契约不是只读决策查询的独立硬前置；当首个新增后台任务消费者进入实现时，必须先完成以下收敛，并供后续消费者复用。

应先引入 Rust 类型化的 `BackgroundTaskKind`、`BackgroundTaskStatus` 和统一 publisher，前端只映射显示文案/图标。全局任务中心只保留摘要和最近有界历史；刷新、签到等命令内的逐 Provider 详细进度继续使用各自的 command-scoped progress channel，避免把所有明细塞进全局事件流。

必须保证：

- 每个任务有稳定 task ID、开始时间、可选进度和可选 parent/action source。
- 一个任务只发布一个终态，完成/失败/取消后一定释放 active 状态。
- 历史数量和驻留时间有上限，已读完成项可以清除，不永久增长。
- 同类周期任务单实例运行；手动触发与定时触发要么复用进行中的任务，要么明确排队，不能重复执行。
- UI 卸载、窗口关闭或 request ID 过期后，旧结果不能覆盖新状态。

### 10. 建议模块形态

```text
src-tauri/src/
  commands/
    availability.rs
    sessions.rs
    automation.rs
    diagnostics.rs
    onboarding.rs
  contracts/
    provider.rs
    availability.rs
    automation.rs
    diagnostics.rs
    onboarding.rs
  models/
    availability.rs
    automation.rs
    diagnostics.rs
    onboarding.rs
  services/
    availability/
    automation/
      evaluator.rs
      runtime_state.rs
    background_tasks/
    diagnostics/
      macos.rs
      linux.rs
      windows.rs
    provider_onboarding/
    scheduler/
      runner.rs
      refresh_job.rs
      check_in_job.rs
      liveness_job.rs
    cli_sessions/index/
      config.rs
      schema.rs
      repository.rs
      search.rs
      build_queue.rs
      capacity.rs
      events.rs

src/
  features/
    availability/
    sessions/
    automation/
    diagnostics/
    onboarding/
```

目录名应按具体领域命名，不新增 `manager`、`common`、`helpers`、`utils` 之类的职责垃圾桶。Provider、Agent、Terminal 注册表保持唯一分发来源。

### 11. 明确禁止的实现模式

- 禁止在 Vue 中重新判断协议能力、Agent 能力、Terminal 可用性或 Provider 重复冲突。
- 禁止为决策中心建立一份未经必要性证明的 Provider/Key/模型全量镜像数据库。
- 禁止搜索输入每变化一次就克隆完整 `AppData` 或把敏感凭据发送给前端。
- 禁止用对象身份比较处理 Vue 异步结果防过期；使用 request ID、revision 或稳定标量主键。
- 禁止让不同 Agent/窗口共享一个全局搜索取消 generation。
- 禁止所有 Agent 索引共用一个全局文件锁。
- 禁止规则引擎解析中文错误文案、DOM 状态或前端 toast 来判断触发条件。
- 禁止规则评估过程中高频重写主配置文件或无界保存执行历史。
- 禁止诊断功能产生业务副作用，或将秘密先传给前端再做视觉脱敏。
- 禁止向导另写协议探测、认证能力或重复合并算法。
- 禁止启动终端、Agent CLI、诊断进程或网络任务时用未收口 Promise 锁死页面/弹窗。
- 禁止在通用代码写死四个现有 Agent；新增 Agent 必须只需注册定义、能力和适配器。

### 12. 分阶段迁移及依赖关系

1. Availability Center：先补齐必要的观测归属、时间和迁移，再实现只读 Rust 投影、查询/分页、事实来源与新鲜度；只拆本任务直接使用的 contracts/type。
2. Session Workbench：先实现作用域化取消和按数据库路径加锁，再接入独立页面/composable；它可在决策中心完成后与诊断并行。
3. Diagnostics：在新增诊断后台任务前建立类型化任务生命周期，再实现只读结构化 probe、三端模块和脱敏导出；复用 Agent/Terminal/网络能力。
4. Onboarding：诊断结果词汇稳定后，在既有 detector/credential/test/conflict/save 上编排步骤；save planner 可在本子任务内先行实现。
5. Automation Rules：在可用性事实投影稳定后，先无行为变化拆分 scheduler，再新增规则定义、实际 schema 迁移、runtime state、类型化 observation、边沿/冷却语义。
6. Integrated Regression：完成大规模数据、并发、迁移、隐私和 macOS/Linux/Windows CI 验证后，才把五项能力视为架构闭环。

每一阶段都应独立可回滚、可验收，不允许用一个长期分支同时改动五个页面与全部后端模块。

### 13. 验证矩阵

| 范围 | 必须验证 |
| --- | --- |
| 通用质量门 | `npm run build`、`npm test`、`cargo fmt --check`、`cargo clippy --all-targets --all-features -- -D warnings`、`cargo test`、`npm run doctor:platform` |
| macOS / Linux / Windows | Agent 与 Terminal 探测不打开窗口；不可用能力按注册表结果降级，不 panic、不写死平台路径 |
| 超时与取消 | 进程超时后杀死进程树；IPC、网络、索引、诊断失败/取消后释放 UI 和后台任务 active 状态 |
| 网络 | 直连、系统代理、显式 HTTP(S) 代理、SOCKS、no-proxy 与系统证书场景返回结构化结果，不生成第二套代理环境 |
| 并发一致性 | Provider revision 过期不得写回；前端旧 request ID 不得覆盖新请求；同类周期任务不重复运行 |
| 决策中心性能 | 200 Provider × 2000 模型 × 100 Key 上限夹具；结果分页；IPC 不含秘密；搜索和排序不复制完整 AppData |
| 会话并发 | 不同 Agent/窗口/工作目录的搜索互不取消；不同 Agent 数据库的重建不全局阻塞读取 |
| 自动化 | 规则数量、历史和 runtime state 有界；边沿与冷却正确；重启后不重复触发；失败动作不会阻塞 scheduler |
| 存储 | schema 11 到 12 迁移、缺省值、畸形数据、高版本拒绝、导入导出往返、原子写入 |
| 诊断隐私 | IPC 与导出断言不存在 API Key、Token、Cookie、密码、代理凭据、完整环境和配置内容 |
| 接入向导 | 探测成功、模糊、失败、用户覆盖协议、认证失败、连接失败、合并/独立新增/覆盖、取消均不产生半成品配置 |
| UI 回归 | 弹窗可及时关闭；后台 Promise 未完成不锁主界面；失败/超时后状态恢复；页面卸载后旧结果不回写 |

## Files Found

- `src-tauri/src/provider_protocol_catalog.rs`：Provider 协议目录入口。
- `src-tauri/src/adapters/protocol/definition.rs`：协议定义和能力合同。
- `src-tauri/src/adapters/detector.rs`：协议并行探测、模糊结果与 fallback。
- `src-tauri/src/services/provider_service.rs`：Provider 请求上下文与 revision/CAS。
- `src-tauri/src/services/provider_service/transaction.rs`：Provider 快照和原子事务边界。
- `src-tauri/src/services/provider_service/persistence.rs`：重复检测及合并、覆盖、独立新增。
- `src-tauri/src/services/provider_service/refresh.rs`：Provider 刷新并发控制。
- `src-tauri/src/services/agent_cli.rs`：Agent CLI 注册表和能力。
- `src-tauri/src/services/agent_cli/contracts.rs`：Agent 会话适配合同。
- `src-tauri/src/services/agent_cli/discovery.rs`：Agent 发现、版本探测和超时。
- `src-tauri/src/services/cli_sessions/mod.rs`：会话摘要缓存。
- `src-tauri/src/services/cli_sessions/index.rs`：会话 SQLite 索引、搜索、队列、容量和维护。
- `src-tauri/src/services/scheduler.rs`：周期刷新、签到和测活调度。
- `src-tauri/src/services/temporary_cli/terminal/mod.rs`：平台终端实现入口。
- `src-tauri/src/terminal_catalog.rs`：Terminal 目录。
- `src-tauri/src/platform/process.rs`：受控进程执行和进程树终止。
- `src-tauri/src/network/proxy.rs`：统一代理解析。
- `src-tauri/src/app_events.rs`：后台任务事件结构。
- `src-tauri/src/models.rs`：`AppData` 与 schema version。
- `src-tauri/src/models/state.rs`：Provider 能力和自动化同步状态。
- `src-tauri/src/models/liveness.rs`：测活记录粒度。
- `src-tauri/src/models/provider_results.rs`：Key 级额度/模型限制和 CLI 配置快照。
- `src-tauri/src/models/cli_sessions.rs`：会话搜索结果与详情合同。
- `src-tauri/src/storage/migration.rs`：配置 schema 迁移与高版本保护。
- `src-tauri/src/limits.rs`：Provider、模型和 API Key 容量限制。
- `src-tauri/src/contracts.rs`：当前集中式 IPC contracts。
- `src/stores/provider-types.ts`：当前集中式前端 Provider 类型。
- `src/composables/useBackgroundTaskCenter.ts`：前端后台任务汇总逻辑。
- `src-tauri/Cargo.toml`：HTTP 客户端 native roots 与 SOCKS feature。

## Related Specs

- `.trellis/workflow.md`：任务阶段、研究和实现流程。
- `.trellis/spec/frontend/index.md`：前端规范索引。
- `.trellis/spec/frontend/component-guidelines.md`：组件职责和拆分约束。
- `.trellis/spec/frontend/composable-guidelines.md`：composable 状态与异步边界。
- `.trellis/spec/guides/code-reuse-thinking-guide.md`：现有能力检索与复用要求。
- `.trellis/spec/guides/cross-layer-thinking-guide.md`：跨前后端事实来源和合同一致性要求。
- `.trellis/spec/guides/agent-cli-routing.md`：Agent CLI 注册、能力和扩展边界。
- `.trellis/tasks/09-04-product-evolution-roadmap/prd.md`：五项产品演进方向与研究目标。

## External References

本轮为仓库内部架构审计，没有引入外部框架或外部产品实现作为事实依据。三端兼容结论仅定义了必须验证的行为，尚未由本轮研究执行完整 macOS/Linux/Windows CI。

## Caveats / Not Found

- 本轮未执行产品代码修改、schema 迁移、性能基准、三端构建或 UI 验收；这里给出的是实施前架构约束和验证计划，不代表五项能力已经实现。
- `file:line` 位置基于 2026-09-04 当前工作树，后续拆分后会变化，应在实现任务中同步更新设计/决策记录。
- 可用性决策中心目前缺少模型专用刷新时间和 API Key 级测活证据；在补齐数据来源前只能保守表达能力，不能推导为实测结论。
- 自动化规则会触发 schema 变更和跨层合同扩展，是五项能力中存储与回归风险最高的一项，不应与基础拆分同时混做。
- 诊断中心三端探测的命令与路径还需逐平台实现和 CI 证实；不能把 macOS 本机结果外推为 Linux/Windows 已兼容。
