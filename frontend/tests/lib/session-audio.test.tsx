import { afterEach, expect, mock, test } from 'bun:test';
import { createElement } from 'react';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
const requests: Array<{ command: string; args: any }> = [];
let resolves: Array<(source: string) => void> = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke(command: string, args: any) {
    requests.push({ command, args });
    if (command === 'workspace_audio_source') return new Promise<string>((resolve) => resolves.push(resolve));
    return Promise.resolve();
  },
  convertFileSrc: (path: string) => `asset:${path}`,
}));
mock.module('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));

class FakeAudio {
  static made: FakeAudio[] = [];
  listeners: Record<string, Array<() => void>> = {};
  src = '';
  duration = NaN;
  currentTime = 0;
  paused = true;
  preload = '';
  play = mock(() => { this.paused = false; this.emit('play'); return Promise.resolve(); });
  pause = mock(() => { this.paused = true; this.emit('pause'); });
  constructor() { FakeAudio.made.push(this); }
  addEventListener(type: string, fn: () => void) { (this.listeners[type] ??= []).push(fn); }
  emit(type: string) { (this.listeners[type] ?? []).forEach((fn) => fn()); }
  removeAttribute(name: string) { if (name === 'src') this.src = ''; }
  load() {}
}
const frames = new Map<number, FrameRequestCallback>();
let nextFrame = 0;
Object.assign(globalThis, {
  Audio: FakeAudio,
  requestAnimationFrame: (fn: FrameRequestCallback) => { frames.set(++nextFrame, fn); return nextFrame; },
  cancelAnimationFrame: (id: number) => { frames.delete(id); },
});
async function paint() {
  const pending = [...frames.values()]; frames.clear();
  await act(async () => { pending.forEach(fn => fn(0)); });
}
async function readySource(source: string) {
  await act(async () => resolves[0](source));
  await paint(); await paint();
}

const { useSessionAudio } = await import('../../src/components/session/hooks');
let renderer: ReactTestRenderer | undefined;
let audio: ReturnType<typeof useSessionAudio>;
function Probe({ id }: { id: string }) { audio = useSessionAudio(id); return null; }
async function mount(id = 'one') {
  await act(async () => { renderer = create(createElement(Probe, { id })); });
  return FakeAudio.made[FakeAudio.made.length - 1];
}
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined; requests.length = 0; resolves = []; FakeAudio.made = []; frames.clear();
});

test('releases a late audio capability after the meeting has unmounted', async () => {
  await mount();
  await act(async () => renderer!.unmount()); renderer = undefined;
  await act(async () => resolves[0]('http://127.0.0.1:45123/test/recording'));
  expect(requests.some((r) => r.command === 'workspace_release_audio' && r.args.source.includes('/test/recording'))).toBe(true);
});

test('preparing playback waits for the first content paint and cancels on leave', async () => {
  const originalFrame = globalThis.requestAnimationFrame;
  const originalCancel = globalThis.cancelAnimationFrame;
  const callbacks = new Map<number, FrameRequestCallback>();
  let next = 0;
  globalThis.requestAnimationFrame = fn => { callbacks.set(++next, fn); return next; };
  globalThis.cancelAnimationFrame = id => { callbacks.delete(id); };
  try {
    const element = await mount();
    await act(async () => resolves[0]('http://127.0.0.1:45123/test/recording'));
    expect(element.src).toBe('');
    // The first frame may paint the transcript before media setup begins.
    const [id, callback] = [...callbacks][0]; callbacks.delete(id);
    await act(async () => callback(0));
    expect(element.src).toBe('');
    await act(async () => renderer!.unmount()); renderer = undefined;
    expect(callbacks.size).toBe(0);
    expect(element.src).toBe('');
  } finally {
    globalThis.requestAnimationFrame = originalFrame;
    globalThis.cancelAnimationFrame = originalCancel;
  }
});

test('a seek before metadata remembers the play request and clamps to the duration', async () => {
  const element = await mount();
  await readySource('http://127.0.0.1:45123/test/recording');
  expect(element.src).toStartWith('http://127.0.0.1:');
  await act(async () => audio.seek(20, true));
  expect(element.play).not.toHaveBeenCalled();
  expect(audio.clock.get()).toBe(20);
  element.duration = 10;
  await act(async () => element.emit('loadedmetadata'));
  expect(element.currentTime).toBe(10);
  expect(element.play).toHaveBeenCalledTimes(1);
});

test('Play pressed before media preparation is retained until metadata arrives', async () => {
  const element = await mount();
  await act(async () => audio.play());
  expect(element.play).not.toHaveBeenCalled();
  await readySource('http://127.0.0.1:45123/test/recording');
  element.duration = 30;
  await act(async () => element.emit('loadedmetadata'));
  expect(element.play).toHaveBeenCalledTimes(1);
});

test('switching meetings pauses playback and revokes the previous capability', async () => {
  const first = await mount();
  await readySource('http://127.0.0.1:45123/first/recording');
  await act(async () => renderer!.update(createElement(Probe, { id: 'two' })));
  expect(first.pause).toHaveBeenCalled();
  expect(requests.some((r) => r.command === 'workspace_release_audio' && r.args.source.includes('/first/recording'))).toBe(true);
});

test('a playback gate skips muted speech while playing', async () => {
  const element = await mount();
  await readySource('http://127.0.0.1:45123/test/recording');
  element.duration = 60;
  await act(async () => element.emit('loadedmetadata'));
  audio.setGate((t) => (t >= 5 && t < 9 ? 9 : t));
  element.currentTime = 6;
  await act(async () => { audio.play(); });
  await paint();
  expect(element.currentTime).toBe(9);
});
