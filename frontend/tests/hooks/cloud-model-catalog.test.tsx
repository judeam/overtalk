import { afterEach, beforeEach, expect, mock, test } from 'bun:test';
import React from 'react';
import { act, create } from 'react-test-renderer';

type Reply = { id: string }[];
const pending: { command: string; key: string; resolve: (rows: Reply) => void; reject: (error: Error) => void }[] = [];
mock.module('@tauri-apps/api/core', () => ({
  invoke: (command: string, args: { apiKey: string }) => new Promise<Reply>((resolve, reject) => {
    pending.push({ command, key: args.apiKey, resolve, reject });
  }),
}));
const { useCloudModelCatalog } = await import('../../src/hooks/useCloudModelCatalog');
let value: ReturnType<typeof useCloudModelCatalog>;
let renderer: ReturnType<typeof create> | undefined;
function Reader({ provider = 'openai', apiKey = 'first-key', enabled = true }: {
  provider?: 'openai' | 'claude' | 'groq'; apiKey?: string | null; enabled?: boolean;
}) {
  value = useCloudModelCatalog(provider, apiKey, enabled);
  return null;
}
beforeEach(() => { pending.length = 0; });
afterEach(() => { if (renderer) act(() => renderer!.unmount()); renderer = undefined; });

test('missing credentials and inactive providers use fallbacks without model calls', () => {
  act(() => { renderer = create(<Reader apiKey=" " />); });
  expect(pending).toHaveLength(0);
  expect(value.loading).toBe(false);
  expect(value.models).toContain('gpt-4o');
  act(() => renderer!.update(<Reader provider="groq" enabled={false} />));
  expect(pending).toHaveLength(0);
  expect(value.models).toContain('llama-3.3-70b-versatile');
});

test('active provider uses its typed service command and resolves the model list', async () => {
  act(() => { renderer = create(<Reader provider="claude" />); });
  expect(pending[0]).toMatchObject({ command: 'get_anthropic_models', key: 'first-key' });
  expect(value.loading).toBe(true);
  await act(async () => { pending[0].resolve([{ id: 'account-model' }]); });
  expect(value).toEqual({ models: ['account-model'], loading: false });
});

test('late replies from previous credentials cannot replace the current account list', async () => {
  act(() => { renderer = create(<Reader />); });
  act(() => renderer!.update(<Reader apiKey="second-key" />));
  expect(pending.map(call => call.key)).toEqual(['first-key', 'second-key']);
  await act(async () => { pending[1].resolve([{ id: 'second-account' }]); });
  await act(async () => { pending[0].resolve([{ id: 'first-account' }]); });
  expect(value).toEqual({ models: ['second-account'], loading: false });
  act(() => renderer!.update(<Reader apiKey={null} />));
  expect(value.loading).toBe(false);
  expect(value.models).not.toContain('second-account');
});

test('failed and disabled requests settle safely without overriding fallback state', async () => {
  act(() => { renderer = create(<Reader provider="groq" />); });
  await act(async () => { pending[0].reject(new Error('unavailable')); });
  expect(value.loading).toBe(false);
  expect(value.models).toContain('llama-3.3-70b-versatile');
  act(() => renderer!.update(<Reader provider="groq" apiKey="new-key" />));
  act(() => renderer!.update(<Reader provider="groq" apiKey="new-key" enabled={false} />));
  await act(async () => { pending[1].resolve([{ id: 'late-result' }]); });
  expect(value.loading).toBe(false);
  expect(value.models).not.toContain('late-result');
});
