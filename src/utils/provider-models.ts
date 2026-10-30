export interface ProviderModelChips {
  /// 排好序的模型名：先按「模型标识」覆盖一遍，再回头补同一标识的其他版本。
  /// 卡片按真实宽度裁到自己的行数，因此顺序决定用户看到的是不是「每个厂商/
  /// 型号一条」，而不是同一家族的一串版本号。
  names: string[];
  /// 去重后的模型总数，芯片行末尾的「+N」用它减去已展示的数量。
  total: number;
}

interface ParsedModel {
  name: string;
  vendor: string;
  family: string;
  version: number[];
  /// 版本号之后还挂着几个修饰词。同一版本里基础型号（修饰词最少）先出现，
  /// gpt-5.6 排在 gpt-5.6-luna 前面。
  variant: number;
  order: number;
}

const TOKEN_SPLIT = /[\s._-]+/;

function cleanModelName(value: string) {
  return value.trim().replace(/\s+/g, " ");
}

function versionOf(tokens: string[]) {
  return tokens.flatMap((token) =>
    Array.from(token.matchAll(/\d+/g), (match) => Number(match[0])),
  );
}

/// 模型名的读法：`vendor/model` 里的 `vendor` 是厂商；剩下的部分里，第一个
/// 带数字的词之前的词就是模型标识（claude-opus、gpt、deepseek），带数字的
/// 词开始是版本号，版本号之后的词是修饰词（luna、mini）。
function parseModel(rawName: string, order: number): ParsedModel {
  const name = cleanModelName(rawName);
  const parts = name.split("/").map((part) => part.trim()).filter(Boolean);
  const vendor = parts.length > 1 ? parts[0].toLowerCase() : "";
  const modelPart = parts.length > 1 ? parts.slice(1).join("/") : name;
  const tokens = modelPart.split(TOKEN_SPLIT).filter(Boolean);
  const versionStart = tokens.findIndex((token) => /\d/.test(token));

  if (versionStart < 0) {
    return {
      name,
      vendor,
      // 没有版本号的模型（`jev-latest` / `jev-preview`）仍然属于同一个标识：
      // 按第一个词分组，卡片上只留一条，后面的词是修饰而不是另一个型号。
      family: (tokens[0] ?? name).toLowerCase(),
      version: [],
      variant: 0,
      order,
    };
  }

  const familyTokens = tokens.slice(0, versionStart);
  return {
    name,
    vendor,
    // 名字以版本号开头（如 `4o-mini`）时没有可读的前缀，就用第一个词当标识。
    family: (familyTokens.length ? familyTokens : tokens.slice(0, 1))
      .join("-")
      .toLowerCase(),
    version: versionOf(tokens.slice(versionStart)),
    variant: tokens.length - versionStart - 1,
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

  if (left.variant !== right.variant) {
    return left.variant - right.variant;
  }

  const byName = left.name.localeCompare(right.name, undefined, {
    numeric: true,
    sensitivity: "base",
  });
  return byName || left.order - right.order;
}

/**
 * 卡片上的模型芯片行。
 *
 * 站点给的顺序通常是按字母或者按自家目录排的，直接截前 N 个会得到同一家族
 * 的一串版本号。这里把名字拆成「模型标识 + 版本号」，同一标识只保留版本最高
 * 的一条，再按标识轮流取，让有限的几行尽量覆盖不同厂商和型号；版本号更高的
 * 标识排在前面。
 */
export function providerModelChips(
  names: string[] | null | undefined,
  limit = 24,
): ProviderModelChips {
  const unique = new Map<string, string>();
  for (const rawName of names ?? []) {
    const name = cleanModelName(rawName);
    const key = name.toLocaleLowerCase();
    if (name && !unique.has(key)) {
      unique.set(key, name);
    }
  }

  const groups = new Map<string, ParsedModel[]>();
  [...unique.values()].forEach((name, index) => {
    const model = parseModel(name, index);
    const key = `${model.vendor}|${model.family}`;
    const group = groups.get(key);
    if (group) {
      group.push(model);
    } else {
      groups.set(key, [model]);
    }
  });

  const ordered = [...groups.entries()]
    .map(([key, models]) => ({
      key,
      models: [...models].sort(compareVersions),
    }))
    .sort((left, right) => {
      const byVersion = compareVersions(left.models[0], right.models[0]);
      return byVersion || left.key.localeCompare(right.key);
    });

  // 轮流取：第一轮每个标识一条（版本最高的那条），第二轮再补第二高的，
  // 直到把所有模型排完或者排到测量上限。
  const names_: string[] = [];
  const safeLimit = Math.max(0, Math.floor(limit));
  for (let round = 0; names_.length < safeLimit; round += 1) {
    let added = false;
    for (const group of ordered) {
      const model = group.models[round];
      if (!model) {
        continue;
      }
      names_.push(model.name);
      added = true;
      if (names_.length >= safeLimit) {
        break;
      }
    }
    if (!added) {
      break;
    }
  }

  return { names: names_, total: unique.size };
}
