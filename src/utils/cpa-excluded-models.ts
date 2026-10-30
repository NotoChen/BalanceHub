/**
 * 模型排除规则 —— 与 CPA 网关同一套语义。
 *
 * - 大小写不敏感；
 * - 只有 `*` 是通配符，其余字符按字面量处理（`gpt-4.1` 里的 `.` 不是正则通配符）；
 * - 去重按小写键，但保留首次出现的拼写。
 *
 * 面板只在这里推导「某个模型是否被排除」，网关仍是最终真源：保存后它会再归一化一次。
 */

const ruleKey = (value: string): string => value.trim().toLowerCase();

/** 规则里含 `*` 才按通配符匹配，否则是精确的模型 ID。 */
export const isWildcardRule = (rule: string): boolean => rule.includes("*");

/// 网关用 `*` 单独一条表示「停用整个通道」，面板不主动产出它。
export const DISABLE_ALL_RULE = "*";

export function normalizeExcludedRules(values: Iterable<string>): string[] {
  const seen = new Set<string>();
  const rules: string[] = [];
  for (const value of values) {
    const rule = value.trim();
    const key = ruleKey(rule);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    rules.push(rule);
  }
  return rules;
}

export const parseExcludedRulesText = (text: string): string[] =>
  normalizeExcludedRules(text.split(/\r?\n/));

export function matchesExcludedRule(rule: string, modelId: string): boolean {
  const normalizedRule = ruleKey(rule);
  const normalizedModel = ruleKey(modelId);
  if (!normalizedRule || !normalizedModel) return false;
  if (!isWildcardRule(normalizedRule)) return normalizedRule === normalizedModel;
  /// 按 `*` 切段，逐段转义正则元字符，再用 `.*` 接回：只有 `*` 是通配符。
  const escaped = normalizedRule
    .split("*")
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join(".*");
  return new RegExp(`^${escaped}$`, "i").test(normalizedModel);
}

export const isMatchedByWildcardRule = (
  rules: Iterable<string>,
  modelId: string,
): boolean =>
  Array.from(rules).some(
    (rule) => isWildcardRule(rule) && matchesExcludedRule(rule, modelId),
  );

/** 一条规则在原列表里的字面位置（忽略大小写），用于增删精确规则。 */
const sameRule = (left: string, right: string): boolean =>
  ruleKey(left) === ruleKey(right) && ruleKey(left) !== "";

export function toggleExcludedRule(
  rules: Iterable<string>,
  candidate: string,
  excluded: boolean,
): string[] {
  const rule = candidate.trim();
  const next = normalizeExcludedRules(rules).filter(
    (existing) => !sameRule(existing, rule),
  );
  if (excluded && ruleKey(rule)) next.push(rule);
  return next;
}

export interface SplitExcludedRules {
  /** 命中目录的精确规则，改写为目录里的拼写。 */
  exactRules: string[];
  /** 含 `*` 的规则，保留配置里的拼写。 */
  wildcardRules: string[];
  /** 精确但目录里没有的规则（如已下线的模型 ID），保留配置里的拼写。 */
  unknownRules: string[];
  /** `wildcardRules ∪ unknownRules`，保持原始出现顺序。 */
  customRules: string[];
}

/**
 * 把一条通道的规则拆成「目录勾选」和「自定义文本」两半。目录未知时传空数组，
 * 于是所有规则都落进文本，界面仍能完整编辑。
 */
export function splitExcludedRules(
  rules: Iterable<string>,
  candidateIds: readonly string[],
): SplitExcludedRules {
  const candidateByKey = new Map(candidateIds.map((id) => [ruleKey(id), id]));
  const exactRules: string[] = [];
  const wildcardRules: string[] = [];
  const unknownRules: string[] = [];
  const customRules: string[] = [];
  for (const rule of normalizeExcludedRules(rules)) {
    if (isWildcardRule(rule)) {
      wildcardRules.push(rule);
      customRules.push(rule);
      continue;
    }
    const candidate = candidateByKey.get(ruleKey(rule));
    if (candidate) {
      exactRules.push(candidate);
      continue;
    }
    unknownRules.push(rule);
    customRules.push(rule);
  }
  return { exactRules, wildcardRules, unknownRules, customRules };
}

/** 用一段文本整体替换自定义半边（通配符 + 目录外精确规则），保留勾选的那一半。 */
export function replaceCustomExcludedRules(
  rules: Iterable<string>,
  candidateIds: readonly string[],
  text: string,
): string[] {
  const { exactRules } = splitExcludedRules(rules, candidateIds);
  return normalizeExcludedRules([...exactRules, ...parseExcludedRulesText(text)]);
}

/**
 * 单个模型的排除态。`both` 表示既被勾选、又被通配符命中：取消勾选后它依然被排除，
 * 界面需要说出来，否则那一行看起来像点击没生效。
 */
export type ModelExclusionState =
  | { state: "included" }
  | { state: "excluded"; by: "exact" }
  | { state: "excluded"; by: "wildcard"; rule: string }
  | { state: "excluded"; by: "both"; rule: string };

export function getModelExclusionState(
  rules: readonly string[],
  modelId: string,
): ModelExclusionState {
  const modelKey = ruleKey(modelId);
  if (!modelKey) return { state: "included" };
  let hasExact = false;
  let wildcard: string | undefined;
  for (const rule of rules) {
    if (isWildcardRule(rule)) {
      if (wildcard === undefined && matchesExcludedRule(rule, modelId)) {
        wildcard = rule;
      }
    } else if (!hasExact && ruleKey(rule) === modelKey) {
      hasExact = true;
    }
  }
  if (hasExact && wildcard !== undefined) {
    return { state: "excluded", by: "both", rule: wildcard };
  }
  if (hasExact) return { state: "excluded", by: "exact" };
  if (wildcard !== undefined) {
    return { state: "excluded", by: "wildcard", rule: wildcard };
  }
  return { state: "included" };
}

export const isModelExcluded = (
  rules: readonly string[],
  modelId: string,
): boolean => getModelExclusionState(rules, modelId).state === "excluded";

/**
 * 目录内被任意规则命中的模型数。用规则条数当分子会把 `gpt-5-*` 这类规则谎报成 1，
 * 所以分子分母都数模型。
 */
export function summarizeExclusion(
  rules: readonly string[],
  candidateIds: readonly string[],
): { total: number; excluded: number } {
  const total = candidateIds.length;
  const excluded = candidateIds.reduce(
    (count, id) => (isModelExcluded(rules, id) ? count + 1 : count),
    0,
  );
  return { total, excluded };
}

/** 两条规则表是否等价：归一化后逐条比较，顺序不影响结论。 */
export function sameExcludedRules(
  left: readonly string[],
  right: readonly string[],
): boolean {
  const keysOf = (rules: readonly string[]) =>
    normalizeExcludedRules(rules)
      .map(ruleKey)
      .sort()
      .join("\n");
  return keysOf(left) === keysOf(right);
}
