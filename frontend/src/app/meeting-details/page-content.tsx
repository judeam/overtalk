"use client";
import { Suspense, lazy, useState, useEffect, useRef, useMemo, useCallback } from 'react';
import { Copy, ChevronDown, ChevronUp, SlidersHorizontal, FileText, Users, AlignLeft } from 'lucide-react';
import { useWorkspacePreference } from '@/hooks/useWorkspacePreference';
import { ReviewActionDialog } from '@/components/session/ReviewActionDialog';
import { sourceUrl } from '@/lib/workspace';
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog';
import { useRouter, useSearchParams } from 'next/navigation';
import { MeetingSummary, SummaryProcessResponse } from '@/types';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
const SummaryPanel = lazy(() => import('@/components/MeetingDetails/SummaryPanel').then(module => ({ default: module.SummaryPanel })));
import { ModelConfig } from '@/components/ModelSettingsModal';

// Custom hooks
import { useMeetingData } from '@/hooks/meeting-details/useMeetingData';
import { useSummaryGeneration } from '@/hooks/meeting-details/useSummaryGeneration';
import { useTemplates } from '@/hooks/meeting-details/useTemplates';
import { useCopyOperations } from '@/hooks/meeting-details/useCopyOperations';
import { ExportMeetingMenu } from '@/components/MeetingDetails/ExportMeetingMenu';
import { persistedSummaryMarkdown } from '@/lib/summary-export';
import { SpeechAnalysisControls } from '@/components/MeetingDetails/SpeechAnalysisControls';
import { PaginatedTranscriptsResponse, Transcript, TranscriptSegmentData } from '@/types';
import { SpeakerManager } from '@/components/MeetingDetails/SpeakerManager';
import { TranscriptButtonGroup } from '@/components/MeetingDetails/TranscriptButtonGroup';
import { MeetingTags } from '@/components/MeetingDetails/MeetingTags';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { TransportBar } from '@/components/session/TransportBar';
import { Arrangement } from '@/components/session/Arrangement';
import { TranscriptSheet, speakerFor, type SheetTab } from '@/components/session/TranscriptSheet';
import { Inspector, type InspectorItem } from '@/components/session/Inspector';
import { useClock, useSessionAudio, useTimeline, useWaveform } from '@/components/session/hooks';
import { clock, noteClips, speechClips, toneOf } from '@/lib/session';
import { indexAtOrBefore, mergeIntervals, playbackGate } from '@/lib/time-index';
import { TranscribeLater } from '@/components/session/TranscribeLater';
import { useMeetingOperations } from '@/hooks/meeting-details/useMeetingOperations';
import { useConfig } from '@/contexts/ConfigContext';

export default function PageContent({
  meeting,
  summaryData,
  initialSummary,
  isSummaryLoading = false,
  shouldAutoGenerate = false,
  onAutoGenerateComplete,
  onMeetingUpdated,
  onRefetchTranscripts,
  // Pagination props for efficient transcript loading
  segments,
  hasMore,
  isLoadingMore,
  totalCount,
  onLoadMore,
}: {
  meeting: any;
  summaryData: MeetingSummary | null;
  initialSummary: SummaryProcessResponse | null;
  isSummaryLoading?: boolean;
  shouldAutoGenerate?: boolean;
  onAutoGenerateComplete?: () => void;
  onMeetingUpdated?: () => Promise<void>;
  onRefetchTranscripts?: () => Promise<void>;
  // Pagination props
  segments?: TranscriptSegmentData[];
  hasMore?: boolean;
  isLoadingMore?: boolean;
  totalCount?: number;
  loadedCount?: number;
  onLoadMore?: () => void;
}) {
  // State
  const [customPrompt, setCustomPrompt] = useWorkspacePreference<string>(`note-context.${meeting.id}`, '');
  const [notesOpen, setNotesOpen] = useState(false);
  const [notesVisited, setNotesVisited] = useState(false);
  useEffect(() => { if (notesOpen) setNotesVisited(true); }, [notesOpen]);
  const [sheetTab, setSheetTab] = useState<SheetTab>('transcript');
  const [timelineOpen, setTimelineOpen] = useWorkspacePreference('timeline-open.v2', true);
  const [timelineHeight, setTimelineHeight] = useWorkspacePreference('timeline-height.v3', 0);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [taskItem, setTaskItem] = useState<InspectorItem | null>(null);

  // Ref to store the modal open function from SummaryGeneratorButtonGroup
  const openModelSettingsRef = useRef<(() => void) | null>(null);
  const autoGenerationStartedMeetingIdRef = useRef<string | null>(null);

  // Sidebar context
  const { isRecording: recordingInProgress } = useRecordingState();
  const router = useRouter();
  const searchParams = useSearchParams();

  // Get model config from ConfigContext
  const { modelConfig, setModelConfig, isModelConfigLoading } = useConfig();

  // Custom hooks
  const meetingData = useMeetingData({ meeting, summaryData, onMeetingUpdated });
  const templates = useTemplates();

  // Callback to register the modal open function
  const handleRegisterModalOpen = (openFn: () => void) => {
    console.log('📝 Registering modal open function in PageContent');
    openModelSettingsRef.current = openFn;
  };

  // Callback to trigger modal open (called from error handler)
  const handleOpenModelSettings = () => {
    console.log('🔔 Opening model settings from PageContent');
    if (openModelSettingsRef.current) {
      openModelSettingsRef.current();
    } else {
      console.warn('⚠️ Modal open function not yet registered');
    }
  };

  // Save model config to backend database and sync via event
  const handleSaveModelConfig = async (config?: ModelConfig) => {
    if (!config) return;
    try {
      await invoke('api_save_model_config', {
        provider: config.provider,
        model: config.model,
        whisperModel: config.whisperModel,
        apiKey: config.apiKey ?? null,
        ollamaEndpoint: config.ollamaEndpoint ?? null,
      });

      // Emit event so ConfigContext and other listeners stay in sync
      const { emit } = await import('@tauri-apps/api/event');
      await emit('model-config-updated', config.provider === 'privatemode' ? { ...config, apiKey: null } : config);

      toast.success('Model settings saved successfully');
    } catch (error) {
      console.error('Failed to save model config:', error);
      toast.error('Failed to save model settings');
    }
  };

  const summaryGeneration = useSummaryGeneration({
    initialSummary,
    meeting,
    transcripts: meetingData.transcripts,
    modelConfig: modelConfig,
    isModelConfigLoading,
    selectedTemplate: templates.selectedTemplate,
    onMeetingUpdated,
    updateMeetingTitle: meetingData.updateMeetingTitle,
    setAiSummary: meetingData.setAiSummary,
    onOpenModelSettings: handleOpenModelSettings,
  });

  const copyOperations = useCopyOperations({
    meeting,
    transcripts: meetingData.transcripts,
    meetingTitle: meetingData.meetingTitle,
    aiSummary: meetingData.aiSummary,
    blockNoteSummaryRef: meetingData.blockNoteSummaryRef,
  });

  const meetingOperations = useMeetingOperations({
    meeting,
  });

  // Track page view
  useEffect(() => {
    Analytics.trackPageView('meeting_details');
  }, []);

  // Notes open when a generation finishes while the meeting is on screen, not for saved notes.
  const previousSummaryStatusRef = useRef(summaryGeneration.summaryStatus);
  useEffect(() => {
    const previous = previousSummaryStatusRef.current;
    previousSummaryStatusRef.current = summaryGeneration.summaryStatus;
    if (summaryGeneration.summaryStatus === 'completed' && previous !== 'completed' && previous !== 'idle') setNotesOpen(true);
  }, [summaryGeneration.summaryStatus]);
  useEffect(() => { setNotesOpen(false); }, [meeting.id]);

  // Auto-generate only after the model configuration has settled.
  useEffect(() => {
    if (
      !shouldAutoGenerate
      || summaryGeneration.summaryStatus !== 'idle'
      || isModelConfigLoading
      || meetingData.transcripts.length === 0
      || autoGenerationStartedMeetingIdRef.current === meeting.id
    ) {
      return;
    }

    autoGenerationStartedMeetingIdRef.current = meeting.id;
    console.log(`🤖 Auto-generating summary with ${modelConfig.provider}/${modelConfig.model}...`);
    onAutoGenerateComplete?.();
    void summaryGeneration.handleGenerateSummary('');
  }, [
    shouldAutoGenerate,
    meeting.id,
    meetingData.transcripts.length,
    isModelConfigLoading,
    modelConfig.provider,
    modelConfig.model,
    summaryGeneration.handleGenerateSummary,
    summaryGeneration.summaryStatus,
    onAutoGenerateComplete,
  ]);

  // ---------- Session: one timeline for speakers, decisions and actions ----------
  const atParam = Number(searchParams.get('at'));
  const initialAt = Number.isFinite(atParam) && atParam > 0 ? atParam : 0;
  const audio = useSessionAudio(meeting.id, initialAt);
  const { timeline, loaded: timelineLoaded, refetch: refetchTimeline } = useTimeline(meeting.id);
  const peaks = useWaveform(meeting.id, timelineOpen);
  const lines = useMemo(() => segments ?? [], [segments]);

  const [summaryMarkdown, setSummaryMarkdown] = useState('');
  useEffect(() => {
    let active = true;
    persistedSummaryMarkdown(meetingData.aiSummary)
      .then((markdown) => { if (active) setSummaryMarkdown(markdown ?? ''); })
      .catch(() => { if (active) setSummaryMarkdown(''); });
    return () => { active = false; };
  }, [meetingData.aiSummary]);

  const [marks, setMarks] = useState<{ id: number; at: number; label: string }[]>([]);
  useEffect(() => {
    let active = true;
    invoke<{ id: number; at: number; label: string }[]>('meeting_marks', { meetingId: meeting.id })
      .then((rows) => { if (active) setMarks(rows ?? []); })
      .catch(() => { if (active) setMarks([]); });
    return () => { active = false; };
  }, [meeting.id]);

  const clips = useMemo(() => speechClips(timeline.turns), [timeline.turns]);
  const notes = useMemo(() => noteClips(summaryMarkdown, timeline.sources), [summaryMarkdown, timeline.sources]);
  const duration = Math.max(timeline.duration, audio.duration);
  const canPlay = Boolean(meeting.folder_path) && !audio.error;

  const [selectedId, setSelectedId] = useState<string | null>(null);
  const settledRef = useRef(false);
  useEffect(() => { settledRef.current = false; if (initialAt >= 0) audio.seek(initialAt); }, [initialAt]);
  useEffect(() => { settledRef.current = false; setSelectedId(null); }, [meeting.id]);

  const speakerAt = useCallback((seconds: number) => {
    const turn = timeline.turns.find((t) => seconds >= t.start - 0.05 && seconds < t.end);
    if (!turn) return null;
    const index = timeline.speakers.findIndex((s) => s.id === turn.speaker_id);
    return index >= 0 ? { name: timeline.speakers[index].name, tone: toneOf(index) } : null;
  }, [timeline]);

  const timedLines = useMemo(() => lines.filter(line => line.hasTiming !== false), [lines]);
  const lineAt = useCallback((seconds: number) =>
    timedLines[indexAtOrBefore(timedLines, seconds + 0.05, line => line.timestamp)] ?? null,
  [timedLines]);

  // Open on the moment a link asked for, otherwise on the meeting's first decision.
  useEffect(() => {
    if (settledRef.current || !timelineLoaded) return;
    if (initialAt > 0) {
      const note = notes.find((n) => Math.abs(n.moment - initialAt) < 1);
      const line = lineAt(initialAt);
      if (!note && !line) return;
      settledRef.current = true;
      setSelectedId(note?.id ?? line?.id ?? null);
      return;
    }
    const first = notes.find((n) => n.kind === 'decision') ?? notes[0];
    if (first) {
      settledRef.current = true;
      setSelectedId(first.id);
    }
  }, [timelineLoaded, notes, initialAt, lineAt]);

  const currentLineId = useClock(audio.clock, (t) => lineAt(t)?.id ?? null);

  // The playhead glides to a chosen moment; during playback it follows the audio frame by frame.
  const [gliding, setGliding] = useState(false);
  const glideTimer = useRef<number>();
  const select = (id: string, seconds: number) => {
    settledRef.current = true;
    setSelectedId(id);
    setSheetTab('transcript');
    window.clearTimeout(glideTimer.current);
    setGliding(true);
    glideTimer.current = window.setTimeout(() => setGliding(false), 420);
    audio.seek(seconds, audio.playing);
  };
  useEffect(() => () => window.clearTimeout(glideTimer.current), []);

  const inspectorItem: InspectorItem | null = useMemo(() => {
    const note = notes.find((n) => n.id === selectedId);
    if (note) {
      let person: InspectorItem['person'] = null;
      if (note.kind === 'action' && note.owner) {
        const index = timeline.speakers.findIndex((s) => s.name.toLowerCase() === note.owner!.toLowerCase());
        person = { label: 'Assignee', name: note.owner, tone: index >= 0 ? toneOf(index) : null };
      } else if (note.kind === 'decision') {
        const said = speakerAt(note.moment);
        if (said) person = { label: 'Said by', ...said };
      }
      return { id: note.id, heading: note.kind === 'decision' ? 'Decision' : 'Action', time: note.moment, quote: note.text, person, interpretation: true, taskText: note.kind === 'action' ? note.text : undefined, evidence: lines.filter(l => note.sourceIds.some(n => timeline.sources[n - 1]?.[0] === l.timestamp)).map(l => ({ text: l.text, time: l.timestamp })) };
    }
    const mark = marks.find((m) => `mark-${m.id}` === selectedId);
    if (mark) {
      const said = lineAt(mark.at);
      const who = speakerAt(mark.at);
      return {
        id: selectedId!,
        heading: mark.label || 'Marked moment',
        time: mark.at,
        quote: said?.text ?? 'Nothing was transcribed at this moment.',
        person: who ? { label: 'Speaking', ...who } : null,
      };
    }
    const clip = clips.find((c) => c.id === selectedId);
    if (clip) {
      const index = timeline.speakers.findIndex((s) => s.id === clip.speakerId);
      const said = lines.filter((l) => l.timestamp >= clip.start - 0.5 && l.timestamp < clip.end).map((l) => l.text).join(' ');
      return {
        id: clip.id,
        heading: index >= 0 ? timeline.speakers[index].name : 'Speaker',
        time: clip.start,
        quote: said.length > 320 ? `${said.slice(0, 317).trimEnd()}…` : said || `Speech from ${clock(clip.start)} to ${clock(clip.end)}`,
        person: null,
      };
    }
    const line = lines.find((l) => l.id === selectedId);
    if (line) {
      const who = speakerFor(line, timeline.speakers);
      return { id: line.id, heading: who.name, time: line.timestamp, quote: line.text, person: null };
    }
    return null;
  }, [notes, marks, clips, lines, selectedId, timeline.speakers, speakerAt, lineAt]);

  // Solo plays only the chosen speakers; mute skips a speaker unless someone else talks over them.
  const [solo, setSolo] = useState<Set<string>>(new Set());
  const [muted, setMuted] = useState<Set<string>>(new Set());
  useEffect(() => { setSolo(new Set()); setMuted(new Set()); }, [meeting.id]);
  useEffect(() => {
    if (!solo.size && !muted.size) { audio.setGate(null); return; }
    const audible = (id: string) => (solo.size ? solo.has(id) : !muted.has(id));
    const keep = mergeIntervals(clips.filter((c) => audible(c.speakerId)));
    const skip = mergeIntervals(clips.filter((c) => !audible(c.speakerId)));
    audio.setGate(t => playbackGate(keep, skip, solo.size > 0, t));
  }, [solo, muted, clips, audio.setGate]);
  const toggleIn = (set: Set<string>, id: string) => { const next = new Set(set); if (next.has(id)) next.delete(id); else next.add(id); return next; };

  // Space plays and pauses unless the reader is typing or on a control.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== ' ' || event.repeat) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest('input, textarea, select, button, [contenteditable="true"], [role="slider"]')) return;
      event.preventDefault();
      if (canPlay) audio.toggle();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [audio, canPlay]);

  const copy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success('Copied');
    } catch {
      toast.error('Could not copy to the clipboard');
    }
  };

  const rename = async (title: string) => {
    try {
      await invoke('api_save_meeting_title', { meetingId: meeting.id, title });
      meetingData.updateMeetingTitle(title);
    } catch (error) {
      toast.error('Could not rename the meeting', { description: String(error) });
    }
  };

  const exportData = async () => {
    const all: Transcript[] = [];
    let more = true;
    while (more) {
      const page = await invoke<PaginatedTranscriptsResponse>('api_get_meeting_transcripts', { meetingId: meeting.id, limit: 500, offset: all.length });
      if (page.has_more && page.transcripts.length === 0) throw new Error('Could not read the complete transcript');
      all.push(...page.transcripts);
      more = page.has_more;
    }
    const editor = meetingData.blockNoteSummaryRef.current;
    let markdown = await editor?.getMarkdown?.() ?? '';
    if (!markdown && !editor?.isDirty) markdown = await persistedSummaryMarkdown(meetingData.aiSummary);
    return { id: meeting.id, title: meetingData.meetingTitle, createdAt: meeting.created_at, summaryMarkdown: markdown, transcripts: all };
  };

  const summaryPanel = (
    <SummaryPanel
      meeting={meeting}
      meetingTitle={meetingData.meetingTitle}
      summaryRef={meetingData.blockNoteSummaryRef}
      isSaving={meetingData.isSaving}
      isSummaryDirty={meetingData.isSummaryDirty}
      onSaveAll={meetingData.saveAllChanges}
      onCopySummary={copyOperations.handleCopySummary}
      aiSummary={meetingData.aiSummary}
      summaryStatus={summaryGeneration.summaryStatus}
      transcripts={meetingData.transcripts}
      modelConfig={modelConfig}
      setModelConfig={setModelConfig}
      onSaveModelConfig={handleSaveModelConfig}
      onGenerateSummary={summaryGeneration.handleGenerateSummary}
      onStopGeneration={summaryGeneration.handleStopGeneration}
      customPrompt={customPrompt}
      onSaveSummary={meetingData.handleSaveSummary}
      onSummaryChange={meetingData.handleSummaryChange}
      onDirtyChange={meetingData.setIsSummaryDirty}
      summaryError={summaryGeneration.summaryError}
      onRegenerateSummary={summaryGeneration.handleRegenerateSummary}
      getSummaryStatusMessage={summaryGeneration.getSummaryStatusMessage}
      availableTemplates={templates.availableTemplates}
      selectedTemplate={templates.selectedTemplate}
      onTemplateSelect={templates.handleTemplateSelection}
      isModelConfigLoading={isModelConfigLoading}
      onOpenModelSettings={handleRegisterModalOpen}
      sourceTimes={timeline.sources}
      onSource={(seconds) => { setNotesOpen(false); select(`source-${seconds}`, seconds); }}
    />
  );

  const inspector = (
    <Inspector
            item={inspectorItem}
            meetingTitle={meetingData.meetingTitle}
            createdAt={meeting.created_at}
            speakers={timeline.speakers}
            canPlay={canPlay}
            onPlayFrom={(seconds) => { settledRef.current = true; audio.seek(seconds, true); }}
            onCopy={(item) => void copy(item.quote)}
            onCopyWithContext={(item) => void copy(`${item.interpretation ? item.quote : `“${item.quote}”`} (${item.heading}, ${clock(item.time)}, ${meetingData.meetingTitle})`)}
            onCreateTask={setTaskItem}
            onOpenNotes={() => setNotesOpen(true)}
          />
  );

  return (
    <div className="ot-session ot-review-session" data-view={notesOpen ? 'notes' : sheetTab} data-timeline-sizing={timelineHeight ? 'manual' : 'auto'} style={{ '--timeline-height': timelineHeight ? `${timelineHeight}px` : 'none' } as React.CSSProperties}>
      <TransportBar
        clock={audio.clock}
        playing={audio.playing}
        canPlay={canPlay}
        recording={recordingInProgress}
        title={meetingData.meetingTitle}
        createdAt={meeting.created_at}
        duration={duration}
        speakers={timeline.speakers}
        notesOpen={notesOpen}
        onReturnToRecording={() => router.push('/')}
        onTogglePlay={audio.toggle}
        onStop={audio.stop}
        onRename={rename}
        onToggleNotes={() => setNotesOpen((open) => !open)}
        onSettings={() => router.push(`/settings?returnTo=${encodeURIComponent(`/meeting-details?id=${meeting.id}&at=${audio.clock.get()}`)}`)}
        rate={audio.rate} onRate={audio.setRate} onSkip={delta => audio.seek(Math.max(0, audio.clock.get() + delta), audio.playing)}
        exportControl={<span data-tour="export"><ExportMeetingMenu onGetData={exportData} /></span>}
      />
      <div className="ot-review-toolbar">
        <div role="tablist" aria-label="Meeting view" className="ot-view-tabs" onKeyDown={event => {
          if (!['ArrowLeft','ArrowRight','Home','End'].includes(event.key)) return;
          event.preventDefault();
          const current = notesOpen ? 1 : sheetTab === 'speakers' ? 2 : 0;
          const next = event.key === 'Home' ? 0 : event.key === 'End' ? 2 : (current + (event.key === 'ArrowRight' ? 1 : 2)) % 3;
          setNotesOpen(next === 1); if (next !== 1) setSheetTab(next === 2 ? 'speakers' : 'transcript');
          event.currentTarget.querySelectorAll<HTMLButtonElement>('[role=tab]')[next]?.focus();
        }}>
          {[{id:'transcript',label:'Transcript',icon:AlignLeft},{id:'notes',label:'Notes',icon:FileText},{id:'speakers',label:'Speakers',icon:Users}].map(v => {
            const active = v.id === (notesOpen ? 'notes' : sheetTab);
            return <button key={v.id} type="button" role="tab" id={`view-${v.id}`} aria-controls="meeting-view" aria-selected={active} tabIndex={active ? 0 : -1} data-tour={v.id === 'notes' ? 'notes-button' : v.id === 'speakers' ? 'speakers-tab' : undefined} onClick={() => { setNotesOpen(v.id === 'notes'); if (v.id !== 'notes') setSheetTab(v.id as SheetTab); }}><v.icon size={17} />{v.label}</button>;
          })}
        </div>
        <div className="ot-review-tools"><button type="button" className="ot-tool ot-open-inspector" onClick={() => setInspectorOpen(true)}>Selected moment</button><button type="button" className="ot-tool" aria-expanded={detailsOpen} onClick={() => setDetailsOpen(!detailsOpen)}><SlidersHorizontal size={16} />Meeting details</button><button type="button" className="ot-tool" aria-expanded={timelineOpen} onClick={() => setTimelineOpen(!timelineOpen)}>{timelineOpen ? <ChevronUp size={16} /> : <ChevronDown size={16} />}Timeline</button></div>
      </div>
      {audio.error && <div className="ot-playback-error" role="alert"><span><strong>Recording unavailable.</strong> {audio.error} The transcript and notes are still available.</span><button type="button" className="ot-tool" onClick={audio.retry}>Retry playback</button><button type="button" className="ot-tool" onClick={meetingOperations.handleOpenMeetingFolder}>Locate recording</button></div>}
      {!meeting.folder_path && <p className="ot-playback-error">This meeting has no saved recording. You can still read, correct and export its transcript.</p>}
      {detailsOpen && <div className="ot-meeting-details-panel"><MeetingTags meetingId={meeting.id} /><SpeechAnalysisControls meetingId={meeting.id} hasRecording={Boolean(meeting.folder_path)} onComplete={async () => { await onRefetchTranscripts?.(); await refetchTimeline(); }} /></div>}
      {timelineOpen && <div className="ot-timeline-panel"><div className="ot-timeline-size"><button type="button" className="ot-tool" aria-pressed={!timelineHeight} onClick={() => setTimelineHeight(0)}>Auto fit</button><label>Height<input type="range" min="150" max="1000" step="10" value={timelineHeight || 600} onChange={e => setTimelineHeight(Number(e.target.value))} /></label></div>

      <Arrangement
        duration={duration}
        speakers={timeline.speakers}
        clips={clips}
        notes={notes}
        peaks={peaks}
        clock={audio.clock}
        selectedId={selectedId}
        solo={solo}
        muted={muted}
        hasSummary={summaryMarkdown.trim().length > 0}
        marks={marks.map((m) => ({ id: `mark-${m.id}`, at: m.at, label: m.label }))}
        onSelectMark={(id, at) => select(id, at)}
        gliding={gliding}
        onToggleSolo={(id) => setSolo((set) => toggleIn(set, id))}
        onToggleMute={(id) => setMuted((set) => toggleIn(set, id))}
        onSelectSpeech={(clip) => select(clip.id, clip.start)}
        onSelectNote={(note) => select(note.id, note.moment)}
        onSeek={(seconds) => audio.seek(seconds, audio.playing)}
      />
      </div>}
      <div id="meeting-view" role="tabpanel" aria-labelledby={`view-${notesOpen ? 'notes' : sheetTab}`} className="ot-meeting-view">
      <div className="ot-deck-lower ot-deck-notes" hidden={!notesOpen}>
        <section className="ot-panel ot-notes" aria-label="Notes">
          <details className="ot-note-context"><summary>Context for your notes</summary><textarea value={customPrompt} onChange={e => setCustomPrompt(e.target.value)} placeholder="Names, project terms, or what to focus on. Used next time you generate notes." rows={2} /></details>
          <div className="ot-notes-body" data-tour="notes-body">{(notesOpen || notesVisited) && (isSummaryLoading
            ? <p className="ot-empty-copy" role="status">Loading saved notes…</p>
            : <Suspense fallback={<p className="ot-empty-copy" role="status">Opening notes…</p>}>{summaryPanel}</Suspense>)}</div>
        </section>
      </div>
      {!notesOpen && (
        <div className="ot-deck-lower">
          <TranscriptSheet
            tab={sheetTab}
            onTab={setSheetTab}
            segments={lines}
            speakers={timeline.speakers}
            currentId={currentLineId}
            follow
            showTabs={false}
            meetingId={meeting.id}
            onSearchSource={source => { router.push(sourceUrl(source)); setSheetTab('transcript'); }}
            onEdit={async (segment, text, speakerId) => {
              await invoke('workspace_correct_transcript', { change: { meeting_id: meeting.id, segment_id: segment.id, text, speaker_id: speakerId, expected_text: segment.text, expected_speaker_id: segment.speaker_id ?? null } });
              await onRefetchTranscripts?.(); await refetchTimeline();
              toast.success('Transcript corrected', { description: 'Your previous transcript is available in Meeting details.', action: { label: 'Undo', onClick: () => { void invoke('speech_restore_transcript', { meetingId: meeting.id }).then(async () => { await onRefetchTranscripts?.(); await refetchTimeline(); }).catch(e => toast.error('Could not undo', { description: String(e) })); } } });
            }}
            onLine={(line) => select(line.id, line.timestamp)}
            hasMore={hasMore}
            isLoadingMore={isLoadingMore}
            onLoadMore={onLoadMore}
            emptyState={<TranscribeLater meetingId={meeting.id} folderPath={meeting.folder_path} onDone={async () => { await onRefetchTranscripts?.(); await refetchTimeline(); }} />}
            tools={lines.length > 0 ? <button type="button" className="ot-tool" onClick={copyOperations.handleCopyTranscript}><Copy aria-hidden="true" />Copy transcript</button> : undefined}
            speakersPanel={(
              <div className="ot-speakers-panel">
                <TranscriptButtonGroup
                  transcriptCount={totalCount ?? lines.length}
                  onCopyTranscript={copyOperations.handleCopyTranscript}
                  onOpenMeetingFolder={meetingOperations.handleOpenMeetingFolder}
                  meetingId={meeting.id}
                  meetingFolderPath={meeting.folder_path}
                  onRefetchTranscripts={onRefetchTranscripts}
                />
                <SpeakerManager meetingId={meeting.id} tones={Object.fromEntries(timeline.speakers.map((sp, i) => [sp.id, toneOf(i)]))} />
              </div>
            )}
          />
          <div className="ot-inspector-dock">{inspector}</div>

        </div>
      )}
      </div>
      <Dialog open={inspectorOpen} onOpenChange={setInspectorOpen}><DialogContent className="ot-moment-dialog" aria-describedby={undefined}><DialogTitle>Selected moment</DialogTitle>{inspector}</DialogContent></Dialog>
      <ReviewActionDialog item={taskItem} meetingId={meeting.id} onClose={() => setTaskItem(null)} />
    </div>
  );
}
