export function retry(fn: () => number): number {
  return fn();
}
