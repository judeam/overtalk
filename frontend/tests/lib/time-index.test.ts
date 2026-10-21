import { expect, test } from 'bun:test';
import { indexAtOrBefore, mergeIntervals, playbackGate } from '../../src/lib/time-index';

test('time lookup preserves equal timestamps, gaps and boundary behavior', () => {
  const times = [1, 2, 2, 8];
  expect([-1, 1, 2, 7, 8, 100].map(t => indexAtOrBefore(times, t, n => n))).toEqual([-1, 0, 2, 2, 3, 3]);
  expect(indexAtOrBefore([], 10, (n: number) => n)).toBe(-1);
});

test('a playback frame examines at most logarithmically many transcript lines', () => {
  const times = Array.from({ length: 100000 }, (_, i) => i * 2);
  let comparisons = 0;
  expect(indexAtOrBefore(times, 180001, n => { comparisons++; return n; })).toBe(90000);
  expect(comparisons).toBeLessThan(18);
});

test('speaker playback unions overlaps without skipping the next audible speaker', () => {
  const skip = mergeIntervals([{ start: 0, end: 10 }, { start: 8, end: 20 }]);
  const keep = mergeIntervals([{ start: 5, end: 7 }, { start: 6, end: 9 }, { start: 25, end: 30 }]);
  expect(skip).toEqual([{ start: 0, end: 20 }]);
  expect(playbackGate(keep, skip, false, 1)).toBe(5);
  expect(playbackGate(keep, skip, false, 6)).toBe(6);
  expect(playbackGate(keep, skip, false, 9)).toBe(20);
  expect(playbackGate(keep, skip, false, 20)).toBe(20);
  expect(playbackGate(keep, skip, true, 20)).toBe(25);
  expect(playbackGate(keep, skip, true, 30)).toBeNull();
});
