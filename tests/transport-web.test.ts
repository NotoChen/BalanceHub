import assert from "node:assert/strict";
import test, { after, before } from "node:test";
import { loadSource } from "./helpers/load-source.ts";

type Transport = typeof import("../src/api/transport.ts");
type FakeResponse = {
  ok: boolean;
  status: number;
  json: () => Promise<unknown>;
};

class FakeSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: FakeSocket[] = [];

  readyState = FakeSocket.CONNECTING;
  onopen: (() => void) | null = null;
  onmessage: ((message: { data: unknown }) => void) | null = null;
  onclose: (() => void) | null = null;
  readonly url: string;

  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }

  deliver(event: string, payload: unknown) {
    this.onmessage?.({ data: JSON.stringify({ event, payload }) });
  }

  close() {
    this.readyState = FakeSocket.CLOSED;
    this.onclose?.();
  }
}

const requests: Array<{ url: string; body: Record<string, unknown> }> = [];
const responses: Array<() => Promise<FakeResponse>> = [];
let transport: Transport;
const originals = {
  WebSocket: (globalThis as { WebSocket?: unknown }).WebSocket,
  fetch: globalThis.fetch,
  location: (globalThis as { location?: unknown }).location,
};

function response(data: unknown): FakeResponse {
  return { ok: true, status: 200, json: async () => ({ ok: true, data }) };
}

async function settle() {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
}

before(() => {
  (globalThis as { location?: unknown }).location = {
    protocol: "http:",
    host: "127.0.0.1:8787",
  };
  (globalThis as { WebSocket?: unknown }).WebSocket = FakeSocket;
  globalThis.fetch = (async (input: unknown, init?: RequestInit) => {
    requests.push({
      url: String(input),
      body: JSON.parse(String(init?.body ?? "{}")) as Record<string, unknown>,
    });
    const next = responses.shift();
    assert.ok(next, "每个请求都要有预设响应");
    return next();
  }) as typeof fetch;
  transport = loadSource<Transport>("api/transport.ts", {
    "@tauri-apps/api/core": {
      invoke: async () => {
        throw new Error("浏览器版不应走 Tauri IPC");
      },
      Channel: class {},
    },
    "@tauri-apps/api/event": { listen: async () => () => {} },
  });
});

after(() => {
  FakeSocket.instances = [];
  if (originals.WebSocket === undefined) {
    delete (globalThis as { WebSocket?: unknown }).WebSocket;
  } else {
    (globalThis as { WebSocket?: unknown }).WebSocket = originals.WebSocket;
  }
  globalThis.fetch = originals.fetch;
  if (originals.location === undefined) {
    delete (globalThis as { location?: unknown }).location;
  } else {
    (globalThis as { location?: unknown }).location = originals.location;
  }
});

test("浏览器通道识别自身并把命令发到 /rpc", async () => {
  assert.equal(transport.isWebHost, true);
  responses.push(async () => response({ providers: [], revision: 7 }));
  const data = await transport.invoke<{ revision: number }>("load_app_data", {
    force: false,
  });
  assert.equal(data.revision, 7);
  assert.equal(requests.length, 1);
  assert.equal(requests[0].url, "/rpc");
  assert.deepEqual(requests[0].body, {
    command: "load_app_data",
    args: { force: false },
  });
});

test("服务端错误按原样抛给调用方，HTTP 失败带状态码", async () => {
  responses.push(async () => ({
    ok: true,
    status: 200,
    json: async () => ({ ok: false, error: "额度读取失败" }),
  }));
  await assert.rejects(transport.invoke("refresh_providers"), /额度读取失败/);

  responses.push(async () => ({
    ok: false,
    status: 502,
    json: async () => ({}),
  }));
  await assert.rejects(transport.invoke("load_app_data"), /HTTP 502/);
});

test("事件订阅连接 /events，取消订阅后不再回调", async () => {
  const received: unknown[] = [];
  const unlisten = await transport.listen("providers-changed", (event) => {
    received.push(event.payload);
  });
  const socket = FakeSocket.instances.at(-1);
  assert.ok(socket, "订阅时应建立事件连接");
  assert.equal(socket.url, "ws://127.0.0.1:8787/events");

  socket.deliver("providers-changed", { ids: ["a"] });
  socket.deliver("unrelated", { ids: ["b"] });
  assert.deepEqual(received, [{ ids: ["a"] }]);

  unlisten();
  socket.deliver("providers-changed", { ids: ["c"] });
  assert.deepEqual(received, [{ ids: ["a"] }], "取消订阅后事件不再送达");
});

test("进度通道按事件名接收结果，参数里不带通道对象", async () => {
  const events: unknown[] = [];
  const channel = new transport.Channel<{ status: string }>((message) => {
    events.push(message);
  });
  let release!: () => void;
  responses.push(
    () =>
      new Promise<FakeResponse>((resolve) => {
        release = () => resolve(response({ done: true }));
      }),
  );
  const pending = transport.invoke("refresh_all_providers_with_progress", {
    ids: ["a"],
    onEvent: channel,
  });
  await settle();
  const socket = FakeSocket.instances.at(-1);
  assert.ok(socket);
  socket.deliver("provider-batch-progress", { status: "running" });
  assert.deepEqual(events, [{ status: "running" }]);
  assert.deepEqual(requests.at(-1)?.body, {
    command: "refresh_all_providers_with_progress",
    args: { ids: ["a"] },
  });

  release();
  await pending;
  socket.deliver("provider-batch-progress", { status: "done" });
  assert.deepEqual(events, [{ status: "running" }], "请求结束后通道即解除");
});
