import { expect, test } from 'bun:test';
import { deviceLabel, deviceValue } from '../../src/lib/audio-devices';
test('identical display labels retain distinct stable capture routes', () => {
  const a = { name: 'pulse:routeA', display_name: 'Microphone – USB', device_type: 'Input' as const };
  const b = { ...a, name: 'pulse:routeB' };
  expect(deviceValue(a)).not.toBe(deviceValue(b));
  expect(deviceValue(a)).toBe('pulse:routeA (input)');
  expect(deviceLabel([a, b], deviceValue(b))).toBe('Microphone – USB');
});
test('default and stale opaque routes never leak IDs into visible labels', () => {
  expect(deviceLabel([], null)).toBe('Default microphone');
  expect(deviceLabel([], 'pulse:unknown (input)')).toBe('Selected audio device');
  expect(deviceLabel([], 'Shure MV6 (input)')).toBe('Shure MV6');
});

test('an enumerated route without a friendly label remains opaque', () => {
  expect(deviceLabel([{ name: 'pulse:routeA', device_type: 'Input' }], 'pulse:routeA (input)')).toBe('Selected audio device');
  expect(deviceLabel([], 'default', 'Default system audio')).toBe('Default system audio');
});
