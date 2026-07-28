import { onBeforeUnmount, onMounted } from "vue";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

const FALLBACK_CARD_WIDTH = 336;
const FALLBACK_CARD_HEIGHT = 350;
const FALLBACK_GRID_GAP = 16;
const FALLBACK_HORIZONTAL_CHROME = 40;
const FALLBACK_VERTICAL_CHROME = 129;
const MAX_GRID_GAP = 36;
const MAX_SECTION_GAP = 24;
const MAX_ACTION_GAP = 4;
const RESIZE_SETTLE_DELAY_MS = 520;

interface GridGeometry {
  cardWidth: number;
  cardHeight: number;
  columnGap: number;
  rowGap: number;
  horizontalChrome: number;
  verticalChrome: number;
}

interface LogicalWindowSize {
  width: number;
  height: number;
}

function readPixels(value: string, fallback: number) {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) ? parsed : fallback;
}

function readGridGeometry(): GridGeometry {
  const board = document.querySelector<HTMLElement>(".provider-board");
  const grid = board?.querySelector<HTMLElement>(".overview-provider-grid");
  const topbar = document.querySelector<HTMLElement>(".topbar");
  const section = board?.querySelector<HTMLElement>(".provider-board-section");
  const sectionHeader = section?.querySelector<HTMLElement>(".provider-board-section-header");
  const cards = Array.from(
    document.querySelectorAll<HTMLElement>(".provider-card:not(.provider-card-dragging)"),
  );

  const cardWidth = Math.max(
    FALLBACK_CARD_WIDTH,
    ...cards.map((card) => Math.round(card.getBoundingClientRect().width)),
  );
  const cardHeight = Math.max(
    FALLBACK_CARD_HEIGHT,
    ...cards.map((card) => Math.round(card.getBoundingClientRect().height)),
  );

  const boardStyle = board ? getComputedStyle(board) : null;
  const gridStyle = grid ? getComputedStyle(grid) : null;
  const sectionStyle = section ? getComputedStyle(section) : null;
  const horizontalPadding = boardStyle
    ? readPixels(boardStyle.paddingLeft, 20) + readPixels(boardStyle.paddingRight, 20)
    : FALLBACK_HORIZONTAL_CHROME;
  const verticalPadding = boardStyle
    ? readPixels(boardStyle.paddingTop, 16) + readPixels(boardStyle.paddingBottom, 24)
    : 40;
  const topbarHeight = topbar?.getBoundingClientRect().height ?? 64;
  const sectionHeaderHeight = sectionHeader?.getBoundingClientRect().height ?? 17;
  const sectionGap = sectionStyle ? readPixels(sectionStyle.rowGap, 8) : 8;
  const scrollbarWidth = board ? Math.max(0, board.offsetWidth - board.clientWidth) : 0;

  return {
    cardWidth,
    cardHeight,
    columnGap: gridStyle ? readPixels(gridStyle.columnGap, FALLBACK_GRID_GAP) : FALLBACK_GRID_GAP,
    rowGap: gridStyle ? readPixels(gridStyle.rowGap, FALLBACK_GRID_GAP) : FALLBACK_GRID_GAP,
    horizontalChrome: Math.ceil(horizontalPadding + scrollbarWidth),
    verticalChrome: board
      ? Math.ceil(topbarHeight + verticalPadding + sectionHeaderHeight + sectionGap)
      : FALLBACK_VERTICAL_CHROME,
  };
}

function snapToTrack(value: number, track: number, gap: number, chrome: number) {
  const count = Math.max(1, Math.round((value - chrome + gap) / (track + gap)));
  return Math.round(chrome + count * track + (count - 1) * gap);
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}

function minimumSize(geometry: GridGeometry): LogicalWindowSize {
  return {
    width: Math.round(geometry.horizontalChrome + geometry.cardWidth),
    height: Math.round(geometry.verticalChrome + geometry.cardHeight),
  };
}

export function useWindowGridSnap() {
  let unlistenResize: (() => void) | null = null;
  let mutationObserver: MutationObserver | null = null;
  let unlistenWindowEvent: (() => void) | null = null;
  let unlistenBrowserResize: (() => void) | null = null;
  let geometryFrame: number | null = null;
  let resizeSettleTimer: number | null = null;
  let scaleFactor = 1;
  let applyingSize = false;
  let lastObservedSize: LogicalWindowSize | null = null;
  let lastConstraintSize: LogicalWindowSize | null = null;
  let resizeInProgress = false;
  let expandedWindow = false;
  let releaseCheckInProgress = false;
  let resizeRevision = 0;

  const clearScheduledGeometryRefresh = () => {
    if (geometryFrame !== null) {
      window.cancelAnimationFrame(geometryFrame);
      geometryFrame = null;
    }
  };

  const clearResizeSettleTimer = () => {
    if (resizeSettleTimer !== null) {
      window.clearTimeout(resizeSettleTimer);
      resizeSettleTimer = null;
    }
  };

  const setConstraints = async (geometry: GridGeometry) => {
    const minimum = minimumSize(geometry);
    if (
      lastConstraintSize &&
      lastConstraintSize.width === minimum.width &&
      lastConstraintSize.height === minimum.height
    ) {
      return;
    }
    try {
      await getCurrentWindow().setSizeConstraints({
        minWidth: minimum.width,
        minHeight: minimum.height,
      });
      lastConstraintSize = minimum;
    } catch {
      // The Vite preview and non-desktop environments do not expose Tauri window controls.
    }
  };

  const snapSize = async (requested: LogicalWindowSize) => {
    if (applyingSize || expandedWindow) {
      return;
    }

    const geometry = readGridGeometry();
    await setConstraints(geometry);
    const target = {
      width: Math.max(
        minimumSize(geometry).width,
        snapToTrack(
          requested.width,
          geometry.cardWidth,
          geometry.columnGap,
          geometry.horizontalChrome,
        ),
      ),
      height: Math.max(
        minimumSize(geometry).height,
        snapToTrack(
          requested.height,
          geometry.cardHeight,
          geometry.rowGap,
          geometry.verticalChrome,
        ),
      ),
    };

    if (target.width === Math.round(requested.width) && target.height === Math.round(requested.height)) {
      return;
    }

    applyingSize = true;
    try {
      await getCurrentWindow().setSize(new LogicalSize(target.width, target.height));
      lastObservedSize = target;
    } catch {
      // Ignore resize calls made while the app is running outside Tauri.
    } finally {
      window.setTimeout(() => {
        applyingSize = false;
      }, 80);
    }
  };

  const updateResponsiveSpacing = (geometry: GridGeometry) => {
    const root = document.documentElement;
    const board = document.querySelector<HTMLElement>(".provider-board");
    const grids = Array.from(document.querySelectorAll<HTMLElement>(".overview-provider-grid"));
    const baseGap = FALLBACK_GRID_GAP;
    const cards = Array.from(
      document.querySelectorAll<HTMLElement>(".provider-card:not(.provider-card-dragging)"),
    );

    for (const grid of grids) {
      grid.style.setProperty("--provider-grid-local-gap", `${baseGap}px`);
      grid.style.setProperty("--provider-grid-local-row-gap", `${baseGap}px`);
    }
    for (const card of cards) {
      card.style.setProperty("--provider-card-action-gap", "1px");
    }

    root.style.setProperty("--provider-grid-gap", `${baseGap}px`);
    root.style.setProperty("--provider-card-action-gap", "1px");
    root.toggleAttribute("data-window-expanded", expandedWindow);

    if (!board) {
      return;
    }

    board.style.setProperty("--provider-board-section-gap", `${baseGap}px`);
    board.style.setProperty("--provider-board-content-justify", "flex-start");

    const gridLayouts = new Map<HTMLElement, { columns: number; rows: number; slack: number }>();

    for (const grid of grids) {
      const cardCount = grid.querySelectorAll(":scope > .provider-card").length;
      const availableWidth = Math.max(geometry.cardWidth, grid.clientWidth);
      const maximumColumns = Math.max(
        1,
        Math.floor((availableWidth + baseGap) / (geometry.cardWidth + baseGap)),
      );
      const columns = Math.max(1, Math.min(maximumColumns, cardCount || 1));
      const extra = Math.max(
        0,
        availableWidth - columns * geometry.cardWidth - (columns - 1) * baseGap,
      );
      const distributedGap =
        expandedWindow && columns > 1
          ? clamp(baseGap + extra / (columns - 1), baseGap, MAX_GRID_GAP)
          : baseGap;
      grid.style.setProperty("--provider-grid-local-gap", `${Math.round(distributedGap)}px`);
      gridLayouts.set(grid, {
        columns,
        rows: Math.max(1, Math.ceil((cardCount || 1) / columns)),
        slack: Math.max(0, extra - (distributedGap - baseGap) * Math.max(0, columns - 1)),
      });
    }

    if (expandedWindow) {
      for (const grid of grids) {
        const desiredGap = clamp(1 + (gridLayouts.get(grid)?.slack ?? 0) / 120, 1, MAX_ACTION_GAP);
        for (const card of grid.querySelectorAll<HTMLElement>(":scope > .provider-card")) {
          const footer = card.querySelector<HTMLElement>(".provider-card-footer");
          const meta = footer?.querySelector<HTMLElement>(".provider-card-footer-meta");
          const actions = footer?.querySelector<HTMLElement>(".provider-card-quick-actions");
          const actionGroups = actions
            ? Array.from(actions.querySelectorAll<HTMLElement>(".provider-card-action-group"))
            : [];
          const gapSlots = actionGroups.reduce(
            (total, group) => total + Math.max(0, group.children.length - 1),
            0,
          );
          if (!footer || !meta || !actions || gapSlots === 0) {
            continue;
          }

          const footerGap = readPixels(getComputedStyle(footer).columnGap, 8);
          const availableSlack = Math.max(
            0,
            footer.clientWidth - meta.getBoundingClientRect().width - actions.getBoundingClientRect().width - footerGap,
          );
          const safeGap = Math.min(desiredGap, 1 + availableSlack / gapSlots);
          card.style.setProperty("--provider-card-action-gap", `${safeGap.toFixed(1)}px`);
        }
      }
    }

    const sections = Array.from(
      board.querySelectorAll<HTMLElement>(":scope > .provider-board-section"),
    );
    if (!expandedWindow || sections.length === 0) {
      return;
    }

    const boardStyle = getComputedStyle(board);
    const availableHeight = Math.max(
      0,
      board.clientHeight -
        readPixels(boardStyle.paddingTop, 16) -
        readPixels(boardStyle.paddingBottom, 24),
    );
    const baseContentHeight =
      sections.reduce((total, section) => total + section.getBoundingClientRect().height, 0) +
      Math.max(0, sections.length - 1) * baseGap;
    let remainingHeight = Math.max(0, availableHeight - baseContentHeight);
    const rowGapSlots = grids.reduce(
      (total, grid) => total + Math.max(0, (gridLayouts.get(grid)?.rows ?? 1) - 1),
      0,
    );

    if (rowGapSlots > 0) {
      const rowGap = clamp(baseGap + remainingHeight / rowGapSlots, baseGap, MAX_GRID_GAP);
      for (const grid of grids) {
        grid.style.setProperty("--provider-grid-local-row-gap", `${Math.round(rowGap)}px`);
      }
      remainingHeight = Math.max(0, remainingHeight - (rowGap - baseGap) * rowGapSlots);
    }

    const sectionGapSlots = Math.max(0, sections.length - 1);
    if (sectionGapSlots > 0) {
      const sectionGap = clamp(
        baseGap + remainingHeight / sectionGapSlots,
        baseGap,
        MAX_SECTION_GAP,
      );
      board.style.setProperty("--provider-board-section-gap", `${Math.round(sectionGap)}px`);
      remainingHeight = Math.max(
        0,
        remainingHeight - (sectionGap - baseGap) * sectionGapSlots,
      );
    }

    board.style.setProperty(
      "--provider-board-content-justify",
      remainingHeight > 0 ? "center" : "flex-start",
    );
  };

  const refreshWindowMode = async () => {
    try {
      const appWindow = getCurrentWindow();
      expandedWindow = (await appWindow.isMaximized()) || (await appWindow.isFullscreen());
    } catch {
      expandedWindow = false;
    }
    updateResponsiveSpacing(readGridGeometry());
  };

  const requestSnapAfterRelease = async (expectedRevision?: number) => {
    if (expectedRevision === undefined) {
      clearResizeSettleTimer();
    }
    if (!resizeInProgress || !lastObservedSize || releaseCheckInProgress) {
      return;
    }

    releaseCheckInProgress = true;
    await refreshWindowMode();
    releaseCheckInProgress = false;
    if (expectedRevision !== undefined && expectedRevision !== resizeRevision) {
      return;
    }
    if (!resizeInProgress || !lastObservedSize) {
      return;
    }
    if (expandedWindow) {
      resizeInProgress = false;
      return;
    }

    const requested = { ...lastObservedSize };
    resizeInProgress = false;
    await snapSize(requested);
  };

  const observeSize = (requested: LogicalWindowSize) => {
    lastObservedSize = requested;
    resizeInProgress = true;
    resizeRevision += 1;
    const observedRevision = resizeRevision;
    clearResizeSettleTimer();
    resizeSettleTimer = window.setTimeout(() => {
      resizeSettleTimer = null;
      void requestSnapAfterRelease(observedRevision);
    }, RESIZE_SETTLE_DELAY_MS);
    updateResponsiveSpacing(readGridGeometry());
  };

  const scheduleGeometryRefresh = () => {
    if (geometryFrame !== null) {
      return;
    }
    geometryFrame = window.requestAnimationFrame(() => {
      geometryFrame = null;
      const geometry = readGridGeometry();
      void setConstraints(geometry);
      updateResponsiveSpacing(geometry);
    });
  };

  onMounted(async () => {
    try {
      const appWindow = getCurrentWindow();
      scaleFactor = await appWindow.scaleFactor();
      await refreshWindowMode();
      const currentSize = await appWindow.innerSize();
      lastObservedSize = {
        width: currentSize.width / scaleFactor,
        height: currentSize.height / scaleFactor,
      };

      unlistenResize = await appWindow.onResized(({ payload }) => {
        if (applyingSize) {
          return;
        }
        void refreshWindowMode();
        observeSize(
          {
            width: payload.width / scaleFactor,
            height: payload.height / scaleFactor,
          },
        );
      });

      // Native resize borders can keep the pointer outside the WebView. Pointer
      // release snaps immediately; the resize-settle timer above is the fallback
      // for native drags whose release event never reaches the WebView.
      const release = () => void requestSnapAfterRelease();
      const releaseOnPointerReturn = (event: MouseEvent | PointerEvent) => {
        if (event.buttons === 0) {
          void requestSnapAfterRelease();
        }
      };
      window.addEventListener("pointerup", release, true);
      window.addEventListener("mouseup", release, true);
      window.addEventListener("pointermove", releaseOnPointerReturn, true);
      window.addEventListener("mouseenter", releaseOnPointerReturn, true);
      unlistenWindowEvent = () => {
        window.removeEventListener("pointerup", release, true);
        window.removeEventListener("mouseup", release, true);
        window.removeEventListener("pointermove", releaseOnPointerReturn, true);
        window.removeEventListener("mouseenter", releaseOnPointerReturn, true);
      };

      const updateForBrowserResize = () => updateResponsiveSpacing(readGridGeometry());
      window.addEventListener("resize", updateForBrowserResize);
      unlistenBrowserResize = () => window.removeEventListener("resize", updateForBrowserResize);

      mutationObserver = new MutationObserver(scheduleGeometryRefresh);
      mutationObserver.observe(document.body, { childList: true, subtree: true });
      scheduleGeometryRefresh();
    } catch {
      // The app can still be rendered by Vite without a native window.
    }
  });

  onBeforeUnmount(() => {
    clearScheduledGeometryRefresh();
    clearResizeSettleTimer();
    mutationObserver?.disconnect();
    mutationObserver = null;
    unlistenResize?.();
    unlistenResize = null;
    unlistenWindowEvent?.();
    unlistenWindowEvent = null;
    unlistenBrowserResize?.();
    unlistenBrowserResize = null;
  });
}
