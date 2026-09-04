# 执行计划

## Phase 0: 冻结路线图

- [x] 用户确认“搜索区上下文入口 -> 独立主体工作区 -> 恢复原面板”的入口形态。
- [x] 完成父任务 PRD 收敛与六个子任务依赖检查。
- [x] 为首个子任务准备真实 implement/check 上下文。
- [ ] 展示最终规划摘要并取得单独的实现授权。

## Phase 1: Agent 环境与运行时中心

- [ ] 启动 `09-04-agent-environment-runtime-center` 子任务，不启动父任务。
- [ ] 先完成原生环境/安装 identity、只读资产与配置清单、installed/latest stable 版本比较。
- [ ] 完成统一 runtime contract、现有临时 CLI 强证据适配和稳定关联 ID。
- [ ] 实现有界 helper/spool 与 Codex Hook 管理试点，再逐 Agent 接入原生三端。
- [ ] 顶栏、Provider 卡片和会话工作台切换到统一投影后，删除旧临时 CLI 独立域和长期轮询。
- [ ] 通过完整前端/Rust/平台脚本检查和 macOS 实物验收后独立提交、归档。

## Phase 2: 会话工作台与诊断中心

- [ ] `09-04-agent-session-workbench`：先拆作用域取消与按库锁，再切换到统一运行时投影并实现独立工作台。
- [ ] `09-04-diagnostics-center`：建立只读诊断合同、runner、三端 probe、脱敏报告。
- [ ] 两项仅在文件所有权无重叠时并行，并分别独立检查与验收。

## Phase 3: 可用性决策中心

- [ ] 启动 `09-04-availability-decision-center` 子任务。
- [ ] 补观测时间、Key 归属、未知语义和迁移/默认值测试。
- [ ] 新增无副作用的 Rust 可用性投影、分页查询和动作能力合同。
- [ ] 按确认后的导航方式实现独立工作区并覆盖大规模数据、隐私和异步防过期测试。

## Phase 4: 中转站接入向导

- [ ] 诊断结果词汇稳定后启动 `09-04-provider-onboarding-wizard`。
- [ ] 先实现只读 save planner 与重复结果 fixture。
- [ ] 复用现有 editor section 和 Rust 能力，替换旧新增流程。
- [ ] 验证取消无半成品、最终动作明确、远端副作用单独确认。

## Phase 5: 自动化规则中心

- [ ] 在可用性事实投影稳定后启动 `09-04-automation-rules-center`。
- [ ] 先拆 scheduler job 并定义类型化 observation。
- [ ] 完成规则 schema 迁移、runtime state、边沿/冷却 evaluator 和通知动作。
- [ ] 最后接入 scheduler 和 UI，验证重启、禁用、去重和有界历史。

## Phase 6: 集成验收

- [ ] 验证六个入口共享 Rust 真源且无前端能力镜像。
- [ ] 验证功能间 deep-link 只传稳定 ID，不产生隐式刷新或写入。
- [ ] 执行跨平台 CI、迁移、隐私、性能和并发矩阵。
- [ ] 检查旧入口、废弃导出、重复实现和未使用依赖均已删除。

## Per-child Quality Gate

```bash
npm run build
npm test
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy --all-targets --all-features -- -D warnings
cd src-tauri && cargo test
npm run doctor:platform
git diff --check
```

对仅文档或只影响单层的中间提交，可按风险缩小命令；每个子任务最终验收仍执行与其影响范围匹配的完整检查。

## Rollback Boundaries

- 每个子任务独立提交和归档，不使用一个提交混合多个产品方向。
- Foundation 拆分必须行为不变，可单独回退。
- 涉及 schema 的自动化规则任务在发布前验证旧数据迁移和高版本拒绝；回退不得假装读取新 schema。
- 出现新的架构分歧、根因不明或跨任务文件冲突时暂停执行，回到规划/研究，不由实现 Agent 猜测。
- Hook 统一 UI 切换必须是独立提交；在切换完成前保留 launcher repository，切换后删除旧公开合同和长期轮询，不保留永久双真源。
