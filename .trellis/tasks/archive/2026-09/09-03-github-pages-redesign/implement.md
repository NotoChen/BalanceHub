# GitHub Pages 产品入口执行计划

## 1. 内容校准

- [x] 保留 README 动态“最新版本”链接，删除重复当前版本号。
- [x] 根据当前 shield 实现修正 README 中 Cloudflare 自动过盾相关表述。
- [x] 修正“跨 Agent 全文检索”和 Agent 简称，确保与实际 IPC/注册表能力一致。
- [x] 从 README 提炼 Pages 所需内容，不逐段复制长表格和全部 FAQ。

## 2. 页面结构重构

- [x] 新增 `docs/_config.yml`、`docs/_layouts/default.html`、本地品牌 mark 和 `docs/assets/site.css`。
- [x] 为现有六个 Markdown 文档增加最小 Jekyll front matter，修复 `.html` 入口 404。
- [x] 重写 `docs/index.html` 的 front matter、导航和 Hero。
- [x] 实现三步产品主链路和非对称能力版图。
- [x] 用现有真实截图实现带说明的产品叙事区。
- [x] 实现本地优先、下载更新、文档导航和 FAQ 区块。
- [x] 精简 footer，并保持 Issues-only 协作边界。

## 3. 视觉与响应式实现

- [x] 建立页面级颜色、排版、间距、圆角和阴影 token。
- [x] 增加桌面/平板/手机断点和窄屏导航策略。
- [x] 增加 hover、active、focus-visible 与 reduced-motion 状态。
- [x] 检查超长中文、平台说明、徽章和截图在窄屏下不溢出。

## 4. 验证

- [x] `git diff --check`
- [x] 搜索 README/Pages，确认没有硬编码当前包版本。
- [x] 搜索 README/Pages，确认没有错误的 Cloudflare 自动过盾宣传。
- [x] 启动本地静态服务并检查所有本地资源返回成功。
- [x] 使用与 Pages workflow 一致的 Jekyll 构建，确认六个 Markdown 文档生成对应 `.html`。
- [x] 以桌面和手机视口渲染截图，人工检查层级、裁切、横向滚动和可读性。
- [x] 检查键盘焦点、跳至正文、标题层级和图片 alt。
- [x] 确认改动仅包含文档、任务记录，不触碰 App 源码或版本文件。

## 5. 最新反馈迭代

- [x] 调整 Provider Board 能力卡片的空间组织，消除标题与示意卡片之间的无意义大留白。
- [x] 调整 Agent CLI chip 的宽桌面布局，四个正式名称与官方图标同一行；中小屏保持可读的自然换行。
- [x] 新增根 `CHANGELOG.md` 到 Pages 的构建投影和可达入口，确保不提交重复内容。
- [x] 重新执行 Jekyll 构建、站内链接/图片检查、1440/1280/1024/390/320 响应式检查和 `git diff --check`。

## Verification Results

- 2026-09-03: Jekyll build completed with `jekyll/jekyll:4.2.2` and generated the landing page plus six document pages.
- 2026-09-03: Checked 174 local links, assets, and anchors; all targets exist. Seven HTML pages each have one `h1`, and all 11 rendered images have `alt` text.
- 2026-09-03: Browser verification passed at desktop, 390 px, and 320 px widths. Mobile navigation, document navigation, keyboard skip link, image loading, and horizontal overflow checks passed with no console errors.
- 2026-09-03: Public-copy scans found no hard-coded `v0.5.9`, Cloudflare/OAuth/WebView claims, cross-Agent search wording, or remote Pages image dependency.
- 2026-09-03: `git diff --check` passed. The work remains uncommitted pending user visual acceptance.
- 2026-09-03: 根据视觉验收反馈，页头与 favicon 改为复用 `src-tauri/icons/balancehub.svg`；Agent CLI 芯片改为复用 `src/assets/logos/{codex,claude,gemini,grok}.svg` 的本地副本，不再使用字母占位符。所有副本已通过 SHA-256 逐字节校验。
- 2026-09-03: 图标专项审查在 1440 / 390 / 320 px 视口通过，Agent 芯片正常换行且无横向溢出；Jekyll 4.2.2 构建和 `git diff --check` 通过。
- 2026-09-04: 最终复核将 README 选型对比中的 `Codex` 统一为正式名称 `Codex CLI`；Jekyll 4.2.2 重新构建生成 7 个页面，188 个站内链接与 15 个图片引用均可解析，所有页面各含一个 `h1` 且图片均有 `alt`，Trellis context 校验和 `git diff --check` 通过。
- 2026-09-04: 真实品牌/Agent 图标逐字节复核通过；README 的历史 `banner.svg` 仍包含独立绘制的旧图标，但本轮需求约束的是 Pages 页头、favicon 和页面品牌位，暂不扩大到 README banner 资产替换。
- 2026-09-04: 根据最新验收反馈补充 Provider Board 紧凑布局、Agent 单行/响应式换行及根 CHANGELOG 单一真源投影方案，研究记录见 `research/latest-feedback-design.md`。
- 2026-09-04: 最终 `trellis-check` 复核确认 Provider Board 在 1440/1280/1024 px 下为紧凑横向信息带，桌面高度约 190 px；390/320 px 下按设计转为普通流单列，没有人为固定高度或绝对定位留白。
- 2026-09-04: 四个 Agent CLI 在 1440/1280/1024 px 下均为单行，在 390/320 px 下自然换行；五个视口的 `scrollWidth` 均等于 `clientWidth`，未发现横向溢出。
- 2026-09-04: `CHANGELOG.md` 投影脚本、忽略规则和 Pages workflow 触发路径复核通过；独立 Jekyll 4.2.2 构建生成 8 个页面，结构检查覆盖 246 个站内链接、72 个锚点和 16 个图片引用，全部可解析，每页恰有一个 `h1`。
- 2026-09-04: 更新记录页在桌面及 390/320 px 下通过阅读布局检查，侧栏当前项、跳至正文和长页无横向溢出检查均正常；`npm run build`、`npm test`（80 项）、Trellis context 校验、脚本语法检查和 `git diff --check` 均通过。
- 2026-09-04: 最终检查发现本地 Jekyll 会在 `docs/` 下生成 `.jekyll-cache/`；已将该可再生目录加入 `.gitignore` 并移至废纸篓，避免文档预览缓存进入提交。
- 2026-09-04: App CI 的 `paths-ignore` 已精确覆盖 Pages 专用 workflow/生成脚本、Trellis 记录与 `.gitignore`；本批纯文档站改动只触发 Pages 构建，不再浪费五平台 Tauri 编译，App 源码、依赖和通用 CI/Release workflow 仍保持完整质量门禁。

## Risk and Rollback Points

- `docs/_layouts/default.html` 和 `docs/assets/site.css` 会影响所有 Pages 页面；先完成共享外壳，再逐页补 front matter，避免出现一半可访问、一半 404 的状态。
- README 能力纠偏只修改当前介绍，不改历史 CHANGELOG。
- Pages workflow 新增 `CHANGELOG.md` 临时投影步骤；若生成或构建行为出现差异，可移除该步骤和 Changelog 入口，其余静态页面仍可独立构建，不需要引入新的站点框架。
