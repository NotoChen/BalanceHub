import { onBeforeUnmount, onMounted } from "vue";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import appConfig from "../../src-tauri/tauri.conf.json" with { type: "json" };

const FALLBACK_CARD_WIDTH = 336;
const FALLBACK_CARD_HEIGHT = 350;
const FALLBACK_GRID_GAP = 16;
const MAX_GRID_GAP = 36;
const RESIZE_SETTLE_DELAY_MS = 520;
const DEFAULT_MINIMUM_SIZE = {
  width: appConfig.app.windows[0].minWidth,
  height: appConfig.app.windows[0].minHeight,
};

interface GridGeometry {
  view: "providers" | "agents";
  cardWidth: number;
  minimumCardWidth?: number;
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

function isVisible(element: HTMLElement | null | undefined): element is HTMLElement {
  if (!element) return false;
  const bounds = element.getBoundingClientRect();
  return bounds.width > 0 && bounds.height > 0;
}

function readGridGeometry(): GridGeometry | null {
  const board = document.querySelector<HTMLElement>(".provider-board");
  if (!isVisible(board)) {
    const dashboard = document.querySelector<HTMLElement>(".agent-dashboard");
    const viewport = dashboard?.querySelector<HTMLElement>(".agent-dashboard-content");
    const grid = viewport?.querySelector<HTMLElement>(".agent-overview-grid");
    if (!isVisible(dashboard) || !isVisible(viewport) || !isVisible(grid)) return null;
    const cards = Array.from(grid.querySelectorAll<HTMLElement>(".agent-overview-card"))
      .filter(isVisible);
    if (cards.length === 0) return null;

    const viewportStyle = getComputedStyle(viewport);
    const gridStyle = getComputedStyle(grid);
    const topbar = document.querySelector<HTMLElement>(".topbar");
    const gridOffset = grid.getBoundingClientRect().top - dashboard.getBoundingClientRect().top
      + viewport.scrollTop;
    return {
      view: "agents",
      cardWidth: Math.max(...cards.map((card) => Math.round(card.getBoundingClientRect().width))),
      cardHeight: Math.max(...cards.map((card) => Math.round(card.getBoundingClientRect().height))),
      columnGap: readPixels(gridStyle.columnGap, FALLBACK_GRID_GAP),
      rowGap: readPixels(gridStyle.rowGap, FALLBACK_GRID_GAP),
      horizontalChrome: Math.ceil(
        readPixels(viewportStyle.paddingLeft, 20) + readPixels(viewportStyle.paddingRight, 20)
          + Math.max(0, viewport.offsetWidth - viewport.clientWidth),
      ),
      verticalChrome: Math.ceil(
        (topbar?.getBoundingClientRect().height ?? 64) + gridOffset
          + readPixels(viewportStyle.paddingBottom, 24),
      ),
    };
  }
  const grid = board.querySelector<HTMLElement>(".overview-provider-grid");
  const topbar = document.querySelector<HTMLElement>(".topbar");
  const section = board.querySelector<HTMLElement>(".provider-board-section");
  const sectionHeader = section?.querySelector<HTMLElement>(".provider-board-section-header");
  const cards = Array.from(
    board.querySelectorAll<HTMLElement>(".provider-card:not(.provider-card-dragging)"),
  ).filter(isVisible);

  const cardWidth = cards.length
    ? Math.max(...cards.map((card) => Math.round(card.getBoundingClientRect().width)))
    : FALLBACK_CARD_WIDTH;
  const cardHeight = cards.length
    ? Math.max(...cards.map((card) => Math.round(card.getBoundingClientRect().height)))
    : FALLBACK_CARD_HEIGHT;

  const boardStyle = getComputedStyle(board);
  const gridStyle = grid ? getComputedStyle(grid) : null;
  const sectionStyle = section ? getComputedStyle(section) : null;
  const horizontalPadding = readPixels(boardStyle.paddingLeft, 20) + readPixels(boardStyle.paddingRight, 20);
  const verticalPadding = readPixels(boardStyle.paddingTop, 16) + readPixels(boardStyle.paddingBottom, 24);
  const topbarHeight = topbar?.getBoundingClientRect().height ?? 64;
  const sectionHeaderHeight = sectionHeader?.getBoundingClientRect().height ?? 17;
  const sectionGap = sectionStyle ? readPixels(sectionStyle.rowGap, 8) : 8;
  const scrollbarWidth = Math.max(0, board.offsetWidth - board.clientWidth);

  return {
    view: "providers",
    cardWidth,
    minimumCardWidth: readPixels(boardStyle.getPropertyValue("--provider-grid-card-min"), FALLBACK_CARD_WIDTH),
    cardHeight,
    columnGap: gridStyle ? readPixels(gridStyle.columnGap, FALLBACK_GRID_GAP) : FALLBACK_GRID_GAP,
    rowGap: gridStyle ? readPixels(gridStyle.rowGap, FALLBACK_GRID_GAP) : FALLBACK_GRID_GAP,
    horizontalChrome: Math.ceil(horizontalPadding + scrollbarWidth),
    verticalChrome: Math.ceil(topbarHeight + verticalPadding + sectionHeaderHeight + sectionGap),
  };
}

function snapToTrack(value: number, track: number, gap: number, chrome: number) {
  const count = Math.max(1, Math.round((value - chrome + gap) / (track + gap)));
  return Math.round(chrome + count * track + (count - 1) * gap);
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.min(maximum, Math.max(minimum, value));
}

function minimumSize(geometry: GridGeometry | null): LogicalWindowSize {
  if (!geometry) return { ...DEFAULT_MINIMUM_SIZE };
  return {
    width: Math.max(
      DEFAULT_MINIMUM_SIZE.width,
      Math.round(geometry.horizontalChrome + (geometry.minimumCardWidth ?? geometry.cardWidth)),
    ),
    height: Math.max(
      geometry.view === "agents" ? DEFAULT_MINIMUM_SIZE.height : 0,
      Math.round(geometry.verticalChrome + geometry.cardHeight),
    ),
  };
}

export function useWindowGridSnap() {
  let unlistenResize: (() => void) | null = null;
  let mutationObserver: MutationObserver | null = null;
  let unlistenWindowEvent: (() => void) | null = null;
  let unlistenBrowserResize: (() => void) | null = null;
  let geometryFrame: number | null = null;
  let resizeSettleTimer: number | null = null;
  let applySizeReleaseTimer: number | null = null;
  let scaleFactor = 1;
  let applyingSize = false;
  let lastObservedSize: LogicalWindowSize | null = null;
  let lastConstraintSize: LogicalWindowSize | null = null;
  let resizeInProgress = false;
  let expandedWindow = false;
  let releaseCheckInProgress = false;
  let resizeRevision = 0;
  let layoutView: GridGeometry["view"] | null = null;
  let layoutRevision = 0;
  let windowModeRevision = 0;
  let releaseCheckRevision = 0;
  let applySizeRevision = 0;
  let constraintUpdate = Promise.resolve();
  let disposed = false;

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

  const clearApplySizeReleaseTimer = () => {
    if (applySizeReleaseTimer !== null) {
      window.clearTimeout(applySizeReleaseTimer);
      applySizeReleaseTimer = null;
    }
  };

  const readCurrentGeometry = () => {
    const geometry = readGridGeometry();
    const view = geometry?.view ?? null;
    if (view !== layoutView) {
      layoutView = view;
      layoutRevision += 1;
      resizeRevision += 1;
      releaseCheckRevision += 1;
      applySizeRevision += 1;
      resizeInProgress = false;
      releaseCheckInProgress = false;
      applyingSize = false;
      clearResizeSettleTimer();
      clearApplySizeReleaseTimer();
    }
    return geometry;
  };

  const isCurrentLayout = (revision: number) => {
    if (disposed) return false;
    readCurrentGeometry();
    return revision === layoutRevision;
  };

  const setConstraints = (geometry: GridGeometry | null) => {
    const expectedLayout = layoutRevision;
    // Serialize native writes so a delayed constraint update from the previous
    // view cannot finish after the current view's constraints.
    constraintUpdate = constraintUpdate.then(async () => {
      if (!isCurrentLayout(expectedLayout)) return;
      const minimum = minimumSize(geometry);
      const current = lastObservedSize;
      if (current) {
        // Changing views updates the resize bounds without growing the window.
        minimum.width = Math.min(minimum.width, Math.round(current.width));
        minimum.height = Math.min(minimum.height, Math.round(current.height));
      }
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
        lastConstraintSize = isCurrentLayout(expectedLayout) ? minimum : null;
      } catch {
        // The Vite preview and non-desktop environments do not expose Tauri window controls.
      }
    });
    return constraintUpdate;
  };

  const snapSize = async (requested: LogicalWindowSize, expectedResize: number) => {
    if (disposed || applyingSize || expandedWindow) {
      return;
    }

    const geometry = readCurrentGeometry();
    if (!geometry) return;
    const expectedLayout = layoutRevision;
    await setConstraints(geometry);
    if (!isCurrentLayout(expectedLayout) || expectedResize !== resizeRevision || expandedWindow) return;
    const target = {
      // Provider columns fill the available width; snapping to a measured,
      // stretched card would fight the width the user has just chosen.
      width: geometry.view === "providers" ? Math.round(requested.width) : Math.max(
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

    const appliedRevision = ++applySizeRevision;
    applyingSize = true;
    try {
      await getCurrentWindow().setSize(new LogicalSize(target.width, target.height));
      if (isCurrentLayout(expectedLayout) && expectedResize === resizeRevision) {
        lastObservedSize = target;
      }
    } catch {
      // Ignore resize calls made while the app is running outside Tauri.
    } finally {
      if (appliedRevision === applySizeRevision) {
        clearApplySizeReleaseTimer();
        if (disposed) {
          applyingSize = false;
        } else {
          applySizeReleaseTimer = window.setTimeout(() => {
            applySizeReleaseTimer = null;
            applyingSize = false;
          }, 80);
        }
      }
    }
  };

  const updateResponsiveSpacing = (geometry: GridGeometry | null) => {
    if (disposed) return;
    const root = document.documentElement;
    root.toggleAttribute("data-window-expanded", expandedWindow);
    root.style.setProperty("--provider-grid-gap", `${FALLBACK_GRID_GAP}px`);

    // Provider columns and gaps are owned by the responsive CSS grid.
    if (geometry?.view !== "agents") return;
    const dashboard = document.querySelector<HTMLElement>(".agent-dashboard");
    const viewport = dashboard?.querySelector<HTMLElement>(".agent-dashboard-content");
    const grid = viewport?.querySelector<HTMLElement>(".agent-overview-grid");
    if (!viewport || !grid) return;

    const baseGap = FALLBACK_GRID_GAP;
    grid.style.setProperty("--provider-grid-local-gap", `${baseGap}px`);
    grid.style.setProperty("--provider-grid-local-row-gap", `${baseGap}px`);
    if (!expandedWindow) return;

    const cardCount = grid.querySelectorAll(":scope > .agent-overview-card").length;
    const availableWidth = Math.max(geometry.cardWidth, grid.clientWidth);
    const columns = Math.max(1, Math.min(
      Math.floor((availableWidth + baseGap) / (geometry.cardWidth + baseGap)),
      cardCount || 1,
    ));
    const extra = Math.max(0, availableWidth - columns * geometry.cardWidth - (columns - 1) * baseGap);
    const columnGap = columns > 1
      ? clamp(baseGap + extra / (columns - 1), baseGap, MAX_GRID_GAP)
      : baseGap;
    // Rounding up can exceed the measured width and push a whole column out.
    grid.style.setProperty("--provider-grid-local-gap", `${Math.floor(columnGap)}px`);

    const rows = Math.max(1, Math.ceil((cardCount || 1) / columns));
    if (rows > 1) {
      const gridOffset = grid.getBoundingClientRect().top - viewport.getBoundingClientRect().top
        + viewport.scrollTop;
      const availableHeight = viewport.clientHeight - gridOffset
        - readPixels(getComputedStyle(viewport).paddingBottom, 24);
      const remainingHeight = Math.max(0, availableHeight - grid.getBoundingClientRect().height);
      const rowGap = clamp(baseGap + remainingHeight / (rows - 1), baseGap, MAX_GRID_GAP);
      grid.style.setProperty("--provider-grid-local-row-gap", `${Math.round(rowGap)}px`);
    }
  };

  const refreshWindowMode = async () => {
    if (disposed) return false;
    readCurrentGeometry();
    const expectedLayout = layoutRevision;
    const expectedMode = ++windowModeRevision;
    let expanded = false;
    try {
      const appWindow = getCurrentWindow();
      expanded = (await appWindow.isMaximized()) || (await appWindow.isFullscreen());
    } catch {
      // Non-desktop environments have no expanded native window mode.
    }
    if (!isCurrentLayout(expectedLayout) || expectedMode !== windowModeRevision) return false;
    expandedWindow = expanded;
    updateResponsiveSpacing(readCurrentGeometry());
    return true;
  };

  const requestSnapAfterRelease = async (expectedRevision?: number) => {
    if (disposed) return;
    readCurrentGeometry();
    if (expectedRevision === undefined) {
      clearResizeSettleTimer();
    }
    if (!resizeInProgress || !lastObservedSize || releaseCheckInProgress) {
      return;
    }

    const expectedLayout = layoutRevision;
    const expectedResize = expectedRevision ?? resizeRevision;
    const expectedCheck = ++releaseCheckRevision;
    releaseCheckInProgress = true;
    let modeRefreshed = false;
    try {
      modeRefreshed = await refreshWindowMode();
    } finally {
      if (expectedCheck === releaseCheckRevision) releaseCheckInProgress = false;
    }
    if (!modeRefreshed || !isCurrentLayout(expectedLayout) || expectedResize !== resizeRevision) {
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
    await snapSize(requested, expectedResize);
  };

  const observeSize = (requested: LogicalWindowSize) => {
    if (disposed) return;
    const geometry = readCurrentGeometry();
    const unchanged = lastObservedSize
      && Math.round(lastObservedSize.width) === Math.round(requested.width)
      && Math.round(lastObservedSize.height) === Math.round(requested.height);
    lastObservedSize = requested;
    if (!geometry || unchanged) {
      updateResponsiveSpacing(geometry);
      return;
    }
    resizeInProgress = true;
    resizeRevision += 1;
    const observedRevision = resizeRevision;
    clearResizeSettleTimer();
    resizeSettleTimer = window.setTimeout(() => {
      resizeSettleTimer = null;
      void requestSnapAfterRelease(observedRevision);
    }, RESIZE_SETTLE_DELAY_MS);
    updateResponsiveSpacing(geometry);
  };

  const scheduleGeometryRefresh = () => {
    if (disposed || geometryFrame !== null) {
      return;
    }
    geometryFrame = window.requestAnimationFrame(() => {
      geometryFrame = null;
      if (disposed) return;
      const geometry = readCurrentGeometry();
      void setConstraints(geometry);
      updateResponsiveSpacing(geometry);
    });
  };

  onMounted(async () => {
    disposed = false;
    try {
      const appWindow = getCurrentWindow();
      scaleFactor = await appWindow.scaleFactor();
      if (disposed) return;
      await refreshWindowMode();
      if (disposed) return;
      const currentSize = await appWindow.innerSize();
      if (disposed) return;
      lastObservedSize = {
        width: currentSize.width / scaleFactor,
        height: currentSize.height / scaleFactor,
      };

      const resizeUnlisten = await appWindow.onResized(({ payload }) => {
        if (disposed) {
          return;
        }
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
      if (disposed) {
        resizeUnlisten();
        return;
      }
      unlistenResize = resizeUnlisten;

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

      const updateForBrowserResize = () => updateResponsiveSpacing(readCurrentGeometry());
      window.addEventListener("resize", updateForBrowserResize);
      unlistenBrowserResize = () => window.removeEventListener("resize", updateForBrowserResize);

      mutationObserver = new MutationObserver((mutations) => {
        const contentChanged = mutations.some((mutation) => mutation.type === "childList");
        const visibilityChanged = mutations.some((mutation) => mutation.type === "attributes"
          && (mutation.target as Element).matches(".provider-board, .agent-dashboard"));
        if (!contentChanged && !visibilityChanged) return;
        const previousRevision = layoutRevision;
        readCurrentGeometry();
        // v-show keeps both workspaces mounted. Only visibility transitions need
        // a refresh; our own spacing style writes must not create an RAF loop.
        if (contentChanged || previousRevision !== layoutRevision) scheduleGeometryRefresh();
      });
      mutationObserver.observe(document.body, {
        childList: true,
        subtree: true,
        attributes: true,
        attributeFilter: ["style"],
      });
      scheduleGeometryRefresh();
    } catch {
      // The app can still be rendered by Vite without a native window.
    }
  });

  onBeforeUnmount(() => {
    disposed = true;
    clearScheduledGeometryRefresh();
    clearResizeSettleTimer();
    clearApplySizeReleaseTimer();
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
