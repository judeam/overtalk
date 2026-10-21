interface SummaryTemplateSection {
  title: string;
  instruction: string;
  format: 'paragraph' | 'list' | 'string';
  item_format?: string;
  example_item_format?: string;
}

export interface SummaryTemplate {
  name: string;
  description: string;
  sections: SummaryTemplateSection[];
}

export function parseTemplateImport(source: string): SummaryTemplate {
  if (new TextEncoder().encode(source).length > 256 * 1024) throw new Error('Template files must be smaller than 256 KiB.');
  const trimmed = source.trim();
  if (trimmed.startsWith('{')) return JSON.parse(trimmed) as SummaryTemplate;
  const lines = trimmed.split(/\r?\n/);
  const title = lines.shift()?.match(/^#\s+(.+)$/)?.[1];
  if (!title) throw new Error('A Markdown template must begin with # Template name.');
  const description: string[] = [];
  const sections: SummaryTemplateSection[] = [];
  for (const line of lines) {
    const heading = line.match(/^##\s+(.+)$/);
    if (heading) sections.push({ title: heading[1], instruction: '', format: 'list' });
    else if (sections.length) {
      const section = sections[sections.length - 1];
      section.instruction += `${line}\n`;
    } else description.push(line);
  }
  return { name: title.trim(), description: description.join('\n').trim(), sections: sections.map(section => ({ ...section, instruction: section.instruction.trim() })) };
}
