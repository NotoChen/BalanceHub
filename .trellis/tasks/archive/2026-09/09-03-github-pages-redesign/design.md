# GitHub Pages 产品入口技术设计

## 1. 边界

本轮保留 GitHub Pages + Jekyll 的静态页面形态，不引入 CSS 框架、Web 字体或 JavaScript 组件体系。`docs/_layouts/default.html` 负责共享页面外壳，`docs/assets/site.css` 负责视觉与响应式；首页和现有 Markdown 文档复用同一导航、页脚和基础样式。README 只同步修正动态版本入口和与源码不一致的能力表述。

## 2. 信息架构

页面按用户决策顺序组织，而非按代码模块罗列：

1. 顶部导航：品牌、能力、界面、隐私、文档、GitHub。
2. Hero：一句明确定位、两项主要操作、跨平台/本地优先/支持协议信息，以及真实总览截图。
3. 三步主链路：接入中转站、验证真实可用性、直接进入 Agent CLI。
4. 能力版图：使用非对称 bento 网格承载中转站、Key 库、后台运维和按 Agent/工作目录检索会话的能力。
5. 截图叙事：用交替布局展示总览、设置/Agent、用量与请求日志，而非无说明图片墙。
6. 本地优先：说明凭据、请求、临时文件和更新边界。
7. 下载区：GitHub Releases 动态入口、平台包说明、更新机制。
8. 文档与 FAQ：保留所有现有详细页面入口和 Issues。
9. Footer：仓库、许可证、反馈和技术栈。

## 3. 视觉系统

- 背景：暖白/冷灰正文基底，Hero 使用墨蓝黑而非纯黑。
- 主强调色：与 App 中蓝色操作色协调的蓝青色；橙色仅作小面积状态点缀。
- 字体：系统中文字体栈，标题通过字重、字距和 `text-wrap: balance` 建立层级，不依赖在线字体。
- 表面：大区块以留白和背景层次区分；仅在需要表达结构时使用卡片，避免所有内容统一边框阴影。
- 截图：使用现有真实 App 图片，统一圆角、轻微透视/层叠和有方向的阴影；不裁掉关键 UI。
- 品牌与 Agent 图标：以 `src-tauri/icons/balancehub.svg` 和 `src/assets/logos/{codex,claude,gemini,grok}.svg` 为图形真源，Pages 只保留 Jekyll 可部署的本地资源副本，不另行重绘。
- 动效：首屏和截图仅做轻量位移/透明度过渡；`prefers-reduced-motion` 下完全关闭。

## 4. 响应式策略

- `> 1024px`：Hero 双栏，能力非对称网格，截图文字交替左右。
- `720px–1024px`：缩减间距，Hero 保持双栏或按内容自动换行，能力网格两列。
- `< 720px`：导航压缩为可换行的关键入口，所有主内容单列，按钮全宽可选，截图取消透视，比较/平台信息纵向排列。
- 容器使用 `min()` / `clamp()` / CSS Grid，避免固定宽度与复杂百分比计算。

## 5. 动态版本策略

- 不通过 GitHub API 获取版本，避免限流、加载延迟和 JavaScript 失败状态。
- Pages 不依赖远程版本图片；使用 `/releases/latest` 作为动态最新版入口，避免第三方图片加载失败留下空位。
- 所有文字 CTA 统一指向 `/releases/latest`，不包含具体版本字符串。

## 6. 内容真源与纠偏

- 产品能力以当前 Rust/前端源码和 AGENTS.md 为事实来源，README 与 Pages 是消费者。
- 当前 `src-tauri/src/network/shield/mod.rs` 的闭集仅包含 `AliyunWaf`，且只处理 AnyRouter 等站点常见的确定性 JS 挑战。因此删除 README 中 Cloudflare/通用过盾宣传；Pages 不把过盾作为核心卖点，只在需要时窄化描述统一代理和确定性阿里云 WAF 自动重试。
- 会话检索 IPC 一次绑定一个 Agent 和工作目录，索引排除大量工具噪声；公开文案不得写成全局跨 Agent 原始全文搜索。
- 不改变 CHANGELOG 的历史描述。

## 7. SEO 与可访问性

- `title` 和 description 包含桌面 App、中转站账号、Agent CLI 等关键词。
- canonical 指向 `https://notochen.github.io/BalanceHub/`。
- Open Graph/Twitter 图片使用 Pages 可访问的真实总览截图绝对地址。
- 添加跳至正文、`:focus-visible`、正确 landmark、标题层级和图片替代文本。

## 8. 回滚与兼容

- 改动集中在 `docs/index.html`、共享 layout/CSS、Markdown front matter 与少量 README 文案，可通过单提交回滚。
- 保持现有文档 URL、Pages workflow 和静态资源目录不变，不影响历史链接。
- 页面核心功能不依赖 JavaScript；外部徽章失败时只损失版本图像，不影响下载入口。

## 9. Jekyll 文档路由

- 新增 `docs/_config.yml`，声明项目站点 URL、baseurl、仓库和默认元信息。
- `docs/index.html` 加 front matter 并使用共享 layout，但保留 landing page 专属结构。
- 六个现有 Markdown 文档只增加 title、description、layout 等最小 front matter，不重写正文。
- 共享 layout 使用 Liquid `relative_url` 生成 CSS、图标和站内链接，兼容 `/BalanceHub` 项目子路径。
- 新增本地品牌 mark SVG；不从 raw GitHub 域加载图标。
- `settings.png` 含像素级 `v0.5.9`，本轮 Pages 不使用该图作为关键展示；待后续真实 App 新截图替换，不通过后期伪造修改。

## 10. 最新反馈调整

- Provider Board 改为横跨能力网格的水平信息带：左侧说明、右侧四个站点状态同处普通文档流，不再用绝对定位和跨两行高度制造空白；平板保持两列状态块，手机改为单列。
- Agent CLI 卡片在 Provider Board 下方获得更宽的七列空间，宽桌面保持四项单行；在中等和窄屏恢复自然换行，不能通过裁切或缩写正式名称解决空间问题。
- Pages 的更新记录以根目录 `CHANGELOG.md` 为唯一内容真源。Pages 构建前临时生成带 front matter 的 `docs/changelog.md` 投影，构建后不提交；共享 layout 和首页文档入口均提供 `changelog.html` 链接，并将 `CHANGELOG.md` 纳入 Pages workflow 的触发路径。
