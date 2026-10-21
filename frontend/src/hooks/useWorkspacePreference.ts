'use client';
import { useCallback, useEffect, useState, type SetStateAction } from 'react';

export function useWorkspacePreference<T>(key: string, fallback: T) {
  const [state, setState] = useState({ key, value: fallback, loaded: false });
  const value = state.key === key ? state.value : fallback;
  useEffect(() => {
    let saved = fallback;
    try {
      const raw = localStorage.getItem(`overtalk.${key}`);
      if (raw !== null) {
        const parsed = JSON.parse(raw);
        if (typeof parsed === typeof fallback) saved = parsed;
      }
    } catch { /* use the default */ }
    setState({ key, value: saved, loaded: true });
  }, [key, fallback]);
  useEffect(() => {
    if (state.loaded && state.key === key) {
      try { localStorage.setItem(`overtalk.${key}`, JSON.stringify(state.value)); }
      catch { /* optional preference */ }
    }
  }, [key, state]);
  const setValue = useCallback((next: SetStateAction<T>) => {
    setState(previous => ({ key, loaded: true, value: typeof next === 'function'
      ? (next as (previous: T) => T)(previous.key === key ? previous.value : fallback)
      : next }));
  }, [key, fallback]);
  return [value, setValue] as const;
}
