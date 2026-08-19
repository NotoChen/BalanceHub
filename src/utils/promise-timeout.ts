export function withTimeout<T>(operation: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  const boundedTimeoutMs = Math.max(1, timeoutMs);
  return new Promise<T>((resolve, reject) => {
    const timeoutId = globalThis.setTimeout(
      () => reject(new Error(message)),
      boundedTimeoutMs,
    );
    operation.then(
      (value) => {
        globalThis.clearTimeout(timeoutId);
        resolve(value);
      },
      (error) => {
        globalThis.clearTimeout(timeoutId);
        reject(error);
      },
    );
  });
}
