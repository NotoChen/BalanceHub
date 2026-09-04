# 重构 GitHub Pages 产品入口

## Goal

将 GitHub Pages 从旧式功能清单页重构为与新版 README 一致、但更适合网页浏览的产品入口：用户进入页面后能快速理解 BalanceHub 解决的问题、核心使用链路、桌面端能力边界，并能直接下载、查看文档或反馈问题。

## Background

- `README.md` 已更新为面向用户的产品入口，但 `docs/index.html` 仍沿用旧版等宽卡片、普通截图网格和较弱的视觉层级。
- Pages 使用 `docs/` 下的纯静态 HTML/CSS，由 `.github/workflows/pages.yml` 通过 Jekyll 构建部署；本轮不引入框架或依赖。
- 当前线上首页能够访问，但 `getting-started.html`、`provider-config.html`、`liveness.html` 等站内文档入口返回 404；对应 Markdown 缺少 Jekyll front matter。
- README 顶部已有 GitHub Release 动态徽章，正文不应继续维护写死的当前版本号。
- 当前源码仅实现阿里云 WAF 的确定性挑战处理；README 中“Cloudflare 过盾”表述与代码不一致，页面重构不能继续传播该表述。

## Requirements

### R1. 信息架构

- 首页必须以“集中管理中转站账号，并验证和启动 Agent CLI”作为核心定位。
- 首屏必须提供下载最新版、查看使用文档和访问 GitHub 的明确入口。
- 使用网页化叙事展示“接入中转站 → 验证可用性 → 直接启动 CLI”的主链路，而不是逐段复制 README。
- 核心能力至少覆盖：中转站与额度、API Key 库、签到与后台任务、CLI 测活与临时启动、会话检索、本地优先与跨平台。
- 文档、常见问题、Issues、许可证和源码入口保持可达。
- 修复当前 Pages 站内文档链接 404，详细 Markdown 文档通过共享 Jekyll layout 形成可直接阅读的 HTML 页面。

### R2. 视觉与响应式体验

- 保留现有纯静态 HTML/CSS，不新增运行时框架、包管理依赖或第三方字体依赖。
- 使用与真实 App 截图协调的冷灰、墨色和蓝绿色视觉体系，减少旧页面的通用卡片模板感。
- 页头、favicon 和品牌位必须复用桌面 App 的真实 BalanceHub 图标；Agent CLI 列表复用 App 已使用的 Codex、Claude、Gemini 和 Grok 官方图标，不使用重绘近似图或字母占位符。
- 使用非对称内容布局、真实 App 截图和清晰的排版层级；避免六张等宽功能卡作为主体。
- 桌面、平板和手机宽度下均不得出现横向溢出、文字挤压、不可点击或截图失真的情况。
- 动效仅使用 `transform` / `opacity`，尊重 `prefers-reduced-motion`。

### R3. 动态版本与内容准确性

- 首页不得写死当前应用版本号；最新版统一链接到 GitHub Releases，版本展示使用动态 Release 徽章或不展示具体数字。
- README 中写死的 `v0.5.9` 同步替换为动态“最新版本”入口。
- README 与 Pages 中不得宣称当前不存在的 Cloudflare 自动过盾能力；只描述源码当前具备的阿里云 WAF/统一代理能力。
- 会话检索必须描述为“在选定 Agent 和工作目录内搜索可见对话”，不能宣称一个入口跨四个 Agent 搜索全部原始记录。
- Agent 名称统一使用 `Codex CLI`、`Claude Code`、`Gemini CLI`、`Grok Build`。
- `CHANGELOG.md` 中的历史版本号、依赖版本和示例版本不在本轮清理范围内。

### R4. 可访问性与分享信息

- 使用语义化的 `header`、`nav`、`main`、`section`、`article`、`footer`。
- 提供跳至正文链接、清晰的键盘焦点、合理的标题层级和有意义的图片替代文本。
- 补齐 canonical、Open Graph、Twitter Card、主题色和 favicon/应用图标引用。
- Pages 品牌图标使用本地资源，不依赖 `raw.githubusercontent.com`；首屏外截图启用 lazy loading 并预留尺寸，避免布局跳动。
- 外部链接和交互状态必须有明确 hover、active、focus 反馈，不保留死链接。

### R5. 范围控制

- 保持现有 Pages 部署方式和 `docs/*.md` 文档地址不变。
- 不修改桌面 App UI、业务逻辑、安装版配置或发布版本号。
- 不伪造产品截图，不在页面中展示真实账号或凭据。

## Acceptance Criteria

- [ ] `docs/index.html` 的首屏、主链路、能力展示、截图叙事、信任边界、下载和文档入口形成完整产品页面。
- [ ] 首页和 README 均不包含当前包版本 `0.5.9` 的重复硬编码。
- [ ] README 和 Pages 不再宣称 Cloudflare 自动过盾，能力描述与当前源码一致。
- [ ] 页面所有本地资源和站内链接可解析，所有图片具有有效 `alt`。
- [ ] 页头使用与桌面 App 一致的 BalanceHub 图标，四个 Agent CLI 使用对应官方图标，不再显示 C / A / G / X 字母占位符。
- [ ] `getting-started.html`、`provider-config.html`、`liveness.html`、`reference.html`、`release.html`、`faq.html` 均由 Pages 构建并可访问。
- [ ] 以桌面和窄屏渲染检查页面，无横向滚动，导航、按钮、文字和截图布局正常。
- [ ] 页面在禁用 JavaScript 时仍能完成主要阅读、下载和文档导航。
- [ ] `git diff --check` 通过；纯文档改动不触发 App 质量 CI。
- [ ] Provider Board 能力卡片不保留无意义的大面积垂直留白；站点示意信息与卡片说明在同一视觉区域内紧凑组织。
- [ ] 桌面宽度下四个 Agent CLI 官方图标与正式名称在同一行展示；仅在空间不足的平板/手机宽度下自然换行，不通过截断或删除名称解决。
- [ ] Pages 提供可直接阅读的“更新记录”页面，并在导航、文档入口和页脚可达；页面内容由根目录 `CHANGELOG.md` 在 Pages 构建时生成，仓库不维护第二份手工复制的更新记录。

## Out of Scope

- 将 Pages 迁移到 Vue、Vite、React、Jekyll 主题或其它站点生成器。
- 重写 `docs/getting-started.md`、`docs/reference.md` 等详细文档正文。
- 新增统计、Cookie、登录、评论或远程后端。
- 发布新版本、创建 tag 或修改应用版本文件。
