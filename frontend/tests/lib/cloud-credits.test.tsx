import { afterEach, expect, mock, test } from 'bun:test';
import React from 'react';
import { act, create } from 'react-test-renderer';

const calls: { command: string; args: any; resolve: (value: any) => void }[] = [];
mock.module('@tauri-apps/api/core', () => ({ invoke: (command: string, args: any) => new Promise(resolve => calls.push({ command, args, resolve })) }));
const { CloudCredits } = await import('../../src/components/CloudCredits');
let renderer: ReturnType<typeof create> | undefined;
afterEach(() => { if (renderer) act(() => renderer!.unmount()); renderer = undefined; calls.length = 0; });
const button = (label: string) => renderer!.root.findAllByType('button').find(node => node.children.join('') === label)!;

test('unavailable billing leaves paid actions disabled and explains local access', async () => {
  act(() => { renderer = create(<CloudCredits onUseCloud={() => {}} />); });
  await act(async () => calls[0].resolve({ configured: false, connected: false }));
  expect(button('One-time').props.disabled).toBe(true);
  expect(button('Monthly').props.disabled).toBe(true);
  expect(JSON.stringify(renderer!.toJSON())).toContain('Local models and your own provider connections keep working');
});

test('funded accounts show balance and deliberately select cloud notes', async () => {
  let selected = 0;
  act(() => { renderer = create(<CloudCredits onUseCloud={() => selected++} />); });
  await act(async () => calls[0].resolve({ configured: true, connected: true, balance: { available_micro_usd: 1_580_000, held_micro_usd: 80_000, suspended: false } }));
  expect(JSON.stringify(renderer!.toJSON())).toContain('$1.58 available');
  act(() => button('Use cloud for notes').props.onClick());
  expect(selected).toBe(1);
});

test('rapid checkout clicks create one request and open only the returned billing link', async () => {
  act(() => { renderer = create(<CloudCredits onUseCloud={() => {}} />); });
  await act(async () => calls[0].resolve({ configured: true, connected: false }));
  act(() => { button('Monthly').props.onClick(); button('Monthly').props.onClick(); });
  expect(calls).toHaveLength(2);
  expect(calls[1].args).toEqual({ action: { op: 'checkout', plan: 'summaries_monthly' } });
  await act(async () => calls[1].resolve({ url: 'https://checkout.stripe.com/c/pay/test' }));
  expect(calls[2].command).toBe('open_external_url');
  await act(async () => calls[2].resolve(null));
});
