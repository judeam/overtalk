import { expect, test } from 'bun:test';
import { parseTemplateImport, SummaryTemplate } from '../../src/lib/summary-template';

test('imports Markdown section instructions and JSON formats', () => {
  expect(parseTemplateImport('# Client call\n\nCapture commitments.\n\n## Actions\n\nList explicit owners and dates.').sections[0]).toEqual({ title: 'Actions', format: 'list', instruction: 'List explicit owners and dates.' });
  const template: SummaryTemplate = { name: 'Review', description: 'A review', sections: [{ title: 'Facts', instruction: 'Extract facts', format: 'paragraph' }] };
  expect(parseTemplateImport(JSON.stringify(template))).toEqual(template);
});

test('rejects oversized imports and malformed Markdown', () => {
  expect(() => parseTemplateImport('x'.repeat(256 * 1024 + 1))).toThrow('256 KiB');
  expect(() => parseTemplateImport('## Missing title')).toThrow('begin with #');
});
