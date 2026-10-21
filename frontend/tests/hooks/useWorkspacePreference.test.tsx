import { expect, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { useWorkspacePreference } from '../../src/hooks/useWorkspacePreference';

test('changing meetings cannot copy the previous meeting context into a missing preference', () => {
  const data = new Map<string, string>([['overtalk.note-context.one', '"Project one"']]);
  const previous = globalThis.localStorage;
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
  } });
  let value = '';
  function Reader({ meeting }: { meeting: string }) {
    [value] = useWorkspacePreference(`note-context.${meeting}`, '');
    return null;
  }
  let renderer!: ReturnType<typeof create>;
  try {
    act(() => { renderer = create(<Reader meeting="one" />); });
    expect(value).toBe('Project one');
    act(() => renderer.update(<Reader meeting="two" />));
    expect(value).toBe('');
    expect(data.get('overtalk.note-context.two')).toBe('""');
    expect(data.get('overtalk.note-context.one')).toBe('"Project one"');
  } finally {
    act(() => renderer.unmount());
    Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: previous });
  }
});
