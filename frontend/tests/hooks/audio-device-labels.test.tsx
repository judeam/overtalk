import { afterEach, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { createElement } from 'react';
import type { DesktopAudioDevice } from '../../src/lib/audio-devices';

let read: () => Promise<DesktopAudioDevice[]> = async () => [];
const invoke = mock((command: string) => {
  if (command !== 'get_audio_devices') throw Error(`Unexpected command: ${command}`);
  return read();
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
const { useAudioDeviceLabels } = await import('../../src/hooks/useAudioDeviceLabels');
let labels: ReturnType<typeof useAudioDeviceLabels>;
let renderer: ReactTestRenderer | undefined;
function Probe({ mic, system }: { mic: string | null; system?: string | null }) {
  labels = useAudioDeviceLabels(mic, system);
  return null;
}
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined;
  invoke.mockClear();
  read = async () => [];
});
const microphone: DesktopAudioDevice = { name: 'pulse:mic', display_name: 'Microphone – Shure MV6', device_type: 'Input' };
const speaker: DesktopAudioDevice = { name: 'pulse:sink', display_name: 'Headphones – Built-in Audio', device_type: 'Output' };

test('resolves input and output labels without changing their capture routes', async () => {
  read = async () => [microphone, speaker];
  await act(async () => { renderer = create(createElement(Probe, { mic: 'pulse:mic (input)', system: 'pulse:sink (output)' })); });
  expect(labels.microphoneLabel).toBe(microphone.display_name!);
  expect(labels.systemAudioLabel).toBe(speaker.display_name!);
});

test('a failed device read keeps opaque identifiers out of the interface', async () => {
  read = async () => { throw Error('Desktop audio service unavailable'); };
  await act(async () => { renderer = create(createElement(Probe, { mic: 'pulse:unknown (input)' })); });
  expect(labels.microphoneLabel).toBe('Selected audio device');
});

test('late discovery cannot replace labels after the selected device changes', async () => {
  let settle: (devices: DesktopAudioDevice[]) => void = () => {};
  read = () => new Promise(resolve => { settle = resolve; });
  await act(async () => { renderer = create(createElement(Probe, { mic: 'pulse:mic (input)' })); });
  read = async () => [speaker];
  await act(async () => renderer!.update(createElement(Probe, { mic: null, system: 'pulse:sink (output)' })));
  await act(async () => settle([microphone]));
  expect(labels.microphoneLabel).toBe('Default microphone');
  expect(labels.systemAudioLabel).toBe(speaker.display_name!);
});
