# 技术设计

## Scope

第一阶段新增 Rust 只读事实投影与独立 Vue 工作区，用已有数据回答“针对这个模型，哪些 Provider + Key 具有最强的可用证据”。本任务不运行探测、不自动切换配置，也不实现自动化规则。

## Data Ownership

- Provider、Key、模型与测活观察仍由现有协议和 Provider service 获取、持久化。
- 新增的观测时间写在拥有该事实的 Rust 模型中：额度、模型列表、Key 目录各自独立。
- 新测活记录保存可选 `api_key_local_id`；旧记录保持无归属。
- `AvailabilityService` 只读取快照并生成 IPC 投影，不修改持久化数据。
- Agent 枚举和默认配置匹配来自动态 registry/config snapshot。

## Query Contract

```text
load catalog -> select/search one model -> evaluate candidates -> paged snapshot
```

- Catalog 由缓存的动态模型名称去重生成，不发网络请求。
- Evaluation 按所选模型创建一次性 HashMap/HashSet，再遍历 Provider + Key；不生成全模型与全 Key 笛卡尔积。
- Snapshot 返回 `generated_at_ms`、`source_revision`、候选总数、分页游标/页码和候选行。
- Candidate 使用 Provider ID + Key local ID；包含显示标签、协议、Key 状态、模型/额度/测活证据、Agent 绑定、动作能力、tier 和 reasons。
- 所有时间在 Rust 统一为毫秒；前端只负责本地格式化。

## Evidence Semantics

- `fresh`: 成功观察仍在其配置周期内。
- `stale`: 有成功观察，但已超过下一次应刷新时间。
- `refreshing`: 当前后台任务正在获取该类事实。
- `unknown`: 从未成功、时间缺失、读取失败或旧记录无 Key 归属。

失败尝试时间与最后成功观察分离。过期的成功记录仍可展示历史证据，但不得显示为当前确认；未知不得显示为失败。
新鲜度按事实来源自身已有的刷新周期或明确 due time 计算；没有周期策略的来源只展示观测时间并返回 `unknown`，不得引入隐藏的统一 TTL。

## Ranking

1. 近期精确 Provider + Key + 模型测活成功。
2. Key 明确允许该模型且 Key 可用。
3. 当前 Key 产生的 Provider 模型列表包含该模型，且无相反限制。
4. 模型支持未知，但 Key 与其他元数据可用。
5. 明确禁用、过期、耗尽、缺少完整 Key 或明确排除模型。

同层依次比较证据新鲜度、已知可用/无限额度、成功观察时间、较低延迟，最后使用持久化顺序稳定排序。不暴露不可解释的综合分数。

## UI Boundary

- 使用 `features/availability/` 承载查询状态、工作区和分解组件。
- 导航已确认为搜索区上下文入口；进入后替换主内容，返回时恢复面板搜索、滚动位置和焦点。
- `AppWorkspace.vue` 只负责 `board | availability` 工作区状态；`App.vue` 继续只做编排。
- 面板搜索与决策模型搜索是两个独立 query；不能把普通搜索词静默解释为模型，也不能写死默认模型。
- 模型搜索在紧凑 catalog 上本地过滤；真正 evaluation 使用防抖、递增 request ID 和 source revision 拒绝旧结果。
- 候选行展示独立证据，不复用 Provider 卡片综合色作为结论。
- 顶栏全局刷新继续复用现有后台刷新；结果区只提供“重新生成比较”，根据本地事实重算且不发网络请求。刷新期间继续展示旧快照并标识 refreshing。
- 376px 窗口切换为两层摘要布局，但仍保留证据等级、额度、测活和 Agent 最小摘要；不得靠隐藏全部证据解决窄宽度。

## Actions

- 启动临时 CLI、选择当前 Key、Agent 默认配置预览/确认、模型详情和测活详情全部复用既有入口。
- IPC 只返回稳定 ID 和 Rust 计算的动作能力；执行命令再次校验最新 Provider revision 和 Key 状态。
- 不提供“立即验证”按钮，因为当前测活可能发起真实收费请求且没有独立手动 command/确认契约。

## Migration

- 新增观测字段需同步 Rust 默认值、输入规范化、协议成功结果写入、TypeScript 接收类型、导入导出和 storage migration。
- 实施时先确认当前 schema；按实际版本新增单步迁移，不预设规划文档中的版本仍未变化。
- 旧数据迁移后全部为 `None/unknown`，不从通用 `last_synced_at` 猜测模型或 Key 目录时间。

## Risks And Rollback

- 最大风险是错误归属 Key、全量复制导致内存峰值、以及旧 evaluation 覆盖新选择。
- 投影服务和 UI 可独立回退；观测字段应保持向前兼容的可选值。若迁移已发布，回退必须遵守高版本 schema 拒绝规则。
- 涉及 `contracts.rs`/`provider-types.ts` 时只拆本功能使用的类型，不顺带迁移无关域。
