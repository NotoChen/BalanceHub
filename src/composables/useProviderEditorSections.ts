import { computed } from "vue";
import type {
  AppSettings,
  ProviderInput,
  ProviderProtocolDescriptor,
} from "../stores/providers";
import type { ProviderEditorSection } from "./provider-editor-shared";
import { formatDuration } from "../utils/duration";
import {
  providerAuthModeDescriptor,
  providerProtocolDescriptor,
} from "../utils/provider-protocol";

/// 编辑窗口左侧的一个分区。`summary` 读的是当前草稿，改完回到导航就能看到
/// 这一块配置成了什么样；`rows` 决定字段用「标签在左、控件在右」的行式布局。
export interface ProviderEditorNavItem {
  id: ProviderEditorSection;
  title: string;
  description: string;
  summary: string;
  rows: boolean;
}

function hostLabel(baseUrl: string) {
  const raw = baseUrl.trim();
  if (!raw) return "未填写地址";
  try {
    return new URL(raw).host || raw;
  } catch {
    return raw.replace(/^https?:\/\//i, "").split("/")[0] || raw;
  }
}

export function useProviderEditorSections(options: {
  draft: ProviderInput;
  settings: AppSettings;
  providerProtocols: ProviderProtocolDescriptor[];
}) {
  const descriptor = computed(() =>
    providerProtocolDescriptor(
      options.providerProtocols,
      options.draft.identity.protocol,
    ),
  );
  const capabilities = computed(() => descriptor.value?.capabilities);

  /// 备用地址、额外请求头和 API Key 发送方式都决定请求怎么发出去；
  /// 协议一项都不支持时（OAuth 账号、CPA 网关）整个分区不出现。
  const requestSupported = computed(
    () =>
      Boolean(capabilities.value?.requestHeaders) ||
      Boolean(capabilities.value?.apiKeyScheme) ||
      (capabilities.value?.maxBackupUrls ?? 0) > 0,
  );

  const authSummary = computed(() => {
    const mode = options.draft.auth.mode;
    return (
      providerAuthModeDescriptor(
        options.providerProtocols,
        options.draft.identity.protocol,
        mode,
      )?.label ?? mode
    );
  });

  const quotaSummary = computed(() =>
    options.draft.quotaSpec.url.trim() ? "已声明余额接口" : "使用默认额度来源",
  );

  const modelsSummary = computed(() => {
    const config = options.draft.modelConfig;
    const overrides = Object.keys(config.overrides).length;
    if (overrides) return `${overrides} 个模型覆盖`;
    if (config.include.length || config.exclude.length) return "已设置展示规则";
    return "展示全部模型";
  });

  const requestSummary = computed(() => {
    const headers = options.draft.request.headers.filter((header) =>
      header.name.trim(),
    ).length;
    const backups = options.draft.identity.backupUrls.filter((url) =>
      url.trim(),
    ).length;
    if (backups && headers) return `${backups} 个备用地址 · ${headers} 条请求头`;
    if (backups) return `${backups} 个备用地址`;
    if (headers) return `${headers} 条请求头`;
    return "默认请求约定";
  });

  const automationSummary = computed(() => {
    const interval = options.draft.automation.refreshInterval;
    if (interval > 0) return `每 ${formatDuration(interval)}刷新`;
    return `刷新跟随全局 · ${formatDuration(options.settings.refreshInterval)}`;
  });

  const networkSummary = computed(() => {
    if (options.draft.proxy.mode === "custom") return "自定义代理";
    if (options.draft.notification.mode === "custom") return "自定义通知";
    return "代理与通知跟随全局";
  });

  const navItems = computed<ProviderEditorNavItem[]>(() => {
    const items: ProviderEditorNavItem[] = [
      {
        id: "basics",
        title: "基础信息",
        description: `${descriptor.value?.label ?? options.draft.identity.protocol} 的接入类型、地址与备注。`,
        summary: `${descriptor.value?.label ?? options.draft.identity.protocol} · ${hostLabel(options.draft.identity.baseUrl)}`,
        rows: false,
      },
      {
        id: "credentials",
        title: "认证凭据",
        description: "登录方式、会话与调用 Key；保存时会补齐后续需要的凭据。",
        summary: authSummary.value,
        rows: false,
      },
    ];
    if (capabilities.value?.quotaSpec) {
      items.push({
        id: "quota",
        title: "额度接口",
        description: "声明这个站点从哪里读取余额和订阅窗口。",
        summary: quotaSummary.value,
        rows: true,
      });
    }
    items.push({
      id: "models",
      title: "模型",
      description: "决定展示哪些模型、元数据如何补全，以及临时 CLI 的首选模型。",
      summary: modelsSummary.value,
      rows: true,
    });
    if (requestSupported.value) {
      items.push({
        id: "request",
        title: "请求配置",
        description: "备用地址、额外请求头和 API Key 的发送方式。",
        summary: requestSummary.value,
        rows: true,
      });
    }
    items.push(
      {
        id: "automation",
        title: "自动任务",
        description: "刷新间隔、自动测活与签到。",
        summary: automationSummary.value,
        rows: true,
      },
      {
        id: "network",
        title: "网络与通知",
        description: "这个站点的代理，以及它的消息走哪些渠道。",
        summary: networkSummary.value,
        rows: true,
      },
    );
    return items;
  });

  return { navItems };
}
