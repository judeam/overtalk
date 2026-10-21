import { expect, test } from 'bun:test';
import { clock, noteClips, rulerSteps, speechClips, timecode } from '../../src/lib/session';

const sources: ([number, number] | null)[] = [[72, 80], [110, 120], [151, 160], [189, 213], null];

test('decisions span their cited lines and happen at the last one', () => {
  const [clip] = noteClips('## Decisions\n- Search ships before export, first version end of October. [S3][S4]', sources);
  expect(clip.kind).toBe('decision');
  expect([clip.start, clip.end, clip.moment]).toEqual([151, 213, 189]);
  expect(clip.label).toBe('Search ships before export');
  expect(clip.text).toBe('Search ships before export, first version end of October.');
});

test('actions keep their owner and fall back to a written timestamp', () => {
  const clips = noteClips('## Action items\n- **Priya:** PDF tables, two or three days. [S3]\n- **Marco:** release note, once search ships [05:10]\n- (Owner: Lena) confirm dates', sources);
  expect(clips.map((c) => [c.owner, c.label, c.moment])).toEqual([
    ['Priya', 'Priya: PDF tables', 151],
    ['Marco', 'Marco: release note', 310],
  ]);
});

test('items without a moment, missing sources and other sections are left off the timeline', () => {
  const md = '## Summary\n- Context [S1]\n## Decisions\n- Untimed decision\n- Uses a missing line [S5]\n- Points past the transcript [S9]';
  expect(noteClips(md, sources)).toEqual([]);
});

test('turns from one speaker merge across short pauses only', () => {
  const clips = speechClips([
    { speaker_id: 'a', start: 0, end: 4 }, { speaker_id: 'a', start: 5, end: 8 },
    { speaker_id: 'a', start: 20, end: 22 }, { speaker_id: 'b', start: 3, end: 6 },
  ]);
  expect(clips.filter((c) => c.speakerId === 'a').map((c) => [c.start, c.end])).toEqual([[0, 8], [20, 22]]);
  expect(clips.filter((c) => c.speakerId === 'b')).toHaveLength(1);
});

test('time formats', () => {
  expect(timecode(189.7)).toBe('00:03:09');
  expect(clock(189)).toBe('03:09');
  expect(clock(3789)).toBe('1:03:09');
  expect(rulerSteps(462, 2.41).major).toBe(60);
});
