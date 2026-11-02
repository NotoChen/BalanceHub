/// 模型展示规则的输入辅助。规则本身仍由后端求值（glob / `re:` 正则），
/// 这里只处理「从站点模型列表里勾选精确 ID」这一种写法，避免在界面里
/// 再实现一套匹配逻辑。
export function toggleExactModelRule(rules: string[], model: string): string[] {
  const id = model.trim();
  if (!id) return rules;
  return rules.includes(id)
    ? rules.filter((rule) => rule !== id)
    : [...rules, id];
}
