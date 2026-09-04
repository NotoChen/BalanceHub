# 执行计划

## Phase 1: Environment And Read-only Inventory

- [x] 定义 native environment、Agent installation、asset/config capability 和 stable opaque ID。
- [x] 实现四个 Agent 的配置、Skills、Plugin/Extension、MCP、Hook、Status UI 只读 adapter 与 fixture。
- [x] 实现受控配置定位/打开/有界预览、scope/effective-state 展示和越界保护。
- [x] 实现 installed/latest stable 比较、来源/channel、缓存、合并请求、超时和失败保留语义。
- [ ] 发现同一 Agent 的全部安装实例，并按 Agent 原生优先级解析 declared/effective、trust、shadow/conflict；当前盘点每个 Agent 只调用一次 `find`，文件存在时仍保守返回 unknown/无 trust。

## Phase 2: Runtime Contract

- [x] 定义 `AgentRuntimeSession`、evidence、actions、runtime scope 和状态转换 fixture。
- [x] 将现有 launcher repository 适配为证据源，保持启动、失败、退出码和终端激活行为。
- [x] 在 Unix/Windows 启动环境注入 `BALANCEHUB_CLI_INSTANCE_ID`，补脚本快照及 `doctor:platform` 覆盖。
- [x] 将现有 session adapter 接入 runtime enrichment producer，按受影响的 Agent、工作目录和 session ID 派发富化；producer 已具备全局 2/单 Agent 1、优先级与公平调度、generation 协作取消、3 秒外层超时、Pending cursor、退避、commit 重试、registry/cursor 上限和单调 snapshot 发布，富化证据不决定进程存活。
- [x] Codex 使用 exact state row；Grok 使用 percent-encoded/canonical cwd 与 exact session ID，只在 workspace 根层有界匹配 `.cwd`，并拒绝 traversal、symlink 和越界候选。
- [ ] Claude/Gemini 长会话仍需实现真正的增量 suffix parser：当前超过 2 MiB 返回的 cursor 虽由 producer 保存，但 adapter 未消费 `previous`，会持续 Pending，尚不满足长会话近似 O(append suffix) 的验收标准。
- [ ] 增加可注入 panic adapter 的 hot-path 测试，以及 producer coalescing、真实 generation stale-commit、commit failure、restart recovery 的端到端测试；当前已有 timeout/cancel、调度公平性、单 Agent 串行约束、cursor 有界和 service revision/recovery projection 单测。

## Phase 3: Hook Bridge

- [x] 实现版本化 normalized event、Agent decoder、输入/隐私上限和 fail-open helper。
- [x] 实现原子单事件 spool、小批次消费、幂等、损坏隔离和容量/保留上限。
- [x] 实现 reducer 的重复、乱序、unknown、ended/resume 和跨 runtime scope 测试。

## Phase 4: Managed Hook Pilot

- [x] 以 Codex CLI 作为试点并固定官方版本 fixture，同时验证 Hook trust、App 离线和权限不接管边界。
- [x] 实现 `inspect/plan/install/remove/enable/disable/health/repair/verify` 公共合同与首个 adapter；`repair` 仅生成 plan。
- [x] 覆盖 revision 竞争、fingerprint 漂移、权限不足、受管/只读配置、用户 Hook 共存、取消、手动修复和无整文件回滚，并断言失败路径字节级不变。
- [ ] App 运行与完全退出两种场景都用真实新会话事件完成验证（代码与 fixture 已覆盖，待用户实物验收）。
- [ ] 收紧配置文件与 ownership manifest 的提交边界；当前两次原子文件操作之间若 manifest 写入意外失败，仍可能留下可检测但需手动处理的半安装/conflict 状态。

## Phase 5: Remaining Native Agents

- [ ] 逐个增加 Agent capability、配置 adapter、decoder 和 fixture；公共 reducer/UI 不添加 Agent switch 分支。
- [ ] macOS、Linux、原生 Windows 分别验证安装、启停、事件、离线 spool、冲突、卸载和 fail-open。
- [ ] 不支持的 Agent/平台组合返回结构化原因，不创建半安装状态。
- [ ] 固定 Grok Build 版本化事件设计：处理阻塞 `Stop` 延续导致的误空闲，并在有官方依据前不要把 `enabled: bool` 声称为原生 TUI 的有效启停事实。

## Phase 6: Unified UI Cutover

- [x] Agent 管理页提供按 Agent/runtime scope 的健康事实、手动操作和 plan/diff 确认。
- [x] 顶栏、Provider 卡片和会话工作台切换到统一 runtime projection。
- [x] 改为事件推送加窗口恢复快照，保留短时启动确认，删除长期四秒轮询。
- [x] 删除 `TemporaryCliInstance` 作为长期公共运行时真源的 IPC/UI 列表与旧入口；保留启动确认所需的单实例短时查询。
- [x] 将 Agent 环境首页从卡片钻取改为动态列表控制台，首层展示安装、版本、Hook 状态和直接操作。
- [x] 将 Hook inspect/plan/apply 编排提升到页面级，使用单一共享 plan 弹窗和按 target 隔离的 busy/error 状态；删除旧的逐详情交互入口。
- [x] 在 Rust inspection 合同中提供操作能力与禁用原因，并补充前后端契约、异步释放、动态列表和响应式回归测试。
- [x] 完成列表控制台复审：状态按 `(Agent, runtime scope)` 隔离，跨行异步互不失效，详情保留完整只读 Hook 诊断，apply 复核计划目标与变更内容。

> 当前 UI 已切换为统一 projection，session adapter 富化链路已接通；Claude/Gemini 大文件增量读取、完整 producer 集成测试、真实 Agent 新会话和跨平台实物验收仍待完成。

## Validation

```bash
npm run build
npm test
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings
cd src-tauri && cargo test
npm run doctor:platform
git diff --check
```

额外执行 Hook fixture、真实事件、App 离线、并发会话、乱序/重复事件、spool 容量、配置冲突、卸载及凭据泄漏检查；三端平台路径由 CI 验证，当前开发机完成 macOS 实物验收。

## Rollback Gates

- 环境/只读盘点、合同/reducer、launcher adaptation、helper/spool、单 Agent 试点、其他 Agent、UI 切换分别提交和验收。
- 任何 revision/fingerprint 冲突立即停止 apply，不能用整文件快照覆盖当前配置。
- UI 切换前不得删除 launcher repository；旧公共合同删除后不得保留第二套长期运行列表。
- 新的官方 Hook schema 差异或无法解释的状态转换返回规划/研究，不由实现 Agent 猜测。
