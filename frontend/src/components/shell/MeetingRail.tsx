'use client';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useVirtualizer } from '@tanstack/react-virtual';
import { usePathname, useRouter, useSearchParams } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Archive, FolderOpen, Library, ListChecks, Mic, MoreHorizontal, Pin, Search, Trash2, Upload, X } from 'lucide-react';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useConfig } from '@/contexts/ConfigContext';
import { clock, shortDate, type MeetingOverview } from '@/lib/session';
import { sourceUrl, type MeetingSource, type SearchResponse } from '@/lib/workspace';
import { useWorkspacePreference } from '@/hooks/useWorkspacePreference';
import { HighlightedText } from '@/components/session/HighlightedText';
import { Menu, MenuItem, MenuSeparator } from './Menu';
import { SpeakingPattern } from './SpeakingPattern';
import { invalidateMeetingPreview, preloadMeeting } from '@/lib/meeting-preview';

interface MeetingState { meeting_id: string; pinned: boolean; archived: boolean; trashed_at: string | null; title: string }
export function MeetingRail() {
  const router = useRouter(); const pathname = usePathname(); const params = useSearchParams();
  const { meetings, setCurrentMeeting, handleRecordingToggle, refetchMeetings } = useSidebar();
  const { openImportDialog } = useImportDialog(); const { betaFeatures } = useConfig();
  const [overviews, setOverviews] = useState<MeetingOverview[] | null>(null);
  const [states, setStates] = useState<MeetingState[]>([]);
  const [query, setQuery] = useState(''); const [matches, setMatches] = useState<MeetingSource[]>([]);
  const [searching, setSearching] = useState(false); const [error, setError] = useState('');
  const [filter, setFilter] = useWorkspacePreference<'active' | 'archived' | 'all'>('meeting-filter', 'active');
  const [sort, setSort] = useWorkspacePreference<'recent' | 'oldest' | 'title'>('meeting-sort', 'recent');
  const [waveforms, setWaveforms] = useWorkspacePreference('meeting-waveforms', false);
  const searchField = useRef<HTMLInputElement>(null);
  const activeId = pathname?.startsWith('/meeting-details') ? params.get('id') : null;
  const loadVersion = useRef(0);
  const load = useCallback(async () => {
    const version = ++loadVersion.current;
    try {
      const [rows, metadata] = await Promise.all([invoke<MeetingOverview[]>('meeting_overviews', { includePattern: waveforms }), invoke<MeetingState[]>('workspace_meeting_states')]);
      if (version !== loadVersion.current) return;
      setOverviews(rows); setStates(metadata || []); setError('');
    } catch { if (version === loadVersion.current) setError('Could not refresh meetings.'); }
  }, [waveforms]);
  useEffect(() => () => { ++loadVersion.current; }, []);
  useEffect(() => { void load(); }, [load, meetings]);
  useEffect(() => { router.prefetch('/meeting-details'); }, [router]);
  useEffect(() => {
    // Refreshing the sidebar list triggers load above; don't query the entire
    // library again here and a third time after an organisation command.
    const stops = ['meeting-speakers-updated', 'workspace-meetings-changed'].map(event => listen(event, () => { invalidateMeetingPreview(); void refetchMeetings(); }));
    return () => { stops.forEach(stop => void stop.then(fn => fn())); };
  }, [load, refetchMeetings]);
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault(); window.dispatchEvent(new Event('overtalk:show-meetings'));
        setTimeout(() => searchField.current?.focus(), 50);
      }
    }; window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey);
  }, []);
  useEffect(() => {
    if (!query.trim()) { setMatches([]); setSearching(false); return; }
    let active = true; setSearching(true);
    const timer = setTimeout(() => {
      void invoke<SearchResponse>('workspace_search', { request: { query, limit: 100, semantic: false } })
        .then(result => { if (active) { setMatches(result.sources); setError(''); } })
        .catch(() => { if (active) setError('Search failed. Change the query to try again.'); })
        .finally(() => { if (active) setSearching(false); });
    }, 250);
    return () => { active = false; clearTimeout(timer); };
  }, [query]);
  const list = useRef<HTMLDivElement>(null);
  const statesById = useMemo(() => new Map(states.map(state => [state.meeting_id, state])), [states]);
  const passagesByMeeting = useMemo(() => {
    const groups = new Map<string, MeetingSource[]>();
    for (const match of matches) {
      if (match.source_kind === 'meeting') continue;
      const group = groups.get(match.meeting_id) ?? [];
      if (group.length < 3) group.push(match);
      groups.set(match.meeting_id, group);
    }
    return groups;
  }, [matches]);
  const matchingIds = useMemo(() => new Set(matches.map(match => match.meeting_id)), [matches]);
  const rows = useMemo(() => {
    const q = query.trim().toLocaleLowerCase();
    const result = (overviews ?? meetings.map(m => ({ ...m, created_at: '', duration: 0, speakers: [], pattern: [] }))).filter(row => {
      const state = statesById.get(row.id);
      if (state?.trashed_at) return false;
      if (q) return row.title.toLocaleLowerCase().includes(q) || matchingIds.has(row.id);
      return filter === 'all' || Boolean(state?.archived) === (filter === 'archived');
    });
    return result.sort((a,b) => {
      const pin = Number(statesById.get(b.id)?.pinned || false) - Number(statesById.get(a.id)?.pinned || false);
      return pin || (sort === 'title' ? a.title.localeCompare(b.title) : sort === 'oldest' ? a.created_at.localeCompare(b.created_at) : b.created_at.localeCompare(a.created_at));
    });
  }, [overviews, meetings, statesById, query, matchingIds, filter, sort]);
  const virtual = rows.length > 80;
  useEffect(() => {
    // A few likely destinations, never an entire library. Hover/focus warms
    // other rows and shares the same bounded cache with the meeting route.
    const timer = window.setTimeout(() => {
      rows.slice(0, 3).forEach(row => { void preloadMeeting(row.id).catch(() => {}); });
    }, 0);
    return () => window.clearTimeout(timer);
  }, [rows]);
  const virtualizer = useVirtualizer({
    count: virtual ? rows.length : 0,
    getScrollElement: () => list.current,
    estimateSize: () => waveforms ? 112 : 80,
    getItemKey: index => rows[index].id,
    overscan: 6,
  });
  const visible = virtual ? virtualizer.getVirtualItems().map(item => ({ row: rows[item.index], item })) : rows.map(row => ({ row, item: null }));
  useEffect(() => { list.current?.scrollTo({ top: 0 }); }, [query, filter, sort]);
  useEffect(() => { virtualizer.measure(); }, [waveforms, virtualizer]);
  const organise = async (row: MeetingOverview, patch: { pinned?: boolean; archived?: boolean; trashed?: boolean }) => {
    try {
      await invoke('workspace_organise_meeting', { change: { meeting_id: row.id, ...patch } });
      if (patch.trashed && activeId === row.id) router.push('/');
      if (patch.trashed || patch.archived) toast.success(patch.trashed ? 'Meeting moved to Trash' : 'Meeting archived', { description: 'The recording, transcript and notes are kept.', action: { label: 'Undo', onClick: () => void organise(row, patch.trashed ? { trashed: false } : { archived: false }) } });
    } catch (e) { toast.error('Could not update the meeting', { description: String(e) }); }
  };
  return <nav className="ot-rail" aria-label="Meetings" data-tour="rail">
    <div className="ot-rail-nav">
      <button type="button" className="ot-button ot-button-primary" onClick={handleRecordingToggle}><Mic size={17} />Record a meeting</button>
      {betaFeatures.importAndRetranscribe && <button type="button" className="ot-tool" onClick={() => openImportDialog()}><Upload size={16} />Import audio</button>}
      <div className="ot-workspace-links">
        <button type="button" data-tour="library-button" aria-current={pathname === '/library' && (!params.get('tab') || params.get('tab') === 'search') ? 'page' : undefined} onClick={() => router.push('/library')}><Library size={17} />Library</button>
        <button type="button" aria-current={pathname === '/library' && params.get('tab') === 'actions' ? 'page' : undefined} onClick={() => router.push('/library?tab=actions')}><ListChecks size={17} />Actions</button>
        <button type="button" aria-current={pathname === '/library' && params.get('tab') === 'trash' ? 'page' : undefined} onClick={() => router.push('/library?tab=trash')}><Trash2 size={17} />Trash</button>
      </div>
      <label className="ot-rail-search" data-tour="search"><Search aria-hidden="true" /><input ref={searchField} value={query} placeholder="Search all meetings" aria-label="Search meeting titles and transcripts" onChange={e => setQuery(e.target.value)} onKeyDown={e => { if (e.key === 'Escape') setQuery(''); }} />{query && <button type="button" className="ot-icon-button" aria-label="Clear search" onClick={() => setQuery('')}><X /></button>}</label>
      <div className="ot-rail-filters"><label className="sr-only" htmlFor="meeting-filter">Show meetings</label><select id="meeting-filter" value={filter} onChange={e => setFilter(e.target.value as typeof filter)}><option value="active">Meetings</option><option value="archived">Archived</option><option value="all">All meetings</option></select>
        <Menu trigger={<button type="button" className="ot-icon-button" aria-label="Meeting list options"><MoreHorizontal /></button>}><MenuItem onSelect={() => setSort('recent')}>Newest first{sort === 'recent' ? ' ✓' : ''}</MenuItem><MenuItem onSelect={() => setSort('oldest')}>Oldest first{sort === 'oldest' ? ' ✓' : ''}</MenuItem><MenuItem onSelect={() => setSort('title')}>By title{sort === 'title' ? ' ✓' : ''}</MenuItem><MenuSeparator /><MenuItem onSelect={() => setWaveforms(!waveforms)}>{waveforms ? 'Hide' : 'Show'} waveform previews</MenuItem></Menu>
      </div>
    </div>
    <div className="ot-rail-list" ref={list}>
      {error && <p className="ot-rail-empty" role="alert">{error} <button className="ot-link" onClick={() => void load()}>Retry</button></p>}
      {searching && <p className="ot-rail-empty" role="status">Searching transcripts…</p>}
      {!rows.length && <p className="ot-rail-empty">{query ? `No matches for “${query}”. Try a shorter phrase.` : filter === 'archived' ? 'Archived meetings appear here.' : 'Your recordings and imports will appear here.'}</p>}
      <div className="ot-meeting-rows" style={virtual ? { height: virtualizer.getTotalSize(), position: 'relative' } : undefined}>
      {visible.map(({ row, item }) => {
        const state = statesById.get(row.id);
        const passages = passagesByMeeting.get(row.id) ?? [];
        return <div key={row.id} className="ot-meeting" data-index={item?.index} ref={item ? virtualizer.measureElement : undefined}
          style={item ? { position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${item.start}px)`, margin: 0, paddingBottom: 4, contentVisibility: 'visible' } : undefined} aria-current={row.id === activeId ? 'page' : undefined}>
          <button type="button" className="ot-meeting-open"
            onPointerEnter={() => { void preloadMeeting(row.id).catch(() => {}); }}
            onFocus={() => { void preloadMeeting(row.id).catch(() => {}); }}
            onPointerDown={() => { void preloadMeeting(row.id).catch(() => {}); }}
            onClick={() => {
              setCurrentMeeting({ id: row.id, title: row.title });
              const url = `/meeting-details?id=${encodeURIComponent(row.id)}`;
              // The route is already mounted; only its locally loaded meeting
              // changes. Next's history integration updates useSearchParams
              // without fetching the same static route payload again.
              if (pathname === '/meeting-details') window.history.pushState(null, '', url);
              else router.push(url);
            }}>
            <span className="ot-meeting-title">{state?.pinned && <Pin size={13} aria-label="Pinned" />}<HighlightedText text={row.title} query={query} /></span>
            <span className="ot-meeting-meta ot-time">{row.created_at && shortDate(row.created_at)}{row.duration > 0 && ` · ${clock(row.duration)}`}{state?.archived && ' · Archived'}</span>
            {waveforms && row.pattern.length > 0 && <SpeakingPattern pattern={row.pattern} />}
          </button>
          <Menu trigger={<button type="button" className="ot-icon-button ot-meeting-more" aria-label={`Actions for ${row.title}`}><MoreHorizontal /></button>}>
            <MenuItem icon={<Pin />} onSelect={() => void organise(row, { pinned: !state?.pinned })}>{state?.pinned ? 'Unpin' : 'Pin'} meeting</MenuItem>
            <MenuItem icon={<Archive />} onSelect={() => void organise(row, { archived: !state?.archived })}>{state?.archived ? 'Unarchive' : 'Archive'} meeting</MenuItem>
            <MenuItem icon={<FolderOpen />} onSelect={() => void invoke('open_meeting_folder', { meetingId: row.id }).catch(e => toast.error('Could not open the recording folder', { description: String(e) }))}>Show recording folder</MenuItem>
            <MenuSeparator /><MenuItem icon={<Trash2 />} danger onSelect={() => void organise(row, { trashed: true })}>Move to Trash</MenuItem>
          </Menu>
          {query && passages.map(passage => <button type="button" className="ot-rail-match" key={passage.id} onClick={() => router.push(sourceUrl(passage))}><span className="ot-time">{passage.start_time == null ? 'Notes' : clock(passage.start_time)}</span><span><HighlightedText text={passage.body} query={query} /></span></button>)}
        </div>;
      })}
      </div>
    </div>
  </nav>;
}
