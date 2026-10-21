import { afterEach, beforeEach, expect, spyOn, test } from 'bun:test';
import {
  IDBDatabase as FakeDatabase, IDBFactory, IDBKeyRange, IDBObjectStore as FakeStore,
  forceCloseDatabase,
} from 'fake-indexeddb';
import { IndexedDBService, type MeetingMetadata } from '../../src/services/indexedDBService';

Object.assign(globalThis, { IDBKeyRange });
let factory: IDBFactory;
let service: IndexedDBService;
const restorers: Array<{ mockRestore(): void }> = [];
beforeEach(() => { factory = new IDBFactory(); service = new IndexedDBService(factory); });
afterEach(() => { restorers.splice(0).reverse().forEach(spy => spy.mockRestore()); });
const meeting = (id: string, extra: Partial<MeetingMetadata> = {}): MeetingMetadata => ({
  meetingId: id, title: id, startTime: 1, lastUpdated: 1, transcriptCount: 0,
  savedToSQLite: false, ...extra,
});
const line = (id: string, sequence_id: number) => ({ id, text: id, timestamp: '12:00', sequence_id });

function abortAfterPut(storeName: string): () => number {
  const put = FakeStore.prototype.put;
  let successes = 0;
  restorers.push(spyOn(FakeStore.prototype, 'put').mockImplementation(function (this: IDBObjectStore, ...args) {
    const request = put.apply(this, args);
    if (this.name === storeName) request.addEventListener('success', () => {
      successes++;
      this.transaction.abort();
    });
    return request;
  }));
  return () => successes;
}

test('concurrent initialization opens once and a failed open can retry', async () => {
  const open = spyOn(factory, 'open');
  restorers.push(open);
  open.mockImplementationOnce(() => { throw new DOMException('temporary failure', 'UnknownError'); });
  await expect(service.init()).rejects.toThrow('temporary failure');
  await Promise.all(Array.from({ length: 16 }, () => service.init()));
  await service.init();
  expect(open).toHaveBeenCalledTimes(2);
  await service.saveMeetingMetadata(meeting('retry'));
  expect((await service.getMeetingMetadata('retry'))?.title).toBe('retry');
});

test('asynchronous database-open errors do not poison the cached initializer', async () => {
  const newer = await new Promise<IDBDatabase>((resolve, reject) => {
    const request = factory.open('SonefoldRecoveryDB', 2);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  newer.close();
  await expect(service.init()).rejects.toThrow();
  await new Promise<void>((resolve, reject) => {
    const request = factory.deleteDatabase('SonefoldRecoveryDB');
    request.onsuccess = () => resolve();
    request.onerror = () => reject(request.error);
  });
  await service.saveMeetingMetadata(meeting('reopened'));
  expect((await service.getMeetingMetadata('reopened'))?.title).toBe('reopened');
});

test('unexpected connection close can reopen without losing committed data', async () => {
  const transaction = FakeDatabase.prototype.transaction;
  let connection: IDBDatabase | undefined;
  restorers.push(spyOn(FakeDatabase.prototype, 'transaction').mockImplementation(function (this: IDBDatabase, ...args) {
    connection = this;
    return transaction.apply(this, args);
  }));
  await service.saveMeetingMetadata(meeting('closed'));
  const closed = new Promise<void>(resolve => connection!.addEventListener('close', () => resolve(), { once: true }));
  // fake-indexeddb 6.2.6 declares a constructor here, but accepts an instance at runtime.
  (forceCloseDatabase as unknown as (db: IDBDatabase) => void)(connection!);
  await closed;
  expect((await service.getMeetingMetadata('closed'))?.title).toBe('closed');
});

test('writes finish only after commit, not on individual request success', async () => {
  const transaction = FakeDatabase.prototype.transaction;
  const events: string[] = [];
  restorers.push(spyOn(FakeDatabase.prototype, 'transaction').mockImplementation(function (this: IDBDatabase, ...args) {
    const tx = transaction.apply(this, args);
    if (tx.mode === 'readwrite') tx.addEventListener('complete', () => events.push('committed'));
    return tx;
  }));
  await service.saveMeetingMetadata(meeting('committed'));
  events.push('returned');
  expect(events).toEqual(['committed', 'returned']);
});

test('aborted replacement rejects and rolls back transcripts and metadata', async () => {
  await service.saveMeetingMetadata(meeting('rollback'));
  await service.replaceTranscriptSnapshot('rollback', [line('original', 0)]);
  const before = await service.getMeetingMetadata('rollback');
  const successes = abortAfterPut('transcripts');
  await expect(service.replaceTranscriptSnapshot('rollback', [line('new', 0), line('other', 1)])).rejects.toThrow();
  expect(successes()).toBe(1);
  expect((await service.getTranscripts('rollback')).map(row => row.text)).toEqual(['original']);
  expect(await service.getMeetingMetadata('rollback')).toEqual(before);
});

test('best-effort checkpoints contain an abort without persisting partial state', async () => {
  const warn = spyOn(console, 'warn').mockImplementation(() => {});
  restorers.push(warn);
  abortAfterPut('meetings');
  await service.saveMeetingMetadata(meeting('discarded'));
  expect(warn).toHaveBeenCalledTimes(1);
  expect(await service.getMeetingMetadata('discarded')).toBeNull();
});

test('snapshot revisions remove stale segments, sort sequences and retain speaker/timing fields', async () => {
  await service.saveMeetingMetadata(meeting('revision'));
  await service.replaceTranscriptSnapshot('revision', [line('stale', 0)]);
  await service.replaceTranscriptSnapshot('revision', [
    { ...line('later', 9), speaker_id: 'speaker-2', audio_start_time: 4, overlap: true }, line('first', 0),
  ]);
  const rows = await service.getTranscripts('revision');
  expect(rows.map(row => row.id)).toEqual(['first', 'later']);
  expect(rows[1]).toMatchObject({ speaker_id: 'speaker-2', audio_start_time: 4, overlap: true, sequenceId: 9 });
  expect((await service.getMeetingMetadata('revision'))?.transcriptCount).toBe(2);
  await service.replaceTranscriptSnapshot('revision', []);
  expect(await service.getTranscripts('revision')).toEqual([]);
  expect((await service.getMeetingMetadata('revision'))?.transcriptCount).toBe(0);
});

test('concurrent appended transcripts retain exact count and sequence order', async () => {
  await service.saveMeetingMetadata(meeting('append'));
  await Promise.all([9, 0, 4].map(sequence_id => service.saveTranscript('append', {
    text: String(sequence_id), timestamp: '12:00', sequence_id,
  })));
  expect((await service.getTranscripts('append')).map(row => row.text)).toEqual(['0', '4', '9']);
  expect((await service.getMeetingMetadata('append'))?.transcriptCount).toBe(3);
});

test('old camelCase and snake_case sequence fields both remain readable', async () => {
  await service.init();
  const db = await new Promise<IDBDatabase>((resolve, reject) => {
    const request = factory.open('SonefoldRecoveryDB', 1);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  await new Promise<void>((resolve, reject) => {
    const tx = db.transaction('transcripts', 'readwrite');
    const store = tx.objectStore('transcripts');
    store.add({ meetingId: 'legacy', text: 'later', sequence_id: 4 });
    store.add({ meetingId: 'legacy', text: 'first', sequenceId: 0 });
    tx.oncomplete = () => resolve();
    tx.onabort = () => reject(tx.error);
  });
  db.close();
  expect((await service.getTranscripts('legacy')).map(row => row.text)).toEqual(['first', 'later']);
});

test('cleanup removes only eligible meetings and their transcripts', async () => {
  for (const metadata of [
    meeting('saved-old', { savedToSQLite: true }), meeting('unsaved-old'),
    meeting('recent', { savedToSQLite: true, lastUpdated: Date.now() }),
  ]) {
    await service.saveMeetingMetadata(metadata);
    await service.replaceTranscriptSnapshot(metadata.meetingId, [line(metadata.meetingId, 0)]);
    await service.saveMeetingMetadata({ ...metadata, transcriptCount: 1 });
  }
  expect(await service.deleteSavedMeetings(24)).toBe(1);
  expect(await service.getMeetingMetadata('saved-old')).toBeNull();
  expect(await service.getTranscripts('saved-old')).toEqual([]);
  expect((await service.getAllMeetings()).map(row => row.meetingId)).toEqual(['unsaved-old']);
  expect(await service.deleteOldMeetings(7)).toBe(1);
  expect(await service.getTranscripts('unsaved-old')).toEqual([]);
  expect((await service.getMeetingMetadata('recent'))?.transcriptCount).toBe(1);
  await service.deleteMeeting('recent');
  expect(await service.getMeetingMetadata('recent')).toBeNull();
  expect(await service.getTranscripts('recent')).toEqual([]);
});

test('read errors retain their safe fallbacks while destructive errors reject', async () => {
  await service.init();
  restorers.push(spyOn(console, 'error').mockImplementation(() => {}));
  restorers.push(spyOn(FakeDatabase.prototype, 'transaction').mockImplementation(() => {
    throw new DOMException('connection unavailable', 'InvalidStateError');
  }));
  expect(await service.getMeetingMetadata('missing')).toBeNull();
  expect(await service.getTranscripts('missing')).toEqual([]);
  expect(await service.getAllMeetings()).toEqual([]);
  expect(await service.deleteOldMeetings(7)).toBe(0);
  await expect(service.deleteMeeting('missing')).rejects.toThrow('connection unavailable');
});

test('a read transaction abort reaches the fallback after its request succeeds', async () => {
  await service.saveMeetingMetadata(meeting('read-abort'));
  restorers.push(spyOn(console, 'error').mockImplementation(() => {}));
  const get = FakeStore.prototype.get;
  restorers.push(spyOn(FakeStore.prototype, 'get').mockImplementation(function (this: IDBObjectStore, ...args) {
    const request = get.apply(this, args);
    request.addEventListener('success', () => this.transaction.abort());
    return request;
  }));
  expect(await service.getMeetingMetadata('read-abort')).toBeNull();
});
