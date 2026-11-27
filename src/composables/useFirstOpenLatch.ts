import { ref, watch } from "vue";

/**
 * 按需加载面板的首次打开闸门。第一次可见后保持为 true，让已加载的组件
 * 继续挂载，关闭再打开不会丢草稿、滚动位置和分区状态。
 *
 * `flush: "sync"` 是刻意的：可见标志通常由操作函数同步置位，闸门必须在
 * 同一帧内打开，否则异步组件会错过第一次打开。
 */
export function useFirstOpenLatch(isVisible: () => boolean) {
  const opened = ref(isVisible());
  watch(
    isVisible,
    (visible) => {
      if (visible) opened.value = true;
    },
    { flush: "sync" },
  );
  return opened;
}
