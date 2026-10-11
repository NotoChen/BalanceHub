/** Text-only segments: the same search terms highlight list snippets and raw blocks. */
export function sessionHighlight(value: string, terms: readonly string[]) {
  const unique = [
    ...new Set(terms.map((term) => term.trim()).filter(Boolean)),
  ].sort((a, b) => b.length - a.length);
  if (!unique.length) return [{ text: value, matched: false }];
  const pattern = new RegExp(
    `(${unique.map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")).join("|")})`,
    "giu",
  );
  return value
    .split(pattern)
    .map((text, index) => ({ text, matched: index % 2 === 1 }))
    .filter((part) => part.text.length > 0);
}
