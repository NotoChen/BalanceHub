# Research: latest feedback design

- Query: 针对最新视觉反馈，判断 Provider Board 消除大空白、桌面 Agent 单行/窄屏换行，以及根 `CHANGELOG.md` 作为唯一真源并生成 Pages `changelog.html` 的最小可靠方案。
- Scope: internal
- Date: 2026-09-04

## Findings

### A. Provider Board 的最小布局方案

现有结构已经足够表达内容，不需要改 `docs/index.html` 的标记：Provider Board 是 `.capability-overview`（`docs/index.html:107-134`），内部 `.mini-board` 包含四个 `.mini-provider`。

造成空白的主要 CSS 组合是：卡片固定 `min-height: 500px` 且跨两行（`docs/assets/site.css:695-702`），而 `.mini-board` 使用 `position: absolute` 固定在卡片底部（`docs/assets/site.css:729-737`）。文字仍在普通文档流顶部，因而文字与底部小卡之间会留下没有语义的垂直空间。窄屏还把卡片 `min-height` 提到 520px（`docs/assets/site.css:1589-1591`、`1720-1722`），同时把小卡改成四行（`docs/assets/site.css:1724-1729`），空白/高度失衡会更明显。

推荐的最小改动是只改 `docs/assets/site.css`，保留当前 bento 的 `grid-row: span 2`，避免重新计算后续 dense grid：

1. 将 `.capability-overview` 设为纵向 flex 容器（`display: flex; flex-direction: column`）。
2. 将 `.mini-board` 从绝对定位改为普通流（`position: static`），保留左右内边距由父卡片提供，并给它 `margin-top`；让它作为剩余空间的 flex item（`flex: 1 1 auto`）。必要时补 `grid-auto-rows: minmax(0, 1fr)`/`align-content: stretch`，使 2×2 小卡填充原本的空白，而不是再次出现一块无结构的底部留白。
3. 保留现有 `@media (max-width: 640px)` 的一列小板规则；普通流会让卡片按内容自然增高，不会像绝对定位那样依赖 520px 固定高度。

这会把空间转化为可读的 Provider 卡片面积，同时不动 HTML、不改变其他 capability 的列占用。若验收目标是连卡片总高度也明显缩短，可另行评估去掉 `grid-row: span 2` 和固定 `min-height`；该方案会改变 dense grid 排列，不属于本次“最小”修复，必须重新检查桌面/窄屏布局。

### B. Agent CLI 桌面单行、窄屏换行

当前四项已经使用真实本地图标和正确名称（`docs/index.html:140-157`）。`.agent-list` 默认 `display: flex; flex-wrap: wrap`（`docs/assets/site.css:820-828`），每个 chip 的图标 22px、内边距 8px 10px（`docs/assets/site.css:830-848`）。在 5/12 宽度的 Agent 卡中，四个自然宽度 chip 很容易变成两行。

推荐仍只改 CSS，不改列表标记：

- 对足够宽的桌面（建议以现有视觉验收的 1280px+ 页面宽度作为单行阈值）设置 `.agent-list { flex-wrap: nowrap; gap: 6px; }`。
- 同一桌面规则下把 chip 轻量收紧（例如 `padding: 6px 8px; gap: 5px; font-size: 11px`，图标 20px），并设 `flex: 1 1 0; min-width: 0; justify-content: center; white-space: nowrap`。等宽 flex 子项能稳定占满可用宽度，避免只靠 `max-content` 造成边缘溢出。
- 在 `@media (max-width: 1100px)`（或经实测确认的可用宽度阈值）显式恢复 `flex-wrap: wrap`，并取消等宽约束（`flex: 0 1 auto`）；现有 860px/640px 断点会继续将 capability 变成两列/单列。窄屏保留自然换行，不使用横向滚动或裁切。

如果产品把 1100px 左右也定义为“桌面且必须单行”，仅靠收紧字体会使 chip 难读；应改为给 Agent 卡更多列宽或使用 container query，而不是强制 `nowrap`。未做浏览器渲染，本结论建议先在 1440px、1280px、1024px、390px 四个宽度核对。

### C. `CHANGELOG.md` 唯一真源与 Pages `changelog.html`

当前 Pages workflow 只在 `docs/**` 或自身 workflow 变化时触发（`.github/workflows/pages.yml:3-10`），构建源目录是 `./docs`（`.github/workflows/pages.yml:32-41`）。根 `CHANGELOG.md` 从普通 Markdown H1 开始，没有 Jekyll front matter（`CHANGELOG.md:1-5`），不能直接依赖现有 Jekyll source 自动生成 HTML。根文件还按 `## <version>` 连续记录历史版本（例如 `CHANGELOG.md:5`、`17`、`24`），不应为 Pages 另维护一份副本。

最小且可靠的 CI 方法是：

1. 在 `on.push.paths` 增加 `CHANGELOG.md`，确保更新日志提交会触发 Pages 部署。
2. checkout 后、`actions/jekyll-build-pages@v1` 前增加一个临时生成步骤，在 CI 工作区生成未提交的 `docs/changelog.md`：先写入 front matter（`layout: default`、`title: 更新日志`、`description`、`permalink: /changelog.html`），再原样 `cat CHANGELOG.md` 追加正文。随后沿用现有 `source: ./docs` 构建；不提交这个生成文件。
3. 在 `docs/_layouts/default.html` 的文档侧栏/页脚增加 `{{ '/changelog.html' | relative_url }}` 链接；如需首页入口，再在 `docs/index.html` 的文档网格追加一项。现有共享 layout 已对非 landing page 使用 `.document-content`（`docs/_layouts/default.html:90-107`），无需新模板或 CSS。

这样根文件仍是唯一内容真源，Pages 文件只是每次构建的临时投影；`permalink` 使最终路由稳定为项目站点下的 `.../BalanceHub/changelog.html`，而 `relative_url` 兼容项目 base path。最少文件变更清单为：

- 必改：`.github/workflows/pages.yml`（触发路径 + 临时投影步骤）；`docs/_layouts/default.html`（可达链接）。
- 按产品可发现性选择：`docs/index.html`（首页文档入口）；`docs/assets/site.css`（仅 A/B 的布局规则）。
- 明确不改/不新增持久副本：根 `CHANGELOG.md` 内容保持不变，不提交 `docs/changelog.md`。

若希望本地预览也复用同一生成逻辑，可把“写 front matter + 追加根文件”的两行逻辑抽成 `.github/scripts/` 脚本，由 workflow 和本地 Jekyll 检查共同调用；这提高可测试性但不是 CI 最小方案。

## Caveats / Not Found

- 本次按要求未运行浏览器、Jekyll 或构建，像素级阈值（尤其 1280px 单行是否刚好容纳四个 chip）仍需实现后渲染确认。
- 生成步骤若只写入 `docs/changelog.md` 而不增加根 `CHANGELOG.md` path filter，根日志更新不会自动发布；这是最容易漏掉的可靠性风险。
- 不要把生成文件提交到仓库：否则它会与根日志漂移并破坏“唯一真源”。CI 生成文件是临时工作区状态，需确保任何本地脚本或检查不会误把它加入提交。
- 根日志未来若加入 front matter，简单拼接会出现重复 front matter；生成脚本可加首行格式校验，当前文件没有此问题。
- 更新日志正文中的相对链接若以后增加，复制到 `/changelog.html` 后的解析上下文可能变化；应使用仓库/Pages 绝对链接或在 Pages 构建检查中验证链接。
