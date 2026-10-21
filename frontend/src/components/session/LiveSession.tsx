'use client';

import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { useRouter } from 'next/navigation';
import { listen } from '@tauri-apps/api/event';
import { PEAKS_PER_SECOND, createClock, useClock, type WritableClock } from './hooks';
import { Timecode } from './TransportBar';
import { Bookmark, ChevronDown, Mic, MoreHorizontal, Settings, Trash2, Volume2 } from 'lucide-react';
import type { Transcript, TranscriptSegmentData } from '@/types';
import { clock, speakerSummary, speechClips, toneOf, type TimelineSpeaker, type TimelineTurn } from '@/lib/session';
import { pendingMarks, usePendingMarks } from '@/lib/live-marks';
import { Menu, MenuItem } from '@/components/shell/Menu';
import { Arrangement } from './Arrangement';
import { TranscriptSheet, speakerFor, type SheetTab } from './TranscriptSheet';
import { invoke } from '@tauri-apps/api/core';
import { SpeechNotice, type SpeechNoticeData } from './SpeechNotice';
import { TRANSCRIBE_AFTER_CALL_KEY } from './TranscribeLater';
import { useWorkspacePreference } from '@/hooks/useWorkspacePreference';

const NEW_VOICE = '__new_voice';

/** "Default microphone" reads as "Default" after the "Mic:" label. */
function shortSource(label: string, kind: string) {
  const trimmed = label.replace(new RegExp(`\\s*${kind}\\s*`, 'i'), ' ').trim();
  return trimmed || label;
}

interface LiveSessionProps {
  /** RecordingControls in its transport variant; receives the clock to place between its buttons. */
  renderControls: (clock: ReactNode) => ReactNode;
  isRecording: boolean;
  isPaused: boolean;
  /** Seconds recorded so far, excluding pauses, as last reported by the recorder. */
  reportedSeconds: number | null;
  title: string;
  onRename: (title: string) => void;
  transcripts: Transcript[];
  microphoneLabel: string;
  systemLabel: string;
  sourcesLocked: boolean;
  onSources: () => void;
  startPanel: ReactNode;
  speakersPanel: ReactNode;
  tools?: ReactNode;
}

/** Peaks of the recording so far, as the recorder reports them, in the saved-waveform format. */
function useLivePeaks(isRecording: boolean, visible: boolean) {
  const buffer = useRef(new Uint8Array(PEAKS_PER_SECOND * 600));
  const dirty = useRef(false);
  const [snapshot, setSnapshot] = useState<Uint8Array | null>(null);
  useEffect(() => {
    if (!isRecording) return;
    buffer.current = new Uint8Array(PEAKS_PER_SECOND * 600);
    dirty.current = false;
    setSnapshot(null);
    const unlisten = listen<{ start: number; peaks: number[] }>('recording-levels', ({ payload }) => {
      const offset = Math.round(payload.start * PEAKS_PER_SECOND);
      if (!Number.isSafeInteger(offset) || offset < 0) return;
      const needed = offset + payload.peaks.length;
      if (needed > buffer.current.length) {
        const grown = new Uint8Array(Math.max(needed, buffer.current.length * 2));
        grown.set(buffer.current);
        buffer.current = grown;
      }
      buffer.current.set(payload.peaks, offset);
      dirty.current = true;
    });
    return () => { void unlisten.then(stop => stop()); };
  }, [isRecording]);
  useEffect(() => {
    if (!visible) return;
    // Keep recording data without waking React for a hidden waveform. Visible
    // waveforms update at most four times a second, independent of event rate.
    const publish = () => {
      if (!dirty.current) return;
      dirty.current = false;
      setSnapshot(buffer.current.slice());
    };
    publish();
    if (!isRecording) return;
    const timer = window.setInterval(publish, 250);
    return () => window.clearInterval(timer);
  }, [visible, isRecording]);
  return snapshot;
}

/** Whether live transcription is loading, running, or off for this recording (and why). */
function useSpeechStatus(isRecording: boolean) {
  const [ready, setReady] = useState(false);
  const [notice, setNotice] = useState<SpeechNoticeData | null>(null);
  const wasRecording = useRef(false);
  useEffect(() => {
    if (isRecording && !wasRecording.current) { setReady(false); setNotice(null); }
    wasRecording.current = isRecording;
    if (!isRecording) return;
    void invoke<SpeechNoticeData | null>('recording_speech_notice').then((n) => { if (n) setNotice(n); }).catch(() => {});
  }, [isRecording]);
  useEffect(() => {
    const stops = [
      listen('live-transcript-ready', () => setReady(true)),
      listen<SpeechNoticeData>('recording-speech-notice', ({ payload }) => {
        setNotice(payload);
        try { sessionStorage.setItem(TRANSCRIBE_AFTER_CALL_KEY, '1'); } catch { /* the meeting still offers it */ }
      }),
    ];
    return () => { stops.forEach((s) => void s.then((stop) => stop())); };
  }, []);
  return { ready, notice };
}

/** A clock that keeps moving between the recorder's reports, without re-rendering the session. */
function useLiveClock(isRecording: boolean, isPaused: boolean, reported: number | null) {
  const clockRef = useRef<WritableClock>(createClock(0));
  const base = useRef({ seconds: 0, at: 0 });
  const live = clockRef.current;
  useEffect(() => {
    if (!isRecording) { base.current = { seconds: 0, at: performance.now() }; live.set(0); return; }
    if (reported != null) base.current = { seconds: reported, at: performance.now() };
  }, [isRecording, reported, live]);
  useEffect(() => {
    if (!isRecording || isPaused) return;
    if (!base.current.at) base.current = { seconds: reported ?? 0, at: performance.now() };
    const timer = window.setInterval(() => {
      live.set(base.current.seconds + (performance.now() - base.current.at) / 1000);
    }, 250);
    return () => window.clearInterval(timer);
  }, [isRecording, isPaused, reported, live]);
  useEffect(() => {
    if (isPaused) { base.current = { seconds: live.get(), at: performance.now() }; }
  }, [isPaused, live]);
  return live;
}

export function LiveSession(props: LiveSessionProps) {
  const { isRecording, isPaused, transcripts } = props;
  const router = useRouter();
  const recordClock = useLiveClock(isRecording, isPaused, props.reportedSeconds);

  const speech = useSpeechStatus(isRecording);
  const marks = usePendingMarks();
  const [tab, setTab] = useState<SheetTab>('transcript');
  const [showTimeline, setShowTimeline] = useWorkspacePreference('live-timeline.v2', true);
  const livePeaks = useLivePeaks(isRecording, showTimeline);
  const [draft, setDraft] = useState(props.title);
  useEffect(() => setDraft(props.title), [props.title]);

  const segments: TranscriptSegmentData[] = useMemo(() => transcripts.map((t) => ({
    id: t.id,
    timestamp: t.audio_start_time ?? 0,
    endTime: t.audio_end_time,
    hasTiming: t.audio_start_time != null,
    text: t.text,
    confidence: t.confidence,
    speaker_id: t.speaker_id,
    speaker_name: t.speaker_name,
    speaker_confidence: t.speaker_confidence,
    overlap: t.overlap,
    is_partial: t.is_partial,
  })), [transcripts]);

  const { speakers, turns } = useMemo(() => {
    const order: TimelineSpeaker[] = [];
    const list: TimelineTurn[] = [];
    for (const t of transcripts) {
      if (t.audio_start_time == null) continue;
      const end = Math.max(t.audio_end_time ?? t.audio_start_time + (t.duration ?? 2), t.audio_start_time + 0.5);
      const id = t.speaker_id ?? NEW_VOICE;
      if (id !== NEW_VOICE && !order.some((s) => s.id === id)) order.push({ id, name: t.speaker_name || `Speaker ${order.length + 1}` });
      list.push({ speaker_id: id, start: t.audio_start_time, end });
    }
    return { speakers: order, turns: list };
  }, [transcripts]);

  const allClips = useMemo(() => speechClips(turns, 2), [turns]);
  const clips = allClips.filter((c) => c.speakerId !== NEW_VOICE);
  const unassigned = allClips.filter((c) => c.speakerId === NEW_VOICE);
  const talkTimes = useMemo(() => {
    const totals: Record<string, number> = {};
    for (const clip of clips) totals[clip.speakerId] = (totals[clip.speakerId] ?? 0) + (clip.end - clip.start);
    return totals;
  }, [clips]);

  const latestEnd = turns.reduce((m, t) => Math.max(m, t.end), 0);
  // Before and after a recording the playhead rests at the end of what was said.
  useEffect(() => { if (!isRecording) recordClock.set(latestEnd); }, [isRecording, latestEnd, recordClock]);
  const span = useClock(recordClock, (t) => Math.max(480, Math.ceil((t + 60) / 300) * 300));
  const lastLine = segments.length ? segments[segments.length - 1] : null;
  const [selected, setSelected] = useState<string | null>(null);

  const lineAt = (at: number) => {
    let found: TranscriptSegmentData | null = null;
    for (const line of segments) { if (line.timestamp <= at + 0.05) found = line; else break; }
    return found;
  };
  const speakerAt = (at: number) => {
    const clip = clips.find((c) => at >= c.start - 1 && at <= c.end + 1);
    const index = clip ? speakers.findIndex((s) => s.id === clip.speakerId) : -1;
    return index >= 0 ? { name: speakers[index].name, tone: toneOf(index) } : null;
  };

  const mark = () => {
    if (!isRecording) return;
    pendingMarks.add({ at: Math.round(recordClock.get() * 10) / 10, label: '' });
  };

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key.toLowerCase() !== 'm' || event.ctrlKey || event.metaKey || event.altKey || event.repeat) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest('input, textarea, select, [contenteditable="true"]')) return;
      event.preventDefault();
      mark();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const commitTitle = () => {
    const next = draft.trim();
    if (next && next !== props.title) props.onRename(next);
    else setDraft(props.title);
  };

  const today = new Date().toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
  const idle = !isRecording && transcripts.length === 0;

  return (
    <div className="ot-session ot-live-session" data-idle={idle ? "true" : undefined}>
      <div className="ot-transport" role="toolbar" aria-label="Recording" data-tour="transport">
        {props.renderControls(<Timecode source={recordClock} label={(s) => `Recorded ${clock(s)}`} />)}
        <span className="ot-transport-divider" aria-hidden="true" />
        <div className="ot-heading">
          <input
            className="ot-title-input"
            data-tour="title"
            value={draft}
            placeholder="Untitled meeting"
            aria-label="Meeting title"
            spellCheck={false}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={commitTitle}
            onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); if (e.key === 'Escape') setDraft(props.title); }}
          />
          <div className="ot-meta ot-time">
            <span>{today}</span>
            <span className="ot-dot-sep" aria-hidden="true" />
            <span>{speakers.length ? speakerSummary(speakers) : isRecording ? 'Listening for voices' : 'Not recording'}</span>
          </div>
        </div>
        <div className="ot-transport-tools">
          <div className="ot-sources" data-tour="sources">
            <button type="button" className="ot-source" data-tour="mic" onClick={props.onSources} disabled={props.sourcesLocked} title={props.sourcesLocked ? 'Sources are fixed while recording' : 'Choose microphone and system audio'}>
              <Mic aria-hidden="true" /><span>Mic: {shortSource(props.microphoneLabel, 'microphone')}</span><ChevronDown aria-hidden="true" />
            </button>
            <button type="button" className="ot-source" data-tour="system" onClick={props.onSources} disabled={props.sourcesLocked} title={props.sourcesLocked ? 'Sources are fixed while recording' : 'Choose microphone and system audio'}>
              <Volume2 aria-hidden="true" /><span>System: {shortSource(props.systemLabel, 'system audio')}</span><ChevronDown aria-hidden="true" />
            </button>
          </div>
          <button type="button" className="ot-icon-button" aria-label="Settings" title="Settings" data-tour="settings" onClick={() => router.push('/settings')}><Settings /></button>
        </div>
      </div>
      {!idle && <div className="ot-live-tools"><span>Speaker labels are provisional. Review them after the meeting.</span><button type="button" className="ot-tool" aria-expanded={showTimeline} onClick={() => setShowTimeline(!showTimeline)}>{showTimeline ? 'Hide' : 'Show'} speaker timeline</button></div>}
      {!idle && showTimeline && <Arrangement
        duration={span}
        speakers={speakers}
        clips={clips}
        notes={[]}
        peaks={livePeaks}
        clock={recordClock}
        selectedId={selected}
        solo={new Set()}
        muted={new Set()}
        hasSummary={false}
        live
        talkTimes={talkTimes}
        unassigned={unassigned}
        marks={marks.map((m, i) => ({ id: `mark-${i}`, at: m.at, label: m.label }))}
        onSelectMark={(id) => setSelected(id)}
        onToggleSolo={() => {}}
        onToggleMute={() => {}}
        onSelectSpeech={(clip) => { const line = lineAt(clip.start); setSelected(line?.id ?? clip.id); }}
        onSelectNote={() => {}}
        onSeek={() => {}}
      />}
      <div className="ot-deck-lower">
        <TranscriptSheet
          tab={tab}
          onTab={setTab}
          segments={segments}
          speakers={speakers}
          currentId={selected && segments.some((s) => s.id === selected) ? selected : lastLine?.id ?? null}
          follow
          onLine={(line) => setSelected(line.id)}
          speakersPanel={props.speakersPanel}
          tools={props.tools}
          emptyState={idle ? props.startPanel : speech.notice ? (
            <div className="ot-sheet-empty">
              <SpeechNotice title="Recording audio only" notice={speech.notice} after="The transcript is made from the recording when you stop." />
            </div>
          ) : speech.ready || transcripts.length > 0 ? (
            <div className="ot-sheet-empty"><strong>Listening</strong><span>Lines appear here a few seconds after someone speaks.</span></div>
          ) : (
            <div className="ot-sheet-empty" role="status"><strong>Recording. The speech model is loading</strong><span>Lines appear in a few seconds. Nothing you say is lost while it loads.</span></div>
          )}
        />
        <aside className="ot-panel ot-marks" aria-label="Marked moments" data-tour="marks">
          <h3 className="ot-marks-head">Marked moments</h3>
          <div className="ot-marks-body">
            <button type="button" className="ot-button ot-button-primary ot-mark-button" data-tour="mark" disabled={!isRecording} onClick={mark}>
              <Bookmark aria-hidden="true" />
              <span>Mark this moment</span>
              <kbd aria-hidden="true">M</kbd>
            </button>
            {marks.length === 0 ? (
              <p className="ot-marks-empty">
                {isRecording ? 'Press M when something matters. The mark keeps the second it happened, and the line said then.' : 'During a call, press M to mark a moment. Marks stay with the meeting.'}
              </p>
            ) : (
              <ol className="ot-mark-list">
                {marks.map((m, index) => {
                  const line = lineAt(m.at);
                  // The dot and name follow the quoted line, so they always agree with the quote.
                  const fromLine = line ? speakerFor(line, speakers) : null;
                  const who = fromLine?.tone ? { name: fromLine.name, tone: fromLine.tone } : speakerAt(m.at);
                  return (
                    <li key={`${m.at}-${index}`} className="ot-mark-item">
                      <span className="ot-line-time ot-time">{clock(m.at)}</span>
                      <span className={`ot-line-dot${who ? ` ot-tone-${who.tone}` : ''}`} aria-hidden="true" style={who ? undefined : { background: 'var(--ot-line-strong)' }} />
                      <span>
                        <span className="ot-mark-title">
                          <input
                            defaultValue={m.label}
                            placeholder={who ? `${who.name} at ${clock(m.at)}` : `Moment at ${clock(m.at)}`}
                            aria-label={`Name the moment at ${clock(m.at)}`}
                            onBlur={(e) => pendingMarks.rename(index, e.target.value.trim())}
                            onKeyDown={(e) => { if (e.key === 'Enter') e.currentTarget.blur(); }}
                          />
                        </span>
                        {line && <span className="ot-mark-quote">“{line.text}”</span>}
                      </span>
                      <Menu trigger={<button type="button" className="ot-icon-button ot-inspector-more" aria-label={`Actions for the moment at ${clock(m.at)}`}><MoreHorizontal /></button>}>
                        <MenuItem icon={<Trash2 />} danger onSelect={() => pendingMarks.remove(index)}>Remove mark</MenuItem>
                      </Menu>
                    </li>
                  );
                })}
              </ol>
            )}
          </div>
        </aside>
      </div>
    </div>
  );
}
