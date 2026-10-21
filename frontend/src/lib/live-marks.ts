// Moments marked during a recording, kept in the session until the meeting is saved.
import { useSyncExternalStore } from 'react';
import { invoke } from '@tauri-apps/api/core';

export interface PendingMark { at: number; label: string }

const KEY = 'overtalk.pendingMarks';
const EMPTY: PendingMark[] = [];
const listeners = new Set<() => void>();
let cache: PendingMark[] | null = null;

function read(): PendingMark[] {
  if (cache) return cache;
  try {
    const parsed = JSON.parse(sessionStorage.getItem(KEY) ?? '[]');
    cache = Array.isArray(parsed) ? parsed.filter((m) => Number.isFinite(m?.at)) : [];
  } catch {
    cache = [];
  }
  return cache;
}

function write(marks: PendingMark[]) {
  cache = marks;
  try { sessionStorage.setItem(KEY, JSON.stringify(marks)); } catch { /* kept in memory only */ }
  listeners.forEach((listener) => listener());
}

export const pendingMarks = {
  list: read,
  add(mark: PendingMark) { write([...read(), mark].sort((a, b) => a.at - b.at)); },
  rename(index: number, label: string) { write(read().map((m, i) => (i === index ? { ...m, label } : m))); },
  remove(index: number) { write(read().filter((_, i) => i !== index)); },
  clear() { write([]); },
  subscribe(listener: () => void) { listeners.add(listener); return () => { listeners.delete(listener); }; },
};

export function usePendingMarks() {
  return useSyncExternalStore(pendingMarks.subscribe, read, () => EMPTY);
}

/** Attach the session's marks to the saved meeting, then forget them. */
export async function savePendingMarks(meetingId: string) {
  const marks = read();
  if (!marks.length) return;
  await invoke('meeting_add_marks', { meetingId, marks: marks.map(({ at, label }) => ({ id: 0, at, label })) });
  write([]);
}
