import { marked, Token, Tokens } from 'marked';

export type MeetingExportFormat = 'md' | 'txt' | 'json' | 'docx' | 'pdf';
export type MeetingExportContent = 'summary' | 'transcript' | 'both';
export interface ExportTranscript {
  text: string;
  timestamp?: string;
  audio_start_time?: number;
  speaker_id?: string | null;
  speaker_name?: string | null;
  speaker?: string | null;
}
export interface MeetingExportData {
  id: string;
  title: string;
  createdAt?: string;
  summaryMarkdown?: string;
  transcripts: ExportTranscript[];
}
export interface MeetingExportResult { bytes: Uint8Array; mimeType: string; filename: string }
export type ExportFontLoader = (filename: string) => Promise<Uint8Array>;
type DocumentBlock = { type: 'paragraph' | 'heading' | 'code'; text: string; level?: number } | { type: 'table'; rows: string[][] };

export function safeExportFilename(title: string, content: MeetingExportContent, format: MeetingExportFormat): string {
  const clean = title.normalize('NFC').replace(/[<>:"/\\|?*\u0000-\u001f\u007f]/g, '-').replace(/[. ]+$/g, '').trim().slice(0, 100) || 'meeting';
  const reserved = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(clean) ? `meeting-${clean}` : clean;
  return `${reserved}-${content === 'both' ? 'notes' : content}.${format}`;
}

export function formatExportTimestamp(seconds?: number, fallback = ''): string {
  if (seconds === undefined || !Number.isFinite(seconds) || seconds < 0) return fallback;
  const whole = Math.floor(seconds);
  const h = Math.floor(whole / 3600), m = Math.floor((whole % 3600) / 60), s = whole % 60;
  return `${h ? `${h}:` : ''}${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`;
}

export function transcriptExportText(transcript: ExportTranscript): string {
  const time = formatExportTimestamp(transcript.audio_start_time, transcript.timestamp);
  const speaker = transcript.speaker_name || transcript.speaker_id;
  const source = !speaker && transcript.speaker ? `source: ${transcript.speaker}` : null;
  const label = [time, speaker, source].filter(Boolean).join(' ');
  return `${label ? `[${label}] ` : ''}${transcript.text}`;
}

function meetingExportMarkdown(data: MeetingExportData, content: MeetingExportContent): string {
  const sections = [`# ${data.title || 'Meeting'}\n`];
  if (data.createdAt) sections.push(`Recorded: ${data.createdAt}\n`);
  if (content !== 'transcript') {
    if (!data.summaryMarkdown?.trim()) throw new Error('This meeting has no summary to export.');
    sections.push(`## Summary\n\n${data.summaryMarkdown.trim()}\n`);
  }
  if (content !== 'summary') {
    if (!data.transcripts.length) throw new Error('This meeting has no transcript to export.');
    sections.push(`## Transcript\n\n${data.transcripts.map(transcriptExportText).join('\n\n')}\n`);
  }
  return sections.join('\n');
}

function inlineText(tokens?: Token[], fallback = ''): string {
  if (!tokens) return fallback;
  return tokens.map(token => {
    if (token.type === 'br') return '\n';
    if ('tokens' in token && token.tokens) return inlineText(token.tokens);
    if (token.type === 'image') return (token as Tokens.Image).text;
    return 'text' in token ? String(token.text) : token.raw;
  }).join('');
}

export function markdownDocumentBlocks(markdown: string): DocumentBlock[] {
  const result: DocumentBlock[] = [];
  const visit = (tokens: Token[], depth = 0) => {
    for (const token of tokens) {
      if (token.type === 'space' || token.type === 'def') continue;
      if (token.type === 'heading') result.push({ type: 'heading', text: inlineText(token.tokens, token.text), level: token.depth });
      else if (token.type === 'table') {
        const table = token as Tokens.Table;
        result.push({ type: 'table', rows: [table.header.map(cell => inlineText(cell.tokens, cell.text)), ...table.rows.map(row => row.map(cell => inlineText(cell.tokens, cell.text)))] });
      }
      else if (token.type === 'list') {
        const list = token as Tokens.List;
        list.items.forEach((item, index) => {
          const itemBlocks = markdownDocumentBlocks(item.text);
          const prefix = `${'  '.repeat(depth)}${list.ordered ? `${Number(list.start) + index}.` : '•'} ${item.task ? item.checked ? '☑ ' : '☐ ' : ''}`;
          itemBlocks.forEach((block, position) => result.push(block.type === 'table' ? block : { ...block, text: `${position === 0 ? prefix : '  '}${block.text}` }));
        });
      } else if (token.type === 'blockquote') visit(token.tokens || [], depth + 1);
      else if (token.type === 'code') result.push({ type: 'code', text: token.text });
      else if (token.type === 'hr') result.push({ type: 'paragraph', text: '────────────────' });
      else result.push({ type: 'paragraph', text: inlineText('tokens' in token ? token.tokens : undefined, 'text' in token ? String(token.text) : token.raw) });
    }
  };
  visit(marked.lexer(markdown, { gfm: true }));
  return result;
}

async function documentWord(blocks: DocumentBlock[], title: string): Promise<Uint8Array> {
  const { Document, Packer, Paragraph, TextRun, Table, TableRow, TableCell, HeadingLevel, WidthType } = await import('docx');
  const children = blocks.map(block => block.type === 'table'
    ? new Table({ width: { size: 100, type: WidthType.PERCENTAGE }, rows: block.rows.map((row, index) => new TableRow({ tableHeader: index === 0, children: row.map(text => new TableCell({ children: [new Paragraph({ children: [new TextRun({ text, bold: index === 0 })] })] })) })) })
    : new Paragraph({
      children: block.text.split('\n').flatMap((text, index) => [new TextRun({ text, ...(index ? { break: 1 } : {}), font: block.type === 'code' ? 'Courier New' : 'Arial' })]),
      heading: block.type === 'heading' ? [HeadingLevel.HEADING_1, HeadingLevel.HEADING_2, HeadingLevel.HEADING_3][Math.min(2, (block.level || 1) - 1)] : undefined,
      spacing: { after: 160 },
      keepNext: block.type === 'heading',
      bidirectional: /^[\s\d\W]*[\u0590-\u08ff]/u.test(block.text),
    }));
  return new Uint8Array(await (await Packer.toBlob(new Document({ title, creator: 'Overtalk', sections: [{ children }] }))).arrayBuffer());
}

const FONT_FILES = {
  latin: 'NotoSans-Regular.ttf', arabic: 'NotoSansArabic-Regular.ttf', hebrew: 'NotoSansHebrew-Regular.ttf',
  devanagari: 'NotoSansDevanagari-Regular.ttf', tamil: 'NotoSansTamil-Regular.ttf', thai: 'NotoSansThai-Regular.ttf',
  symbols: 'NotoSansSymbols2-Regular.ttf', cjk: 'NotoSansCJKsc-Regular.otf',
} as const;
type FontKey = keyof typeof FONT_FILES;
const fontBytes = new Map<string, Promise<Uint8Array>>();
const bundledFontLoader: ExportFontLoader = filename => {
  let loaded = fontBytes.get(filename);
  if (!loaded) {
    loaded = fetch(`/fonts/${filename}`).then(async response => {
      if (!response.ok) throw new Error('The bundled PDF fonts could not be loaded. Reinstall the app or export DOCX/Markdown.');
      return new Uint8Array(await response.arrayBuffer());
    }).catch(error => { fontBytes.delete(filename); throw error; });
    fontBytes.set(filename, loaded);
  }
  return loaded;
};

function scriptFont(code: number): FontKey {
  if (code >= 0x590 && code <= 0x5ff) return 'hebrew';
  if ((code >= 0x600 && code <= 0x8ff) || (code >= 0xfb50 && code <= 0xfefc)) return 'arabic';
  if (code >= 0x900 && code <= 0x97f) return 'devanagari';
  if (code >= 0xb80 && code <= 0xbff) return 'tamil';
  if (code >= 0xe00 && code <= 0xe7f) return 'thai';
  if ((code >= 0x2e80 && code <= 0xd7af) || (code >= 0xf900 && code <= 0xfaff) || code >= 0x20000) return 'cjk';
  return 'symbols';
}

async function documentPdf(blocks: DocumentBlock[], title: string, loadFont: ExportFontLoader): Promise<Uint8Array> {
  const { PDFDocument, rgb } = await import('pdf-lib');
  const { pdfFontkit } = await import('./pdf-fontkit');
  const doc = await PDFDocument.create();
  doc.setTitle(title); doc.setCreator('Overtalk'); doc.setProducer('Overtalk offline export');
  doc.registerFontkit(pdfFontkit);
  const fonts = new Map<FontKey, { font: Awaited<ReturnType<typeof doc.embedFont>>; characters: Set<number> }>();
  const getFont = async (key: FontKey) => {
    let value = fonts.get(key);
    if (!value) {
      const font = await doc.embedFont(await loadFont(FONT_FILES[key]), { subset: true });
      value = { font, characters: new Set(font.getCharacterSet()) }; fonts.set(key, value);
    }
    return value;
  };
  const base = await getFont('latin');
  type FontRun = { text: string; font: typeof base.font };
  const runsFor = async (text: string): Promise<FontRun[]> => {
    const runs: FontRun[] = [];
    for (const character of text.normalize('NFC')) {
      const code = character.codePointAt(0)!;
      let selected = base;
      if (!base.characters.has(code)) selected = await getFont(scriptFont(code));
      if (!selected.characters.has(code)) {
        if (code === 0x200d || code === 0xfe0f) continue; // shaping selectors have no independent glyph
        throw new Error(`PDF font coverage is missing U+${code.toString(16).toUpperCase()}. Export DOCX or Markdown to preserve this text.`);
      }
      const last = runs[runs.length - 1];
      if (last?.font === selected.font) last.text += character;
      else runs.push({ text: character, font: selected.font });
    }
    return runs;
  };
  const measure = async (text: string, size: number) => (await runsFor(text)).reduce((sum, run) => sum + run.font.widthOfTextAtSize(run.text, size), 0);
  const wrap = async (text: string, size: number, width: number): Promise<string[]> => {
    const lines: string[] = [];
    for (const paragraph of text.split('\n')) {
      let line = '';
      // Whitespace opportunities preserve words; long tokens and CJK can break
      // at Unicode scalars without corrupting UTF-8 or dropping characters.
      for (const word of paragraph.match(/\S+\s*|\s+/gu) || ['']) {
        if (line && await measure(line + word, size) > width) { lines.push(line.trimEnd()); line = ''; }
        if (await measure(word, size) <= width) line += word;
        else for (const scalar of word) {
          if (line && await measure(line + scalar, size) > width) { lines.push(line); line = ''; }
          line += scalar;
        }
      }
      lines.push(line.trimEnd());
    }
    return lines;
  };
  const pageWidth = 595.28, pageHeight = 841.89, margin = 48, width = pageWidth - margin * 2;
  let page = doc.addPage([pageWidth, pageHeight]), y = pageHeight - margin;
  const ensureSpace = (height: number) => { if (y - height < margin + 20) { page = doc.addPage([pageWidth, pageHeight]); y = pageHeight - margin; } };
  const draw = async (text: string, size: number, x: number, baseline: number, maxWidth: number) => {
    const runs = await runsFor(text);
    const rtl = /^[\s\d\W]*[\u0590-\u08ff]/u.test(text);
    if (rtl) { runs.reverse(); x += Math.max(0, maxWidth - await measure(text, size)); }
    for (const run of runs) {
      page.drawText(run.text, { x, y: baseline, font: run.font, size, color: rgb(0.12, 0.15, 0.2) });
      x += run.font.widthOfTextAtSize(run.text, size);
    }
  };
  for (const block of blocks) {
    if (block.type === 'table') {
      const columns = Math.max(1, ...block.rows.map(row => row.length)), cellWidth = width / columns, size = 10, lineHeight = 15;
      for (let rowIndex = 0; rowIndex < block.rows.length; rowIndex++) {
        const cells = await Promise.all(Array.from({ length: columns }, (_, index) => wrap(block.rows[rowIndex][index] || '', size, cellWidth - 12)));
        const count = Math.max(1, ...cells.map(lines => lines.length));
        let start = 0;
        while (start < count) {
          ensureSpace(lineHeight + 12);
          const available = Math.max(1, Math.floor((y - margin - 32) / lineHeight));
          const take = Math.min(count - start, available), height = take * lineHeight + 12;
          cells.forEach((_, index) => page.drawRectangle({ x: margin + index * cellWidth, y: y - height, width: cellWidth, height, borderColor: rgb(0.78, 0.8, 0.83), borderWidth: 0.5, color: rowIndex === 0 ? rgb(0.93, 0.95, 0.97) : rgb(1, 1, 1) }));
          for (let index = 0; index < columns; index++) for (let line = 0; line < take; line++) await draw(cells[index][start + line] || '', size, margin + index * cellWidth + 6, y - 6 - size - line * lineHeight, cellWidth - 12);
          y -= height; start += take;
        }
      }
      y -= 12;
    } else {
      const size = block.type === 'heading' ? block.level === 1 ? 20 : 15 : 11;
      const lineHeight = size * 1.5;
      if (block.type === 'heading') { y -= 8; ensureSpace(lineHeight * 2); }
      for (const line of await wrap(block.text, size, width)) { ensureSpace(lineHeight); await draw(line, size, margin, y - size, width); y -= lineHeight; }
      y -= 8;
    }
  }
  doc.getPages().forEach((item, index, pages) => item.drawText(`${index + 1} / ${pages.length}`, { x: pageWidth / 2 - 12, y: 26, font: base.font, size: 9, color: rgb(0.45, 0.48, 0.52) }));
  return doc.save();
}

export async function createMeetingExport(data: MeetingExportData, format: MeetingExportFormat, content: MeetingExportContent = 'both', options: { loadFont?: ExportFontLoader } = {}): Promise<MeetingExportResult> {
  const markdown = meetingExportMarkdown(data, content);
  let bytes: Uint8Array, mimeType: string;
  if (format === 'md') { bytes = new TextEncoder().encode(markdown); mimeType = 'text/markdown;charset=utf-8'; }
  else if (format === 'json') { bytes = new TextEncoder().encode(JSON.stringify({ id: data.id, title: data.title, createdAt: data.createdAt, ...(content !== 'transcript' ? { summary: data.summaryMarkdown } : {}), ...(content !== 'summary' ? { transcripts: data.transcripts } : {}) }, null, 2)); mimeType = 'application/json'; }
  else {
    const blocks = markdownDocumentBlocks(markdown);
    if (format === 'txt') { bytes = new TextEncoder().encode(blocks.map(block => block.type === 'table' ? block.rows.map(row => row.join('\t')).join('\n') : block.text).join('\n\n')); mimeType = 'text/plain;charset=utf-8'; }
    else if (format === 'docx') { bytes = await documentWord(blocks, data.title); mimeType = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'; }
    else { bytes = await documentPdf(blocks, data.title, options.loadFont || bundledFontLoader); mimeType = 'application/pdf'; }
  }
  return { bytes, mimeType, filename: safeExportFilename(data.title || data.id, content, format) };
}

export function downloadMeetingExport(result: MeetingExportResult): void {
  const url = URL.createObjectURL(new Blob([new Uint8Array(result.bytes)], { type: result.mimeType }));
  const link = document.createElement('a'); link.href = url; link.download = result.filename;
  document.body.appendChild(link); link.click(); link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
