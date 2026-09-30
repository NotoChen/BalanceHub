import assert from "node:assert/strict";
import test, { type TestContext } from "node:test";
import { createRenderer, ref } from "vue";
import { useCardDragSort } from "../src/composables/useCardDragSort.ts";
import { mergeVisibleCardOrder } from "../src/utils/card-drag-geometry.ts";
import type { Provider } from "../src/stores/provider-types.ts";

function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

async function settle() {
  for (let index = 0; index < 12; index++) await Promise.resolve();
}

class LayoutElement {
  isConnected = true;
  children: LayoutElement[] = [];
  parent: LayoutElement | null = null;
  dataset: { providerId?: string };
  left = 0;
  width = 300;
  constructor(id?: string) { this.dataset = { providerId: id }; }
  closest(selector: string) { return selector === ".overview-provider-grid" ? this.parent : null; }
  querySelectorAll() { return this.children; }
  getBoundingClientRect() {
    return { left: this.left, right: this.left + this.width, top: 0, bottom: 200, width: this.width, height: 200 } as DOMRect;
  }
  setCards(ids: string[]) {
    this.children = ids.map((id, index) => {
      const card = new LayoutElement(id);
      card.left = index * 316;
      card.parent = this;
      return card;
    });
  }
}

function mountDrag(t: TestContext, initialIds = ["a", "hidden", "c", "outside"]) {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const events = new Map<string, (event: PointerEvent) => void>();
  const classes = new Set<string>();
  const frames = new Map<number, FrameRequestCallback>();
  let frame = 0;
  const globals = {
    HTMLElement: LayoutElement,
    Element: LayoutElement,
    document: { body: { classList: { add: (value: string) => classes.add(value), remove: (value: string) => classes.delete(value) } } },
    window: {
      addEventListener: (name: string, callback: (event: PointerEvent) => void) => events.set(name, callback),
      removeEventListener: (name: string) => events.delete(name),
      requestAnimationFrame: (callback: FrameRequestCallback) => { frames.set(++frame, callback); return frame; },
      cancelAnimationFrame: (id: number) => frames.delete(id),
      setTimeout: (callback: () => void, delay: number) => globalThis.setTimeout(callback, delay),
      clearTimeout: (id: ReturnType<typeof setTimeout>) => globalThis.clearTimeout(id),
    },
  };
  const originals = new Map<string, PropertyDescriptor | undefined>();
  for (const [name, value] of Object.entries(globals)) {
    originals.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { value, configurable: true, writable: true });
  }
  const providers = ref(initialIds.map((id) => ({ identity: { id } }) as Provider));
  const grid = new LayoutElement();
  grid.setCards(["a", "c"]);
  const writes: { ids: string[]; pending: ReturnType<typeof deferred> }[] = [];
  const errors: unknown[] = [];
  let drag!: ReturnType<typeof useCardDragSort<Provider>>;
  const renderer = createRenderer<object, object>({
    patchProp() {}, insert() {}, remove() {},
    createElement: () => ({}), createText: () => ({}), createComment: () => ({}),
    setText() {}, setElementText() {}, parentNode: () => null, nextSibling: () => null,
  });
  const app = renderer.createApp({
    setup() {
      drag = useCardDragSort({
        items: providers,
        getId: (provider) => provider.identity.id,
        gridSelector: ".overview-provider-grid",
        dataId: "providerId",
        dragGroup: (provider) => provider.identity.id === "outside" ? "liveness" : "regular",
        reorder: (ids) => {
          const pending = deferred();
          writes.push({ ids, pending });
          return pending.promise;
        },
        onError: (error) => errors.push(error),
      });
      return () => null;
    },
  });
  app.mount({});
  t.after(() => {
    app.unmount();
    for (const [name, descriptor] of originals) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    }
  });
  function down(id: string) {
    const card = grid.children.find((item) => item.dataset.providerId === id)!;
    const provider = providers.value.find((item) => item.identity.id === id)!;
    drag.handlePointerDown(provider, {
      target: card, currentTarget: card, button: 0, clientX: card.left + 150, clientY: 100,
    } as unknown as PointerEvent);
  }
  function move(x: number) {
    events.get("pointermove")?.({ clientX: x, clientY: 100, preventDefault() {} } as PointerEvent);
  }
  function up() { events.get("pointerup")?.({} as PointerEvent); }
  const order = () => drag.orderedGroups.value.get("regular")!.map((provider) => provider.identity.id);
  return { providers, grid, drag, writes, errors, classes, events, frames, down, move, up, order };
}

test("visible reordering keeps hidden slots and ignores removed or duplicate IDs", () => {
  assert.deepEqual(mergeVisibleCardOrder(["a", "hidden", "c", "other"], ["c", "a"]), ["c", "hidden", "a", "other"]);
  assert.deepEqual(mergeVisibleCardOrder(["a", "new", "c"], ["c", "removed", "a", "c"]), ["c", "new", "a"]);
  assert.deepEqual(mergeVisibleCardOrder(["a", "b"], []), ["a", "b"]);
});

test("filtered drag persists a complete order and a failed save restores the original view", async (t) => {
  const context = mountDrag(t);
  context.down("c"); context.move(80); context.up();
  assert.deepEqual(context.writes[0].ids, ["c", "hidden", "a", "outside"]);
  assert.deepEqual(context.order(), ["c", "hidden", "a"]);
  assert.equal(context.drag.state.id, null);
  assert.equal(context.classes.has("workspace-card-drag-active"), false);
  assert.equal(context.events.size, 0);
  context.writes[0].pending.reject(new Error("保存失败"));
  await settle();
  assert.deepEqual(context.order(), ["a", "hidden", "c"]);
  assert.equal(context.errors.length, 1);
  context.down("c"); context.move(80); context.up();
  assert.equal(context.writes.length, 2);
  context.writes[1].pending.resolve();
  await settle();
});

test("an unfinished save does not block a new drag or clear its preview on late completion", async (t) => {
  const context = mountDrag(t);
  context.down("c"); context.move(80); context.up();
  context.grid.setCards(["c", "a"]);
  context.down("a"); context.move(80);
  assert.equal(context.drag.state.dragging, true);
  t.mock.timers.tick(181);
  assert.equal(context.drag.clickSuppressed.value, true);
  context.writes[0].pending.resolve();
  await settle();
  assert.deepEqual(context.order(), ["c", "hidden", "a"]);
  assert.equal(context.drag.state.id, "a");
  context.drag.reset(true);
  assert.deepEqual(context.order(), ["a", "hidden", "c"]);
  assert.deepEqual(context.errors, []);
});

test("pointer cancellation removes listeners and preview without persisting", (t) => {
  const context = mountDrag(t);
  context.down("c"); context.move(80);
  context.events.get("pointercancel")?.({} as PointerEvent);
  assert.equal(context.writes.length, 0);
  assert.equal(context.drag.state.id, null);
  assert.equal(context.events.size, 0);
  assert.equal(context.frames.size, 0);
  assert.equal(context.classes.has("workspace-card-drag-active"), false);
  t.mock.timers.tick(181);
  assert.equal(context.drag.clickSuppressed.value, false);
});

test("a card that disappears from the current filter cancels its unfinished drag", (t) => {
  const context = mountDrag(t);
  context.down("c"); context.move(80);
  context.grid.children.find((card) => card.dataset.providerId === "c")!.isConnected = false;
  context.up();
  assert.equal(context.writes.length, 0);
  assert.equal(context.drag.state.id, null);
  assert.equal(context.events.size, 0);
  assert.equal(context.classes.has("workspace-card-drag-active"), false);
});
