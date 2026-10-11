type DiffLineKind = "context" | "removed" | "added" | "hunk";

interface DiffLine {
  id: string;
  kind: DiffLineKind;
  text: string;
  oldLine: number | null;
  newLine: number | null;
}

interface DiffFileState {
  lines: DiffLine[];
  trailingNewline: boolean;
}

export type DiffDisplayRow =
  | DiffLine
  | {
      id: string;
      kind: "omitted";
      from: number;
      count: number;
    };

/** Fold unchanged ranges without dropping their positions in the full diff. */
export function configurationDiffRows(
  lines: readonly DiffLine[],
  contextLines = 3,
): DiffDisplayRow[] {
  const context = Math.max(0, Math.floor(contextLines));
  const ranges: Array<{ start: number; end: number }> = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (lines[index].kind === "context") continue;
    const start = Math.max(0, index - context);
    const end = Math.min(lines.length, index + context + 1);
    const previous = ranges[ranges.length - 1];
    if (previous && start <= previous.end) previous.end = end;
    else ranges.push({ start, end });
  }

  const rows: DiffDisplayRow[] = [];
  let cursor = 0;
  function omit(until: number) {
    if (until > cursor) {
      rows.push({
        id: `omitted-${cursor}`,
        kind: "omitted",
        from: cursor,
        count: until - cursor,
      });
    }
  }
  for (const range of ranges) {
    omit(range.start);
    for (let index = range.start; index < range.end; index += 1)
      rows.push(lines[index]);
    cursor = range.end;
  }
  omit(lines.length);
  return rows;
}

const MAX_DIFF_MATRIX_CELLS = 250_000;

function splitLines(content: string) {
  const normalized = content.replace(/\r\n/g, "\n");
  const trailingNewline = normalized.endsWith("\n");
  const lines = normalized.split("\n");
  if (trailingNewline) {
    lines.pop();
  }
  return {
    lines: lines.length === 1 && lines[0] === "" ? [] : lines,
    trailingNewline,
  };
}

export function buildConfigurationDiff(
  original: string,
  target: string,
): DiffFileState {
  const before = splitLines(original).lines;
  const after = splitLines(target);
  if (before.length * after.lines.length > MAX_DIFF_MATRIX_CELLS) {
    return buildCoarseDiff(before, after);
  }
  const matrix = Array.from({ length: before.length + 1 }, () =>
    new Array<number>(after.lines.length + 1).fill(0),
  );

  for (let oldIndex = before.length - 1; oldIndex >= 0; oldIndex -= 1) {
    for (let newIndex = after.lines.length - 1; newIndex >= 0; newIndex -= 1) {
      matrix[oldIndex][newIndex] =
        before[oldIndex] === after.lines[newIndex]
          ? matrix[oldIndex + 1][newIndex + 1] + 1
          : Math.max(
              matrix[oldIndex + 1][newIndex],
              matrix[oldIndex][newIndex + 1],
            );
    }
  }

  const lines: DiffLine[] = [];
  let oldIndex = 0;
  let newIndex = 0;
  let sequence = 0;
  while (oldIndex < before.length || newIndex < after.lines.length) {
    if (
      oldIndex < before.length &&
      newIndex < after.lines.length &&
      before[oldIndex] === after.lines[newIndex]
    ) {
      lines.push({
        id: `context-${oldIndex + 1}-${newIndex + 1}-${sequence++}`,
        kind: "context",
        text: after.lines[newIndex],
        oldLine: oldIndex + 1,
        newLine: newIndex + 1,
      });
      oldIndex += 1;
      newIndex += 1;
    } else if (
      oldIndex < before.length &&
      shouldRemoveLine(before, after.lines, matrix, oldIndex, newIndex)
    ) {
      lines.push({
        id: `removed-${oldIndex + 1}-${sequence++}`,
        kind: "removed",
        text: before[oldIndex],
        oldLine: oldIndex + 1,
        newLine: null,
      });
      oldIndex += 1;
    } else {
      lines.push({
        id: `added-${newIndex + 1}-${sequence++}`,
        kind: "added",
        text: after.lines[newIndex],
        oldLine: null,
        newLine: newIndex + 1,
      });
      newIndex += 1;
    }
  }

  return { lines, trailingNewline: after.trailingNewline };
}

function shouldRemoveLine(
  before: string[],
  after: string[],
  matrix: number[][],
  oldIndex: number,
  newIndex: number,
) {
  if (
    newIndex >= after.length ||
    matrix[oldIndex + 1][newIndex] > matrix[oldIndex][newIndex + 1]
  ) {
    return true;
  }
  if (matrix[oldIndex + 1][newIndex] < matrix[oldIndex][newIndex + 1]) {
    return false;
  }
  const oldLineAppearsLater =
    after.indexOf(before[oldIndex], newIndex + 1) !== -1;
  const newLineAppearsLater =
    before.indexOf(after[newIndex], oldIndex + 1) !== -1;
  return !(oldLineAppearsLater && !newLineAppearsLater);
}

function buildCoarseDiff(
  before: string[],
  after: { lines: string[]; trailingNewline: boolean },
): DiffFileState {
  let prefix = 0;
  while (
    prefix < before.length &&
    prefix < after.lines.length &&
    before[prefix] === after.lines[prefix]
  ) {
    prefix += 1;
  }
  let suffix = 0;
  while (
    suffix < before.length - prefix &&
    suffix < after.lines.length - prefix &&
    before[before.length - suffix - 1] ===
      after.lines[after.lines.length - suffix - 1]
  ) {
    suffix += 1;
  }

  const lines: DiffLine[] = [];
  let sequence = 0;
  for (let index = 0; index < prefix; index += 1) {
    lines.push({
      id: `context-${index + 1}-${index + 1}-${sequence++}`,
      kind: "context",
      text: before[index],
      oldLine: index + 1,
      newLine: index + 1,
    });
  }
  for (let index = prefix; index < before.length - suffix; index += 1) {
    lines.push({
      id: `removed-${index + 1}-${sequence++}`,
      kind: "removed",
      text: before[index],
      oldLine: index + 1,
      newLine: null,
    });
  }
  for (let index = prefix; index < after.lines.length - suffix; index += 1) {
    lines.push({
      id: `added-${index + 1}-${sequence++}`,
      kind: "added",
      text: after.lines[index],
      oldLine: null,
      newLine: index + 1,
    });
  }
  for (let index = 0; index < suffix; index += 1) {
    const oldLine = before.length - suffix + index;
    const newLine = after.lines.length - suffix + index;
    lines.push({
      id: `context-${oldLine + 1}-${newLine + 1}-${sequence++}`,
      kind: "context",
      text: before[oldLine],
      oldLine: oldLine + 1,
      newLine: newLine + 1,
    });
  }
  return { lines, trailingNewline: after.trailingNewline };
}

/** Render an existing unified patch without inventing missing file contents. */
export function parseUnifiedDiff(patch: string): DiffFileState {
  const lines: DiffLine[] = [];
  let oldLine: number | null = null;
  let newLine: number | null = null;
  let inHunk = false;
  let oldRemaining: number | null = null;
  let newRemaining: number | null = null;
  for (const [index, text] of patch
    .replace(/\r\n/g, "\n")
    .split("\n")
    .entries()) {
    const hunk = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(text);
    if (hunk) {
      oldLine = Number(hunk[1]);
      newLine = Number(hunk[3]);
      oldRemaining = Number(hunk[2] ?? 1);
      newRemaining = Number(hunk[4] ?? 1);
      inHunk = true;
    } else if (text.startsWith("*** Add File: ")) {
      oldLine = null;
      newLine = 1;
      oldRemaining = newRemaining = null;
      inHunk = true;
    } else if (
      text.startsWith("*** Update File: ") ||
      text === "@@" ||
      text.startsWith("@@ ")
    ) {
      oldLine = null;
      newLine = null;
      oldRemaining = newRemaining = null;
      inHunk = true;
    } else if (text.startsWith("diff --git ") || text.startsWith("*** ")) {
      oldLine = null;
      newLine = null;
      inHunk = false;
    }
    if (
      inHunk &&
      !hunk &&
      (text.startsWith("+") || text.startsWith("-") || text.startsWith(" "))
    ) {
      const kind =
        text[0] === "+" ? "added" : text[0] === "-" ? "removed" : "context";
      lines.push({
        id: `patch-${index}`,
        kind,
        text: text.slice(1),
        oldLine: kind === "added" ? null : oldLine,
        newLine: kind === "removed" ? null : newLine,
      });
      if (kind !== "added" && oldLine !== null) oldLine += 1;
      if (kind !== "removed" && newLine !== null) newLine += 1;
      if (kind !== "added" && oldRemaining !== null) oldRemaining -= 1;
      if (kind !== "removed" && newRemaining !== null) newRemaining -= 1;
      if (oldRemaining === 0 && newRemaining === 0) inHunk = false;
    } else if (text) {
      lines.push({
        id: `patch-${index}`,
        kind: "hunk",
        text,
        oldLine: null,
        newLine: null,
      });
    }
  }
  return { lines, trailingNewline: patch.endsWith("\n") };
}
