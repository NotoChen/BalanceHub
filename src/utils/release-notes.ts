export type ReleaseNoteInlineKind = "text" | "code" | "strong";

export interface ReleaseNoteInline {
  kind: ReleaseNoteInlineKind;
  value: string;
}

export interface ReleaseNoteSection {
  title: string;
  items: ReleaseNoteInline[][];
  paragraphs: ReleaseNoteInline[][];
}

const INLINE_MARKDOWN_PATTERN = /(`[^`\n]+`|\*\*[^*\n]+\*\*|\[[^\]\n]+\]\([^)\n]+\))/g;
const LIST_ITEM_PATTERN = /^\s*(?:[-*+]|\d+[.)])\s+(.+?)\s*$/;
const HEADING_PATTERN = /^(#{1,6})\s+(.+?)\s*$/;

function parseInline(value: string): ReleaseNoteInline[] {
  const segments: ReleaseNoteInline[] = [];
  let cursor = 0;

  for (const match of value.matchAll(INLINE_MARKDOWN_PATTERN)) {
    const index = match.index ?? 0;
    if (index > cursor) {
      segments.push({ kind: "text", value: value.slice(cursor, index) });
    }

    const token = match[0];
    if (token.startsWith("`")) {
      segments.push({ kind: "code", value: token.slice(1, -1) });
    } else if (token.startsWith("**")) {
      segments.push({ kind: "strong", value: token.slice(2, -2) });
    } else {
      const label = token.match(/^\[([^\]]+)]/)?.[1] ?? token;
      segments.push({ kind: "text", value: label });
    }
    cursor = index + token.length;
  }

  if (cursor < value.length) {
    segments.push({ kind: "text", value: value.slice(cursor) });
  }

  return segments.length > 0 ? segments : [{ kind: "text", value }];
}

function isVersionHeading(title: string, version: string) {
  const headingVersion = title.replace(/^v/i, "").split(/\s+-\s+/, 1)[0]?.trim();
  return headingVersion === version.replace(/^v/i, "").trim();
}

export function parseReleaseNotes(markdown: string, version: string): ReleaseNoteSection[] {
  const sections: ReleaseNoteSection[] = [];
  const paragraphLines: string[] = [];
  let currentSection: ReleaseNoteSection | null = null;

  function ensureSection(title = "") {
    if (!currentSection) {
      currentSection = { title, items: [], paragraphs: [] };
      sections.push(currentSection);
    }
    return currentSection;
  }

  function flushParagraph() {
    if (paragraphLines.length === 0) return;
    const paragraph = paragraphLines.join(" ").replace(/\s+/g, " ").trim();
    paragraphLines.length = 0;
    if (paragraph) {
      ensureSection().paragraphs.push(parseInline(paragraph));
    }
  }

  const lines = markdown.replace(/\r\n?/g, "\n").split("\n");
  for (const line of lines) {
    const headingMatch = line.match(HEADING_PATTERN);
    if (headingMatch) {
      flushParagraph();
      const level = headingMatch[1].length;
      const title = headingMatch[2].trim();

      if (level <= 2 && title === "安装包说明") {
        break;
      }
      if (level <= 2 && isVersionHeading(title, version)) {
        currentSection = null;
        continue;
      }
      if (level <= 3) {
        currentSection = { title, items: [], paragraphs: [] };
        sections.push(currentSection);
        continue;
      }
    }

    const listItemMatch = line.match(LIST_ITEM_PATTERN);
    if (listItemMatch) {
      flushParagraph();
      ensureSection().items.push(parseInline(listItemMatch[1].trim()));
      continue;
    }

    if (!line.trim()) {
      flushParagraph();
      continue;
    }

    paragraphLines.push(line.trim());
  }

  flushParagraph();
  return sections.filter((section) => section.items.length > 0 || section.paragraphs.length > 0);
}
