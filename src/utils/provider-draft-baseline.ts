import type { ProviderInput } from "../stores/provider-types";

type JsonObject = Record<string, unknown>;

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/**
 * 把后台同步（Key 轮换、令牌续期）真正改过的字段并入「已保存」基线。
 *
 * `beforeJson` 是同步前的草稿快照，`draft` 是同步后的草稿。只把前后发生
 * 变化的字段写进基线：用户在同一窗口里改过、但后台没有触碰的字段仍然是
 * 未保存修改，关闭时仍会确认。无法读取快照时退回整份草稿，避免把同步结果
 * 误判成用户编辑。
 */
export function adoptSyncedDraftFields(
  baseline: ProviderInput,
  beforeJson: string | null,
  draft: ProviderInput,
): ProviderInput {
  if (!beforeJson) return clone(draft);

  let before: JsonObject;
  try {
    before = JSON.parse(beforeJson) as JsonObject;
  } catch {
    return clone(draft);
  }

  const after = clone(draft) as unknown as JsonObject;
  const base = clone(baseline) as unknown as JsonObject;

  for (const key of Object.keys(after)) {
    if (key === "auth") continue;
    if (JSON.stringify(before[key]) !== JSON.stringify(after[key])) {
      base[key] = after[key];
    }
  }

  // 授权字段逐项比较：同步可能只轮换了令牌，用户手改的另一个字段不能
  // 跟着被当作已保存。
  const beforeAuth = (before.auth ?? {}) as JsonObject;
  const afterAuth = (after.auth ?? {}) as JsonObject;
  const baseAuth = (base.auth ?? {}) as JsonObject;
  for (const field of Object.keys(afterAuth)) {
    if (JSON.stringify(beforeAuth[field]) !== JSON.stringify(afterAuth[field])) {
      baseAuth[field] = afterAuth[field];
    }
  }
  base.auth = baseAuth;
  return base as unknown as ProviderInput;
}
