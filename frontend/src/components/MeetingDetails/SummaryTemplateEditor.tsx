'use client';

import { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Copy, Download, FilePenLine, Plus, Trash2, Upload } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';
import { parseTemplateImport, SummaryTemplate } from '@/lib/summary-template';

interface EditableTemplate { id: string; is_custom: boolean; template: SummaryTemplate }
interface TemplateInfo { id: string; name: string; description: string }
interface Props { templates: TemplateInfo[]; selectedTemplate: string; onSelect: (id: string, name: string) => void; disabled?: boolean }
const newTemplate = (): SummaryTemplate => ({ name: '', description: '', sections: [{ title: 'Key points', instruction: 'Summarize the main discussion and preserve source references.', format: 'list' }] });

export function SummaryTemplateEditor({ templates, selectedTemplate, onSelect, disabled }: Props) {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<SummaryTemplate>(newTemplate);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [custom, setCustom] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [deletePending, setDeletePending] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const reportError = (cause: unknown) => setError(String(cause));
  const notify = () => window.dispatchEvent(new Event('sonefold-templates-changed'));
  const load = async (id: string, copy = false) => {
    setBusy(true); setError(''); setDeletePending(false);
    try {
      const result = await invoke<EditableTemplate>('api_get_template', { templateId: id });
      setDraft(copy ? { ...result.template, name: `${result.template.name} (copy)` } : result.template);
      setEditingId(copy ? null : result.id);
      setCustom(copy || result.is_custom);
    } catch (cause) { reportError(cause); } finally { setBusy(false); }
  };
  const save = async () => {
    setBusy(true); setError('');
    try {
      const result = await invoke<EditableTemplate>('api_save_template', { templateId: editingId, templateJson: JSON.stringify(draft) });
      setEditingId(result.id); setCustom(true); notify(); onSelect(result.id, result.template.name);
      toast.success('Template saved');
    } catch (cause) { reportError(cause); } finally { setBusy(false); }
  };
  const remove = async () => {
    if (!editingId || !custom) return;
    setBusy(true); setError('');
    try {
      await invoke('api_delete_template', { templateId: editingId });
      if (selectedTemplate === editingId) onSelect('standard_meeting', 'Standard Meeting Notes');
      setEditingId(null); setDraft(newTemplate()); setDeletePending(false); notify(); toast.success('Template deleted');
    } catch (cause) { reportError(cause); } finally { setBusy(false); }
  };
  const importFile = async (file?: File) => {
    if (!file) return;
    setBusy(true); setError('');
    try {
      if (file.size > 256 * 1024) throw new Error('Template files must be smaller than 256 KiB.');
      const template = parseTemplateImport(await file.text());
      await invoke('api_validate_template', { templateJson: JSON.stringify(template) });
      setDraft(template); setEditingId(null); setCustom(true); setDeletePending(false);
    } catch (cause) { reportError(cause); } finally { setBusy(false); if (fileRef.current) fileRef.current.value = ''; }
  };
  const exportTemplate = () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(draft, null, 2)], { type: 'application/json' }));
    const link = document.createElement('a'); link.href = url; link.download = 'overtalk-template.json'; link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const updateSection = (index: number, patch: Partial<SummaryTemplate['sections'][number]>) => setDraft(previous => ({ ...previous, sections: previous.sections.map((section, position) => position === index ? { ...section, ...patch } : section) }));

  return <Dialog open={open} onOpenChange={value => { setOpen(value); if (value) void load(selectedTemplate); }}>
    <DialogTrigger asChild><Button variant="outline" size="sm" disabled={disabled} title="Manage summary templates"><FilePenLine size={16} /><span>Edit templates</span></Button></DialogTrigger>
    <DialogContent className="max-w-3xl max-h-[90vh] flex flex-col overflow-hidden">
      <DialogHeader><DialogTitle>Summary templates</DialogTitle><DialogDescription>Define what to extract from your meetings. Built-in templates can be copied; edits apply when you generate a summary.</DialogDescription></DialogHeader>
      <div className="flex gap-2 flex-wrap">
        <select aria-label="Template to edit" className="border rounded px-2 py-1 flex-1 min-w-0" value={editingId || ''} disabled={busy} onChange={event => { if (event.target.value) void load(event.target.value); else { setEditingId(null); setDraft(newTemplate()); setCustom(true); setError(''); } }}><option value="">New custom template</option>{templates.map(template => <option key={template.id} value={template.id}>{template.name}</option>)}</select>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => { setEditingId(null); setDraft(newTemplate()); setCustom(true); setError(''); setDeletePending(false); }}><Plus size={16} />New</Button>
        <Button size="sm" variant="outline" disabled={busy || !editingId} onClick={() => editingId && void load(editingId, true)}><Copy size={16} />Copy</Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={() => fileRef.current?.click()}><Upload size={16} />Import</Button>
        <Button size="sm" variant="outline" disabled={busy} onClick={exportTemplate}><Download size={16} />JSON</Button>
        <input ref={fileRef} type="file" accept=".json,.md,application/json,text/markdown" hidden onChange={event => void importFile(event.target.files?.[0])} />
      </div>
      <div className="overflow-y-auto space-y-4 pr-2">
        <label className="block text-sm font-medium">Name<Input value={draft.name} maxLength={120} disabled={busy || !custom} onChange={event => setDraft({ ...draft, name: event.target.value })} /></label>
        <label className="block text-sm font-medium">Description<Textarea value={draft.description} maxLength={1000} disabled={busy || !custom} onChange={event => setDraft({ ...draft, description: event.target.value })} /></label>
        {draft.sections.map((section, index) => <fieldset key={index} className="border rounded-lg p-3 space-y-2" disabled={busy || !custom}>
          <legend className="text-sm px-1">Section {index + 1}</legend>
          <div className="flex gap-2"><Input aria-label={`Section ${index + 1} title`} value={section.title} maxLength={120} placeholder="Section title" onChange={event => updateSection(index, { title: event.target.value })} /><select aria-label={`Section ${index + 1} format`} className="border rounded px-2" value={section.format} onChange={event => updateSection(index, { format: event.target.value as 'paragraph' | 'list' | 'string' })}><option value="paragraph">Paragraph</option><option value="list">List</option><option value="string">Short text</option></select></div>
          <Textarea aria-label={`Section ${index + 1} instructions`} value={section.instruction} maxLength={8000} placeholder="Tell the AI what to extract. Ask for source references and explicitly stated owners and dates." onChange={event => updateSection(index, { instruction: event.target.value })} />
          <Textarea aria-label={`Section ${index + 1} table format`} value={section.item_format || section.example_item_format || ''} maxLength={4000} placeholder="Optional table columns or item format" rows={2} onChange={event => updateSection(index, { item_format: event.target.value, example_item_format: undefined })} />
          {custom && <Button size="sm" variant="ghost" disabled={draft.sections.length === 1} onClick={() => setDraft({ ...draft, sections: draft.sections.filter((_, position) => position !== index) })}><Trash2 size={14} />Remove section</Button>}
        </fieldset>)}
        {custom && <Button variant="outline" size="sm" disabled={busy || draft.sections.length >= 32} onClick={() => setDraft({ ...draft, sections: [...draft.sections, { title: '', instruction: '', format: 'list' }] })}><Plus size={16} />Add section</Button>}
      </div>
      {error && <p role="alert" className="text-sm text-red-700">{error}</p>}
      <div className="flex justify-between gap-2 border-t pt-3">
        {custom && editingId ? <Button variant="outline" size="sm" disabled={busy} onClick={() => deletePending ? void remove() : setDeletePending(true)}><Trash2 size={16} />{deletePending ? 'Confirm permanent deletion' : 'Delete'}</Button> : <span className="text-sm text-gray-500">{!custom ? 'Copy this built-in template to customize it.' : 'JSON and Markdown imports supported.'}</span>}
        {custom && <Button size="sm" disabled={busy} onClick={() => void save()}>{busy ? 'Saving…' : editingId ? 'Save template' : 'Create template'}</Button>}
      </div>
    </DialogContent>
  </Dialog>;
}
