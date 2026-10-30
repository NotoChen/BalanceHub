import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { providerProtocolLabel, providerTransportProtocol } from "../src/utils/provider-display.ts";

const headerPath = fileURLToPath(
  new URL("../src/components/provider-card/ProviderCardHeader.vue", import.meta.url),
);
const stylePath = fileURLToPath(
  new URL("../src/styles/modules/provider-card/base.css", import.meta.url),
);
const sharedCardStylePath = fileURLToPath(
  new URL("../src/styles/modules/workspace-card.css", import.meta.url),
);
const managedStylePath = fileURLToPath(
  new URL("../src/styles/modules/managed-card.css", import.meta.url),
);
const managedBodyPath = fileURLToPath(
  new URL("../src/components/provider-card/ManagedProviderBody.vue", import.meta.url),
);
const bodyPath = fileURLToPath(
  new URL("../src/components/provider-card/ProviderCardBody.vue", import.meta.url),
);
const previewPath = fileURLToPath(
  new URL("../src/components/ProviderModelPreview.vue", import.meta.url),
);
const cardPath = fileURLToPath(
  new URL("../src/components/ProviderCard.vue", import.meta.url),
);
const boardPath = fileURLToPath(
  new URL("../src/components/ProviderBoard.vue", import.meta.url),
);
const workspacePath = fileURLToPath(
  new URL("../src/components/AppWorkspace.vue", import.meta.url),
);
const appPath = fileURLToPath(new URL("../src/App.vue", import.meta.url));

test("API Key cards keep a fixed height without exposing a duplicate default-key row", () => {
  const header = readFileSync(headerPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");
  const card = readFileSync(new URL("../src/components/ProviderCard.vue", import.meta.url), "utf8");
  const sharedCard = readFileSync(new URL("../src/components/workspace-card/WorkspaceCard.vue", import.meta.url), "utf8");

  assert.doesNotMatch(header, /<dt>默认 Key<\/dt>/);
  // Key 卡不再自带一套头部排版：图标、标题、副标题和状态位都走其他卡片的
  // 同名插槽，卡型只决定副标题里写什么（地址与 Key 两行）。
  assert.doesNotMatch(header, /provider-card-api-header/);
  assert.doesNotMatch(header, /workspace-card-logo/);
  assert.doesNotMatch(header, /#default/);
  assert.match(header, /<WorkspaceCardHeader :title="providerHeaderTitle">/);
  assert.match(header, /<template #icon>/);
  assert.match(header, /<template #subtitle>/);
  assert.match(header, /<ProviderCardHeaderMeta/);
  assert.match(header, /provider-card-api-key-name/);
  assert.match(header, /ProviderApiKeySwitcher/);
  assert.match(header, /aria-label="地址"/);
  assert.match(header, /aria-label="当前 API Key"/);
  assert.match(header, /provider-card-api-protocol/);
  assert.doesNotMatch(header, /provider-card-api-protocol-group/);
  assert.match(header, /provider-card-api-transport/);
  assert.match(header, /provider-card-api-endpoint-text/);
  assert.match(header, /providerApiKeyLocalRemark/);
  assert.doesNotMatch(header, /replace\(\/\^https/);
  assert.doesNotMatch(header, /当前调用 · 共/);
  assert.doesNotMatch(header, /apiKeyCount\.value} 把/);
  assert.match(styles, /\.provider-card-api-key-row\s*\{[^}]*overflow:\s*visible;/s);
  assert.match(styles, /\.provider-card-api-key-value\s*\{[^}]*overflow:\s*hidden;/s);
  assert.match(card, /:fixed-height="!showLivenessTimeline"/);
  assert.match(sharedCard, /'workspace-card-standard': fixedHeight/);
  assert.match(readFileSync(sharedCardStylePath, "utf8"), /\.workspace-card-standard\s*\{[^}]*height:\s*312px;/s);
  assert.doesNotMatch(styles, /\.provider-card-api-remark-empty/);
});

test("card marks render without a frame at one shared size", () => {
  const sharedHeader = readFileSync(
    new URL("../src/components/workspace-card/WorkspaceCardHeader.vue", import.meta.url),
    "utf8",
  );
  const sharedStyles = readFileSync(sharedCardStylePath, "utf8");
  const managedStyles = readFileSync(managedStylePath, "utf8");
  // 图标是图形本身，不是徽章：四类卡片共用同一个无框 logo 位，授权卡不再
  // 单独放大图标，中转站卡也不再给站点图标垫白底描边。
  assert.doesNotMatch(sharedHeader, /framed/);
  assert.doesNotMatch(sharedStyles, /workspace-card-logo-framed/);
  assert.doesNotMatch(managedStyles, /\.provider-card-managed \.workspace-card-logo \{/);
  assert.doesNotMatch(managedStyles, /workspace-card-logo-framed/);
});

test("icon, title and status share one top line no matter how many subtitle rows a card has", () => {
  const sharedHeader = readFileSync(
    new URL("../src/components/workspace-card/WorkspaceCardHeader.vue", import.meta.url),
    "utf8",
  );
  const sharedStyles = readFileSync(sharedCardStylePath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  // 头部三样东西按标题首行对齐：中转站卡副标题两行、Key 卡三行，居中对齐
  // 会把图标和状态点各自推到不同高度。
  assert.match(sharedHeader, /<div v-if="\$slots\.meta" class="workspace-card-meta">/);
  assert.match(sharedStyles, /\.workspace-card-brand \{[^}]*align-items:\s*flex-start;/s);
  const meta = sharedStyles.match(/\.workspace-card-meta\s*\{([^}]*)\}/s)?.[1] ?? "";
  const copy = sharedStyles.match(/\.workspace-card-brand-copy\s*\{([^}]*)\}/s)?.[1] ?? "";
  const offset = /padding-top:\s*calc\(\s*\(var\(--workspace-card-icon-box-size\) - var\(--workspace-card-title-size\) \* 1\.25\) \/ 2\s*\);/;
  assert.match(meta, offset);
  assert.match(copy, offset);
  // Key 卡的地址行和账号卡的「协议 · 账号」同一级字号，不是第二个标题。
  const endpoint = styles.match(/\.provider-card-api-endpoint-value\s*\{([^}]*)\}/s)?.[1] ?? "";
  assert.match(endpoint, /font-size:\s*11px;/);
  assert.match(endpoint, /color:\s*var\(--surface-ink-soft\);/);
});

test("Key-scoped cards show the key's own balance without disturbing the fixed height", () => {
  const body = readFileSync(bodyPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  // 额度区块对账号卡与 Key 卡都在：Key 卡没有额度接口时写「未公开」，额度
  // 不受限时写「不限量」，和中转站卡片同一句话，不再按卡型隐藏整块。
  assert.doesNotMatch(body, /quotaVisible/);
  assert.match(body, /<section\s+class="provider-card-quota"/);
  assert.match(body, /return providerQuotaHeadline\(props\.provider\);/);
  assert.match(body, /aria-label="isApiKeyAuth \? 'API Key 额度' : '账户余额'"/);
  assert.match(body, /provider-card-quota-compact/);
  // 模型区和页脚一样是固定层：所有卡型都排三行，行数不由卡型决定。
  assert.doesNotMatch(body, /modelRows|:rows=/);
  assert.match(styles, /\.provider-card-model-list\s*\{[^}]*height:\s*70px;/s);
  assert.doesNotMatch(styles, /-five-rows/);
  assert.match(styles, /\.provider-card-quota-high\s*\{[^}]*--provider-quota-color:\s*#e34d59;/s);
  assert.match(styles, /\.provider-card-quota-compact\s*\{[^}]*gap:\s*5px;/s);
});

test("the section heading already names the protocol, so the card subtitle does not", () => {
  const header = readFileSync(headerPath, "utf8");
  const board = readFileSync(
    new URL("../src/components/ProviderBoard.vue", import.meta.url),
    "utf8",
  );
  const section = readFileSync(
    new URL("../src/components/ProviderSectionCards.vue", import.meta.url),
    "utf8",
  );

  // 分组标题写着 NewAPI / Sub2API / 通用 API Key，副标题再写一遍只是把账号
  // 名挤窄；独立展示卡片时没有分组标题，协议名仍然要写。
  assert.match(header, /providerBoardSectionLabelKey/);
  assert.match(header, /section\.includes\(label\) \? "" : label/);
  assert.match(header, /v-if="providerHeaderSubtitle" class="provider-card-api-protocol"/);
  assert.match(section, /provide\(providerBoardSectionLabelKey/);
  assert.match(board, /<ProviderSectionCards/);
  assert.doesNotMatch(board, /<section v-for="section in providerSections"/);
});

test("model preview fills its rows with full names and folds the rest into +N", () => {
  const styles = readFileSync(stylePath, "utf8");
  const preview = readFileSync(
    new URL("../src/components/ProviderModelPreview.vue", import.meta.url),
    "utf8",
  );

  // 按真实宽度排满：能放下多少名字由脚本量出来，剩下的折成「+N」，
  // 完整列表在模型面板里。
  assert.match(preview, /providerModelChips/);
  assert.match(preview, /useBoundedChipPreview/);
  assert.match(preview, /data-preview-measure-chip/);
  assert.match(preview, /\+\{\{ hidden \}\}/);
  // 固定三行：行数写死在组件里，芯片不够也不收缩，卡片之间始终对齐。
  assert.match(preview, /const MODEL_ROWS = 3;/);
  assert.match(preview, /rows: MODEL_ROWS,/);
  assert.doesNotMatch(preview, /rows\?;/);
  const list = styles.match(/\.provider-card-model-list\s*\{([^}]*)\}/s)?.[1] ?? "";
  assert.match(list, /flex-wrap:\s*wrap;/);
  assert.match(list, /gap:\s*5px;/);
  assert.match(list, /height:\s*70px;/);
  assert.match(list, /overflow:\s*hidden;/);
  // 没有模型时说明文字占住同样的三行，卡片高度不随同步状态跳动。
  assert.match(styles, /\.provider-card-model-empty\s*\{[^}]*height:\s*70px;/s);
  // 芯片观感由卡片通用芯片一处给出，中转站卡不再自带一套几何。
  assert.match(preview, /class="workspace-card-chip"/);
  assert.match(preview, /class="workspace-card-chip workspace-card-chip-more"/);
  const shared = readFileSync(sharedCardStylePath, "utf8");
  const chip = shared.match(/\.workspace-card-chip\s*\{([^}]*)\}/s)?.[1] ?? "";
  assert.match(chip, /border:\s*1px solid var\(--glass-border\);/);
  assert.match(chip, /border-radius:\s*5px;/);
  assert.match(chip, /font-size:\s*10px;/);
  assert.match(chip, /text-overflow:\s*ellipsis;/);
  assert.doesNotMatch(chip, /ui-monospace/);
  // 「+N」沿用同一套几何，只换色，并且不参与压缩。
  const more = shared.match(/\.workspace-card-chip-more\s*\{([^}]*)\}/s)?.[1] ?? "";
  assert.match(more, /flex:\s*0 0 auto;/);
  assert.match(more, /color:\s*#165dff;/);
  assert.doesNotMatch(styles, /\.provider-card-model-(chip|more)\s*\{/s);
  // 量宽层与可见行同字号、同间距，但不参与布局。
  assert.match(styles, /\.provider-card-model-measure\s*\{[^}]*position:\s*absolute;/s);
  assert.match(styles, /\.provider-card-model-measure\s*\{[^}]*visibility:\s*hidden;/s);
});

test("the model row reports the count in the heading and the sync time as a tooltip", () => {
  const preview = readFileSync(
    new URL("../src/components/ProviderModelPreview.vue", import.meta.url),
    "utf8",
  );

  // 标题右侧只报总数或失败原因；同步时刻属于排查信息，放进悬停提示，四类
  // 卡片一视同仁，不再有的写「同步 xx:xx」有的什么都不写。
  assert.match(preview, /"同步失败"/);
  assert.match(preview, /`\$\{total\.value\} 个`/);
  assert.match(preview, /"未同步"/);
  assert.match(preview, /同步于 \$\{props\.syncTime\}/);
  assert.doesNotMatch(preview, /同步 \{\{ syncTime \}\}/);
  // 没有模型时保留一行说明，讲清是未同步、暂无还是加载失败。
  assert.match(preview, /provider-card-model-empty/);
  assert.match(preview, /模型列表未能加载/);
  assert.match(preview, /暂未获取模型列表/);
});

test("the relay footer keeps icon-only actions and one menu per business group", () => {
  const actions = readFileSync(
    new URL("../src/components/provider-card/ProviderCardActions.vue", import.meta.url),
    "utf8",
  );
  const menus = readFileSync(
    new URL("../src/components/provider-card/ProviderCardActionMenus.vue", import.meta.url),
    "utf8",
  );
  const actionStyles = readFileSync(
    new URL("../src/styles/modules/provider-card/actions.css", import.meta.url),
    "utf8",
  );
  const footer = actions.slice(actions.indexOf("<WorkspaceCardFooter"));

  // 底栏是纯图标的一排：签到、刷新、编辑、启停、删除；图标就是图标，
  // 不再附带文字说明。
  assert.match(footer, /<ProviderCheckInAction/);
  assert.match(footer, /<CardIconButton[\s\S]*?tone="refresh"/);
  assert.match(footer, /<CardIconButton[\s\S]*?tone="edit"/);
  assert.match(footer, /<CardIconButton[\s\S]*?tone="danger"/);
  assert.match(footer, /<ProviderCardActionMenus/);
  assert.doesNotMatch(footer, /#label|provider-card-action-button/);
  assert.doesNotMatch(actions, /ProviderCardActionButton|ProviderCardEditMenu/);
  // 其余动作按业务分组各自成菜单，而不是全部收进同一个「编辑」。默认 CLI
  // 切换和中转站卡片一样留在底栏，菜单里不再有第二套切换入口。
  for (const tone of ["copy", "data", "site", "integration"]) {
    assert.ok(
      menus.includes(`tone="${tone}"`),
      `功能菜单缺少 ${tone} 分组`,
    );
  }
  assert.doesNotMatch(menus, /tone="configuration"/);
  assert.ok(menus.includes("ProviderAppImportList"));
  assert.match(menus, /emit\("openUsage", props\.provider\)/);
  assert.match(menus, /emit\("openAvailableModels", props\.provider\)/);
  // 菜单里每一项保留自己的色相：收起来不等于抹掉，颜色是行内唯一的指向。
  assert.match(actionStyles, /\.provider-card-auth-summary\s*\{/);
  for (const icon of [
    "url",
    "invite",
    "usage",
    "logs",
    "liveness",
    "checkin-records",
    "keys",
    "models",
    "password",
  ]) {
    assert.ok(
      actionStyles.includes(`provider-card-action-list button .provider-card-action-icon-${icon}`),
      `菜单图标缺少 ${icon} 色相`,
    );
  }
});

test("subscription windows reuse the OAuth quota bar and the bar never gets a left label", () => {
  const body = readFileSync(bodyPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  // 窗口条直接复用 OAuth / CPA 卡片用的 AccountQuotaBar，同一份组件只有一套读法。
  assert.match(body, /import AccountQuotaBar from "\.\.\/oauth\/AccountQuotaBar\.vue"/);
  assert.match(body, /v-if="quotaWindowRows\.length" class="provider-card-quota-windows"/);
  assert.match(body, /<AccountQuotaBar[\s\S]*?:window="providerQuotaBarWindow\(row\)"/);
  assert.doesNotMatch(body, /provider-card-quota-meter-(label|track|value|ok|warn|high|unlimited)/);
  // 窗口行自己的标签 / 进度条样式已经删掉，只剩钱包比例条复用的数值色。
  assert.doesNotMatch(styles, /\.provider-card-quota-meter-(label|track|group|ok|warn|high|unlimited)/);
  // 钱包比例条只写已用比例和条，左边不再立一列文字。
  assert.match(body, /<div v-else-if="quotaProgressVisible" class="provider-card-progress-row">/);
  assert.doesNotMatch(body, /provider-card-progress-row">\s*<span>[^<]*已用/);
  assert.match(styles, /\.provider-card-progress-row \{([^}]*)\}/s);
  assert.doesNotMatch(styles, /\.provider-card-progress-row > span \{[^}]*min-width/s);
  // 不限量窗口也画出来，否则卡片会退回钱包余额，同一张卡读成两套额度。
  assert.match(body, /const quotaWindowRows = computed\(\(\) => providerQuotaWindowRows\(props\.provider\)\)/);
  // 卡片高度由内容决定：三行模型芯片不能被裁掉，页脚也不靠裁切收口；
  // 额度区块按自身内容排布，页脚靠自身的 margin-top: auto 钉在卡片底部。
  assert.doesNotMatch(styles, /--workspace-card-footer-reserve/);
  assert.doesNotMatch(readFileSync(sharedCardStylePath, "utf8"), /--workspace-card-footer-reserve/);
  assert.match(styles, /\.provider-card-models \{[^}]*flex: 0 0 auto/s);
  assert.doesNotMatch(styles, /\.provider-card-models \{[^}]*overflow:\s*hidden/s);
  assert.match(styles, /\.provider-card-quota,\s*\.provider-card-identity \{\s*flex: 0 0 auto;/);
  assert.match(styles, /\.provider-card\.workspace-card-standard \{[^}]*height: auto;/s);
  // 卡片只在数据过期时提同步时间：一整天以内的刷洗结果不需要一句「3 小时前」，
  // 每张卡都挂一行时间只是把额度标签挤短。说明文字和提示里的时刻由共享组件
  // 给出，四类卡片只有一份实现。
  const stale = readFileSync(
    new URL("../src/components/provider-card/ProviderCardStaleNote.vue", import.meta.url),
    "utf8",
  );
  assert.match(body, /<ProviderCardStaleNote :synced-at="provider\.automation\.lastSyncedAt" \/>/);
  assert.match(stale, /providerStaleSyncNote\(props\.syncedAt\)/);
  assert.match(stale, /最近一次同步：/);
  assert.doesNotMatch(body, /provider-card-sync-age/);
  assert.doesNotMatch(body, /providerSyncStampLabel/);
  // 只有一个余额数字时不挂「账号可用额度」标签，未同步时直接写「未同步」。
  assert.doesNotMatch(body, /等待首次同步/);
  assert.match(body, /value: "未同步"/);
});

test("API Key cards expose transport and provider protocol labels independently", () => {
  assert.equal(providerTransportProtocol("https://example.test/v1"), "HTTPS");
  assert.equal(providerTransportProtocol("http://example.test"), "HTTP");
  assert.equal(providerTransportProtocol("example.test"), "");
  assert.equal(providerProtocolLabel("newApi"), "NewAPI");
  assert.equal(providerProtocolLabel("sub2Api"), "Sub2API");
  assert.equal(providerProtocolLabel("api"), "通用 API Key");
});

test("OAuth and CPA cards read windows like relay cards and keep the footer glued to the card bottom", () => {
  const body = readFileSync(managedBodyPath, "utf8");
  const styles = readFileSync(managedStylePath, "utf8");
  // 窗口条和同步时间都复用中转站卡片的同一套组件与读法，授权卡不再有自己的一套。
  assert.match(body, /import AccountQuotaBar from "\.\.\/oauth\/AccountQuotaBar\.vue"/);
  assert.match(body, /managedQuotaWindowRows\(subscription\.value\?\.windows \|\| \[\]\)/);
  assert.match(body, /<AccountQuotaBar[\s\S]*?:window="providerQuotaBarWindow\(row\)"/);
  // 同步时间和中转站卡片同一读法：只在数据过期时出现，卡片右下角不再挂
  // 一行绝对时刻。四类卡片共用同一个过期说明组件。
  assert.doesNotMatch(body, /更新于 \{\{/);
  assert.match(body, /<ProviderCardStaleNote :synced-at="provider\.automation\.lastSyncedAt" \/>/);
  assert.doesNotMatch(body, /managed-card-synced/);
  assert.doesNotMatch(body, /accountTimestamp/);
  // 授权卡片也不预留页脚高度：正文吃掉剩余高度，页脚靠 margin-top: auto 到底。
  // 底栏的换行、动作组和间距由中转站卡片同一份 actions.css 给出，授权卡不再
  // 单独声明一套。
  assert.doesNotMatch(styles, /--workspace-card-footer-reserve/);
  assert.doesNotMatch(styles, /\.workspace-card-quick-actions \{/);
  assert.doesNotMatch(styles, /\.workspace-card-content \{/);
  const actionStyles = readFileSync(
    new URL("../src/styles/modules/provider-card/actions.css", import.meta.url),
    "utf8",
  );
  assert.match(
    actionStyles,
    /\.provider-card \.workspace-card-quick-actions \{[^}]*border-left: 0;/s,
  );
});

test("cards in one row share a height so their footers line up", () => {
  const layout = readFileSync(
    new URL("../src/styles/modules/provider-layout.css", import.meta.url),
    "utf8",
  );

  // 卡片高度由内容决定，但页脚是最后一层：同一排卡片各自缩在自己内容的高
  // 度上，底栏的图标就会一个高一个低。行内拉伸，页脚靠 margin-top: auto
  // 落到同一条线。
  assert.match(layout, /\.overview-provider-grid \{[^}]*align-items:\s*stretch;/s);
  assert.match(layout, /\.overview-provider-grid > \* \{[^}]*align-self:\s*stretch;/s);
  assert.match(
    readFileSync(sharedCardStylePath, "utf8"),
    /\.workspace-card-footer \{[^}]*margin-top:\s*auto;/s,
  );
});

test("an uncapped window reads 不限 because the bar reads the window itself", () => {
  const bar = readFileSync(
    new URL("../src/components/oauth/AccountQuotaBar.vue", import.meta.url),
    "utf8",
  );
  const body = readFileSync(bodyPath, "utf8");
  const managedBody = readFileSync(managedBodyPath, "utf8");

  assert.match(bar, /quotaWindowUnlimited\(props\.window\)/);
  // 窗口自己带着上限和使用量，卡片不再各传一次读法。
  assert.doesNotMatch(bar, /unlimited\?: boolean/);
  assert.doesNotMatch(body, /:unlimited=/);
  assert.doesNotMatch(managedBody, /:unlimited=/);
});

test("all four card types are cut into the same three layers", () => {
  const styles = readFileSync(stylePath, "utf8");
  const managed = readFileSync(managedStylePath, "utf8");
  const shared = readFileSync(sharedCardStylePath, "utf8");

  // 上分隔线由卡片通用规则给出：中转站、API Key、OAuth 账号、CPA 网关共用
  // 一条，授权卡片只在这一层上叠厂商色的渐变，不再各自声明。
  assert.match(
    styles,
    /\.provider-card > \.workspace-card-header \{[^}]*border-bottom:\s*1px solid var\(--glass-border\);/s,
  );
  // 下分隔线来自 workspace-card 的页脚规则；授权卡片曾经用 border-top: 0
  // 抹掉它，于是同屏两排卡片里只有它在正文和页脚之间没有分界。
  assert.match(
    shared,
    /\.workspace-card-footer \{[^}]*border-top:\s*1px solid var\(--glass-border\);/s,
  );
  assert.doesNotMatch(managed, /\.workspace-card-footer \{[^}]*border/s);
  // 卡片高度跟着内容走，三行模型不被裁掉；312px 是下限而不再是上限。
  assert.match(
    styles,
    /\.provider-card\.workspace-card-standard \{[^}]*height: auto;[^}]*min-height: 312px;/s,
  );
});

test("all four card types render the same model block", () => {
  const managedBody = readFileSync(managedBodyPath, "utf8");
  const relayBody = readFileSync(bodyPath, "utf8");
  const managed = readFileSync(managedStylePath, "utf8");

  // 中转站、API Key、OAuth 账号、CPA 网关卡共用 ProviderModelPreview：标题、
  // 计数、三行芯片和空态只有一套实现，卡型只决定传进来的数据。账号卡和网关
  // 卡各有一处模型区，两处都指向共享组件。
  assert.match(
    managedBody,
    /import ProviderModelPreview from "\.\.\/ProviderModelPreview\.vue"/,
  );
  assert.equal(
    managedBody.match(/<ProviderModelPreview/g)?.length,
    2,
    "授权卡和网关卡都应走共享模型区",
  );
  assert.match(
    relayBody,
    /import ProviderModelPreview from "\.\.\/ProviderModelPreview\.vue"/,
  );
  assert.match(relayBody, /<ProviderModelPreview/);
  // 卡片自己不再画芯片：量宽、折叠、「+N」都收口在共享组件里。
  assert.doesNotMatch(
    managedBody,
    /useBoundedChipPreview|providerModelChips|data-preview-measure-chip/,
  );
  assert.doesNotMatch(managed, /\.managed-card-models\b/);
  // 同步时间对所有卡型都传，卡片正文不再按认证方式决定报不报。
  assert.doesNotMatch(relayBody, /isApiKeyAuth \? modelSyncTime : ""/);
});

test("the model block on every card type reaches the models panel", () => {
  const preview = readFileSync(previewPath, "utf8");
  const card = readFileSync(cardPath, "utf8");
  const board = readFileSync(boardPath, "utf8");
  const workspace = readFileSync(workspacePath, "utf8");
  const app = readFileSync(appPath, "utf8");

  // 共享模型区只发 open，两种正文各自把它转成 models，卡片再统一转成
  // openAvailableModels，一路传到工作区的模型面板。少一环，模型行就是点了
  // 没反应的样子，而且四类卡片里只会坏掉其中一类。
  assert.match(preview, /@click\.stop="emit\('open'\)"/);
  assert.equal(
    card.match(/@models="emit\('openAvailableModels', provider\)"/g)?.length,
    2,
    "授权卡和网关卡两条正文都要把 open 接到 openAvailableModels",
  );
  assert.match(
    board,
    /@open-available-models="emit\('openAvailableModels', \$event\)"/,
  );
  assert.match(
    workspace,
    /@open-available-models="emit\('openAvailableModels', \$event\)"/,
  );
  assert.match(app, /@open-available-models="app\.openAvailableModels"/);
});

test("the gateway body stops at the model chips instead of counting vendors again", () => {
  const body = readFileSync(managedBodyPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  // 「可用授权 N/M」和成功率条已经说明了授权分布，正文再按厂商数一遍只是
  // 把同一件事说两遍，而且胶囊长得和模型芯片一样，容易被读成模型行。
  assert.doesNotMatch(body, /managed-gateway-chips/);
  assert.doesNotMatch(styles, /managed-gateway-chips/);
  // 网关卡只换数据源不换排版：主数字和成功率条走中转站卡片同一组额度区块
  // class 与同一支进度条，不再有自己的一套大字号、进度条和柱状图。
  assert.match(body, /class="provider-card-quota"[\s\S]*?aria-label="网关授权与请求"/);
  assert.match(body, /<div class="provider-card-balance">/);
  assert.match(body, /class="provider-card-progress-row"/);
  assert.match(body, /class="provider-quota-progress"/);
  assert.doesNotMatch(body, /managed-gateway-meter|managed-gateway-trend|managed-gateway-stat/);
  assert.doesNotMatch(body, /\.managed-gateway-meter|\.managed-gateway-trend|\.managed-gateway-stat/);
  assert.doesNotMatch(styles, /managed-gateway-meter|managed-gateway-trend/);
  // 模型芯片下面不再另起一格「可用模型 N」：芯片本身就是模型清单，总数由
  // 「+N」给出，网关摘要只留授权数。
  assert.doesNotMatch(body, /<span>可用模型<\/span>/);
  assert.doesNotMatch(body, /modelCount \?\? "—"/);
  // 页脚本身就在正文盒里，给正文盒加 padding-bottom 会把整排动作按钮从卡片
  // 底边顶开一截（页脚下方多出一条谁都不属于的空白）。正文和页脚之间的间距
  // 由正文列的 gap 给，卡片底边那一层由卡片自身的 padding 给。
  assert.doesNotMatch(styles, /\.provider-card \.workspace-card-content \{[^}]*padding-bottom:/s);
});

test("the gateway menus reach the API Key manager and the switch button stays an arrow", () => {
  const actions = readFileSync(
    new URL("../src/components/provider-card/ProviderCardActions.vue", import.meta.url),
    "utf8",
  );
  const menus = readFileSync(
    new URL("../src/components/provider-card/ProviderCardActionMenus.vue", import.meta.url),
    "utf8",
  );
  const card = readFileSync(
    new URL("../src/components/ProviderCard.vue", import.meta.url),
    "utf8",
  );

  // 菜单发出的事件没有人转发时，点「API Key 管理」不会有任何反应——事件
  // 必须一路送到卡片之外，卡片也要把它接到打开凭据面板的动作上。
  assert.match(menus, /openApiKeyManager: \[provider: Provider\];/);
  assert.match(actions, /openApiKeyManager: \(provider: Provider\) =>\s*emit\("openApiKeyManager", provider\),?/s);
  assert.match(card, /openApiKeyManager: \(provider: Provider\) => emit\("openApiKeyManager", provider\)/);
  assert.match(card, /@manage-api-keys="emit\('openApiKeyManager', \$event\)"/);

  // 网关能同时喂给多个 Agent：这枚按钮的动作是「把默认授权换到本卡」，
  // 画其中一个 Agent 的图标会被读成「点它会启动它」。谁在用由高亮和
  // title 说明，具体目标在菜单里逐条列出。
  const dropdown = actions.slice(
    actions.indexOf("switchControlVisible && !singleSwitchKind"),
  );
  // 只看触发按钮：下拉里的每个目标本来就该画各自 Agent 的图标。
  const trigger = dropdown.slice(0, dropdown.indexOf("</CardIconButton>"));
  assert.match(trigger, /<ArrowLeftRight :size="15"/);
  assert.doesNotMatch(trigger, /<AgentCliIcon/);
});

test("the model chips are the body's last layer and sit on the footer divider", () => {
  const body = readFileSync(managedBodyPath, "utf8");
  const relayBody = readFileSync(bodyPath, "utf8");
  const styles = readFileSync(stylePath, "utf8");

  // 正文不再留一条被 margin-top: auto 顶到底部的说明行：多余的高度留在额度
  // 区块和芯片之间，芯片紧贴页脚分界线，和中转站卡片同一排法。
  assert.doesNotMatch(body, /managed-card-bottom/);
  assert.match(styles, /\.provider-card-models-tail \{[^}]*margin-top: auto;/s);
  // 四个卡型的模型区都挂这个尾行类名，芯片落在同一条线上。
  assert.match(body, /class="provider-card-models-tail"/);
  // 续期异常和「还有 N 项」紧跟额度区块，属于读数的一部分。
  assert.match(body, /class="managed-card-notes"/);
  assert.doesNotMatch(body, /\.managed-card-notes \{[^}]*margin-top:\s*auto;/s);
  // 中转站卡片的模型行本来就是正文最后一层，两个卡型的芯片落在同一条线上。
  assert.match(relayBody, /<ProviderModelPreview/);
});

test("the export menu names what it writes and where", () => {
  const menus = readFileSync(
    new URL("../src/components/provider-card/ProviderCardActionMenus.vue", import.meta.url),
    "utf8",
  );
  const actionStyles = readFileSync(
    new URL("../src/styles/modules/provider-card/actions.css", import.meta.url),
    "utf8",
  );

  // 「添加到应用」不说明加到哪、加的是什么；菜单标题写成导出的结果，面板
  // 顶部一句话说明写入的内容和确认方。
  assert.doesNotMatch(menus, /添加到应用/);
  assert.match(menus, /label="导出到其他应用"/);
  assert.match(menus, /把这张卡的接口地址和 API Key 写入目标应用，由它确认导入。/);
  assert.match(actionStyles, /\.provider-card-panel-hint \{/);
});
