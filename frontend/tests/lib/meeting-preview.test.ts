import { afterEach, expect, mock, test } from 'bun:test';
let size = 20;
const calls: string[] = [];
mock.module('@tauri-apps/api/core', () => ({ invoke: async (command: string, args: { meetingId: string }) => {
  calls.push(command);
  return command === 'api_get_meeting_metadata'
    ? { id: args.meetingId, title: args.meetingId, created_at: '', updated_at: '' }
    : { transcripts: [{ id: args.meetingId, text: 'x'.repeat(size), timestamp: '' }], total_count: 1, has_more: false };
} }));
const { invalidateMeetingPreview, peekMeetingPreview, preloadMeeting } = await import('../../src/lib/meeting-preview');
afterEach(() => { invalidateMeetingPreview(); calls.length = 0; size = 20; });

test('hover, focus and opening share one first-page request', async () => {
  const first = preloadMeeting('one');
  expect(preloadMeeting('one')).toBe(first);
  await first;
  expect(peekMeetingPreview('one')?.page.transcripts[0].id).toBe('one');
  await preloadMeeting('one');
  expect(calls).toHaveLength(2);
  invalidateMeetingPreview('one');
  await preloadMeeting('one');
  expect(calls).toHaveLength(4);
});

test('first-page cache stays bounded and recent access protects a meeting from eviction', async () => {
  for (let i = 0; i < 6; i++) await preloadMeeting(String(i));
  await preloadMeeting('0');
  await preloadMeeting('6');
  expect(peekMeetingPreview('0')).not.toBeNull();
  expect(peekMeetingPreview('1')).toBeNull();
});

test('one huge transcript line is shown without retaining it or evicting other meetings', async () => {
  await preloadMeeting('normal');
  size = 600_000;
  const huge = await preloadMeeting('huge');
  expect(huge.page.transcripts[0].text.length).toBe(size);
  expect(peekMeetingPreview('huge')).toBeNull();
  expect(peekMeetingPreview('normal')).not.toBeNull();
});
