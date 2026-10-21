import { describe, expect, test } from 'bun:test';
import { readFile, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PDFDocument } from 'pdf-lib';
import { createMeetingExport, markdownDocumentBlocks, safeExportFilename, formatExportTimestamp, transcriptExportText, MeetingExportData } from '../../src/lib/meeting-export';

const data: MeetingExportData = {
  id: 'meeting-1', title: 'Roadmap / Q4', createdAt: '2026-09-30',
  summaryMarkdown: '## Decisions\n\nShip **the beta**.\n\n| Owner | Task | Due |\n| --- | --- | --- |\n| Zoë | Review | Not stated |',
  transcripts: [{ text: 'Grüße из Москвы. 会議 東京 한글. مرحبا שלום. नमस्ते தமிழ் ไทย.', audio_start_time: 3662, speaker_name: 'Zoë' }],
};
const loadFont = async (filename: string) => new Uint8Array(await readFile(new URL(`../../public/fonts/${filename}`, import.meta.url)));

describe('portable meeting exports', () => {
  test('preserves timestamp, speaker, text and selection without leaking other content', async () => {
    const md = await createMeetingExport(data, 'md', 'transcript');
    const text = new TextDecoder().decode(md.bytes);
    expect(text).toContain('[1:01:02 Zoë] Grüße');
    expect(text).not.toContain('Ship');
    const json = JSON.parse(new TextDecoder().decode((await createMeetingExport(data, 'json', 'summary')).bytes));
    expect(json.summary).toContain('beta'); expect(json.transcripts).toBeUndefined();
    expect(transcriptExportText({ text: 'Hello', speaker: 'mic' })).toBe('[source: mic] Hello');
    expect(transcriptExportText({ text: 'Hello', speaker: 'mic', speaker_id: 'speaker_2' })).toBe('[speaker_2] Hello');
  });

  test('sanitizes filenames, formats timestamps and refuses absent selected content', async () => {
    expect(safeExportFilename('../../CON<>', 'summary', 'pdf')).not.toContain('/');
    expect(safeExportFilename('CON', 'summary', 'pdf')).toBe('meeting-CON-summary.pdf');
    expect(formatExportTimestamp(-1, '12:30')).toBe('12:30');
    expect(formatExportTimestamp(0)).toBe('00:00');
    await expect(createMeetingExport({ ...data, summaryMarkdown: '' }, 'md', 'summary')).rejects.toThrow('no summary');
  });

  test('parses GFM tables, lists, code and inline content safely', () => {
    const blocks = markdownDocumentBlocks('## Notes\n\n- **Strong** [link](https://example.com)\n\n```ts\nconst a = 1;\n```\n\n| Owner | Task |\n| --- | --- |\n| A | B |');
    expect(blocks).toContainEqual({ type: 'heading', text: 'Notes', level: 2 });
    expect(blocks).toContainEqual({ type: 'paragraph', text: '• Strong link' });
    expect(blocks).toContainEqual({ type: 'code', text: 'const a = 1;' });
    expect(blocks).toContainEqual({ type: 'table', rows: [['Owner', 'Task'], ['A', 'B']] });
  });

  test('creates a real DOCX ZIP with Unicode, heading and table XML', async () => {
    const result = await createMeetingExport(data, 'docx');
    expect([...result.bytes.slice(0, 2)]).toEqual([0x50, 0x4b]);
    if (!Bun.which('unzip')) return;
    const directory = await mkdtemp(join(tmpdir(), 'sonefold-docx-'));
    try {
      const file = join(directory, 'notes.docx'); await writeFile(file, result.bytes);
      const output = Bun.spawnSync(['unzip', '-p', file, 'word/document.xml']);
      expect(output.exitCode).toBe(0);
      const xml = output.stdout.toString();
      expect(xml).toContain('<w:tbl>'); expect(xml).toContain('Heading1'); expect(xml).toContain('会議 東京 한글'); expect(xml).toContain('Zoë');
    } finally { await rm(directory, { recursive: true, force: true }); }
  });

  test('creates an offline Unicode PDF with extractable CJK text', async () => {
    const result = await createMeetingExport(data, 'pdf', 'both', { loadFont });
    expect(new TextDecoder().decode(result.bytes.slice(0, 5))).toBe('%PDF-');
    const pdf = await PDFDocument.load(result.bytes);
    expect(pdf.getPageCount()).toBeGreaterThan(0); expect(pdf.getTitle()).toBe(data.title);
    if (!Bun.which('pdftotext')) return;
    const directory = await mkdtemp(join(tmpdir(), 'sonefold-pdf-'));
    try {
      const file = join(directory, 'notes.pdf'); await writeFile(file, result.bytes);
      const output = Bun.spawnSync(['pdftotext', '-layout', file, '-']);
      expect(output.exitCode).toBe(0);
      expect(output.stderr.toString()).toBe('');
      const text = output.stdout.toString();
      expect(text).toContain('Grüße'); expect(text).toContain('Москвы'); expect(text).toContain('会議'); expect(text).toContain('한글'); expect(text).toContain('Zoë');
      expect(text).toContain('नमस्ते'); expect(text).toContain('தமிழ்'); expect(text).toContain('ไทย');
      if (Bun.which('pdftoppm')) {
        const rendered = Bun.spawnSync(['pdftoppm', '-f', '1', '-singlefile', '-scale-to', '900', '-png', file, join(directory, 'preview')]);
        expect(rendered.exitCode).toBe(0); expect(rendered.stderr.toString()).toBe('');
      }
    } finally { await rm(directory, { recursive: true, force: true }); }
  }, 30000);

  test('paginates long transcripts without losing the final turn', async () => {
    const long = { ...data, transcripts: Array.from({ length: 160 }, (_, index) => ({ text: `Turn ${index}: ${'A substantive discussion of delivery and project risks. '.repeat(5)}`, audio_start_time: index * 30 })) };
    const result = await createMeetingExport(long, 'pdf', 'transcript', { loadFont });
    expect((await PDFDocument.load(result.bytes)).getPageCount()).toBeGreaterThan(5);
    if (Bun.which('pdftotext')) {
      const extracted = Bun.spawnSync({ cmd: ['pdftotext', '-layout', '-', '-'], stdin: result.bytes });
      expect(extracted.exitCode).toBe(0); expect(extracted.stderr.toString()).toBe('');
      const text = extracted.stdout.toString();
      for (let index = 0; index < long.transcripts.length; index++) expect(text).toContain(`Turn ${index}:`);
    }
  }, 30000);
});
