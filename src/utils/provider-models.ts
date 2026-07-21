export interface ProviderModelDisplay {
  name: string;
  group: string;
  vendor: string;
  family: string;
}

export interface ProviderModelGroup {
  key: string;
  label: string;
  models: ProviderModelDisplay[];
}

export interface ProviderModelSelection {
  models: ProviderModelDisplay[];
  groups: ProviderModelGroup[];
  hiddenCount: number;
}

interface ParsedModel extends ProviderModelDisplay {
  version: number[];
  order: number;
}

const KNOWN_FAMILIES: Array<{ label: string; vendor: string; pattern: RegExp }> = [
  { label: "GPT", vendor: "OpenAI", pattern: /(?:^|[-_.])gpt(?:[-_.]|$)|openai/i },
  { label: "Claude", vendor: "Anthropic", pattern: /claude|anthropic/i },
  { label: "Gemini", vendor: "Google", pattern: /gemini|google/i },
  { label: "DeepSeek", vendor: "DeepSeek", pattern: /deepseek/i },
  { label: "Qwen", vendor: "Alibaba", pattern: /qwen|tongyi/i },
  { label: "Grok", vendor: "xAI", pattern: /grok|xai/i },
  { label: "Llama", vendor: "Meta", pattern: /llama|meta/i },
  { label: "Mistral", vendor: "Mistral", pattern: /mistral|mixtral/i },
  { label: "Kimi", vendor: "Moonshot", pattern: /kimi|moonshot/i },
  { label: "GLM", vendor: "Zhipu", pattern: /(?:^|[-_.])glm(?:[-_.]|$)|chatglm|zhipu/i },
];

function cleanModelName(value: string) {
  return value.trim().replace(/\s+/g, " ");
}

function titleCaseSegment(value: string) {
  return value
    .replace(/[-_.]+/g, " ")
    .trim()
    .replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function versionNumbers(value: string) {
  const match = value.match(/(?:^|[-_.])v?(\d+(?:[._-]\d+)*)/i);
  if (!match) {
    return [];
  }
  return match[1]
    .split(/[._-]/)
    .map((part) => Number(part))
    .filter((part) => Number.isFinite(part));
}

function unknownFamily(value: string) {
  const withoutVersion = value
    .replace(/(?:^|[-_.])v?\d+(?:[._-]\d+)*(?:[a-z]+)?/gi, " ")
    .replace(/[-_.]+/g, " ")
    .trim();
  const parts = withoutVersion.split(/\s+/).filter(Boolean);
  return parts.slice(0, 2).join(" ") || value;
}

function parseModel(rawName: string, order: number): ParsedModel {
  const name = cleanModelName(rawName);
  const parts = name.split("/").map((part) => part.trim()).filter(Boolean);
  const explicitVendor = parts.length > 1 ? titleCaseSegment(parts[0]) : "";
  const modelPart = parts.length > 1 ? parts.slice(1).join("/") : name;
  const knownFamily = KNOWN_FAMILIES.find(({ pattern }) => pattern.test(name));
  const vendor = explicitVendor || knownFamily?.vendor || "其他";
  const family = knownFamily?.label || titleCaseSegment(unknownFamily(modelPart));
  const group = `${vendor} / ${family}`;

  return {
    name,
    group,
    vendor,
    family,
    version: versionNumbers(modelPart),
    order,
  };
}

function compareVersions(left: ParsedModel, right: ParsedModel) {
  const length = Math.max(left.version.length, right.version.length);
  for (let index = 0; index < length; index += 1) {
    const leftValue = left.version[index] ?? -1;
    const rightValue = right.version[index] ?? -1;
    if (leftValue !== rightValue) {
      return rightValue - leftValue;
    }
  }

  const byName = right.name.localeCompare(left.name, undefined, {
    numeric: true,
    sensitivity: "base",
  });
  return byName || left.order - right.order;
}

/** Select a compact, representative model set without losing vendor diversity. */
export function selectProviderModels(
  names: string[] | null | undefined,
  limit = 5,
): ProviderModelSelection {
  const unique = new Map<string, string>();
  for (const rawName of names ?? []) {
    const name = cleanModelName(rawName);
    const key = name.toLocaleLowerCase();
    if (name && !unique.has(key)) {
      unique.set(key, name);
    }
  }

  const parsed = [...unique.values()].map((name, index) => parseModel(name, index));
  const grouped = new Map<string, ProviderModelGroup>();
  for (const model of parsed) {
    const existing = grouped.get(model.group);
    if (existing) {
      existing.models.push(model);
    } else {
      grouped.set(model.group, {
        key: model.group,
        label: model.group,
        models: [model],
      });
    }
  }

  const groups = [...grouped.values()]
    .map((group) => ({
      ...group,
      models: [...group.models].sort((left, right) =>
        compareVersions(left as ParsedModel, right as ParsedModel),
      ),
    }))
    .sort((left, right) => compareVersions(left.models[0] as ParsedModel, right.models[0] as ParsedModel));

  const safeLimit = Math.max(0, Math.floor(limit));
  const selected: ProviderModelDisplay[] = [];
  for (const group of groups) {
    if (selected.length >= safeLimit) {
      break;
    }
    selected.push(group.models[0]);
  }

  for (let index = 1; selected.length < safeLimit; index += 1) {
    let added = false;
    for (const group of groups) {
      const model = group.models[index];
      if (!model) {
        continue;
      }
      selected.push(model);
      added = true;
      if (selected.length >= safeLimit) {
        break;
      }
    }
    if (!added) {
      break;
    }
  }

  return {
    models: selected,
    groups,
    hiddenCount: Math.max(0, parsed.length - selected.length),
  };
}
