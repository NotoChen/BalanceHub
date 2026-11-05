/** Native message facts and cursor positions come from the Rust session index. */
export type SessionMessageKind =
  "text" | "thinking" | "toolCall" | "toolResult" | "image" | "event";
export type SessionToolKind =
  "shell" | "read" | "search" | "edit" | "write" | "mcp" | "agent" | "other";
export type SessionToolStatus =
  "succeeded" | "failed" | "running" | "cancelled";
export interface SessionMessageMetadata {
  kind: SessionMessageKind;
  timestamp: string | null;
  model: string | null;
  toolName: string | null;
  toolKind: SessionToolKind | null;
  turnId: string | null;
  groupId: string | null;
  callId: string | null;
  status: SessionToolStatus | null;
  title: string | null;
  exitCode: number | null;
  injected: boolean;
}
export interface SessionSearchHit {
  messageId: string;
  offset: number;
  snippet: string;
  terms: string[];
  kind: SessionMessageKind;
}
export interface SessionReadWindow {
  /** 窗口首字节，已对齐到下一条完整记录。 */
  start: number;
  /** 窗口内最后一条记录的结束位置。 */
  end: number;
  fileBytes: number;
  /** 从 1 开始的片段序号；窗口边界按完整记录对齐，因此是估算值。 */
  index: number;
  /** 文件中的片段总数估算值；首尾片段始终是 1 和 total。 */
  total: number;
  hasEarlier: boolean;
  hasLater: boolean;
}
export interface SessionReadPage {
  revision: string;
  previousCursor: string | null;
  nextCursor: string | null;
  totalMessages: number;
  start: number;
  indexing: boolean;
  /** 会话原文超过全量读取上限时，只显示截断的尾部内容。 */
  truncated: boolean;
  /** 当前页所属的字节窗口，仅大文件按需读取时存在。 */
  window: SessionReadWindow | null;
}
export interface SessionFindState {
  query: string;
  total: number;
  position: number | null;
  activeMessageId: string | null;
}
export interface SessionTurnSummary {
  id: string;
  messageId: string;
  title: string;
  timestamp: string | null;
  steps: number;
  failures: number;
}
export interface SessionFileChange {
  path: string;
  before: string;
  after: string;
  patch: string | null;
  beforeKnown: boolean;
}
export interface SessionReaderNavigation {
  cursor?: string | null;
  anchor?: string | null;
  query?: string;
  matchPosition?: number | null;
}
export interface AgentSessionSearchRequest {
  consumerId: string;
  requestId: number;
  scopeRevision: string;
  sessionRef: string;
  query: string;
  cursor: string | null;
}
export interface AgentSessionSearchHit extends SessionSearchHit {
  /** Revision of the fragment containing this hit. */
  cursor: string;
  /** Fragment facts for labeling the hit; null for a full-transcript read. */
  window: SessionReadWindow | null;
}
export interface AgentSessionSearchResult {
  hits: AgentSessionSearchHit[];
  nextCursor: string | null;
  scannedWindows: number;
  totalWindows: number;
  complete: boolean;
}
export interface SessionReaderSearchState {
  query: string;
  hits: AgentSessionSearchHit[];
  nextCursor: string | null;
  scannedWindows: number;
  totalWindows: number;
  complete: boolean;
  loading: boolean;
  error: string;
}
export interface AgentSessionMessageRequest {
  contentVersion: number;
  consumerId: string;
  requestId: number;
  scopeRevision: string;
  sessionRef: string;
  revision: string;
  messageId: string;
  offset: number;
}
export interface AgentSessionMessageContent {
  messageId: string;
  content: string;
  offset: number;
  totalChars: number;
  nextOffset: number | null;
  changes: SessionFileChange[];
}
export interface SessionMessageLoadState {
  loading: boolean;
  error: string;
}
import type { VirtualItem } from "@tanstack/vue-virtual";

/** Ephemeral layout state; it does not participate in native session identity. */
export interface SessionListPosition {
  key: string;
  offset: number;
  measurements: VirtualItem[];
}
