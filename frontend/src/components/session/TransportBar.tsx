'use client';

import { useEffect, useState } from 'react';
import { Pause, Play, Settings, Square, RotateCcw, RotateCw } from 'lucide-react';
import { clock, shortDate, speakerSummary, timecode, type TimelineSpeaker } from '@/lib/session';
import { useClock, type Clock } from './hooks';

/** The big clock; re-renders once a second, not on every audio frame. */
export function Timecode({ source, label }: { source: Clock; label: (seconds: number) => string }) {
  const second = useClock(source, (t) => Math.floor(t));
  return <span className="ot-timecode ot-time" data-tour="timecode" aria-label={label(second)} role="timer">{timecode(second)}</span>;
}

interface TransportBarProps {
  clock: Clock;
  playing: boolean;
  canPlay: boolean;
  recording: boolean;
  title: string;
  createdAt?: string;
  duration: number;
  speakers: TimelineSpeaker[];
  notesOpen: boolean;
  /** Shown only while a recording is running elsewhere; review never starts one. */
  onReturnToRecording: () => void;
  onTogglePlay: () => void;
  onStop: () => void;
  onRename: (title: string) => void;
  onToggleNotes: () => void;
  onSettings: () => void;
  rate: number;
  onRate: (rate: number) => void;
  onSkip: (seconds: number) => void;
  exportControl?: React.ReactNode;
}

export function TransportBar(props: TransportBarProps) {
  const { playing, canPlay, recording, title, createdAt, duration, speakers } = props;
  const [draft, setDraft] = useState(title);
  useEffect(() => setDraft(title), [title]);

  const commit = () => {
    const next = draft.trim();
    if (next && next !== title) props.onRename(next);
    else setDraft(title);
  };

  return (
    <div className="ot-transport" role="toolbar" aria-label="Transport" data-tour="transport">
      <div className="ot-deck">
        <button type="button" className="ot-icon-button ot-skip" aria-label="Back 10 seconds" disabled={!canPlay} onClick={() => props.onSkip(-10)}><RotateCcw /></button>
        <button type="button" className="ot-deck-button" aria-label={playing ? 'Pause' : 'Play'} title={playing ? 'Pause (Space)' : 'Play (Space)'} disabled={!canPlay} onClick={props.onTogglePlay}>
          {playing ? <Pause fill="currentColor" strokeWidth={0} /> : <Play fill="currentColor" strokeWidth={0} />}
        </button>
        <button type="button" className="ot-deck-button" aria-label="Stop and return to the start" title="Stop" disabled={!canPlay} onClick={props.onStop}>
          <Square fill="currentColor" strokeWidth={0} />
        </button>
        <button type="button" className="ot-icon-button ot-skip" aria-label="Forward 10 seconds" disabled={!canPlay} onClick={() => props.onSkip(10)}><RotateCw /></button>
      </div>
      <Timecode source={props.clock} label={(s) => `Playhead at ${clock(s)}`} />
      {recording && (
        <button type="button" className="ot-return-live" onClick={props.onReturnToRecording} title="Go back to the recording in progress">
          <span className="ot-record-dot" aria-hidden="true" />Recording
        </button>
      )}
      <span className="ot-transport-divider" aria-hidden="true" />
      <div className="ot-heading">
        <input
          className="ot-title-input"
          data-tour="title"
          value={draft}
          aria-label="Meeting title"
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur();
            if (e.key === 'Escape') { setDraft(title); requestAnimationFrame(() => e.currentTarget?.blur()); }
          }}
        />
        <div className="ot-meta ot-time">
          {createdAt && <span>{shortDate(createdAt)}</span>}
          {createdAt && duration > 0 && <span className="ot-dot-sep" aria-hidden="true" />}
          {duration > 0 && <span>{clock(duration)}</span>}
          {speakers.length > 0 && <span className="ot-dot-sep" aria-hidden="true" />}
          {speakers.length > 0 && <span>{speakerSummary(speakers)}</span>}
        </div>
      </div>
      <div className="ot-transport-tools">
        <label className="ot-speed"><span className="sr-only">Playback speed</span><select value={props.rate} onChange={e => props.onRate(Number(e.target.value))}>{[0.75,1,1.25,1.5,1.75,2].map(rate => <option key={rate} value={rate}>{rate}×</option>)}</select></label>
        {props.exportControl}
        <button type="button" className="ot-icon-button" aria-label="Settings" title="Settings" data-tour="settings" onClick={props.onSettings}>
          <Settings />
        </button>
      </div>
    </div>
  );
}
