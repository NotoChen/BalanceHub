# Phase 1 前端审查遗留

## GUI PATH 深度扫描入口回归

### 现状

- 被替换的 `SettingsCliManager.vue` 在用户点击“重新扫描 Agent”时调用
  `useCliRuntimeStore().probeCliTools(true)`。
- 该结果通过 `captureCliEnvironmentSettings` 与
  `applyCliEnvironmentProbeResult` 合并到设置草稿；只有扫描期间未被用户修改的
  `agentCliPaths` 才会被采用，之后仍由既有设置保存流程持久化。
- 新的 Agent 环境页“刷新”调用 `get_agent_environment_inventory`。当前 Rust
  inventory 使用 `agent_cli::find(settings, kind, false)`，只做普通发现，不执行
  GUI PATH 场景需要的 login-shell 深度扫描，也不会采用发现路径。
- 删除旧组件后，`useSettingsController.probeCliTools()` 虽然仍存在，但已经没有 UI
  调用入口。因此 macOS GUI PATH、非标准 shell PATH 等场景可能从“可手动恢复”退化为
  “环境页持续显示不可用”。

### 后续实现合同

1. 不恢复 `SettingsCliManager.vue`，也不在环境中心复制一套 CLI 探测状态。
2. 环境页提供显式的“深度扫描并采用路径”动作；普通只读盘点和深度扫描保持不同语义。
3. Rust 返回结构化候选路径、版本、来源和诊断，不在扫描命令内直接修改设置。
4. 前端采用候选时继续使用既有设置快照比较，避免覆盖扫描期间的用户修改；持久化仍走
   当前统一设置保存流程。
5. 增加回归测试，覆盖普通扫描失败后深度扫描找到 CLI、并发修改路径不被覆盖、取消或
   扫描失败不修改设置，以及 macOS/Linux/Windows 的候选来源表达。

该缺口不阻断 Phase 1 只读环境页验收，但必须在删除旧入口的同一功能迭代内补齐，避免
发布 GUI PATH 行为回归。
