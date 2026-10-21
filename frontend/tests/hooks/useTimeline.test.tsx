import { expect, mock, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import type { MeetingTimeline } from '../../src/lib/session';

const pending: Array<{ id: string; resolve: (value: MeetingTimeline) => void }> = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke: (_: string, args: { meetingId: string }) => new Promise<MeetingTimeline>(resolve => pending.push({ id: args.meetingId, resolve })),
  convertFileSrc: (source: string) => source,
}));
mock.module('@tauri-apps/api/event', () => ({ listen: async () => () => {} }));
const { useTimeline } = await import('../../src/components/session/hooks');

test('rapid meeting switches ignore late timelines from previously opened meetings', async () => {
  let current: ReturnType<typeof useTimeline>;
  function View({ id }: { id: string }) { current = useTimeline(id); return null; }
  let renderer: ReturnType<typeof create>;
  await act(async () => { renderer = create(<View id="old" />); });
  await act(async () => { renderer.update(<View id="new" />); });
  expect(pending.map(request => request.id)).toEqual(['old', 'new']);
  await act(async () => { pending[1].resolve({ duration: 42, speakers: [], turns: [], sources: [] }); });
  expect(current!.loaded).toBe(true);
  expect(current!.timeline.duration).toBe(42);
  await act(async () => { pending[0].resolve({ duration: 999, speakers: [], turns: [], sources: [] }); });
  expect(current!.timeline.duration).toBe(42);
  await act(async () => { renderer.unmount(); });
});
