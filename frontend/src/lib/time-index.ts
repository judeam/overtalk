/** Last item at or before a time in an ascending sequence. Equal timestamps keep
 * the last item, just like the transcript's stable database ordering. */
export function indexAtOrBefore<T>(items: readonly T[], time: number, start: (item: T) => number): number {
  let lo = 0, hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (start(items[mid]) <= time) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

interface Interval { start: number; end: number }

/** Union overlapping speakers once, rather than scanning every clip each frame. */
export function mergeIntervals(intervals: readonly Interval[]): Interval[] {
  const result: Interval[] = [];
  for (const span of [...intervals].sort((a, b) => a.start - b.start)) {
    if (!(span.end > span.start)) continue;
    const last = result[result.length - 1];
    if (last && span.start <= last.end) last.end = Math.max(last.end, span.end);
    else result.push({ start: span.start, end: span.end });
  }
  return result;
}

/** Retains an audible overlapping speaker, including one beginning inside a skipped span. */
export function playbackGate(keep: readonly Interval[], skip: readonly Interval[], solo: boolean, time: number): number | null {
  const kept = indexAtOrBefore(keep, time, span => span.start);
  if (kept >= 0 && time < keep[kept].end) return time;
  const next = keep[kept + 1]?.start;
  if (solo) return next ?? null;
  const skipped = indexAtOrBefore(skip, time, span => span.start);
  if (skipped < 0 || time >= skip[skipped].end) return time;
  return Math.min(skip[skipped].end, next ?? Infinity);
}
