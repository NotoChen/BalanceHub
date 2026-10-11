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
export interface SessionReadPage {
  revision: string;
  previousCursor: string | null;
  nextCursor: string | null;
  totalMessages: number;
  start: number;
  indexing: boolean;
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
