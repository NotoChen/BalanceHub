import assert from "node:assert/strict";
import test from "node:test";
import type { AccountQuotaWindow } from "../src/stores/oauth-types.ts";
import {
  durationLabel,
  firstQuotaExhaustion,
  nextQuotaReset,
  quotaAlertWindows,
  quotaBurnRate,
  quotaExhaustionLabel,
  quotaTrendSummary,
  quotaWindowPercent,
  quotaWindowUnlimited,
  splitQuotaWindows,
  tokenExpiryLabel,
  tokenExpirySoon,
} from "../src/utils/oauth-display.ts";

function window(patch: Partial<AccountQuotaWindow>): AccountQuotaWindow {
  return {
    id: "window",
    label: "周额度",
    model: null,
    usedPercent: null,
    used: null,
    limit: null,
    unit: null,
    resetAt: null,
    windowSeconds: null,
    history: [],
    ...patch,
  };
}

test("a window percentage prefers the reported value and falls back to used over limit", () => {
  assert.equal(quotaWindowPercent(window({ usedPercent: 42.5 })), 42.5);
  assert.equal(
    quotaWindowPercent(window({ used: 30, limit: 120 })),
    25,
  );
  assert.equal(quotaWindowPercent(window({ used: 30 })), null);
  assert.equal(
    quotaWindowPercent(window({ usedPercent: Number.NaN, used: 5, limit: 10 })),
    50,
  );
});

test("only windows at or above ninety percent count as alerts", () => {
  const windows = [
    window({ id: "ok", usedPercent: 89.9 }),
    window({ id: "gone", usedPercent: 90 }),
    window({ id: "full", usedPercent: 100 }),
  ];
  assert.deepEqual(
    quotaAlertWindows(windows).map((w) => w.id),
    ["gone", "full"],
  );
});

test("the next reset ignores windows that already reset or never report one", () => {
  const now = 1_000_000;
  const past = window({ id: "past", resetAt: now / 1000 - 10 });
  const soon = window({ id: "soon", resetAt: now / 1000 + 60 });
  const later = window({ id: "later", resetAt: now / 1000 + 600 });
  const unknown = window({ id: "unknown" });

  assert.equal(nextQuotaReset([later, past, unknown, soon], now)?.id, "soon");
  assert.equal(nextQuotaReset([past, unknown], now), null);
});

test("model-scoped windows are separated from account-wide ones", () => {
  const shared = window({ id: "shared" });
  const scoped = window({ id: "scoped", model: "Opus" });
  assert.deepEqual(splitQuotaWindows([shared, scoped]), {
    shared: [shared],
    scoped: [scoped],
  });
});

test("token expiry is stated relative to now", () => {
  const now = 1_700_000_000_000;
  assert.equal(tokenExpiryLabel(null, now), "");
  assert.equal(tokenExpiryLabel(now / 1000 - 1, now), "已过期");
  assert.equal(tokenExpiryLabel(now / 1000 + 600, now), "10 分钟后到期");
  assert.equal(tokenExpiryLabel(now / 1000 + 3 * 3600, now), "3 小时后到期");
  assert.equal(tokenExpiryLabel(now / 1000 + 2 * 86_400, now), "2 天后到期");
  assert.equal(tokenExpirySoon(now / 1000 + 3600, now), true);
  assert.equal(tokenExpirySoon(now / 1000 + 2 * 86_400, now), false);
  assert.equal(tokenExpirySoon(null, now), false);
});

test("the trend tooltip reports the observed range, not a rule", () => {
  assert.equal(quotaTrendSummary([]), "");
  assert.equal(
    quotaTrendSummary([{ at: 1, usedPercent: 12.04 }]),
    "",
  );
  assert.equal(
    quotaTrendSummary([
      { at: 1, usedPercent: 12 },
      { at: 2, usedPercent: 47.2 },
    ]),
    "近 2 次刷新 · 最低 12% · 最高 47.2% · 累计 +35.2%",
  );
  assert.equal(
    quotaTrendSummary([
      { at: 1, usedPercent: 40 },
      { at: 2, usedPercent: 30 },
    ]),
    "近 2 次刷新 · 最低 30% · 最高 40% · 累计 -10%",
  );
});

test("burn rate needs two readings far enough apart and a rising window", () => {
  assert.equal(quotaBurnRate([]), null);
  assert.equal(quotaBurnRate([{ at: 0, usedPercent: 10 }]), null);
  // 两次相隔 5 分钟，只能说明刷新间隔，不能外推成趋势。
  assert.equal(
    quotaBurnRate([
      { at: 0, usedPercent: 10 },
      { at: 300, usedPercent: 20 },
    ]),
    null,
  );
  assert.equal(
    quotaBurnRate([
      { at: 0, usedPercent: 10 },
      { at: 3600, usedPercent: 25 },
    ]),
    15,
  );
  // 重置后回落不算消耗速度。
  assert.equal(
    quotaBurnRate([
      { at: 0, usedPercent: 90 },
      { at: 7200, usedPercent: 20 },
    ]),
    null,
  );
});

test("exhaustion is predicted only when it lands before the reset", () => {
  const now = 1_700_000_000_000;
  const base = Math.floor(now / 1000);
  const history = [
    { at: base - 7200, usedPercent: 40 },
    { at: base - 3600, usedPercent: 60 },
  ];
  const windowNow = window({
    usedPercent: 60,
    resetAt: base + 86_400,
    history,
  });
  // 每小时涨 20 个点，从 60% 算起还差 2 小时；最后一次采样在 1 小时前。
  assert.equal(quotaExhaustionLabel(windowNow, now), "按当前速度约 1 小时后耗尽");
  // 同样的用速，但额度会在耗尽前重置，就不提示。
  assert.equal(
    quotaExhaustionLabel({ ...windowNow, resetAt: base + 3600 }, now),
    "",
  );
  // 没有记录足够长的历史时不猜测。
  assert.equal(
    quotaExhaustionLabel({ ...windowNow, history: [] }, now),
    "",
  );
});

test("the most urgent exhaustion is the one reported", () => {
  const now = 1_700_000_000_000;
  const base = Math.floor(now / 1000);
  const slow = window({
    id: "slow",
    label: "周额度",
    usedPercent: 50,
    resetAt: base + 86_400,
    history: [
      { at: base - 7200, usedPercent: 46 },
      { at: base - 3600, usedPercent: 48 },
    ],
  });
  const fast = window({
    id: "fast",
    label: "5 小时",
    usedPercent: 80,
    resetAt: base + 86_400,
    history: [
      { at: base - 7200, usedPercent: 40 },
      { at: base - 3600, usedPercent: 60 },
    ],
  });
  assert.equal(firstQuotaExhaustion([slow, fast], now)?.window.id, "fast");
  assert.equal(firstQuotaExhaustion([], now), null);
});

test("durations read the same way everywhere", () => {
  assert.equal(durationLabel(0), "即将");
  assert.equal(durationLabel(600), "10 分钟");
  assert.equal(durationLabel(7200), "2 小时");
  assert.equal(durationLabel(3 * 86_400), "3 天");
});

test("a window with no cap at all reads as unlimited, not as missing data", () => {
  // Grok 的月额度窗口只有名字和重置时间：没有比例，也没有上限，说「未提供
  // 比例」等于把一条不限量的额度报成数据缺失。
  assert.equal(quotaWindowUnlimited(window({})), true);
  assert.equal(
    quotaWindowUnlimited(window({ used: 0, limit: 0 })),
    true,
  );
  // 有比例就不是不限量。
  assert.equal(
    quotaWindowUnlimited(window({ usedPercent: 0 })),
    false,
  );
  // 有上限但缺使用量：仍然是有上限，卡片该写「未提供比例」。
  assert.equal(quotaWindowUnlimited(window({ limit: 200 })), false);
});
