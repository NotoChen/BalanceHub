# Research: README 与 GitHub Pages 能力准确性审计

- Query: 核对 `README.md`、`docs/index.html` 的用户可见能力宣称是否与当前 HEAD 源码一致，重点覆盖盾处理、Agent CLI、会话检索、API Key、配置导入导出、自动更新、跨平台发布包与 Issues-only 协作边界。
- Scope: internal
- Date: 2026-09-03
- Source snapshot: App 源码与发布配置基于 HEAD `4c2f13c`；`README.md` 当前工作区另有一处未提交的“最新版本”动态链接修改，审计按该工作区文案核对。

## Findings

### 必须修正

1. **删除所有 Cloudflare 自动过盾宣称，并收窄阿里云 WAF 表述。**
   - `README.md:120`、`README.md:137`、`README.md:149`、`README.md:175` 宣称“阿里云 WAF 与 Cloudflare 过盾”或 “WAF / Cloudflare 挑战凭证隔离”。
   - 当前源码的盾类型闭集只有 `AliyunWaf`：`src-tauri/src/network/shield/mod.rs:3-5`、`src-tauri/src/network/shield/mod.rs:15-22`；探测也只识别阿里云挑战：`src-tauri/src/network/shield/mod.rs:100-102`。仓库 App 源码中没有 Cloudflare 实现。
   - 现有能力也不是泛化的“所有阿里云 WAF”：它只复刻带 `var arg1` 的确定性 JS 挑战并生成 `acw_sc__v2`，无需 WebView 或人工验证：`src-tauri/src/network/shield/aliyun.rs:1-6`、`src-tauri/src/network/shield/aliyun.rs:17-35`。
   - 推荐公开表述：**“自动识别并重试 AnyRouter 等站点常见的确定性阿里云 WAF JS 挑战”**。不要写 Cloudflare，也不要暗示能处理验证码、Turnstile 或任意 WAF。

2. **把“跨 Agent 会话全文检索”改为按 Agent、按工作目录的可见对话检索。**
   - `README.md:119`、`README.md:136`、`README.md:147` 的“跨 Agent”“全文检索”容易被理解为一个搜索框同时搜索四个 Agent 的全部原始记录。
   - 实际 IPC 每次必须指定单一 `cliKind` 和 `workdir`：`src/api/app.ts:205-218`、`src-tauri/src/commands/cli.rs:49-79`，不是全局跨 Agent 聚合检索。
   - 搜索范围包含标题、Resume ID、预览、模型和目录，缺失时再检索正文：`src-tauri/src/services/cli_sessions/mod.rs:264-333`；UI 占位文案也明确列出这些字段：`src/components/WorkspaceSessionHistoryPanel.vue:90-100`。
   - SQLite FTS 只索引各 Agent 解析出的可见消息正文：`src-tauri/src/services/cli_sessions/index.rs:942-975`、`src-tauri/src/services/cli_sessions/index.rs:1057-1079`。Codex 索引只取 user/assistant 消息：`src-tauri/src/services/agent_cli/codex/sessions/rollout.rs:35-104`；Claude、Gemini、Grok 同样对工具输出、元数据或不可见内容做过滤：`src-tauri/src/services/agent_cli/claude/sessions.rs:145-220`、`src-tauri/src/services/agent_cli/gemini/sessions.rs:564-588`、`src-tauri/src/services/agent_cli/grok/sessions.rs:188-246`。
   - 推荐公开表述：**“分别支持 Codex CLI、Claude Code、Gemini CLI、Grok Build 的历史会话检索；可在选定工作目录搜索标题、Resume ID、模型、目录和可见对话正文，并查看详情或恢复会话。”**

3. **统一四个 Agent 的正式名称，避免简称混用。**
   - Rust 注册表当前恰好四个 Agent：`src-tauri/src/agent_cli_catalog.rs:6-13`。
   - 权威展示名分别为 `Codex CLI`、`Claude Code`、`Gemini CLI`、`Grok Build`：
     - `src-tauri/src/services/agent_cli/codex/mod.rs:17-22`
     - `src-tauri/src/services/agent_cli/claude/mod.rs:17-22`
     - `src-tauri/src/services/agent_cli/gemini/mod.rs:17-22`
     - `src-tauri/src/services/agent_cli/grok/mod.rs:16-21`
   - `docs/index.html:410`、`docs/index.html:449`、`docs/index.html:453` 已使用正确名称，可以保留；`README.md:46`、`README.md:71`、`README.md:133`、`README.md:144` 中的 “Codex / Gemini / Grok” 应统一为上述正式名称。

### 可以保留

1. **四个 Agent 均支持测活、临时启动与会话能力。** 四个定义均注册 `temporary_launch`、`sessions`、`liveness` 和 `default_config`；证据见各 Agent `mod.rs` 的 `definition`（Codex `:17-51`、Claude `:17-51`、Gemini `:17-52`、Grok `:16-50`）。因此 Pages 关于四个 Agent 测活和卡片内临时启动的文案成立。

2. **自动更新周期与交互准确。**
   - 启动延迟 30 秒、之后每 6 小时检查：`src/composables/useAppUpdater.ts:15-16`、`src/composables/useAppUpdater.ts:236-245`。
   - 检测到更新只打开提示弹窗，只有用户调用安装动作才下载：`src/composables/useAppUpdater.ts:76-125`、`src/composables/useAppUpdater.ts:152-212`。
   - 下载阶段可取消；45 秒无进度、20 分钟总时限、256 MiB 上限均有实现：`src-tauri/src/services/app_updater.rs:14-19`、`src-tauri/src/services/app_updater.rs:193-230`、`src-tauri/src/limits.rs:14`。
   - 因此 `README.md:88` 与 `docs/index.html:511-525` 的周期、确认后下载、签名校验和限制说明可以保留。

3. **跨平台包说明与发布矩阵一致。**
   - Release 由 `v*` tag 触发：`.github/workflows/release.yml:3-6`。
   - 构建矩阵包含 macOS Apple Silicon/Intel 的 app+dmg、Windows x64/ARM64 的 NSIS、Linux x64/ARM64 的 AppImage/deb/rpm：`.github/workflows/release.yml:100-125`。
   - Updater 清单也解析相同架构与包名：`.github/scripts/generate-updater-json.mjs:104-141`、`.github/scripts/generate-updater-json.mjs:169-199`。
   - 因此 `README.md:82-86` 与 `docs/index.html:517-519` 可以保留。

4. **Issues-only 表述准确。** `CONTRIBUTING.md:1-14` 明确只接受 Issue；PR workflow 会自动说明并关闭 PR：`.github/workflows/close-pull-requests.yml:1-23`。`README.md:235`、`docs/index.html:481-482`、`docs/index.html:582-584` 可以保留。

5. **动态最新版入口已经正确。** 当前工作区 `README.md:39` 使用 `/releases/latest`，`README.md` 与 `docs/index.html` 均不再硬编码 `v0.5.9`；Pages 的“下载最新版”链接可以继续指向 Releases Latest。

6. **API Key 库和配置导入导出有源码支撑。**
   - 多 Key、备注、当前调用 Key、同步/创建/本地添加均在编辑页实现：`src/components/provider-editor/ProviderApiKeyVault.vue:26-84`、`src/components/provider-editor/ProviderApiKeyVault.vue:200-310`；后端提供本地添加、备注、设为默认及远端同步/创建：`src-tauri/src/services/provider_service/api_keys.rs:14-140`。
   - Agent 配置可绑定具体 `api_key_local_id`：`src-tauri/src/services/agent_cli/config_support/mod.rs:163-211`、`src-tauri/src/services/agent_cli/config_support/mod.rs:214-243`。
   - 设置页保留配置导入/导出入口：`src/components/settings/SettingsSystemSection.vue:91-114`；后端命令和完整 JSON 读写存在：`src-tauri/src/commands/app.rs:117-143`、`src-tauri/src/storage.rs:54-82`。
   - 因此 README 的多 Key、备注、当前 Key、Agent 绑定和导入导出能力可保留。Pages 的“CC Switch 导入”虽能对应 deeplink，但更直白的用户文案是“添加到 CC Switch”。

## Files Found

- `README.md` — 当前产品入口及主要能力宣称。
- `docs/index.html` — 当前 GitHub Pages 首页文案。
- `src-tauri/src/network/shield/mod.rs`、`aliyun.rs` — 盾类型闭集、探测、求解及缓存边界。
- `src-tauri/src/agent_cli_catalog.rs`、`src-tauri/src/services/agent_cli/*/mod.rs` — Agent 数量、正式名称与能力注册。
- `src/api/app.ts`、`src-tauri/src/commands/cli.rs`、`src-tauri/src/services/cli_sessions/` — 会话检索 IPC、工作目录边界、索引与详情。
- `src/composables/useAppUpdater.ts`、`src-tauri/src/services/app_updater.rs` — 更新调度、提示、下载、取消与超时。
- `.github/workflows/release.yml`、`.github/scripts/generate-updater-json.mjs` — 三平台双架构发布包与 updater 映射。
- `CONTRIBUTING.md`、`.github/workflows/close-pull-requests.yml` — Issues-only 协作规则。

## Related Specs

- `AGENTS.md`：项目定位、三种协议、Rust 真源、README/Pages 同步、Issues-only、Release 与版本规则。
- `.trellis/spec/frontend/quality-guidelines.md`：文档/配置改动至少执行 `git diff --check`，不保留重复或虚假能力入口。
- `.trellis/tasks/09-03-github-pages-redesign/prd.md`：明确要求动态版本与移除 Cloudflare 误宣称。
- `.trellis/tasks/09-03-github-pages-redesign/design.md`：Pages 内容以当前 Rust/前端源码为事实来源。

## External References

- 无。本审计只判断仓库当前实现与公开文案是否一致，没有用第三方产品文档推断 BalanceHub 能力。

## Caveats / Not Found

- 源码和 workflow 能证明“计划构建哪些包”，不能单独证明 GitHub 当前 latest release 的每个资产此刻都上传成功；若页面要写“当前每个平台包均可下载”，发布前还需检查真实 Release 资产。
- “未做系统级付费代码签名”涉及外部证书、GitHub Secrets 与实际产物签名状态；仓库 workflow 只明确配置了 Tauri updater 签名，不能仅凭源码对所有已发布产物作绝对结论。
- 会话详情会展示部分工具记录，但搜索索引刻意聚焦可见用户/助手正文；公开文案不应把“详情可展示工具记录”与“所有原始记录均进入全文检索”混为一谈。
