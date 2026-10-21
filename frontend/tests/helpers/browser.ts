// Minimal browser state for viewport-dependent hook tests; restore between cases.
export function installWindowFixture(width = 1440): () => void {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'window');
  Object.defineProperty(globalThis, 'window', {
    configurable: true, value: { innerWidth: width },
  });
  return () => {
    if (original) Object.defineProperty(globalThis, 'window', original);
    else Reflect.deleteProperty(globalThis, 'window');
  };
}
