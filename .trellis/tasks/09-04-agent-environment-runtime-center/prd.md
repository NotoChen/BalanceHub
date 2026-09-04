# Agent 环境与运行时中心

## Goal

建立统一的 Agent 环境与安装视图，把 BalanceHub 启动的临时 CLI 和外部终端启动的 Agent 会话归并为一份可信运行时投影，并提供可见、可撤销、不接管用户配置的 Hook 管理闭环。

## Requirements

### R0. Agent 环境只读盘点

- 以原生运行环境和 Agent 安装实例为根，统一展示配置文件、Skills、Plugin/Extension、MCP、Hook 和 Status UI；每项携带来源、scope、路径、声明/有效状态、信任、覆盖关系和诊断。
- 展示 installed/latest stable 版本、安装来源、channel、版本来源和检查时间；失败返回 unknown 或上次成功事实，首版不执行 Agent 更新。
- 配置文件通过 Rust allow-list 的 opaque ID 定位，提供打开文件、打开目录、复制路径和有界只读预览；不接受前端任意路径，不执行第三方 Hook/Statusline 来预览。
- 第一阶段不安装、删除、同步、迁移或通用启停 Skills、Plugin/Extension、MCP、Status UI；只有 BalanceHub-owned Hook 进入后续受管写流程。

### R1. 统一运行时

- Rust 定义唯一 `AgentRuntimeSession` 合同，合并 BalanceHub launch status、官方 Hook 事件、精确进程/终端证据及现有 Agent 会话解析器富化结果。
- 保留现有 launcher repository 中 Provider/账号/Key 本地来源、工作目录、终端 locator、PID、启动失败和退出码等强证据；撤销“临时 CLI 实例”作为独立产品域和永久 UI 真源。
- BalanceHub 启动会话注入 `BALANCEHUB_CLI_INSTANCE_ID`，Hook 事件只通过该稳定标识合并，不按目录、时间窗口、Agent 名或当前默认配置猜测。
- 外部会话缺少 PID、终端、Provider 或结束证据时保持字段缺失或进入 `unknown`，不得伪造终态或可执行动作。
- 顶栏、Provider 卡片和 Agent 会话工作台最终只消费统一投影；迁移完成后删除旧公开 IPC、前端状态和长期四秒轮询，保留命令作用域的短时启动确认。

### R2. Hook 事件桥接

- 只接入各 Agent 官方 Hook，Agent adapter 负责配置位置、schema、信任/启停语义和 payload decoder；公共层负责事件合同、spool、ownership、revision 和 reducer。
- Hook helper 必须 fail open，在严格时间预算内只保存 allow-list 运行元数据，不保存提示词、回复正文、工具输入输出、完整环境变量或凭据。
- 使用有界、原子、单事件文件 spool 支持 App 未运行期间的事件；损坏、重复、乱序、未知 schema 和容量耗尽不能阻塞 Agent 或后续事件消费。
- 第一阶段覆盖 macOS、Linux 和原生 Windows；WSL 的 distro、Linux home、guest PID 和路径映射作为后续独立任务。

### R3. Hook 管理

- 按 Agent 和运行环境提供 `inspect`、`plan`、`install`、`remove`、`enable`、`disable`、`health`、`repair`、`verify`。
- 用户可以手动添加、删除、启用、禁用和修复 BalanceHub 注入项；所有写操作先展示结构化变更计划，并在应用前复核磁盘 revision。
- 只修改能通过 structural identity 与 fingerprint 证明由 BalanceHub 拥有的节点或独立资源；不保存或恢复整份历史配置，不覆盖用户 Hook。
- BalanceHub 不接管配置、不争抢权限：健康扫描只读，检测到缺失、漂移、权限不足或冲突时只报告并提供手动修复；无法安全执行时保持磁盘现状。
- 启动扫描、文件监听、`inspect`、`health` 和 `verify` 严格只读；`repair` 只生成修复 plan，必须再次由用户确认才能 apply。
- 首轮写入只允许 user scope 的 BalanceHub-owned Hook 资源；不修改 Agent trust store、权限策略、system/managed/project 配置、Shell profile 或其他 Hook/MCP/Statusline/插件设置，也不请求管理员权限。
- 安装或修复完成只进入 `installed_unverified`；只有后续真实 Agent Hook 事件才能进入 `healthy`。
- 健康结果分别表达 installed、enabled、trusted、helper、spool、last event、revision/conflict，不折叠为一个容易误导的布尔值。

### R4. 动态扩展

- 公共 reducer、IPC 和 UI 不写死 Claude Code、Codex CLI、Gemini CLI、Grok Build 四分支。
- 新增 Agent 只注册 capability、Hook 配置 adapter、事件 decoder 与 fixture；不修改公共状态机。
- 首轮只建设运行时与 Hook 受管资源基础；完整 Skill、Plugin、MCP、Statusline 管理另行规划并复用该所有权模型。

### R5. Agent 列表式控制台

- Agent 设置首页必须以动态列表集中展示全部已注册 Agent 的安装、版本和 Hook 状态；不得要求用户逐个进入详情才能判断或操作 Hook。
- 未安装 Hook 时直接提供“安装 Hook”；已安装时直接提供启停开关，并提供单 Agent 健康检查、验证、修复和删除入口。所有写操作仍先生成共享变更计划并由用户确认，不做乐观切换。
- Rust 返回每项 Hook 操作的可用性和禁用原因，前端只渲染能力，不根据 Agent 名称、状态字符串或 CLI 是否存在重复推断权限。
- Agent 详情只承载安装证据、完整诊断、资产和配置等低频信息；列表中的开关与操作不能通过整行点击误触发详情。
- 列表按稳定 Hook target 去重，并为未来同一 Agent 多安装实例和其他 runtime scope 留出分组空间；公共列表、行组件和 Hook 工作流不写死当前四个 Agent。

## Acceptance Criteria

- [ ] 无 Hook 时，BalanceHub 启动的 CLI 仍能正确展示启动、运行、退出码和已有终端激活能力。
- [ ] 当前 Agent 安装与配置、Skills、Plugin/Extension、MCP、Hook、Status UI 可按环境只读盘点，并准确表达 scope、有效状态、冲突和不支持能力。
- [ ] 配置文件可通过受控入口定位、打开、复制路径和有界预览；前端任意路径、越界 symlink 和超大文件不能被读取。
- [ ] 每个安装实例可比较 installed/latest stable 版本；网络失败不清空上次成功事实、不触发前台重试风暴，也不自动升级。
- [ ] Hook 启用后，同一 BalanceHub 会话只显示一次；外部会话可被发现，未知字段不会被虚构。
- [ ] App 完全退出时产生的事件可在重启后有界消费；helper、spool 或配置异常不阻止 Agent CLI 启动和退出。
- [ ] 同目录并发会话、不同 Agent 相同 session ID、重复/乱序事件均不会串联或重复展示。
- [ ] 用户能按 Agent 手动安装、删除、启停、检查、修复和验证 Hook；每次写入前都能看到变更计划。
- [ ] Hook 被其他工具修改、删除或因权限无法写入时，BalanceHub 不自动恢复、不覆盖、不争抢；无法安全修复时保持原状并解释原因。
- [ ] `inspect/health/verify/repair plan`、取消、冲突、unsupported、只读文件、受管配置和权限不足后，原配置保持字节级不变；成功写入后未知字段、非 owned 节点和文件权限保持不变。
- [ ] 安装后在真实新会话事件到达前显示“等待验证”，不能仅凭文件存在显示健康。
- [ ] IPC、日志和 spool 不包含提示词、回复、工具数据、API Key、Token、Cookie、密码或完整环境变量。
- [ ] macOS、Linux、原生 Windows 分别覆盖安装、事件、App 离线、冲突、卸载和 fail-open 测试。
- [ ] 统一 UI 切换后不存在第二套临时 CLI 公共合同、永久列表或长期轮询。
- [ ] 打开 Agent 设置即可同时看到全部 Agent 的安装、版本与 Hook 状态，并能直接完成安装、启停、检查、验证、修复和删除；进入详情不是这些操作的前置条件。
- [ ] 操作 Agent A 时只禁用 Agent A；共享计划弹窗关闭不应用，确认后立即关闭并在成功、失败或超时后释放忙碌状态。

## Dependencies

- 本任务依赖现有 Agent registry、session adapters、terminal registry 与 launcher status repository，但不依赖可用性决策中心。
- Agent 会话工作台必须在本任务统一 runtime contract 稳定后再接入运行实例列表。
- 单 Agent 试点验证 ownership 与事件合同后，才能批量接入其余当前 Agent。

## Out of Scope

- 通用进程扫描、按 cwd/时间窗口猜测会话归属，或承诺外部会话都能精确定位终端窗口。
- 对外部会话执行 kill、输入注入、审批接管或自动恢复。
- WSL Hook 安装、发行版发现和 guest/native 进程映射。
- 完整 Skill、Plugin、MCP、Statusline 管理。
- Agent CLI 自动更新、安装源迁移或包管理器执行。
- 自动修复或后台静默写回 Agent 配置。

## Rollback Boundary

- Runtime contract/reducer 在 UI 切换前可独立回退，不影响现有临时 CLI 启动。
- Hook 卸载只删除 fingerprint 仍匹配的 BalanceHub-owned 节点和 helper；存在漂移时停止，不恢复整份旧配置。
- UI 切换独立提交；切换完成前保留 launcher repository，切换后删除旧公开合同与长期轮询，不保留永久双真源。
