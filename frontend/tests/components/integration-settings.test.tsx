import { afterAll, afterEach, describe, expect, mock, test } from 'bun:test';
import { act, create, type ReactTestRenderer } from 'react-test-renderer';

const originalCore = { ...await import('@tauri-apps/api/core') };
const originalToast = { ...await import('sonner') };
afterAll(() => {
  mock.module('@tauri-apps/api/core', () => originalCore);
  mock.module('sonner', () => originalToast);
});

const settings = {
  api_enabled: false, api_port: 0, api_token: '', cli_token: '', mcp_token: '',
  cli_enabled: false, mcp_enabled: false, record_control: false, write_notes: false,
  read_meetings: false, read_transcripts: false, read_summaries: false,
  webhook_enabled: false, webhook_url: '', webhook_secret: '', webhook_include_summary: false,
};
const savedToken = 'a'.repeat(64);
const invoke = mock(async (command: string, args?: { settings: typeof settings }) => {
  const value = command === 'integrations_save_settings'
    ? { ...args!.settings, api_token: savedToken, cli_token: 'b'.repeat(64), mcp_token: 'c'.repeat(64) }
    : { ...settings };
  return { settings: value, config_path: '/private/integrations.json', companion_path: '/app/sonefold.py' };
});
mock.module('@tauri-apps/api/core', () => ({ invoke }));
mock.module('sonner', () => ({ toast: { success() {}, error() {} } }));
// The test renderer has no DOM; Radix Switch checks `instanceof HTMLFormElement` after mount.
globalThis.HTMLFormElement ??= class {} as unknown as typeof HTMLFormElement;
const { IntegrationSettings } = await import('../../src/components/IntegrationSettings');
const { Switch } = await import('../../src/components/ui/switch');

let renderer: ReactTestRenderer | undefined;
afterEach(async () => {
  if (renderer) await act(async () => renderer!.unmount());
  renderer = undefined; invoke.mockClear();
});
async function show() {
  await act(async () => { renderer = create(<IntegrationSettings />); });
  return renderer!.root;
}
function text(value: unknown): string {
  if (typeof value === 'string') return value;
  if (Array.isArray(value)) return value.map(text).join('');
  return value && typeof value === 'object' && 'children' in value ? text(value.children) : '';
}

describe('integration access grants', () => {
  test('enabling a client preserves independent read and action permissions', async () => {
    const root = await show();
    const switches = root.findAllByType(Switch);
    expect(switches.length).toBeGreaterThan(0);
    expect(switches.every(control => control.props.checked === false)).toBe(true);
    const api = switches.find(control => control.props.id === 'connection-api_enabled')!;
    await act(async () => api.props.onCheckedChange(true));
    expect(root.findAllByType('input').find(input => input.props.type === 'number')!.props.value).toBe(8421);
    const save = root.findAllByType('button').find(button => text(button.children) === 'Save permissions')!;
    await act(async () => save.props.onClick());
    const [, args] = invoke.mock.calls.find(([command]) => command === 'integrations_save_settings')!;
    expect(args!.settings.api_enabled).toBe(true);
    expect(args!.settings.record_control).toBe(false);
    expect(args!.settings.write_notes).toBe(false);
    expect(args!.settings.read_transcripts).toBe(false);
    expect(args!.settings.cli_enabled).toBe(false);
    const token = root.findByProps({ id: 'integration-api-token' });
    expect(token.props.readOnly).toBe(true); expect(token.props.type).toBe('password');
    expect(token.props.value).toBe(savedToken);
  });

  test('rejects invalid ports before sending settings to the native app', async () => {
    const root = await show();
    const port = root.findAllByType('input').find(input => input.props.type === 'number')!;
    await act(async () => port.props.onChange({ target: { value: '70000' } }));
    const save = root.findAllByType('button').find(button => text(button.children) === 'Save permissions')!;
    await act(async () => save.props.onClick());
    expect(invoke.mock.calls.some(([command]) => command === 'integrations_save_settings')).toBe(false);
    expect(text(root.findByProps({ role: 'alert' }).children)).toContain('1024 to 65535');
  });
});
