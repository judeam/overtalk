'use client';
import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import { useCreateBlockNote } from '@blocknote/react';
import { BlockNoteView } from '@blocknote/shadcn';
import type { PartialBlock } from '@blocknote/core';
import type { Summary, SummaryDataResponse } from '@/types';
import { useColorScheme } from '@/hooks/useColorScheme';
import { persistedSummaryMarkdown } from '@/lib/summary-export';
import { readNoteDraft, storeNoteDraft, saveNoteDraft, type NoteDraft } from '@/lib/note-drafts';
import '@blocknote/shadcn/style.css';

interface Props {
  summaryData: SummaryDataResponse | Summary | null;
  onSave?: (data: NoteDraft) => Promise<void>;
  onSummaryChange?: (summary: Summary) => void;
  onDirtyChange?: (dirty: boolean) => void;
  status?: 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';
  error?: string | null; onRegenerateSummary?: () => void;
  meeting?: { id: string; title: string; created_at: string };
  sourceTimes?: ([number, number] | null)[];
  onSource?: (seconds: number) => void;
}
export interface BlockNoteSummaryViewRef { saveSummary: () => Promise<void>; getMarkdown: () => Promise<string>; isDirty: boolean }

/** A single editor for saved Markdown, legacy notes and recovered drafts. */
export const BlockNoteSummaryView = forwardRef<BlockNoteSummaryViewRef, Props>(function BlockNoteSummaryView(props, ref) {
  const scheme = useColorScheme();
  const editor = useCreateBlockNote();
  const id = props.meeting?.id ?? 'unsaved';
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [ready, setReady] = useState(false);
  const pending = useRef<NoteDraft | null>(null);
  const loading = useRef(true);
  const initialized = useRef(false);
  const savedSignature = useRef('');
  const alive = useRef(true);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const persist = useRef(props.onSave); persist.current = props.onSave;

  const flush = useCallback(async () => {
    clearTimeout(saveTimer.current);
    const snapshot = pending.current; const save = persist.current;
    if (!snapshot || !save) return;
    if (alive.current) { setSaving(true); setError(''); }
    try {
      savedSignature.current = JSON.stringify(snapshot);
      await saveNoteDraft(id, snapshot, save);
      if (pending.current === snapshot) { pending.current = null; if (alive.current) setDirty(false); }
    } catch (e) {
      if (alive.current) setError('Could not save notes. Your draft is kept on this device.');
      throw e;
    } finally { if (alive.current) setSaving(false); }
  }, [id]);
  useEffect(() => {
    alive.current = true;
    return () => { alive.current = false; clearTimeout(saveTimer.current); void flush().catch(() => {}); };
  }, [flush]);
  useEffect(() => { props.onDirtyChange?.(dirty); }, [dirty, props.onDirtyChange]);
  useEffect(() => {
    let active = true;
    // Incoming polling updates never overwrite an edit in progress.
    if (pending.current || (initialized.current && savedSignature.current === JSON.stringify(props.summaryData))) return;
    const recovered = !initialized.current ? readNoteDraft(id) : null;
    loading.current = true;
    const load = async () => {
      const source = recovered ?? props.summaryData;
      const blocks = source && 'summary_json' in source && Array.isArray(source.summary_json) && source.summary_json.length
        ? source.summary_json as PartialBlock[]
        : await editor.tryParseMarkdownToBlocks((await persistedSummaryMarkdown(source)).replace(/\[S(\d+)\](?!\()/g, (marker, n) => {
          const time = props.sourceTimes?.[Number(n) - 1]?.[0];
          return time == null ? marker : `[S${n}](/meeting-details?id=${encodeURIComponent(id)}&at=${time})`;
        }));
      if (!active) return;
      editor.replaceBlocks(editor.document, blocks.length ? blocks : [{ type: 'paragraph', content: '' }]);
      initialized.current = true;
      loading.current = false;
      setReady(true);
      if (recovered) { pending.current = recovered; setDirty(true); saveTimer.current = setTimeout(() => void flush().catch(() => {}), 800); }
    };
    void load().catch(() => { if (active) { setError('Could not open these notes. Try reopening the meeting.'); loading.current = false; } });
    return () => { active = false; };
  }, [editor, id, props.summaryData, props.sourceTimes, flush]);
  const changed = () => {
    if (loading.current) return;
    const snapshot: NoteDraft = { summary_json: editor.document as NoteDraft['summary_json'] };
    pending.current = snapshot;
    storeNoteDraft(id, snapshot);
    setDirty(true);
    clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => void flush().catch(() => {}), 800);
  };
  useImperativeHandle(ref, () => ({ saveSummary: flush, getMarkdown: () => editor.blocksToMarkdownLossy(editor.document), isDirty: dirty }), [flush, editor, dirty]);
  return <div className="ot-note-editor" onClickCapture={event => {
    const anchor = (event.target as HTMLElement).closest('a[href]');
    if (!anchor || !props.onSource) return;
    const url = new URL(anchor.getAttribute('href') || '', window.location.origin);
    if (url.pathname === '/meeting-details' && url.searchParams.get('id') === id && url.searchParams.has('at')) {
      event.preventDefault(); event.stopPropagation(); props.onSource(Number(url.searchParams.get('at')));
    }
  }}>
    <div className="ot-autosave" role="status" aria-live="polite">
      {error ? <><span>{error}</span><button type="button" className="ot-link" onClick={() => void flush().catch(() => {})}>Retry save</button></>
        : !ready ? 'Opening notes…' : saving ? 'Saving notes…' : dirty ? 'Draft kept on this device · saving…' : 'All changes saved'}
    </div>
    {ready && <BlockNoteView editor={editor} editable onChange={changed} theme={scheme} />}
  </div>;
});
