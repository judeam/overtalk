import type { BlockNoteBlock } from '@/types';

export type NoteDraft = { summary_json?: BlockNoteBlock[]; markdown?: string };
const writes = new Map<string, Promise<void>>();
const noteDraftKey = (id: string) => `overtalk.note-draft.${id}`;

export function readNoteDraft(id: string): NoteDraft | null {
  try {
    const value = JSON.parse(localStorage.getItem(noteDraftKey(id)) || 'null');
    return value && (Array.isArray(value.summary_json) || typeof value.markdown === 'string') ? value : null;
  } catch { return null; }
}

export function storeNoteDraft(id: string, draft: NoteDraft) {
  // Keep the in-memory editor usable even if browser storage is full or unavailable.
  try { localStorage.setItem(noteDraftKey(id), JSON.stringify(draft)); } catch { /* save to the native database still runs */ }
}

/** Serialize saves across editor mounts; a late save must never erase a newer draft. */
export function saveNoteDraft(id: string, draft: NoteDraft, persist: (draft: NoteDraft) => Promise<void>): Promise<void> {
  const previous = writes.get(id) ?? Promise.resolve();
  const snapshot = JSON.stringify(draft);
  const next = previous.catch(() => {}).then(async () => {
    await persist(draft);
    try {
      if (localStorage.getItem(noteDraftKey(id)) === snapshot) localStorage.removeItem(noteDraftKey(id));
    } catch { /* storage is optional */ }
  });
  writes.set(id, next);
  void next.finally(() => { if (writes.get(id) === next) writes.delete(id); }).catch(() => {});
  return next;
}
