# 执行计划

## 1. Contract And Observation Groundwork

- [ ] 确认当前 schema、Provider 观察字段所有者和现有 fixture。
- [ ] 为额度、模型列表、Key 目录增加独立成功观测时间，为模型和未来测活增加 Key local ID 归属。
- [ ] 同步默认值、规范化、导入导出、迁移和 Rust/TypeScript 接收结构。
- [ ] 测试旧数据只迁移为 unknown，不错误回填归属。

## 2. Rust Projection

- [ ] 新增 availability IPC contract、domain normalizer 和只读 service。
- [ ] 实现紧凑模型 catalog、单模型 evaluation、分页和稳定排序。
- [ ] 由 Rust 生成动作能力、tier、reasons、来源与新鲜度。
- [ ] 注册 command，并断言打开/查询路径不触发网络、CLI 或持久化。

## 3. Frontend Workspace

- [ ] 按父任务确认的导航方式接入工作区。
- [ ] 新建 feature store/composable 与分责组件，不扩大 ProviderCard 或 AppController。
- [ ] 实现模型搜索、候选列表、证据展示、空/未知/过期/刷新状态。
- [ ] 使用 request ID/revision 拒绝过期结果，所有 busy 状态在 finally 收口。
- [ ] 接入既有临时 CLI、Key 选择、Agent 配置预览、模型/测活详情。

## 4. Verification

- [ ] Rust fixture 覆盖精确测活、Key 限制、未知 Key、失效 Key、额度零/无限/未知、旧测活和稳定排序。
- [ ] 前端测试覆盖空结果、筛选、防抖、旧结果拒写、刷新时保留快照、失败/取消释放状态。
- [ ] 性能 fixture 覆盖仓库上限但不构造模型 x Key 全量结果。
- [ ] 检查 IPC 和日志无明文凭据。
- [ ] 运行前端、Rust、platform doctor 和 `git diff --check`。
- [ ] 通过 macOS 本机验证与 Linux/Windows CI 后再完成本子任务。

## Rollback Point

观测模型/迁移、Rust 投影、前端工作区分成可审查步骤；任一步失败时回退到最近一个已通过行为测试的边界，不保留未调用导出或平行旧实现。
