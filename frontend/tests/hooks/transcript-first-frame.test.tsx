import { expect, test } from 'bun:test';
import { act, create } from 'react-test-renderer';
import { TranscriptSheet } from '../../src/components/session/TranscriptSheet';

test('a long transcript renders a first window before DOM measurement is available', async () => {
  let renderer: ReturnType<typeof create>;
  await act(async () => {
    renderer = create(<TranscriptSheet tab="transcript" onTab={() => {}} follow={false}
      segments={Array.from({ length: 1000 }, (_, i) => ({ id: `t${i}`, text: `Line ${i}`, timestamp: i }))}
      speakers={[]} currentId={null} onLine={() => {}} speakersPanel={null} />);
  });
  const rows = renderer!.root.findAllByType('article');
  expect(rows.length).toBeGreaterThan(0);
  expect(rows.length).toBeLessThan(30);
  await act(async () => { renderer.unmount(); });
});
