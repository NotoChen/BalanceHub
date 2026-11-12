import type { AgentCliKind } from "./provider-types";

export interface AgentHandoffRequest {
  consumerId: string;
  requestId: number;
  scopeRevision: string;
  sessionRef: string;
}

export interface AgentHandoff {
  /** 交接文件绝对路径；文件位于应用数据目录，不写入项目。 */
  path: string;
  content: string;
  sessionTitle: string;
  sessionId: string;
  workdir: string;
  cliKind: AgentCliKind;
  model: string | null;
  messages: number;
  truncated: boolean;
  generatedAt: string;
}
