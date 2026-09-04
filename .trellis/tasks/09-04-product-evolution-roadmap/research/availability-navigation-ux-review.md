# Research: 可用性决策中心入口与工作区导航 UX 评审

- Query: 评审当前交互稿中的“搜索框内上下文入口 -> 替换主体 -> 返回面板”，判断它是否优于永久并列工作区或大型弹窗，并明确实现边界与后续扩展方式。
- Scope: internal
- Date: 2026-09-04

## Findings

### 1. 明确推荐

推荐采用交互稿的主方向：**在现有搜索区域提供“模型决策”上下文入口，进入后替换中转站面板主体，并通过明确的返回按钮恢复原面板**。

但它必须被实现为真实的工作区模式切换，而不是交互稿中简单切换两个 DOM 区块的 `hidden` 状态。建议把模式建模为 `board | availability`，由 `AppWorkspace` 附近的专用导航状态或 feature composable 管理；中转站搜索、滚动位置、当前决策模型、排序和候选展开状态分别保存。返回面板时应恢复进入前的搜索与滚动上下文。

在当前产品阶段，这一方案同时优于：

- **大型弹窗**：决策中心是需要持续搜索、比较、展开理由并进入 CLI/配置/详情的完整工作流，不是短事务。
- **永久并列工作区/常驻 Tab**：当前只有中转站面板是成熟主界面，为一个新能力长期占用导航空间会过早建立产品层级；搜索区正好是用户从“找一个模型”转向“比较这个模型在哪里可用”的自然语境。

这不是对所有后续功能入口的通用模板。Agent 会话搜索可以复用“搜索语境进入独立工作区”的原则；诊断、自动化规则和接入向导不应继续向搜索框追加按钮。未来达到多主工作区条件时，应统一升级导航，而不是累加局部入口。

### 2. 当前产品结构为何适合主体替换

`AppWorkspace` 当前正好直接编排 `AppTopbar` 和 `ProviderBoard`：`src/components/AppWorkspace.vue:103-133`。搜索状态也只在 `AppWorkspace` 内部维护，再投影为两类 Provider 列表：`src/components/AppWorkspace.vue:53-63`。因此在这一层引入工作区模式，不需要把导航状态抬到 Rust，也不需要把决策中心塞进 `AppOverlays`。

`App.vue` 已经承载大量 Modal、Drawer 和 Overlay：`src/App.vue:88-250`。如果决策中心继续采用大型弹窗，它内部的“启动 CLI、选择 Key、切换 Agent 配置、查看模型/测活详情”又会打开现有弹窗，形成弹窗套弹窗、焦点返回不稳定和关闭语义不清的问题。主体替换可让这些既有事务弹窗保持第二层表面，层级更清楚。

中转站面板本身是可滚动的主内容区，并按自动测活、账户认证、API Key 分段展示：`src/components/ProviderBoard.vue:121-137`、`src/components/ProviderBoard.vue:181-235`。卡片宽度固定在 336px 并自动适应列数：`src/styles/modules/provider-layout.css:12-42`。决策中心则是围绕同一模型横向比较 Provider/Key/证据/额度/测活/Agent 的另一种信息组织方式，不适合继续插入现有卡片分组下方；替换主体能避免两套信息架构在同一滚动面互相稀释。

### 3. 交互稿成立的部分

交互稿把入口放在当前搜索框尾部，并用图标、分隔线和“模型决策”文本表达语境升级：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:3-20`。进入后，同一位置改为“返回 + 模型搜索”，主体切换为决策结果：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:62-81`。这有三个明显优点：

- 用户无需先理解抽象模块名，再寻找一个永久导航页；入口与现有“搜索模型”任务直接相邻。
- 顶栏其余全局动作位置不变，降低切换后的空间迷失。
- 返回按钮与输入语境同时变化，比只改页面标题更能说明当前已经离开卡片面板。

1024px 交互稿中，决策页能够在首屏同时容纳模型、来源新鲜度、筛选和三条候选；每条候选的主要动作仍在行尾，展开解释保持在当前行之内。这种信息密度适合主工作区，不适合常规 Modal。

候选行用“已验证 / 明确支持 / 未验证”区分证据等级，并在选中后展开排序原因：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:85-121`。它符合可用性中心“解释为什么推荐”的核心目标，后续应保留这种就地解释，而不是再打开说明弹窗。

### 4. 当前交互稿不能原样落地的部分

#### 4.1 不能在现有 `label` 内直接增加按钮

真实 `AppTopbar` 的搜索容器是 `<label>`，内部包含输入框和清除按钮：`src/components/AppTopbar.vue:88-108`。继续把“模型决策”按钮放进该 `label` 会让标签点击、输入聚焦和独立按钮形成混合交互区域，也会使键盘和辅助技术语义不够稳定。

实现时应把它改为明确的搜索组合容器，例如 `role="search"` 的 wrapper、独立 input label 和尾部上下文动作；输入框、清除按钮、决策入口都必须保持独立 focus target、可见 focus 和明确 `aria-label`。这属于搜索组件结构调整，不能只通过绝对定位覆盖一颗按钮。

#### 4.2 不能只用 `hidden` 切换，必须定义导航状态

交互稿的 `setMode` 只在 board/decision 两套元素上切换 `hidden`：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:417-431`。产品实现至少要定义：

- 当前 workspace mode。
- 面板搜索与决策模型搜索两个独立 query，禁止复用同一个字符串造成返回后条件丢失。
- 进入前的面板滚动位置和焦点来源。
- 决策请求的 request ID、loading/error/empty 状态和过期结果保护。
- 返回行为：先关闭当前事务弹窗，再返回决策页；只有决策页无 overlay 时，返回按钮或快捷键才回到面板。

该状态是纯前端导航/视图状态，符合 composable 管理工作流、本地状态管理视觉状态的现有规范：`.trellis/spec/frontend/hook-guidelines.md:19-30`、`.trellis/spec/frontend/state-management.md:19-43`。Rust 仍只返回决策事实，不感知当前展示哪个工作区。

#### 4.3 不能写死默认模型

交互稿直接显示 `gpt-5.6-sol`：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:16-20`、`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:63-67`。正式实现不能把任何模型作为默认值写死。

进入决策中心后的初始模型只能来自：用户在可用模型上下文中明确选择的模型，或仍存在于当前动态模型目录中的最近一次选择。否则显示空查询与最近/可选模型建议，不自动选择某个内置模型。普通中转站搜索中的任意字符串也不能直接解释为模型，必须先匹配 Rust 返回的模型事实或由用户确认。

#### 4.4 “刷新数据”语义与顶栏全局刷新冲突

交互稿在保留顶栏刷新图标的同时，在决策标题右侧再提供“刷新数据”：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:23-31`、`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:63-70`。而现有顶栏刷新明确表示“刷新全部中转站和模型列表”：`src/components/AppTopbar.vue:121-131`。

正式产品必须拆清两种行为：

- `重新生成比较`：只根据本地已有事实重新投影，不发起网络请求。
- `更新来源数据`：显式触发现有全局刷新/测活能力，并进入后台任务；不能因打开页面自动执行。

不建议同时展示两个外观相同的刷新图标。首版可保留顶栏全局刷新，在结果区只展示事实生成时间和“重新生成比较”文字动作；若要更新来源，应在动作菜单中明确说明将刷新哪些数据。

#### 4.5 390px 稿件已经出现关键内容裁切

应用允许窗口缩到 376px：`src-tauri/tauri.conf.json:16-19`。交互稿在 560px 以下把候选压成 `1fr auto`，并隐藏证据、额度、测活和 Agent 四列：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:377-408`。现有 390px 截图中，右侧启动按钮仍被视口裁掉，用户只能看到候选名称，几乎失去“决策”所需证据。

窄窗口不能通过简单隐藏列来解决，应切换为两层摘要卡：

- 首行：排名、Provider/Key、证据等级、主动作。
- 次行：额度、最近测活、Agent 三项最小摘要。
- 展开区：完整来源、新鲜度、排序理由和次要动作。

所有候选必须 `min-width: 0` 并在 376px 内无横向溢出。可以降低同屏密度，但不能隐藏决定排序所依赖的全部事实。桌面宽屏继续使用比较表/行式布局，不应为了窄窗口统一退化成卡片海洋。

#### 4.6 不需要交互教学文案

交互稿在面板底部增加“点击搜索框中的‘模型决策’进入按模型比较”：`/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html:59`。入口本身若需要常驻教学文字才能被理解，说明可发现性设计仍未完成。正式界面应依靠清楚的入口标签、tooltip 和首次轻量高亮，不在主面板长期放置功能说明。

### 5. 与三种导航方案的比较

| 维度 | 搜索区上下文入口 + 主体替换 | 永久并列工作区 | 大型弹窗 |
| --- | --- | --- | --- |
| 当前任务匹配 | 最好；从“找模型”自然进入“比较模型” | 一般；先要求用户理解模块结构 | 较差；决策不是短事务 |
| 内容空间 | 使用完整主体，可承载比较与展开 | 使用完整主体 | 被窗口内边距和弹窗层级压缩 |
| 返回上下文 | 可恢复原搜索、滚动和卡片位置 | 切 Tab 后可恢复，但需永久导航状态 | 关闭即可返回，但深层弹窗焦点复杂 |
| 现有架构改动 | 中等；在 `AppWorkspace` 增加模式和独立 feature | 中到高；先设计全局导航体系 | 表面较小，实际会放大 overlay 编排 |
| 当前顶栏压力 | 增加一个与搜索关联的入口 | 需要新增常驻 Tab/侧栏/切换器 | 顶栏压力低，但弹窗压力高 |
| 多功能扩展 | 只能容纳少量搜索相关入口 | 最适合多个同级高频工作区 | 不适合长期工作流扩展 |
| 键盘/无障碍 | 结构调整后可做好 | 容易建立标准 tab/navigation | 多层焦点栈风险最高 |
| 窄窗口 | 需专门重排候选 | 同样需重排候选 | 可用空间更差 |

结论不是“搜索入口永远优于工作区导航”，而是它最适合当前第一个、强搜索语境的决策功能；大型弹窗应排除，永久并列导航暂缓。

### 6. 建议的交互规格

#### 面板模式

- 顶栏左侧仍是中转站搜索；尾部提供单一“模型决策”上下文动作。
- 当用户已输入普通搜索时，入口不应抢占清除按钮或改变搜索结果。点击入口后进入独立模型选择状态，不静默把任意搜索词当模型。
- 进入前记录面板 query、scroll offset 和触发元素；返回后恢复三者并把焦点放回入口。
- 顶栏右侧全局动作维持位置，不随主体切换重新排序。

#### 决策模式

- 搜索区变为“返回面板 + 动态模型选择/搜索”，并有明确的“模型”scope 标识。
- 未选模型时展示模型选择空状态，而不是空表，也不默认网络刷新。
- 选定模型后展示候选总数、事实生成时间、Agent/可用性筛选和候选列表。
- 行点击只控制解释区展开；`启动 CLI`、选择 Key、配置 Agent 和详情入口是独立按钮，避免整行点击直接产生副作用。
- 返回不会取消已经转入后台任务中心的刷新或 CLI 启动，但必须取消/忽略仅属于当前页面的未完成查询结果。

#### 键盘与窗口

- `Tab` 顺序为返回、模型输入/选择、清除、筛选、候选动作；候选解释支持 Enter/Space 展开。
- `Escape` 只在没有上层弹窗/菜单时返回面板，避免与现有 overlay 关闭冲突。
- 在 376、600、930、1100px 四个关键宽度验证；930px 是现有 Topbar 开始换行的断点：`src/styles/modules/topbar.css:646-663`，600px 以下搜索独占一行：`src/styles/modules/topbar.css:665-698`。
- 尊重 `prefers-reduced-motion`，工作区切换最多使用短淡入，不做大面积滑动或位移动画。

### 7. 可扩展性边界

当前交互可以支持一到两个与搜索天然相关的上下文工作流，例如模型决策和会话搜索，但必须通过一个可扩展的 scope/command 入口组织，不能在搜索框右侧并排新增五个文本按钮。

建议以后按以下条件升级：

- 当至少三个独立工作区都成为高频、长期驻留任务，并且用户需要在它们之间反复切换时，引入统一的 workspace switcher（紧凑下拉、命令面板或经过单独设计的侧栏）。
- 升级后，中转站面板、可用性决策、会话工作台可以成为同级 workspace；搜索框随 workspace 改变 scope。
- 自动化规则更接近设置/管理页面；一键诊断是按需工具；中转站接入向导是替换新增流程的分步事务。它们不应为了形式统一被强行提升为永久主工作区。
- 即使以后引入统一 switcher，本次 `board | availability` 状态和独立 feature 组件仍可平滑迁移，不需要重写决策中心。

### 8. 替代方案边界

大型弹窗只适用于候选项的局部详情、配置确认或危险动作确认，不适用于整个决策中心。永久并列工作区只应在多工作区真实成立后引入，不能为预期中的未来功能提前占据界面。

如果实现阶段发现搜索区在 930px 以下无法同时容纳输入、清除和上下文入口，首选退化方式是把“模型决策”缩为带 tooltip 的 route 图标或放入一个明确的搜索 scope 菜单；不应把它移回无关联的右侧全局图标堆，也不应改成首次可见、之后难以找到的卡片内入口。

## Files Found

- `/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-concept.html`：入口、主体替换、候选比较及响应式交互稿。
- `/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-1024.png`：1024px 决策工作区截图，宽屏信息密度成立。
- `/Users/zangbai-wsh/.codex/visualizations/balancehub-product-roadmap/availability-navigation-390.png`：390px 截图，显示候选右侧内容被裁切和关键信息缺失。
- `docs/assets/screenshots/overview.png`：当前真实 App 总览截图，顶栏右侧动作密集、主体为固定宽度 Provider 卡片网格。
- `src/components/AppWorkspace.vue`：当前 Topbar、搜索状态和 ProviderBoard 的编排层。
- `src/components/AppTopbar.vue`：真实搜索容器、全局动作及响应事件。
- `src/components/ProviderBoard.vue`：当前中转站面板分区和卡片事件。
- `src/App.vue`：应用主壳及现有 Modal/Drawer/Overlay 编排。
- `src/styles/modules/topbar.css`：顶栏搜索尺寸、拖拽区域和 930/600px 响应式行为。
- `src/styles/modules/provider-layout.css`：Provider 主体滚动区和 336px 卡片网格。
- `src-tauri/tauri.conf.json`：默认窗口 1100x720，最小窗口 376x480。

## Code Patterns

- 视图编排边界：`AppWorkspace` 直接连接 Topbar 和 ProviderBoard，适合承载工作区模式，见 `src/components/AppWorkspace.vue:103-177`。
- 搜索是当前工作区本地状态，见 `src/components/AppWorkspace.vue:53-63`；决策搜索应另建独立状态，不能复用覆盖。
- 顶栏搜索已有清除动作，见 `src/components/AppTopbar.vue:88-108`；新增入口需要重构为多控件搜索组合而非继续嵌入 label。
- 顶栏已经有刷新、签到、后台任务、CLI、公告、更新、GitHub 和设置，见 `src/components/AppTopbar.vue:113-250`；不能再为每个新模块追加全局图标。
- 顶栏在 930px 换行、600px 让搜索独占一行，见 `src/styles/modules/topbar.css:635-698`；新入口必须在这些断点下验证。
- 应用主壳通过 `overflow: hidden` 和内部 `.content` 滚动管理完整窗口，决策页应复用同类主内容滚动面，而不是额外套全屏 Modal。

## Related Specs

- `.trellis/tasks/09-04-product-evolution-roadmap/prd.md`：确认可用性决策中心是第一项交付，并要求入口、刷新语义和直接操作具备可执行设计。
- `.trellis/spec/frontend/component-guidelines.md`：组件保持有界视觉职责、语义事件、键盘和 loading 状态。
- `.trellis/spec/frontend/hook-guidelines.md`：多步骤状态由 composable 管理，异步请求使用 request ID/取消和最终收口。
- `.trellis/spec/frontend/state-management.md`：区分持久数据、派生显示状态和瞬时导航状态。
- `.trellis/tasks/09-04-product-evolution-roadmap/research/architecture-risk-audit.md`：决策中心采用只读 Rust 投影，前端不自行拼接业务事实。

## External References

本轮没有依赖外部产品或通用 Web 导航规范作结论，判断来自当前 BalanceHub 组件层级、交互稿、真实 App 截图和窗口约束。

## Caveats / Not Found

- 浏览器安全策略禁止直接打开本地 `file://` 交互稿，因此没有进行新的运行态点击录屏；本轮已检查完整 HTML/CSS/脚本，并直接查看现有 1024px、390px 渲染截图。
- 现有 390px 截图已经足以确认窄窗口信息裁切，但正式实现仍需在真实 Tauri App 的 376、600、930、1100px 窗口中做视觉与键盘验收。
- 交互稿没有覆盖 loading、error、empty、超长模型名、候选分页、返回状态恢复和上层弹窗打开时的 Escape 行为；这些必须进入第一项子任务的设计与验收。
- 先前关于永久并列工作区的开放问题已依据本研究收口为“上下文入口触发的独立主体工作区”，并在父任务与首个子任务中写明未来统一 workspace switcher 的升级条件。
