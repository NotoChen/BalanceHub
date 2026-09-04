# Research: GitHub Pages redesign audit

- Query: 对比新版 README 与当前 GitHub Pages，形成无新依赖、纯静态交付的重构方案和可验收标准。
- Scope: internal + limited live verification
- Date: 2026-09-03

## Files found

- `README.md` — 当前公开项目入口，已经形成“产品定位 → 30 秒流程 → 适用人群 → 截图 → 安装 → 能力 → 对比 → 隐私 → 文档”的完整叙事。
- `docs/index.html` — 当前 Pages 单页首页，内联 CSS，内容仍是早期“六张功能卡 + 边界 + 截图 + 下载 + 技术栈 + 文档 + FAQ”结构。
- `docs/assets/screenshots/overview.png` — 最新主面板全景，最适合作为 Hero 的产品实物主视觉。
- `docs/assets/screenshots/settings.png` — Agent 与终端设置截图，画面中直接包含 `v0.5.9`，属于会随发布过期的像素级版本号。
- `docs/assets/screenshots/usage-trends.png`、`request-logs.png`、`checkin-records.png` — 数据能力辅助截图；当前 Pages 把用量趋势图当作整幅 Hero 背景。
- `docs/assets/screenshots/context-menu-expanded.png`、`provider-editor.png` — 未被新版 README 使用，视觉与当前产品形态是否同步需在采用前重新确认。
- `.github/workflows/pages.yml` — `main` 上 `docs/**` 变化触发，使用 GitHub Pages 官方 Jekyll 构建与部署 Action，无需增加前端构建工具。
- `.trellis/tasks/09-03-github-pages-redesign/prd.md` — 目标已确定，但 Requirements 和 Acceptance Criteria 仍待本审计转化。

## Findings

### 1. README 与 Pages 已经不是同一代产品表达

- README 的核心叙事是三个连续动作：接入中转站、用本机 Agent CLI 做真实测活、直接启动或恢复 CLI 会话（`README.md:33-50`）。这比 Pages Hero 中罗列余额、签到、日志、API Key 和四个 Agent 名称更易理解（`docs/index.html:404-424`）。
- README 已覆盖多 Key、Agent 独立绑定、会话检索、后台任务和站点公告等当前差异化能力（`README.md:109-138`）；Pages 的六张等权卡片仍停留在“中转站管理、余额、签到、测活、工具、通知”的浅层罗列（`docs/index.html:429-459`）。
- Pages 当前先展示“核心能力”，再展示“能力边界”，没有解释用户为何需要它，也没有建立从痛点到结果的路径。新版页面应继承 README 的产品定位和事实，但不复制 README 的全部篇幅。

### 2. 当前视觉无法承担产品首页角色

- Hero 把 `usage-trends.png` 作为全幅背景，并覆盖从深到浅的遮罩（`docs/index.html:94-109`）。桌面实测中右半部分几乎变成灰白空区，背景图的数据文字既不可读又干扰标题；移动端则放大并裁切图表，标题和真实数据互相叠压。
- 页面使用 Inter/system 字体、青绿色主色、六个等宽白卡、统一 8px 圆角（`docs/index.html:9-35, 213-237`），与 App 截图里的蓝色主操作、明亮中性底色和品牌彩色图标不一致，也呈现明显的通用模板感。
- Hero 没有展示主面板实物；真正最能解释 BalanceHub 的 `overview.png` 只出现在 README，Pages 完全没有使用。
- 页面总长在桌面约 3831px、390px 移动端约 6219px。移动端没有横向溢出，但顶部七个导航链接换成三行，使固定头部达到约 135px；Hero 仍保持约 510px，首屏信息密度和导航占用都不理想。
- 截图区只有图片，没有 `figcaption`，用户无法在扫视时理解各截图代表什么（`docs/index.html:487-505`）。

### 3. 文档导航当前存在发布级故障

- `docs/index.html` 导航和文档卡均指向 `getting-started.html`、`provider-config.html`、`liveness.html` 等生成页面（`docs/index.html:391-398, 552-585`）。
- 线上抽查首页返回 200，但上述三个 `.html` 链接均返回 404。当前 Markdown 文档没有 YAML front matter，Jekyll 没有为这些入口生成预期页面。
- 这是本轮应优先于视觉优化修复的 P0：要么给 Markdown 增加 front matter 并提供共享 layout，要么把 Pages 首页链接改到真实存在的 GitHub Markdown URL。推荐前者，因为现有 workflow 已经使用 Jekyll，新增 layout/CSS 不会增加项目依赖，且能维持站内阅读体验。

### 4. 版本号不应再进入人工同步链路

- README 正文没有写死当前版本；Release badge 使用 GitHub Shields 动态读取最新 Release（`README.md:16-18`），正文 CTA 也统一指向 `/releases/latest`（`README.md:22-27, 80-88`），这是正确方式。
- 真正写死的版本在 `settings.png` 像素中。PNG 无法动态更新，因此 Pages 不应把带具体版本角标的截图作为关键文案来源。可优先使用 `overview.png`，设置截图若继续展示，应重新从真实 Tauri App 截取不含版本角标的构图，或在容器中做不破坏内容的裁切；不得后期伪造 UI。
- Pages 若希望展示版本，只使用“最新稳定版”文字链接或动态 Shields badge，不在 HTML 写 `0.x.y`，也不在 workflow 用 `package.json` 注入版本。后者会把 tag、main 分支和 Pages 发布时序重新耦合，并要求修改触发路径。

### 5. 已失效或不宜同步到 Pages 的宣传

- 不得同步 OAuth 登录、WebView 账号复用、Linux DO/GitHub 账号托管等能力。本轮相关产品功能已经撤回，内部类型中的预留值也不构成用户可用能力。
- README 当前仍把“阿里云 WAF 与 Cloudflare 过盾”作为通用能力和核心差异点（`README.md:120, 137, 149, 175`）。这类表述超出了当前可稳定承诺的范围，尤其 Cloudflare/WebView 人工验证能力已移除。Pages 应完全不宣传“过盾”；如确需描述 AnyRouter，只能在详细兼容性文档中按事实写窄范围兼容，不能写成所有站点、所有盾的解决方案。
- 不建议把 README 的完整第三方产品对比表同步到 Pages。它依赖外部项目持续变化，维护成本高，也会让首页从“解释自身价值”变成“维护竞品事实”。Pages 只需一段“适合谁 / 不适合谁”的自我边界。
- 不同步仓库目录树、Rust 文件路径、CI 发布前检查、精确调度周期、完整 FAQ 和实现细节。这些内容属于 `docs/` 参考页；首页只提供准确摘要和入口。
- 不使用“开箱即用地处理复杂网络和有盾站点”“全自动通过验证”等不可稳定验收的宽泛宣传。

## Recommended information architecture

页面建议保持单页产品入口，详细说明由 Jekyll 文档页承接。顺序如下：

1. **顶部导航**
   - 左侧：本地 App 图标 + BalanceHub。
   - 中部：`工作方式`、`核心能力`、`界面`、`安装`、`文档`，均为首页锚点或真实站内页面。
   - 右侧：GitHub 图标入口 + 主按钮“下载最新版”。
   - 移动端使用 CSS 原生 `<details>` 菜单或紧凑的单行可展开导航，不让全部链接常驻换行。

2. **Hero：一句定位 + 真实产品实物**
   - 标题示例：`把多个 AI 中转站，收进一个本地桌面面板。`
   - 副文案只说明集中观察、真实 CLI 验证、直接启动三件事，不罗列所有功能名。
   - CTA：`下载最新版`、`30 秒开始使用`；附 macOS / Windows / Linux 和 x64 / ARM64 的简洁支持信息。
   - 右侧或下方使用 `overview.png` 的完整可读窗口画面，不能再当作低可读性的铺底背景。

3. **三步工作方式**
   - `接入`：粘贴地址，识别 NewAPI / Sub2API / 通用 API。
   - `验证`：使用本机 Agent CLI 做真实测活。
   - `使用`：从卡片启动 CLI 或恢复历史会话。
   - 用一条连续路径表达，不做三张完全相同的营销卡。

4. **非对称能力网格**
   - 大模块：中转站总览；Agent CLI 与会话；多 API Key 与独立绑定。
   - 次模块：余额/用量/日志；签到与自动调度；后台任务/公告/通知；统一代理与本地存储。
   - 采用 12 列 CSS Grid 或 `grid-template-areas` 的 2+1 布局，避免六张等权卡片。

5. **界面实物**
   - 先放一张大幅 `overview.png`。
   - 再用不对称 2 列布局展示 Agent 设置、用量趋势、请求日志、签到记录；每张都有标题和一句说明。
   - 仅采用确认仍与当前 App 一致的真实截图；不使用旧的 `context-menu-expanded.png` / `provider-editor.png` 充数。

6. **本地优先与明确边界**
   - 只保留四个可验证事实：凭据存本机、请求本机直发、桌面 App、只支持三类协议。
   - 补充“不提供 Web 自部署”和“通用 API Key 不等同账号登录”的边界。
   - 不出现 OAuth、Cloudflare、WebView 或泛化过盾承诺。

7. **下载区**
   - 一个主 CTA 指向 Releases Latest。
   - 三个平台以简洁行项目展示包格式；不在静态页猜测用户平台或直接绑定具体 asset 名称。
   - “最新版本”可使用动态 badge，但不把具体版本号写进正文。

8. **文档入口**
   - 快速开始、中转站配置、CLI 测活、功能与架构、发布与更新、FAQ。
   - 入口必须落到可访问的站内 Jekyll 页面；每个卡片只说明用户能解决什么问题，不写源码路径。

9. **简短 FAQ 与页脚**
   - 首页只留 3 个最高频问题：系统未知开发者、测活是否消耗额度、API Key 额度含义。
   - 页脚包含 GitHub、Issues、License、文档和项目边界；不要做多列链接农场。

## Visual system

### Direction

- 以 App 的浅色工具界面为基底：暖白/冷灰背景、深墨色正文、BalanceHub 蓝色作为唯一主操作色。
- 品牌图标的彩色轨道只用于 Hero 光晕、细线或小面积强调，不用大面积紫蓝渐变，不让页面变成通用“AI 营销站”。
- 页面应像桌面工具的产品陈列：真实、精确、克制；截图是主角，装饰不能盖过产品内容。

### Suggested tokens

- `--canvas: #f6f8fc`
- `--surface: #ffffff`
- `--surface-muted: #eef2f8`
- `--ink: #141821`
- `--muted: #667085`
- `--line: #dfe5ef`
- `--primary: #165dff`
- `--primary-strong: #0f48d7`
- 状态绿/橙/红仅用于对应真实语义，不作为章节装饰色。
- 圆角分级：大截图框 24px、内容区 16px、按钮 10-12px、小标签 8px；避免所有组件同一圆角。

### Typography and spacing

- 不引入 Web Font。使用 `ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Microsoft YaHei", sans-serif`，规避当前“声明 Inter 但未加载”的不确定行为。
- Hero 标题 `clamp(2.75rem, 6vw, 5.5rem)`，紧字距、约 1.02 行高；正文宽度控制在约 62-68 个中文字符视觉宽度。
- 内容容器最大宽度 1200-1280px；章节之间用 88-120px 的留白，移动端降为 56-72px。
- 数字、版本 badge、平台架构使用 `font-variant-numeric: tabular-nums` 或系统等宽栈。

### Motion

- 仅使用 `transform` 和 `opacity` 做 180-260ms 的 hover/focus 反馈；截图可有非常轻的上浮或边缘高光。
- 不做滚动劫持、无限背景动画、复杂光标追踪或依赖 JavaScript 的入场动画。
- `@media (prefers-reduced-motion: reduce)` 下关闭平滑滚动和所有非必要过渡。

## Static implementation shape

- 保留现有 Pages workflow 和 Jekyll 构建，不新增 npm 包、CSS 框架或前端运行时。
- 将 `docs/index.html` 的大段内联 CSS 移到 `docs/assets/site.css`，首页保持语义 HTML。
- 为 `docs/*.md` 增加最小 YAML front matter，并新增共享 `docs/_layouts/default.html`；首页与文档页复用品牌头部、页脚和 CSS，避免每页复制导航。
- 所有图标优先使用已有本地图标或少量内联 SVG；不要依赖远程 `raw.githubusercontent.com` 图标。当前远程图标会多一次跨域请求，也使 Pages 在 raw 域不可用时丢失品牌标识（`docs/index.html:384-389`）。
- 图片使用本地相对路径，首屏主图可 `fetchpriority="high"`；首屏以下图片使用 `loading="lazy" decoding="async"` 并明确 `width`/`height` 或 `aspect-ratio`，防止布局跳动。
- 页面不需要常驻 JavaScript。移动导航优先 `<details>`；动态版本通过 Release Latest 链接或 Shields 图片实现。

## Responsive acceptance criteria

### Desktop (>= 1024px)

- Header、Hero 和内容容器在 1440/1920px 下不无限拉宽，正文行长稳定。
- Hero 为 5:7 或 6:6 双栏，主截图完整可辨，标题/CTA 不覆盖截图数据。
- 能力区呈非对称网格，截图区大图与辅图有明确主次，不出现六张同质等宽卡。

### Tablet (768-1023px)

- Hero 可切换为上下布局，主图保持不低于约 16:10 的可读比例。
- 能力网格改为两列；导航仅保留关键入口，不能出现三行链接。
- 任意截图、卡片和按钮不造成横向滚动。

### Mobile (320-767px)

- 在 320、375、390、430px 宽度检查 `documentElement.scrollWidth === innerWidth`。
- Header 高度保持约 56-64px，菜单展开前只显示品牌、下载和菜单控制。
- Hero 标题不与截图重叠；CTA 在 320px 下可纵向排列且铺满可用宽度。
- 正文至少 16px，说明文字至少 14px；交互目标实际尺寸建议不小于 44×44px。
- 截图按单列展示，`figcaption` 可见；不得为了塞入双列而把 App 文案缩到不可读。

## Accessibility acceptance criteria

- 页面首个可聚焦元素是“跳到主要内容”的 skip link；结构使用 `<header>`、`<nav>`、`<main id="main">`、`<section>`、`<footer>`。
- 只有一个 `h1`，章节按 `h2`，卡片按 `h3`，不跳级。
- 所有链接和按钮有清晰名称；纯装饰 SVG `aria-hidden="true"`，信息型截图提供具体 alt 和可见 caption。
- 所有键盘可操作元素都有明显 `:focus-visible`，不能只依赖 hover；焦点环与背景保持可辨。
- 普通文本对比度至少 4.5:1，大文本至少 3:1；不能用浅灰文字承载关键说明。
- 颜色不是唯一状态表达；平台、能力、成功/异常都同时有文字。
- 尊重 `prefers-reduced-motion`；页面在禁用动画时内容和交互不受影响。
- 200% 浏览器缩放下不丢内容、不遮挡 CTA、不出现双向滚动。

## SEO and sharing acceptance criteria

- `<title>` 使用可检索标题，如 `BalanceHub — AI 中转站与 Agent CLI 桌面管理工具`，不是只有产品名。
- `meta description` 明确 NewAPI/Sub2API、Agent CLI、本地桌面和三端支持，控制在约 120-160 个字符。
- 增加 canonical、Open Graph (`og:title/description/type/url/image`) 和 Twitter card；社交图使用专门的 1200×630 本地图片，不直接拿超宽 App 截图硬裁。
- 增加本地 favicon / apple-touch-icon、`theme-color`；不从 raw GitHub 地址加载品牌图标。
- 链接文字应描述目的，避免多个只写“查看详情”的同名链接。

## Validation checklist

- 内容：`rg -ni 'oauth|cloudflare|webview|过盾' README.md docs`，公开文案中不得残留已撤回能力；若保留 AnyRouter 的窄范围说明，必须限定位置和语义。
- 版本：`rg` 检查 README/HTML 正文不存在当前 `0.x.y`；人工检查图片中是否仍出现会过期的版本角标。
- 构建：运行与 workflow 一致的 Jekyll Pages 构建，确认首页和六个文档入口均实际生成。
- 链接：本地和部署后逐一检查导航、文档、GitHub、Issues、License、Releases Latest，所有预期入口返回 200。
- 桌面视觉：1440×900 与 1920×1080 截图检查 Hero、截图清晰度、网格节奏和正文行长。
- 响应式：1024×768、768×1024、390×844、320×568 检查无横向溢出、导航可用、按钮可点、截图可读。
- 无障碍：仅键盘完成导航与 CTA；检查 focus visible、heading outline、alt/caption、reduced motion、200% zoom 和颜色对比。
- 性能：不新增 JS 框架；首屏只有必要的品牌资源与 Hero 图，非首屏截图 lazy-load；避免远程字体和 raw GitHub 图片请求。
- 发布：修改 `docs/**` 后确认 Pages workflow 成功；README 单独变化不应假定会自动同步 Pages 文案，两者通过验收清单保持事实一致。

## External references

- GitHub Docs — Adding content to a GitHub Pages site using Jekyll: Markdown/HTML 需要 YAML front matter 才能使用页面变量和 layout。<https://docs.github.com/en/pages/setting-up-a-github-pages-site-with-jekyll/adding-content-to-your-github-pages-site-using-jekyll>
- WCAG 2.2 — Contrast Minimum: 普通文本 4.5:1，大文本 3:1。<https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html>
- WCAG 2.2 — Focus Visible: 键盘操作必须有可见焦点指示。<https://www.w3.org/WAI/WCAG22/Understanding/focus-visible.html>
- MDN — `prefers-reduced-motion`: 根据用户系统设置减少非必要动态效果。<https://developer.mozilla.org/en-US/docs/Web/CSS/@media/prefers-reduced-motion>

## Related specs

- `.trellis/spec/frontend/component-guidelines.md` — 复用现有视觉资源，交互需有可访问标签、键盘操作和明确状态。
- `.trellis/spec/frontend/quality-guidelines.md` — 文档/配置改动至少运行 `git diff --check`，不得引入无意义依赖或遗留死实现。
- `AGENTS.md` — README 保持入口风格；详细说明进入 `docs/`；Pages 与 README 截图和文案同步；截图必须来自真实 Tauri App 且不得包含真实凭据。

## Caveats / Not Found

- 本审计没有重画或生成截图。`settings.png` 的版本角标只能通过真实 App 重新截图或调整展示构图解决，不能通过 HTML 动态替换 PNG 像素。
- `context-menu-expanded.png` 和 `provider-editor.png` 是否仍代表当前 UI 未在本轮启动 App 验证，默认不得进入新版 Pages。
- 未建议同步 README 的竞品对比，避免 Pages 形成高维护、易过期的第三方事实库。
