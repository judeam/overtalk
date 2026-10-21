import type { PartialBlock } from '@blocknote/core';

type SummaryObject = Record<string, unknown>;
export type SummaryBlockConverter = (blocks: PartialBlock[]) => Promise<string>;

function object(value: unknown): SummaryObject | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as SummaryObject : null;
}

async function convertSavedBlocks(blocks: PartialBlock[]): Promise<string> {
  const { BlockNoteEditor } = await import('@blocknote/core');
  // Conversion uses the same schema as the editor; an editor that is never mounted stays headless.
  const editor = BlockNoteEditor.create();
  return editor.blocksToMarkdownLossy(blocks);
}

/** Prefer the saved editable document over a potentially stale Markdown copy. */
export async function persistedSummaryMarkdown(
  value: unknown,
  convertBlocks: SummaryBlockConverter = convertSavedBlocks,
): Promise<string> {
  const summary = object(value);
  if (!summary) return '';
  if (Array.isArray(summary.summary_json)) {
    if (!summary.summary_json.length) return '';
    try {
      return await convertBlocks(summary.summary_json as PartialBlock[]);
    } catch {
      throw new Error('Could not convert the saved summary. Open the summary editor and try again.');
    }
  }
  if (typeof summary.markdown === 'string') return summary.markdown;

  const declaredOrder = Array.isArray(summary._section_order)
    ? summary._section_order.filter((key): key is string => typeof key === 'string') : [];
  const keys = [...new Set([...declaredOrder, ...Object.keys(summary)])];
  return keys.map(key => {
    if (['MeetingName', '_section_order', 'reasoning_stripped', 'normalization_fallback'].includes(key)) return '';
    const section = object(summary[key]);
    if (!section || typeof section.title !== 'string' || !Array.isArray(section.blocks)) return '';
    const blocks = section.blocks.map(value => {
      const block = object(value);
      if (typeof block?.content !== 'string') return '';
      return `${block.type === 'paragraph' ? '' : '- '}${block.content}`;
    }).filter(Boolean).join('\n\n');
    return blocks ? `## ${section.title}\n\n${blocks}` : '';
  }).filter(Boolean).join('\n\n');
}
