import type {
  AgentAssetCategory,
  AgentAssetDiagnostic,
  AgentAssetDiscoveryIncompleteReason,
  AgentAssetDocumentFormat,
  AgentAssetIoErrorKind,
  AgentAssetLimitKind,
  AgentAssetRelationKind,
  AgentAssetSuppressionReason,
  AgentAssetSourceKind,
  AgentDiscoverySource,
  AgentExecutableProbeErrorKind,
} from "../stores/provider-types";

const categoryLabels: Record<AgentAssetCategory, string> = {
  skill: "Skill",
  plugin: "插件",
  extension: "扩展",
  mcp: "MCP",
  hook: "Hook",
  statusUi: "Status UI",
};

const limitLabels: Record<AgentAssetLimitKind, string> = {
  candidatePathsPerAgent: "候选路径",
  installationsPerAgent: "安装实例",
  sourcesPerContext: "配置来源",
  firstLevelEntries: "一级条目",
  frontmatterLines: "Skill 头部行数",
  frontmatterBytes: "Skill 头部字节数",
  bytesPerSource: "单来源读取量",
  bytesPerRefresh: "单次盘点读取量",
  refreshBudget: "盘点耗时",
  cliOutput: "CLI 输出",
  cliConcurrency: "CLI 并发",
  watchers: "监听器",
  diagnostics: "诊断信息",
};

const formatLabels: Record<AgentAssetDocumentFormat, string> = {
  json: "JSON",
  toml: "TOML",
  yaml: "YAML",
  manifest: "目录清单",
  unknown: "未知格式",
};

const relationLabels: Record<AgentAssetRelationKind, string> = {
  providedBy: "提供方",
  actionOwner: "操作归属",
  explicitImpact: "影响关系",
};

const suppressionLabels: Record<AgentAssetSuppressionReason, string> = {
  untrustedWorkspace: "工作区未信任",
  compatibilitySourceDisabled: "兼容来源已禁用",
  unsupportedContext: "上下文不支持",
  duplicatePhysicalSource: "同一文件的重复引用，原生已去重",
  parentNotSelected: "父插件已被其他来源覆盖",
  unknown: "原因未知",
};

const sourceKindLabels: Record<AgentAssetSourceKind, string> = {
  file: "文件",
  directory: "目录",
};

const discoveryLabels: Record<AgentDiscoverySource, string> = {
  configured: "配置路径",
  automatic: "自动发现路径",
};

const probeErrorLabels: Record<AgentExecutableProbeErrorKind, string> = {
  notFound: "文件不存在",
  permissionDenied: "权限不足",
  timedOut: "探测超时",
  invalidVersion: "版本信息无效",
  changedDuringProbe: "探测期间可执行文件发生变化",
  failed: "执行失败",
};

const incompleteDiscoveryLabels: Record<AgentAssetDiscoveryIncompleteReason, string> = {
  unsupportedVersion: "BalanceHub 尚未适配当前版本的这类来源；可在 Agent 中核对配置",
  installationUnverified: "未能核验安装包来源",
  sourceUnavailable: "部分来源无法读取或已变化；请检查文件及读取权限后刷新",
  runtimeStateUnobserved: "已读取静态配置，实际加载结果由 Agent 运行时确定",
  unsupportedEntryPoint: "发现尚未支持的加载入口",
  nativeEquivalenceUnobserved: "已读取多个配置来源，Agent 运行时的去重结果尚未确认",
};

const readFailureMessages: Record<AgentAssetIoErrorKind, string> = {
  notFound: "配置文件已不存在；请刷新重新盘点",
  permissionDenied: "无法读取配置文件，权限不足；请检查文件权限后刷新",
  invalidData: "配置文件数据或编码无法读取；请检查文件内容和编码后刷新",
  other: "配置来源读取失败；可查看源文件并刷新重试",
};

const skillMetadataIssues: Record<string, string> = {
  "frontmatter.encoding": "Skill 文件无法按 UTF-8 读取；请检查文件编码后刷新",
  "frontmatter.unterminated": "Skill 头部缺少结束分隔线 ---；请检查 SKILL.md 头部后刷新",
  "frontmatter.syntax": "Skill 头部 YAML 语法有误或字段重复；请检查 SKILL.md 头部后刷新",
  "frontmatter.root": "Skill 头部 YAML 应为字段映射；请检查 SKILL.md 头部后刷新",
  "frontmatter.name": "Skill 的 name 应为非空单行文本；请检查该字段后刷新",
  "frontmatter.disable-model-invocation": "Skill 的 disable-model-invocation 应为布尔值 true 或 false；请检查该字段后刷新",
  "frontmatter.complexity": "Skill 头部结构超过解析复杂度上限，BalanceHub 无法完整判断状态；可查看源文件或在 Agent 中核对加载状态",
};

export function formatAgentAssetDiagnostic(
  diagnostic: AgentAssetDiagnostic,
): string {
  switch (diagnostic.kind) {
    case "truncated":
      if (diagnostic.limit === "candidatePathsPerAgent") {
        return `Agent 安装扫描达到 ${diagnostic.accepted} 个可执行文件候选的上限，部分安装位置尚未核对；这不是资源数量上限，可在“版本与路径”中检查`;
      }
      if (diagnostic.limit === "frontmatterLines" || diagnostic.limit === "frontmatterBytes") {
        const unit = diagnostic.limit === "frontmatterLines" ? "行" : "字节";
        return `Skill 头部超过 ${diagnostic.accepted} ${unit}读取上限，BalanceHub 无法完整判断状态；可查看源文件或在 Agent 中核对加载状态`;
      }
      return `${limitLabels[diagnostic.limit]}达到上限：保留 ${diagnostic.accepted}，实际至少 ${diagnostic.observedAtLeast}`;
    case "malformed":
      if (diagnostic.format === "yaml" && diagnostic.location && skillMetadataIssues[diagnostic.location]) {
        return skillMetadataIssues[diagnostic.location];
      }
      return `${formatLabels[diagnostic.format]} 内容无法解析${diagnostic.location ? `（${diagnostic.location}）` : ""}`;
    case "duplicateNativeId":
      return `${categoryLabels[diagnostic.category]} 标识重复：${diagnostic.nativeId}`;
    case "unknownField":
      return `发现未识别字段：${diagnostic.fieldPath}`;
    case "symlinkRejected":
      return "出于安全原因已拒绝符号链接来源";
    case "budgetExceeded":
      return `盘点超过 ${diagnostic.budgetMs} ms 时间预算`;
    case "readFailed":
      return readFailureMessages[diagnostic.errorKind];
    case "invalidNativeId":
      return `${categoryLabels[diagnostic.category]} 标识无效`;
    case "unresolvedRelationship":
      return `${relationLabels[diagnostic.relation]}无法解析：${diagnostic.nativeId}`;
    case "invalidProjection":
      return `逻辑资产投影无效：${diagnostic.projectionKey}`;
    case "invalidResolution":
      return `逻辑资产解析关系无效：${diagnostic.projectionKey}`;
    case "installationProbeFailed":
      return `${discoveryLabels[diagnostic.candidateSource]}探测失败：${probeErrorLabels[diagnostic.errorKind]}`;
    case "sourceOutsideAllowedRoot":
      return "配置来源超出允许目录";
    case "sourceTypeMismatch":
      return `配置来源类型不符：预期${sourceKindLabels[diagnostic.expected]}，实际为${sourceKindLabels[diagnostic.actual]}`;
    case "invalidCompatibleInstallation":
      return "配置上下文引用了无效安装实例";
    case "declarationSuppressed":
      return `声明已抑制：${suppressionLabels[diagnostic.reason]}`;
    case "discoveryIncomplete":
      return `${categoryLabels[diagnostic.category]} 来源说明：${incompleteDiscoveryLabels[diagnostic.reason]}`;
    case "policyBlocked":
      return "资产被策略阻断";
  }
}

export function formatAgentAssetDiagnostics(
  diagnostics: AgentAssetDiagnostic[],
): string[] {
  return diagnostics.map(formatAgentAssetDiagnostic);
}

export function mergeAgentAssetDiagnostics(
  ...groups: AgentAssetDiagnostic[][]
): AgentAssetDiagnostic[] {
  const seen = new Set<string>();
  const merged: AgentAssetDiagnostic[] = [];
  for (const diagnostic of groups.flat()) {
    const key = JSON.stringify(diagnostic);
    if (seen.has(key)) continue;
    seen.add(key);
    merged.push(diagnostic);
  }
  return merged;
}
