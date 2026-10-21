import { afterEach, expect, test } from 'bun:test';
import { readNoteDraft, storeNoteDraft, saveNoteDraft } from '../../src/lib/note-drafts';
const memory = new Map<string, string>();
Object.assign(globalThis, { localStorage: { getItem: (k: string) => memory.get(k) ?? null, setItem: (k: string,v: string) => memory.set(k,v), removeItem: (k: string) => memory.delete(k) } });
afterEach(() => memory.clear());

test('a late save retains the newer draft and writes are serialized across remounts', async () => {
  const older = { markdown: 'first' }, newer = { markdown: 'second' };
  let release!: () => void;
  const order: string[] = [];
  storeNoteDraft('meeting', older);
  const first = saveNoteDraft('meeting', older, () => new Promise<void>(resolve => { release = resolve; order.push('first'); }));
  await Promise.resolve(); await Promise.resolve();
  storeNoteDraft('meeting', newer);
  const second = saveNoteDraft('meeting', newer, async () => { expect(readNoteDraft('meeting')).toEqual(newer); order.push('second'); });
  expect(order).toEqual(['first']);
  release(); await first; await second;
  expect(order).toEqual(['first','second']); expect(readNoteDraft('meeting')).toBeNull();
});
test('failed writes keep a recoverable draft and do not poison the next save', async () => {
  const draft = { markdown: 'offline changes' };
  storeNoteDraft('meeting', draft);
  await expect(saveNoteDraft('meeting', draft, async () => { throw Error('offline'); })).rejects.toThrow();
  expect(readNoteDraft('meeting')).toEqual(draft);
  await saveNoteDraft('meeting', draft, async () => {});
  expect(readNoteDraft('meeting')).toBeNull();
});
