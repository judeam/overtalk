import { describe, expect, mock, test } from 'bun:test';
import { persistedSummaryMarkdown } from '../../src/lib/summary-export';

describe('saved summary export compatibility', () => {
  test('prefers saved editable blocks over stale Markdown and supports block-only summaries', async () => {
    const blocks = [{ type: 'paragraph' as const, content: 'Current decision' }];
    const convert = mock(async () => 'Current decision');
    expect(await persistedSummaryMarkdown({ summary_json: blocks, markdown: 'Old decision' }, convert)).toBe('Current decision');
    expect(convert).toHaveBeenCalledWith(blocks);
    expect(await persistedSummaryMarkdown({ summary_json: blocks }, convert)).toBe('Current decision');
  });

  test('does not restore deleted content or silently export stale data after conversion failure', async () => {
    expect(await persistedSummaryMarkdown({ summary_json: [], markdown: 'Deleted decision' })).toBe('');
    await expect(persistedSummaryMarkdown({ summary_json: [{ type: 'paragraph' }], markdown: 'Stale' }, async () => {
      throw new Error('Unsupported block');
    })).rejects.toThrow('Could not convert');
  });

  test('respects legacy section order and includes sections added later', async () => {
    const result = await persistedSummaryMarkdown({
      MeetingName: 'Metadata', _section_order: ['Decisions', 'Notes', 'Decisions'],
      Notes: { title: 'Notes', blocks: [{ type: 'paragraph', content: 'Discussed a proposal.' }] },
      Decisions: { title: 'Decisions', blocks: [{ content: 'Ship it.' }] },
      Actions: { title: 'Actions', blocks: [{ content: 'Review.' }] },
    });
    expect(result).toBe('## Decisions\n\n- Ship it.\n\n## Notes\n\nDiscussed a proposal.\n\n## Actions\n\n- Review.');
    expect(await persistedSummaryMarkdown({ markdown: 'Plain notes' })).toBe('Plain notes');
    expect(await persistedSummaryMarkdown(null)).toBe('');
  });
});
