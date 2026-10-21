import { beforeEach, expect, mock, test } from 'bun:test';
import type { PaginatedTranscriptsResponse, Transcript } from '../../src/types';

const calls: { meetingId: string; limit: number; offset: number }[] = [];
let pages: (PaginatedTranscriptsResponse | Error)[] = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke: async (command: string, args: { meetingId: string; limit: number; offset: number }) => {
    expect(command).toBe('api_get_meeting_transcripts');
    calls.push(args);
    const result = pages.shift();
    if (result instanceof Error) throw result;
    if (!result) throw new Error('Unexpected extra request');
    return result;
  },
}));
mock.module('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
const { transcriptService } = await import('../../src/services/transcriptService');
const line = (id: string): Transcript => ({ id, text: id, timestamp: '', audio_start_time: 0 });
beforeEach(() => { calls.length = 0; pages = []; });

test('a complete small transcript takes one request and each operation reads current data', async () => {
  pages.push({ transcripts: [line('original')], total_count: 1, has_more: false });
  expect((await transcriptService.getMeetingTranscripts('meeting'))[0].id).toBe('original');
  pages.push({ transcripts: [line('edited')], total_count: 1, has_more: false });
  expect((await transcriptService.getMeetingTranscripts('meeting'))[0].id).toBe('edited');
  expect(calls).toEqual([
    { meetingId: 'meeting', limit: 500, offset: 0 },
    { meetingId: 'meeting', limit: 500, offset: 0 },
  ]);
});

test('large transcripts follow actual page lengths in source order', async () => {
  pages.push(
    { transcripts: [line('one'), line('two')], total_count: 3, has_more: true },
    { transcripts: [line('three')], total_count: 3, has_more: false },
  );
  expect((await transcriptService.getMeetingTranscripts('long')).map(row => row.id)).toEqual(['one', 'two', 'three']);
  expect(calls.map(call => call.offset)).toEqual([0, 2]);
});

test('empty transcripts return no rows without an unnecessary second request', async () => {
  pages.push({ transcripts: [], total_count: 0, has_more: false });
  expect(await transcriptService.getMeetingTranscripts('empty')).toEqual([]);
  expect(calls).toHaveLength(1);
});

test('failure after the first page rejects instead of returning an incomplete transcript', async () => {
  pages.push({ transcripts: [line('one')], total_count: 2, has_more: true }, new Error('service unavailable'));
  await expect(transcriptService.getMeetingTranscripts('long')).rejects.toThrow('service unavailable');
});

test('inconsistent pagination terminates with an error instead of looping', async () => {
  pages.push({ transcripts: [], total_count: 1, has_more: true });
  await expect(transcriptService.getMeetingTranscripts('long')).rejects.toThrow('made no progress');
  expect(calls).toHaveLength(1);
});
