import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, defineComponent, h, nextTick } from "vue";
import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { useWindowGridSnap } from "../src/composables/useWindowGridSnap.ts";

type Size = { width: number; height: number };
type View = "providers" | "agents" | "catalog";
type Deferred = { promise: Promise<unknown>; resolve: (value?: unknown) => void; reject: (error: Error) => void };

function deferred(): Deferred {
  let resolve!: Deferred["resolve"];
  let reject!: Deferred["reject"];
  const promise = new Promise<unknown>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

// The hook uses layout measurements, not DOM rendering. Keep those measurements
// explicit while exercising the real Vue lifecycle, Tauri API and async flow.
class LayoutElement {
  classes: Set<string>;
  children: LayoutElement[] = [];
  parent: LayoutElement | null = null;
  visible = true;
  scrollTop = 0;
  width: number;
  height: number;
  top: number;
  styles = new Map<string, string>();
  attributes = new Set<string>();
  style = { setProperty: (name: string, value: string) => this.styles.set(name, value) };

  constructor(classes: string, width = 1100, height = 656, top = 64) {
    this.classes = new Set(classes.split(" "));
    this.width = width;
    this.height = height;
    this.top = top;
  }

  append(...elements: LayoutElement[]) {
    for (const element of elements) { element.parent = this; this.children.push(element); }
    return this;
  }

  get clientWidth() { return this.width; }
  get offsetWidth() { return this.width; }
  get clientHeight() { return this.height; }
  get displayed(): boolean { return this.visible && (this.parent?.displayed ?? true); }

  getBoundingClientRect() {
    let scroll = 0;
    for (let ancestor = this.parent; ancestor; ancestor = ancestor.parent) scroll += ancestor.scrollTop;
    return {
      width: this.displayed ? this.width : 0,
      height: this.displayed ? this.height : 0,
      top: this.top - scroll,
      bottom: this.top - scroll + this.height,
    };
  }

  matches(selector: string) {
    return selector.split(",").some((part) => {
      const [required, excluded] = part.trim().split(":not(");
      return this.classes.has(required.slice(1))
        && (!excluded || !this.classes.has(excluded.slice(1, -1)));
    });
  }

  querySelectorAll(selector: string): LayoutElement[] {
    const direct = selector.startsWith(":scope > ");
    const match = direct ? selector.slice(9) : selector;
    return this.children.flatMap((child) => [
      ...(child.matches(match) ? [child] : []),
      ...(direct ? [] : child.querySelectorAll(match)),
    ]);
  }

  querySelector(selector: string) { return this.querySelectorAll(selector)[0] ?? null; }
  toggleAttribute(name: string, enabled: boolean) {
    if (enabled) this.attributes.add(name); else this.attributes.delete(name);
  }

  computedStyle() {
    return {
      getPropertyValue: (name: string) => this.styles.get(name) ?? "",
      paddingLeft: "20px", paddingRight: "20px", paddingTop: "16px", paddingBottom: "24px",
      columnGap: this.styles.get("--provider-grid-local-gap") ?? "16px",
      rowGap: this.classes.has("provider-board-section") ? "8px"
        : this.styles.get("--provider-grid-local-row-gap") ?? "16px",
    };
  }
}

async function settle() {
  await nextTick();
  for (let index = 0; index < 40; index += 1) await Promise.resolve();
}

async function mountSnap(t: TestContext, options: { view?: View; size?: Size; agentHeight?: number; maximized?: boolean } = {}) {
  const initial = options.size ?? { width: 1100, height: 720 };
  const topbar = new LayoutElement("topbar", initial.width, 64, 0);
  const provider = new LayoutElement("provider-board", initial.width, initial.height - 64);
  provider.styles.set("--provider-grid-card-min", "300px");
  const providerCard = new LayoutElement("provider-card", 336, 312, 105);
  const draggingCard = new LayoutElement("provider-card provider-card-dragging", 900, 900, 105);
  const providerGrid = new LayoutElement("overview-provider-grid", initial.width - 40, 350, 105)
    .append(providerCard, draggingCard);
  const section = new LayoutElement("provider-board-section", initial.width - 40, 375, 80)
    .append(new LayoutElement("provider-board-section-header", 336, 17, 80), providerGrid);
  provider.append(section);
  const dashboard = new LayoutElement("agent-dashboard", initial.width, initial.height - 64);
  const agentCards = Array.from({ length: 4 }, () => new LayoutElement("agent-overview-card", 336, options.agentHeight ?? 220, 164));
  const agentGrid = new LayoutElement("agent-overview-grid", initial.width - 40, 456, 164).append(...agentCards);
  const agentViewport = new LayoutElement("agent-dashboard-content").append(agentGrid);
  dashboard.append(agentViewport);
  const body = new LayoutElement("body").append(topbar, provider, dashboard);
  const root = new LayoutElement("root");
  const frames = new Map<number, () => void>();
  const timers = new Map<number, { callback: () => void; delay: number }>();
  const listeners = new Map<string, Set<(event: { buttons: number }) => void>>();
  const pending = new Map<string, Deferred[]>();
  const sizes: Size[] = [];
  const constraints: Size[] = [];
  const calls: string[] = [];
  let currentSize = { ...initial };
  let appliedConstraints: Size | null = null;
  let maximized = options.maximized ?? false;
  let sequence = 0;
  let mutationCallback: ((records: { type: string; target: LayoutElement }[]) => void) | null = null;
  let mutationOptions: MutationObserverInit | null = null;
  const originals = new Map(["window", "document", "MutationObserver", "getComputedStyle"].map((name) => [name, Object.getOwnPropertyDescriptor(globalThis, name)]));

  const browserWindow = {
    crypto: globalThis.crypto,
    requestAnimationFrame(callback: () => void) { const id = ++sequence; frames.set(id, callback); return id; },
    cancelAnimationFrame(id: number) { frames.delete(id); },
    setTimeout(callback: () => void, delay: number) { const id = ++sequence; timers.set(id, { callback, delay }); return id; },
    clearTimeout(id: number) { timers.delete(id); },
    addEventListener(name: string, listener: (event: { buttons: number }) => void) {
      if (!listeners.has(name)) listeners.set(name, new Set());
      listeners.get(name)!.add(listener);
    },
    removeEventListener(name: string, listener: (event: { buttons: number }) => void) { listeners.get(name)?.delete(listener); },
  };
  Object.defineProperties(globalThis, {
    window: { configurable: true, value: browserWindow },
    document: { configurable: true, value: { body, documentElement: root, querySelector: (selector: string) => body.querySelector(selector) } },
    getComputedStyle: { configurable: true, value: (element: LayoutElement) => element.computedStyle() },
    MutationObserver: { configurable: true, value: class {
      constructor(callback: typeof mutationCallback) { mutationCallback = callback; }
      observe(_target: unknown, options: MutationObserverInit) { mutationOptions = options; }
      disconnect() { mutationCallback = null; mutationOptions = null; }
    } },
  });

  function layout(size: Size) {
    for (const board of [provider, dashboard]) { board.width = size.width; board.height = size.height - 64; }
    for (const grid of [providerGrid, agentGrid]) grid.width = size.width - 40;
    const columns = Math.max(1, Math.floor((agentGrid.width + 16) / 352));
    const rows = Math.ceil(agentCards.length / columns);
    agentGrid.height = rows * Math.max(...agentCards.map((card) => card.height)) + (rows - 1) * 16;
  }

  function show(view: View, notify = true) {
    provider.visible = view === "providers";
    dashboard.visible = view !== "providers";
    agentGrid.visible = view === "agents";
    if (notify && mutationCallback) {
      const records = mutationOptions?.attributes
        ? [{ type: "attributes", target: provider }, { type: "attributes", target: dashboard }]
        : [];
      if (view === "catalog" && mutationOptions?.childList) records.push({ type: "childList", target: dashboard });
      mutationCallback(records);
    }
  }

  mockWindows("main");
  mockIPC(async (command, payload) => {
    const name = command.replace("plugin:window|", "");
    calls.push(name);
    const value = JSON.parse(JSON.stringify(payload ?? {})).value;
    const constraint = name === "set_size_constraints"
      ? { width: value.minWidth.Logical, height: value.minHeight.Logical } : null;
    const size = name === "set_size" ? value.Logical as Size : null;
    if (constraint) constraints.push(constraint);
    if (size) sizes.push(size);
    const blocked = pending.get(name)?.shift();
    if (blocked) await blocked.promise;
    if (constraint) { appliedConstraints = constraint; return; }
    if (size) { currentSize = size; layout(size); return; }
    if (name === "scale_factor") return 2;
    if (name === "inner_size") return { width: currentSize.width * 2, height: currentSize.height * 2 };
    if (name === "is_maximized") return maximized;
    if (name === "is_fullscreen") return false;
    throw new Error(`Unexpected native window call: ${command}`);
  }, { shouldMockEvents: true });

  show(options.view ?? "providers", false);
  layout(initial);
  const renderer = createRenderer<Record<string, unknown>, Record<string, unknown>>({
    patchProp() {}, insert() {}, remove() {}, createElement: (type) => ({ type }), createText: (text) => ({ text }), createComment: (text) => ({ text }),
    setText(node, text) { node.text = text; }, setElementText(node, text) { node.text = text; }, parentNode: () => null, nextSibling: () => null,
    querySelector: () => null, setScopeId() {}, cloneNode: (node) => ({ ...node }), insertStaticContent: () => [{}, {}],
  });
  const app = renderer.createApp(defineComponent({ setup() { useWindowGridSnap(); return () => h("div"); } }));
  app.mount({});
  t.after(async () => {
    app.unmount();
    await settle();
    clearMocks();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    }
    assert.equal(frames.size, 0);
    assert.equal(timers.size, 0);
    assert.ok([...listeners.values()].every((entries) => entries.size === 0));
  });

  async function frame() {
    const scheduled = [...frames.values()]; frames.clear();
    scheduled.forEach((callback) => callback());
    await settle();
  }
  async function resize(width: number, height: number) {
    currentSize = { width, height }; layout(currentSize);
    await emit("tauri://resize", { width: width * 2, height: height * 2 });
    await settle();
  }
  async function timer(delay: number) {
    for (const [id, scheduled] of [...timers]) {
      if (scheduled.delay !== delay) continue;
      timers.delete(id); scheduled.callback();
    }
    await settle();
  }
  function release() { listeners.get("pointerup")?.forEach((listener) => listener({ buttons: 0 })); }
  function block(command: string) {
    const waiting = deferred();
    pending.set(command, [...(pending.get(command) ?? []), waiting]);
    return waiting;
  }
  await settle();
  await frame();
  return {
    provider, providerCard, providerGrid, dashboard, agentViewport, agentCards, agentGrid, root,
    sizes, constraints, calls, show, frame, resize, timer, release, block,
    get appliedConstraints() { return appliedConstraints; },
    setMaximized(value: boolean) { maximized = value; },
    notifySpacingStyle() { mutationCallback?.([{ type: "attributes", target: provider }]); },
    get pendingFrames() { return frames.size; },
  };
}

test("mount and an unchanged native resize keep the configured 1100 by 720 window", async (t) => {
  const window = await mountSnap(t);
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 441 });
  await window.resize(1100, 720);
  window.release();
  await window.timer(520);
  assert.deepEqual(window.sizes, []);
});

test("Provider resize preserves the requested width and measures rows without the dragging card", async (t) => {
  const window = await mountSnap(t);
  await window.resize(760, 800);
  window.release();
  await settle();
  assert.deepEqual(window.sizes, [{ width: 760, height: 769 }]);
  assert.equal(window.providerGrid.styles.get("--provider-grid-local-gap"), undefined);
  window.notifySpacingStyle();
  assert.equal(window.pendingFrames, 0);
});

test("stretched Provider cards do not increase the minimum width or snap the chosen width", async (t) => {
  const window = await mountSnap(t);
  window.providerCard.width = 700;
  await window.resize(1120, 800);
  window.release();
  await settle();
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 441 });
  assert.deepEqual(window.sizes, [{ width: 1120, height: 769 }]);
});

test("Agent resize measures its visible card height and ignores the retained Provider DOM", async (t) => {
  const window = await mountSnap(t, { view: "agents", agentHeight: 220 });
  window.providerCard.height = 700;
  window.agentViewport.scrollTop = 70;
  await window.resize(760, 640);
  window.release();
  await settle();
  assert.deepEqual(window.sizes, [{ width: 728, height: 644 }]);
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 480 });
});

test("Agent geometry follows taller measured cards without a Provider height floor", async (t) => {
  const window = await mountSnap(t, { view: "agents", agentHeight: 298 });
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 486 });
  await window.resize(760, 810);
  await window.timer(520);
  assert.deepEqual(window.sizes, [{ width: 728, height: 800 }]);
});

test("v-show switches only refresh constraints and cancel an unfinished resize", async (t) => {
  const window = await mountSnap(t);
  await window.resize(760, 800);
  window.show("agents");
  await window.frame();
  window.release();
  await window.timer(520);
  assert.deepEqual(window.sizes, []);
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 480 });
  window.show("providers");
  await window.frame();
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 441 });
  assert.deepEqual(window.sizes, []);
});

test("catalog pages restore ordinary bounds and do not snap on resize release", async (t) => {
  const window = await mountSnap(t, { view: "agents", agentHeight: 420 });
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 608 });
  window.show("catalog");
  await window.frame();
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 480 });
  await window.resize(815, 610);
  window.release();
  await window.timer(520);
  assert.deepEqual(window.sizes, []);
});

test("view constraints do not grow the current window to fit taller content", async (t) => {
  const window = await mountSnap(t, { view: "agents" });
  window.providerCard.height = 900;
  window.show("providers");
  await window.frame();
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 720 });
  assert.deepEqual(window.sizes, []);
});

test("delayed Provider constraints cannot resize Agent or replace its final constraints", async (t) => {
  const window = await mountSnap(t);
  window.providerCard.height = 500;
  const waiting = window.block("set_size_constraints");
  await window.resize(760, 800);
  window.release();
  await settle();
  assert.deepEqual(window.constraints.at(-1), { width: 376, height: 629 });
  window.show("agents");
  await window.frame();
  waiting.resolve();
  await settle();
  assert.deepEqual(window.sizes, []);
  assert.deepEqual(window.appliedConstraints, { width: 376, height: 480 });
});

test("late window mode results do not snap the next view or block its resize", async (t) => {
  const window = await mountSnap(t);
  await window.resize(760, 800);
  const waiting = window.block("is_maximized");
  window.release();
  await settle();
  window.show("agents");
  await window.frame();
  await window.resize(760, 640);
  window.release();
  await settle();
  assert.deepEqual(window.sizes, [{ width: 728, height: 644 }]);
  waiting.resolve();
  await settle();
  assert.deepEqual(window.sizes, [{ width: 728, height: 644 }]);
});

test("a newer resize invalidates a pending release check in the same view", async (t) => {
  const window = await mountSnap(t);
  await window.resize(760, 800);
  const waiting = window.block("is_maximized");
  window.release();
  await settle();
  await window.resize(1080, 800);
  waiting.resolve();
  await settle();
  assert.deepEqual(window.sizes, []);
  await window.timer(520);
  assert.deepEqual(window.sizes, [{ width: 1080, height: 769 }]);
});

test("maximized Provider spacing stays with CSS and both card views skip snapping", async (t) => {
  const window = await mountSnap(t, { maximized: true });
  await window.resize(1600, 1000);
  window.release();
  await settle();
  assert.equal(window.root.attributes.has("data-window-expanded"), true);
  assert.equal(window.providerGrid.styles.get("--provider-grid-local-gap"), undefined);
  assert.equal(window.root.styles.get("--provider-grid-gap"), "16px");
  window.show("agents");
  await window.frame();
  await window.resize(1400, 900);
  window.release();
  await settle();
  assert.equal(window.agentGrid.styles.get("--provider-grid-local-gap"), "36px");
  assert.deepEqual(window.sizes, []);
});

test("a failed native resize releases its guard so another resize can finish", async (t) => {
  const window = await mountSnap(t);
  const waiting = window.block("set_size");
  await window.resize(760, 800);
  window.release();
  await settle();
  waiting.reject(new Error("native resize failed"));
  await settle();
  await window.timer(80);
  await window.resize(1080, 800);
  window.release();
  await settle();
  assert.deepEqual(window.sizes, [{ width: 760, height: 769 }, { width: 1080, height: 769 }]);
});
