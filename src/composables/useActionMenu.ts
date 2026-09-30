import {
  computed,
  nextTick,
  onBeforeUnmount,
  ref,
  useId,
  useTemplateRef,
  watch,
  type CSSProperties,
} from "vue";

/** 共用的操作菜单焦点与键盘导航，不等待菜单动作的异步结果。 */
export function useActionMenu(options: { fitViewport?: boolean } = {}) {
  const visible = ref(false);
  const triggerRef = useTemplateRef<HTMLButtonElement>("actionMenuTrigger");
  const menuRef = useTemplateRef<HTMLElement>("actionMenuPanel");
  const menuId = useId();
  const availableHeight = ref<number | null>(null);
  const viewportStyle = computed<CSSProperties>(() =>
    availableHeight.value === null
      ? {}
      : {
          "--action-menu-available-height": `${availableHeight.value}px`,
        },
  );

  function measureViewport() {
    const rect = triggerRef.value?.getBoundingClientRect();
    if (!rect || rect.bottom <= 0 || rect.top >= window.innerHeight) {
      visible.value = false;
      return;
    }
    // 给浮层内边距、箭头和窗口边缘留出空间；Arco 根据空间自动向上或向下展开。
    availableHeight.value = Math.max(
      0,
      Math.max(rect.top, window.innerHeight - rect.bottom) - 40,
    );
  }

  function stopMeasuring() {
    if (!options.fitViewport) return;
    window.removeEventListener("resize", measureViewport);
    window.removeEventListener("scroll", measureViewport, true);
  }

  function menuItems() {
    return Array.from(
      menuRef.value?.querySelectorAll<HTMLElement>(
        ':is(button, summary)[role="menuitem"]:not(:disabled)',
      ) ?? [],
    ).filter((item) => item.getClientRects().length > 0);
  }

  watch(visible, async (value) => {
    if (!value) {
      stopMeasuring();
      return;
    }
    if (options.fitViewport) {
      measureViewport();
      window.addEventListener("resize", measureViewport);
      window.addEventListener("scroll", measureViewport, true);
    }
    await nextTick();
    if (!visible.value) return;
    if (menuRef.value) menuRef.value.scrollTop = 0;
    menuItems()[0]?.focus({ preventScroll: true });
  });
  onBeforeUnmount(stopMeasuring);

  function close() {
    visible.value = false;
    triggerRef.value?.focus({ preventScroll: true });
  }

  function select(event: MouseEvent) {
    // summary 只展开分组，按钮执行具体操作后才收起菜单。
    if (
      event.target instanceof Element &&
      event.target.closest('button[role="menuitem"]:not(:disabled)')
    )
      close();
  }

  function navigate(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    if (event.key === "Tab") {
      close();
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const items = menuItems();
    if (!items.length) return;
    const current = items.indexOf(document.activeElement as HTMLElement);
    const index =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
            items.length;
    items[index]?.focus();
  }

  return { visible, menuId, viewportStyle, select, navigate };
}
