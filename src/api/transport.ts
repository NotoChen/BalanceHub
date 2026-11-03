//! 前端唯一的宿主通道。
//!
//! 桌面版走 Tauri IPC，浏览器版走 HTTP RPC 与 WebSocket 事件流。调用方仍然
//! 使用 `invoke` / `listen` / `Channel` 这组名字，因此页面代码不需要区分
//! 当前运行在哪种宿主里。

import { invoke as tauriInvoke, Channel as TauriChannel } from "@tauri-apps/api/core";
import {
  listen as tauriListen,
  type EventCallback,
  type UnlistenFn,
} from "@tauri-apps/api/event";

/** 浏览器版：没有 Tauri 注入的内部对象。 */
export const isWebHost = !("__TAURI_INTERNALS__" in globalThis);

/** 浏览器版里由服务端推送的进度通道。 */
interface WebChannelMarker {
  readonly __webChannel: true;
  deliver(message: unknown): void;
}

export interface Channel<T> {
  onmessage?: (message: T) => void;
}

class WebChannel<T> implements WebChannelMarker {
  readonly __webChannel = true as const;
  onmessage?: (message: T) => void;

  constructor(onmessage?: (message: T) => void) {
    this.onmessage = onmessage;
  }

  deliver(message: unknown): void {
    this.onmessage?.(message as T);
  }
}

export const Channel: new <T>(onmessage?: (message: T) => void) => Channel<T> =
  isWebHost
    ? (WebChannel as unknown as new <T>(
        onmessage?: (message: T) => void,
      ) => Channel<T>)
    : (TauriChannel as unknown as new <T>(
        onmessage?: (message: T) => void,
      ) => Channel<T>);

/** 命令用到的进度通道与事件名；服务端把进度当作普通事件推送。 */
const channelEvents: Record<string, string> = {
  refresh_all_providers_with_progress: "provider-batch-progress",
  get_agent_asset_catalog: "agent-asset-catalog-progress",
  read_agent_catalog_content: "agent-catalog-content-progress",
};

/// 浏览器会话标识：区分同一站点下不同标签页的资产访问身份。
const clientId = (() => {
  if (!isWebHost || typeof crypto === "undefined" || typeof sessionStorage === "undefined") {
    return "web";
  }
  try {
    const stored = sessionStorage.getItem("balancehub-client");
    if (stored) return stored;
    const id = crypto.randomUUID().replace(/-/g, "");
    sessionStorage.setItem("balancehub-client", id);
    return id;
  } catch {
    return "web";
  }
})();

type Listener = (payload: unknown) => void;

const listeners = new Map<string, Set<Listener>>();
const channelTargets = new Map<string, Set<WebChannelMarker>>();
let socket: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectDelay = 1_000;

function eventSocketUrl(): string {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}/events`;
}

function dispatch(event: string, payload: unknown) {
  for (const listener of listeners.get(event) ?? []) {
    try {
      listener(payload);
    } catch (error) {
      console.error(`事件 ${event} 处理失败`, error);
    }
  }
  for (const channel of channelTargets.get(event) ?? []) {
    try {
      channel.deliver(payload);
    } catch (error) {
      console.error(`进度通道 ${event} 处理失败`, error);
    }
  }
}

function ensureSocket() {
  if (socket && socket.readyState <= WebSocket.OPEN) return;
  // 没有浏览器定位能力的环境（测试、非浏览器宿主）不建立事件流。
  if (typeof WebSocket === "undefined" || typeof location === "undefined") return;
  socket = new WebSocket(eventSocketUrl());
  socket.onopen = () => {
    reconnectDelay = 1_000;
  };
  socket.onmessage = (message) => {
    if (typeof message.data !== "string") return;
    try {
      const body = JSON.parse(message.data) as { event?: string; payload?: unknown };
      if (typeof body.event === "string") dispatch(body.event, body.payload);
    } catch {
      // 非 JSON 帧不是本应用的协议内容，直接忽略。
    }
  };
  socket.onclose = () => {
    socket = null;
    if (reconnectTimer !== null) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (listeners.size > 0) ensureSocket();
    }, reconnectDelay);
    reconnectDelay = Math.min(reconnectDelay * 2, 30_000);
  };
}

async function rpcCall<T>(command: string, args: Record<string, unknown>): Promise<T> {
  const channels = Object.entries(args).filter(
    (entry): entry is [string, WebChannelMarker] =>
      Boolean(entry[1]) && (entry[1] as WebChannelMarker).__webChannel === true,
  );
  const event = channelEvents[command];
  if (event) {
    for (const [, channel] of channels) {
      const set = channelTargets.get(event) ?? new Set<WebChannelMarker>();
      set.add(channel);
      channelTargets.set(event, set);
    }
  }
  const payload = Object.fromEntries(
    Object.entries(args).filter(([, value]) => !(value as WebChannelMarker)?.__webChannel),
  );
  try {
    const response = await fetch("/rpc", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        "x-balancehub-client": clientId,
      },
      body: JSON.stringify({ command, args: payload }),
    });
    const body = (await response.json().catch(() => ({}))) as {
      ok?: boolean;
      data?: T;
      error?: string;
    };
    if (!response.ok || body.ok === false) {
      throw body.error || `命令 ${command} 失败（HTTP ${response.status}）`;
    }
    return body.data as T;
  } finally {
    for (const [, channel] of channels) {
      channelTargets.get(event)?.delete(channel);
    }
  }
}

export async function invoke<T>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  if (!isWebHost) return tauriInvoke<T>(command, args);
  return rpcCall<T>(command, args);
}

export function listen<T>(
  event: string,
  handler: EventCallback<T>,
): Promise<UnlistenFn> {
  if (!isWebHost) return tauriListen<T>(event, handler);
  const listener: Listener = (payload) => handler({ event, id: 0, payload: payload as T });
  const set = listeners.get(event) ?? new Set<Listener>();
  set.add(listener);
  listeners.set(event, set);
  ensureSocket();
  return Promise.resolve(() => {
    set.delete(listener);
  });
}

export type { UnlistenFn };
