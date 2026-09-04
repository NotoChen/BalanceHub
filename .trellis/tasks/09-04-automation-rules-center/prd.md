# 自动化规则中心

## Goal

让用户针对余额、测活、签到、模型和 Key 状态定义可解释、可控且不会重复骚扰的通知规则。

## Requirements

- 规则包含稳定 ID、启停、作用域、类型化条件、冷却期和通知动作。
- evaluator 只消费 Rust 可用性事实或类型化 operation observation，不解析中文错误文案、toast 或前端状态。
- 仅在 false -> true 状态边沿触发；冷却、re-arm、失败退避和重启恢复语义必须确定。
- 未知或过期事实不触发；同一事实 revision 不重复执行。
- 用户规则持久化走明确 schema 迁移；运行期状态与有界历史存放于小型 cache repository，避免频繁重写主数据。
- scheduler 拆为独立 job，规则失败不得延迟或阻塞刷新、签到和测活。
- 第一版动作仅为已有通知渠道。

## Acceptance Criteria

- [ ] 规则重启后仍存在，启停立即影响未来评估。
- [ ] false -> true 只触发一次，冷却与重新满足条件的行为可由确定性测试复现。
- [ ] 未知、过期和重复 revision 均不产生通知。
- [ ] 规则、运行状态和历史均有数量/容量上限，执行失败不阻塞 scheduler。
- [ ] 数据迁移、默认值、导入导出、高版本拒绝和原子写入均有回归测试。

## Out of Scope

- 自动切换 Provider/Key、修改 Agent 配置、发起收费请求、执行任意脚本或自动修复。

## Dependency

- 硬依赖已稳定的可用性事实投影。
- UI 在规则模型、持久化和 transition evaluator 完成后再接入。

## Rollback Boundary

- scheduler 行为不变拆分、规则持久化/evaluator、通知接入和 UI 分步交付，任一步均不得要求回退其他产品子任务。
- 若规则 schema 已发布，回退必须保留对新版本数据的明确拒绝或恢复路径；不得静默丢弃规则或把运行期状态混回主配置。
