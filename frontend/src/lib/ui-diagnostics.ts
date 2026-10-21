// Writes interface stalls into the app log, so a report of lag comes with evidence: how long the
// page was blocked and on which screen. Nothing leaves the device. It never wraps Tauri's own
// functions (they are read-only), and a failure here never reaches the interface.

import { invoke } from '@tauri-apps/api/core';

const STALL_MS = 200;
const TICK_MS = 100;

let installed = false;

export function installUiDiagnostics() {
  if (installed || typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return;
  installed = true;
  try {
    const pending: string[] = [];
    // A timer that fires late means the page's main thread was busy for that long.
    let expected = performance.now() + TICK_MS;
    window.setInterval(() => {
      const now = performance.now();
      const late = now - expected;
      expected = now + TICK_MS;
      if (late >= STALL_MS && document.visibilityState === 'visible' && pending.length < 200) {
        pending.push(`stall ${Math.round(late)} ms on ${location.pathname}`);
      }
    }, TICK_MS);
    window.setInterval(() => {
      if (!pending.length) return;
      void invoke('ui_log_diagnostics', { lines: pending.splice(0, pending.length) }).catch(() => {});
    }, 5000);
  } catch (error) {
    console.warn('Interface diagnostics are off:', error);
  }
}
