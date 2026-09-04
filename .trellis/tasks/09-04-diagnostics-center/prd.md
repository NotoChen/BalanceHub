# 一键诊断中心

## Goal

以只读、可取消、可导出的方式检查 BalanceHub 的本地运行条件，让用户知道失败发生在哪一层以及下一步应检查什么。

## Requirements

- Rust 统一编排代理、DNS/TLS/系统证书、Agent CLI、Terminal、Agent 配置、站点协议和 updater 链路检查。
- 每项结果包含稳定 ID、类别、状态、摘要、脱敏证据、耗时、适用平台和可选建议。
- 支持增量进度、独立失败、单项超时、整体取消和进程树清理；失败不得中止无关检查。
- 所有 probe 只读，不刷新凭据、不保存 Provider、不启动终端、不改写 Agent 配置、不改变 updater pending 状态。
- 报告在 Rust IPC 前集中脱敏；导出预览就是最终文件内容。
- 平台不适用必须返回 skipped 与原因，不伪装三端能力一致。

## Acceptance Criteria

- [ ] 一次运行可增量展示各项结果，取消/失败/超时均释放 UI 和后台任务状态。
- [ ] 诊断前后 App 设置、Provider 数据、Agent 配置和 updater 状态一致。
- [ ] API Key、Token、Cookie、密码、代理凭据、真实 Provider 名称和用户标识不出现在 IPC 或导出中。
- [ ] 单项失败不阻断其他检查，完成/失败/取消各自有且仅有一个终态。
- [ ] macOS、Linux、Windows 均有明确适用性测试与降级结果。

## Out of Scope

- 自动修复、安装 CLI、切换配置、签到或付费模型测活。
- 复用开发期 `npm run doctor` 作为运行时产品实现。

## Dependency

- 可独立于可用性页面实现。
- 其诊断结果词汇应在接入向导开始 UI 实现前稳定。

## Rollback Boundary

- 类型化任务生命周期、只读 probe、报告投影和 UI 分步验证；诊断入口及其服务可整体移除而不改变 Provider、Agent、Terminal 或 updater 的现有行为。
- 回退后不得残留后台诊断进程、活动任务或导出文件；共享的安全 probe 只有在仍有调用方和独立测试时保留。
