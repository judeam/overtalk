import { expect, test } from 'bun:test';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { GROUPS, LESSONS, TOURS } from '../../src/components/guide/lessons';
import markers from '../../src/components/guide/markers.json';

const shots = markers as Record<string, Record<string, unknown>>;

test('every chapter has a screenshot and every step points at something on it', () => {
  const missing: string[] = [];
  for (const lesson of LESSONS) {
    expect(existsSync(join(import.meta.dir, '../../public/guide', `${lesson.shot}.webp`))).toBe(true);
    for (const step of lesson.steps) {
      if (step.target && !shots[lesson.shot]?.[step.target]) missing.push(`${lesson.id}: ${step.target} on ${lesson.shot}`);
    }
  }
  expect(missing).toEqual([]);
});

test('chapters are unique, grouped and written in full sentences', () => {
  expect(new Set(LESSONS.map((l) => l.id)).size).toBe(LESSONS.length);
  for (const lesson of LESSONS) {
    expect(GROUPS).toContain(lesson.group);
    for (const step of lesson.steps) expect(step.body.trim().endsWith('.')).toBe(true);
  }
});

test('tours only name controls the app marks for the tour', () => {
  const known = new Set(Object.values(shots).flatMap((shot) => Object.keys(shot)));
  const unknown = Object.values(TOURS).flat().map((s) => s.target).filter((t) => !known.has(t));
  expect(unknown).toEqual([]);
});
