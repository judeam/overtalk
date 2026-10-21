import { afterEach, beforeEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { createElement } from 'react';
let ready = true;
let enabled = true;
const invoke = mock(async (command: string) => {
  if (command === 'whisper_get_available_models') return [{ name: 'large-v3-turbo', size_mb: 1600, status: 'Available' }];
  if (command === 'parakeet_get_available_models') return [{ name: 'parakeet-tdt-0.6b-v3-int8', size_mb: 500, status: 'Available' }];
  if (command === 'speech_get_settings') return { enabled, model: 'qwen3-asr-1.7b' };
  if (command === 'speech_health') return { ready };
  throw Error(`Unexpected command ${command}`);
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
// Bun's module mocks are shared across files. Re-establish this suite's backend
// at execution time so another hook suite cannot replace its command responses.
beforeEach(() => mock.module('@tauri-apps/api/core', () => ({ invoke })));
const { useTranscriptionModels } = await import('../../src/hooks/useTranscriptionModels');
let state: ReturnType<typeof useTranscriptionModels>;
let renderer: ReactTestRenderer | undefined;
const config = { provider: 'localSpeech', model: 'qwen3-asr-1.7b' };
function Probe() { state = useTranscriptionModels(config); return null; }
afterEach(async () => { if (renderer) await act(async () => renderer!.unmount()); renderer = undefined; ready = true; enabled = true; });
async function load() {
  await act(async () => { renderer = create(createElement(Probe)); });
  await act(async () => { await state.fetchModels(); });
}
describe('local speech selection in saved-audio dialogs', () => {
  test('preserves the configured Qwen baseline when legacy models are also installed', async () => {
    await load();
    expect(state.selectedModelKey).toBe('localSpeech:qwen3-asr-1.7b');
    expect(state.availableModels.some(x => x.provider === 'localSpeech')).toBe(true);
    expect(state.modelAvailabilityError).toBeNull();
  });
  test('an unavailable baseline requires an explicit model choice', async () => {
    ready = false;
    await load();
    expect(state.selectedModelKey).toBe('');
    expect(state.modelAvailabilityError).toContain('Settings');
    expect(state.availableModels.some(x => x.provider === 'whisper')).toBe(true);
  });
  test('refresh preserves a deliberate override', async () => {
    await load();
    await act(async () => state.setSelectedModelKey('whisper:large-v3-turbo'));
    await act(async () => { await state.fetchModels(); });
    expect(state.selectedModelKey).toBe('whisper:large-v3-turbo');
  });
});
