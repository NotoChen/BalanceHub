/// 首屏可交互之后再执行不影响首屏的工作。没有 requestIdleCallback 的宿主
/// 用短超时兜底；隐藏窗口也能在后台完成一次，不会永远等不到空闲。
export function runWhenIdle(task: () => void, timeoutMs = 1500) {
  if (typeof window !== "undefined") {
    const idleHost = window as Window & {
      requestIdleCallback?: (
        callback: () => void,
        options?: { timeout: number },
      ) => number;
    };
    if (typeof idleHost.requestIdleCallback === "function") {
      idleHost.requestIdleCallback(task, { timeout: timeoutMs });
      return;
    }
  }
  if (typeof setTimeout === "undefined") {
    task();
    return;
  }
  setTimeout(task, Math.min(timeoutMs, 600));
}
