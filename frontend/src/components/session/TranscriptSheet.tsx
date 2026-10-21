'use client';
import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import { defaultRangeExtractor, useVirtualizer } from '@tanstack/react-virtual';
import { invoke } from '@tauri-apps/api/core';
import { Check, Edit3, Search, X } from 'lucide-react';
import type { TranscriptSegmentData } from '@/types';
import { clock, toneOf, type TimelineSpeaker } from '@/lib/session';
import type { MeetingSource, SearchResponse } from '@/lib/workspace';
import { HighlightedText } from './HighlightedText';

export type SheetTab = 'transcript' | 'speakers';
interface TranscriptSheetProps {
  tab: SheetTab; onTab: (tab: SheetTab) => void;
  segments: TranscriptSegmentData[]; speakers: TimelineSpeaker[];
  currentId: string | null; follow: boolean;
  onLine: (segment: TranscriptSegmentData) => void;
  hasMore?: boolean; isLoadingMore?: boolean; onLoadMore?: () => void;
  speakersPanel: ReactNode; emptyState?: ReactNode; tools?: ReactNode;
  meetingId?: string; showTabs?: boolean;
  onEdit?: (segment: TranscriptSegmentData, text: string, speakerId: string | null) => Promise<void>;
  onSearchSource?: (source: MeetingSource) => void;
}
export function speakerFor(segment: TranscriptSegmentData, speakers: TimelineSpeaker[]) {
  const index = segment.speaker_id ? speakers.findIndex((s) => s.id === segment.speaker_id) : -1;
  const name = segment.speaker_name || (index >= 0 ? speakers[index].name : '') || 'Unassigned voice';
  return { index, name, tone: index >= 0 ? toneOf(index) : null };
}
export function TranscriptSheet(props: TranscriptSheetProps) {
  const { tab, segments, speakers, currentId } = props;
  const body = useRef<HTMLDivElement>(null);
  const tabs = useRef<HTMLDivElement>(null);
  const id = useId();
  const [follow, setFollow] = useState(props.follow);
  const [query, setQuery] = useState('');
  const [matches, setMatches] = useState<MeetingSource[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState('');
  const [editing, setEditing] = useState<TranscriptSegmentData | null>(null);
  const [draft, setDraft] = useState('');
  const [voice, setVoice] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    if (!props.meetingId || !query.trim()) { setMatches([]); setSearching(false); return; }
    let active = true;
    setSearching(true); setSearchError('');
    const timer = setTimeout(() => {
      void invoke<SearchResponse>('workspace_search', { request: { query, meeting_id: props.meetingId, semantic: false, limit: 100 } })
        .then(result => { if (active) setMatches(result.sources.filter(s => s.source_kind === 'transcript')); })
        .catch(() => { if (active) setSearchError('Search could not finish. Try again or clear the search.'); })
        .finally(() => { if (active) setSearching(false); });
    }, 250);
    return () => { active = false; clearTimeout(timer); };
  }, [query, props.meetingId]);
  const edit = (line: TranscriptSegmentData) => { setEditing(line); setDraft(line.text); setVoice(line.speaker_id || ''); setError(''); setFollow(false); };
  const save = async () => {
    if (!editing || !props.onEdit) return;
    setSaving(true); setError('');
    try { await props.onEdit(editing, draft, voice || null); setEditing(null); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setSaving(false); }
  };
  const shown = query && !props.meetingId ? segments.filter(s => s.text.toLowerCase().includes(query.toLowerCase())) : segments;
  const virtual = tab === 'transcript' && !query && shown.length > 80;
  const editingIndex = editing ? shown.findIndex(line => line.id === editing.id) : -1;
  const virtualizer = useVirtualizer({
    count: virtual ? shown.length : 0,
    getScrollElement: () => body.current,
    estimateSize: () => 110,
    // Paint a small first window immediately. Waiting for a measured viewport
    // otherwise renders an empty transcript and requires another layout pass.
    initialRect: { width: 0, height: 600 },
    getItemKey: index => shown[index].id,
    overscan: 6,
    rangeExtractor: range => {
      const indices = defaultRangeExtractor(range);
      if (editingIndex >= 0 && !indices.includes(editingIndex)) indices.push(editingIndex);
      return indices.sort((a, b) => a - b);
    },
  });
  useEffect(() => {
    if (!follow || !currentId || tab !== 'transcript' || editing || query) return;
    if (virtual) {
      const index = shown.findIndex(line => line.id === currentId);
      if (index >= 0) virtualizer.scrollToIndex(index, { align: 'auto' });
    } else {
      body.current?.querySelector<HTMLElement>(`[data-line="${CSS.escape(currentId)}"]`)?.scrollIntoView({ block: 'nearest', behavior: 'auto' });
    }
  }, [currentId, follow, tab, editing, query, virtual, shown, virtualizer]);
  const visible = virtual ? virtualizer.getVirtualItems().map(item => ({ segment: shown[item.index], item })) : shown.map(segment => ({ segment, item: null }));
  return <section className="ot-panel ot-sheet" aria-label="Transcript and speakers" data-tour="transcript">
    <div className="ot-sheet-header">
      {props.showTabs !== false && <div className="ot-tabs" role="tablist" aria-label="Meeting content" ref={tabs} onKeyDown={event => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
        event.preventDefault();
        const next = event.key === 'Home' ? 'transcript' : event.key === 'End' ? 'speakers' : tab === 'transcript' ? 'speakers' : 'transcript';
        props.onTab(next); tabs.current?.querySelector<HTMLButtonElement>(`[data-value="${next}"]`)?.focus();
      }}>
        {(['transcript', 'speakers'] as const).map(value => <button type="button" key={value} id={`${id}-${value}`} data-value={value} role="tab" tabIndex={tab === value ? 0 : -1} className="ot-tab" aria-selected={tab === value} aria-controls={`${id}-body`} data-tour={value === 'speakers' ? 'speakers-tab' : undefined} onClick={() => props.onTab(value)}>{value === 'transcript' ? 'Transcript' : 'Speakers'}</button>)}
      </div>}
      {tab === 'transcript' && <div className="ot-transcript-tools">
        <label className="ot-find"><Search size={16} aria-hidden="true" /><input aria-label="Find in this transcript" placeholder="Find in transcript…" value={query} onChange={e => { setQuery(e.target.value); setFollow(false); }} onKeyDown={e => { if (e.key === 'Escape') setQuery(''); }} />{query && <button type="button" aria-label="Clear transcript search" onClick={() => setQuery('')}><X size={16} /></button>}</label>
        <label className="ot-follow"><input type="checkbox" checked={follow} onChange={e => setFollow(e.target.checked)} />Follow playback</label>
        {props.tools}
      </div>}
    </div>
    <div className="ot-sheet-body" id={`${id}-body`} role={props.showTabs === false ? undefined : 'tabpanel'} aria-labelledby={props.showTabs === false ? undefined : `${id}-${tab}`} ref={body}
      onWheel={() => setFollow(false)} onTouchMove={() => setFollow(false)}
      onScroll={() => { const el = body.current; if (!query && el && props.hasMore && !props.isLoadingMore && el.scrollHeight - el.scrollTop - el.clientHeight < 240) props.onLoadMore?.(); }}>
      {tab === 'speakers' ? props.speakersPanel : query && props.meetingId ? <div className="ot-transcript-results">
        <p role="status">{searching ? 'Searching the complete transcript…' : `${matches.length}${matches.length === 100 ? '+' : ''} matching passages`}</p>
        {searchError && <p role="alert" className="ot-danger-text">{searchError}</p>}
        {matches.map(match => <button type="button" className="ot-search-passage" key={match.id} onClick={() => { setQuery(''); props.onSearchSource?.(match); }}><span className="ot-time">{clock(match.start_time ?? 0)}</span><span><HighlightedText text={match.body} query={query} /></span></button>)}
        {!searching && !matches.length && !searchError && <p>Try a shorter phrase or a different spelling.</p>}
      </div> : segments.length === 0 ? props.emptyState ?? <div className="ot-sheet-empty"><strong>No transcript yet</strong><span>Lines appear here when speech is transcribed.</span></div> : <>
        <div style={virtual ? { height: virtualizer.getTotalSize(), position: 'relative' } : undefined}>
        {visible.map(({ segment, item }) => {
          const who = speakerFor(segment, speakers);
          return <article key={segment.id} data-line={segment.id} data-index={item?.index} ref={item ? virtualizer.measureElement : undefined}
            style={item ? { position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${item.start}px)` } : undefined} className={`ot-line${who.tone ? ` ot-tone-${who.tone}` : ''}`} aria-current={segment.id === currentId ? 'true' : undefined} data-partial={segment.is_partial ? 'true' : undefined}>
            <button type="button" className="ot-line-time ot-time" aria-label={`Jump to ${clock(segment.timestamp)}`} onClick={() => props.onLine(segment)}>{segment.hasTiming === false ? '—' : clock(segment.timestamp)}</button>
            <span className="ot-line-dot" aria-hidden="true" />
            <span className="ot-line-speaker" title={who.name}>{who.name}</span>
            {editing?.id === segment.id ? <form className="ot-line-editor" onSubmit={e => { e.preventDefault(); void save(); }}>
              <label className="sr-only" htmlFor={`${id}-edit`}>Transcript text</label><textarea id={`${id}-edit`} autoFocus value={draft} onChange={e => setDraft(e.target.value)} disabled={saving} rows={4} onKeyDown={e => { if (e.key === 'Escape' && !saving) setEditing(null); if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); void save(); } }} />
              <label>Speaker <select value={voice} onChange={e => setVoice(e.target.value)} disabled={saving}><option value="">Unassigned voice</option>{speakers.map(s => <option key={s.id} value={s.id}>{s.name}</option>)}</select></label>
              <div className="ot-inline-form"><button type="submit" className="ot-button ot-button-primary ot-button-small" disabled={saving || !draft.trim()}><Check size={16} />{saving ? 'Saving…' : 'Save correction'}</button><button type="button" className="ot-button ot-button-secondary ot-button-small" disabled={saving} onClick={() => setEditing(null)}>Cancel</button></div>
              {error && <p role="alert" className="ot-danger-text">{error}</p>}
            </form> : <div className="ot-line-content"><p className="ot-line-text" onDoubleClick={() => props.onEdit && edit(segment)}><HighlightedText text={segment.text} query={query} /></p>{props.onEdit && <button type="button" className="ot-edit-line" aria-label={`Edit transcript at ${clock(segment.timestamp)}`} onClick={() => edit(segment)}><Edit3 size={15} /><span>Edit</span></button>}</div>}
          </article>;
        })}
        </div>
        {props.hasMore && <button type="button" className="ot-sheet-more" onClick={props.onLoadMore} disabled={props.isLoadingMore}>{props.isLoadingMore ? 'Loading more…' : 'Load more of the transcript'}</button>}
      </>}
    </div>
  </section>;
}
