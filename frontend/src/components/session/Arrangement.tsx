'use client';

import { memo, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import { DenseSpeechTrack } from './DenseSpeechTrack';
import { clock, rulerSteps, toneOf, type NoteClip, type SpeechClip, type TimelineSpeaker } from '@/lib/session';
import { PEAKS_PER_SECOND, type Clock } from './hooks';

interface ArrangementProps {
  duration: number;
  speakers: TimelineSpeaker[];
  clips: SpeechClip[];
  notes: NoteClip[];
  peaks: Uint8Array | null;
  clock: Clock;
  selectedId: string | null;
  solo: ReadonlySet<string>;
  muted: ReadonlySet<string>;
  hasSummary: boolean;
  /** Live recording: no note lanes, talk time per speaker, a lane for voices not yet assigned. */
  live?: boolean;
  talkTimes?: Record<string, number>;
  unassigned?: SpeechClip[];
  marks?: { id: string; at: number; label: string }[];
  onSelectMark?: (id: string, at: number) => void;
  /** Set while the playhead travels to a chosen moment, so it glides instead of jumping. */
  gliding?: boolean;
  onToggleSolo: (speakerId: string) => void;
  onToggleMute: (speakerId: string) => void;
  onSelectSpeech: (clip: SpeechClip) => void;
  onSelectNote: (note: NoteClip) => void;
  onSeek: (seconds: number) => void;
}

/** One bitmap per clip: cheap to repaint, unlike hundreds of SVG bars. */
const Wave = memo(function Wave({ start, end, width, peaks }: { start: number; end: number; width: number; peaks: Uint8Array | null }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  useLayoutEffect(() => {
    const el = canvas.current;
    if (!el || !peaks || width < 8) return;
    const ratio = Math.min(window.devicePixelRatio || 1, 2);
    const height = el.clientHeight || 40;
    el.width = Math.round(width * ratio);
    el.height = Math.round(height * ratio);
    const ctx = el.getContext('2d');
    if (!ctx) return;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = getComputedStyle(el).color;
    ctx.globalAlpha = 0.55;
    ctx.fillRect(3, height / 2 - 0.5, Math.max(0, width - 6), 1);
    ctx.globalAlpha = 1;
    const pitch = 2.2;
    const usable = height * (42 / 56);
    const bars = Math.max(1, Math.floor((width - 6) / pitch));
    const span = end - start;
    for (let i = 0; i < bars; i++) {
      const from = Math.floor((start + (i / bars) * span) * PEAKS_PER_SECOND);
      const to = Math.max(from + 1, Math.floor((start + ((i + 1) / bars) * span) * PEAKS_PER_SECOND));
      let peak = 0;
      for (let k = from; k < to && k < peaks.length; k++) peak = Math.max(peak, peaks[k]);
      const h = Math.max(1.5, (peak / 255) * usable);
      ctx.fillRect(3 + i * pitch, (height - h) / 2, 1.25, h);
    }
  }, [start, end, width, peaks]);
  if (!peaks || width < 8) return null;
  return <canvas ref={canvas} className="ot-wave" aria-hidden="true" />;
});

export function Arrangement(props: ArrangementProps) {
  const { duration, speakers, clips, notes, peaks, clock: playClock, selectedId, solo, muted, live = false, talkTimes, unassigned = [], marks = [] } = props;
  const ruler = useRef<HTMLDivElement>(null);
  const [track, setTrack] = useState({ width: 0, left: 0 });
  const clipsBySpeaker = useMemo(() => {
    const groups = new Map<string, SpeechClip[]>();
    for (const clip of clips) {
      const group = groups.get(clip.speakerId) ?? [];
      group.push(clip); groups.set(clip.speakerId, group);
    }
    return groups;
  }, [clips]);

  useLayoutEffect(() => {
    const element = ruler.current;
    if (!element) return;
    const measure = () => setTrack({ width: element.clientWidth, left: element.offsetLeft });
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const span = Math.max(duration, 1);
  const pps = track.width / span;
  const steps = rulerSteps(span, pps || 1);
  const x = (seconds: number) => Math.max(0, Math.min(track.width, seconds * pps));
  const timeAt = (clientX: number, element: HTMLElement) => {
    const box = element.getBoundingClientRect();
    return Math.max(0, Math.min(span, ((clientX - box.left) / box.width) * span));
  };

  // The playhead follows the clock directly; React does not re-render for it.
  const playhead = useRef<HTMLSpanElement>(null);
  const cap = useRef<HTMLSpanElement>(null);
  useLayoutEffect(() => {
    if (!track.width) return;
    let lastSecond = -1;
    const move = () => {
      const t = playClock.get();
      const transform = `translateX(${Math.max(0, Math.min(track.width, t * (track.width / span)))}px)`;
      if (playhead.current) playhead.current.style.transform = transform;
      if (cap.current) cap.current.style.transform = transform;
      const second = Math.floor(t);
      if (second !== lastSecond && ruler.current) {
        lastSecond = second;
        ruler.current.setAttribute('aria-valuenow', String(second));
        ruler.current.setAttribute('aria-valuetext', clock(second));
      }
    };
    move();
    return playClock.subscribe(move);
  }, [playClock, track.width, span]);

  const scrub = (event: ReactPointerEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    element.setPointerCapture(event.pointerId);
    props.onSeek(timeAt(event.clientX, element));
    const move = (e: PointerEvent) => props.onSeek(timeAt(e.clientX, element));
    const up = () => { element.removeEventListener('pointermove', move); element.removeEventListener('pointerup', up); element.removeEventListener('pointercancel', up); element.removeEventListener('lostpointercapture', up); };
    element.addEventListener('pointermove', move);
    element.addEventListener('pointerup', up);
    element.addEventListener('pointercancel', up);
    element.addEventListener('lostpointercapture', up);
  };

  const onTrack = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) props.onSeek(timeAt(event.clientX, event.currentTarget));
  };

  const gridStyle = { '--grid-step': `${steps.minor * pps}px` } as CSSProperties;
  const decisions = notes.filter((n) => n.kind === 'decision');
  const actions = notes.filter((n) => n.kind === 'action');

  const noteLane = (kind: 'decision' | 'action', items: NoteClip[]) => (
    <div className={`ot-lane ot-tone-${kind}`} key={kind} data-tour={kind === 'decision' ? 'decisions' : 'actions'}>
      <div className="ot-lane-head">
        <span className="ot-lane-name"><span className="ot-chip" aria-hidden="true" /><span>{kind === 'decision' ? 'Decisions' : 'Actions'}</span></span>
      </div>
      <div className="ot-track" style={gridStyle} onPointerDown={onTrack}>
        {track.width > 0 && items.map((note) => {
          const left = x(note.start);
          const spanPx = Math.max(0, x(note.end) - left);
          const style: CSSProperties = kind === 'decision'
            ? { left, width: spanPx, minWidth: 'calc(120 * var(--u))' }
            : { left, minWidth: spanPx, maxWidth: Math.max(160, track.width - left) };
          return (
            <button
              key={note.id}
              type="button"
              className="ot-note"
              data-kind={kind}
              style={style}
              aria-pressed={selectedId === note.id}
              aria-label={`${kind === 'decision' ? 'Decision' : 'Action'} at ${clock(note.moment)}: ${note.text}`}
              onClick={() => props.onSelectNote(note)}
            >
              <span>{note.label}</span>
            </button>
          );
        })}
        {items.length === 0 && (
          <span className="ot-lane-empty">
            {props.hasSummary
              ? `No ${kind === 'decision' ? 'decisions' : 'actions'} in the notes point to a moment yet`
              : `Generate notes to place ${kind === 'decision' ? 'decisions' : 'actions'} here`}
          </span>
        )}
      </div>
    </div>
  );

  return (
    <section className="ot-arrangement" aria-label="Arrangement" data-live={live ? 'true' : undefined} data-gliding={props.gliding ? 'true' : undefined}>
      <div className="ot-ruler" data-tour="ruler" ref={ruler} onPointerDown={live ? undefined : scrub} role={live ? undefined : 'slider'} aria-label={live ? 'Recording timeline' : 'Playhead'} aria-valuemin={live ? undefined : 0} aria-valuemax={live ? undefined : Math.round(span)} tabIndex={live ? undefined : 0}
        onKeyDown={(e) => {
          if (live) return;
          const step = e.shiftKey ? 10 : 1;
          const time = playClock.get();
          if (e.key === 'ArrowRight') { e.preventDefault(); props.onSeek(Math.min(span, time + step)); }
          if (e.key === 'ArrowLeft') { e.preventDefault(); props.onSeek(Math.max(0, time - step)); }
          if (e.key === 'Home') { e.preventDefault(); props.onSeek(0); }
          if (e.key === 'End') { e.preventDefault(); props.onSeek(span); }
        }}
      >
        {track.width > 0 && steps.ticks.map((tick) => (
          <span key={tick.at} className="ot-ruler-tick" data-major={tick.major} style={{ left: x(tick.at) }} />
        ))}
        {track.width > 0 && steps.ticks.filter((t) => t.major && x(t.at) <= track.width - 24).map((tick) => (
          <span key={`l${tick.at}`} className="ot-ruler-label ot-time" style={{ left: x(tick.at) }}>{clock(tick.at)}</span>
        ))}
        {track.width > 0 && duration > 0 && (
          <span ref={cap} className="ot-playhead-cap" aria-hidden="true" />
        )}
        {track.width > 0 && marks.map((mark) => (
          <button
            key={mark.id}
            type="button"
            className="ot-mark-pin"
            style={{ left: x(mark.at) }}
            aria-pressed={selectedId === mark.id}
            aria-label={`Marked moment at ${clock(mark.at)}${mark.label ? `: ${mark.label}` : ''}`}
            title={mark.label || `Marked at ${clock(mark.at)}`}
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => props.onSelectMark?.(mark.id, mark.at)}
          />
        ))}
      </div>
      <div className="ot-lanes" data-tour="lanes">
        {track.width > 0 && duration > 0 && (
          <span ref={playhead} className="ot-playhead" aria-hidden="true" style={{ left: track.left }} />
        )}
        {speakers.map((speaker, index) => {
          const tone = toneOf(index);
          const isSolo = solo.has(speaker.id);
          const isMuted = muted.has(speaker.id) || (solo.size > 0 && !isSolo);
          return (
            <div key={speaker.id} className={`ot-lane ot-tone-${tone}`} data-muted={isMuted ? 'true' : undefined} data-live={live ? 'true' : undefined}>
              <div className="ot-lane-head">
                <span className="ot-lane-name" title={speaker.name}><span className="ot-chip" aria-hidden="true" /><span>{speaker.name}</span></span>
                {live && (
                  <span className="ot-lane-sub ot-time" aria-label={`${clock(talkTimes?.[speaker.id] ?? 0)} talk time`}>
                    {clock(talkTimes?.[speaker.id] ?? 0)}<span className="ot-lane-sub-unit"> talk time</span>
                  </span>
                )}
                {!live && <span className="ot-lane-keys" data-tour={index === 0 ? 'keys' : undefined}>
                  <button type="button" className="ot-key" aria-pressed={isSolo} aria-label={`Play only ${speaker.name}`} title="Play only this speaker" onClick={() => props.onToggleSolo(speaker.id)}>Only</button>
                  <button type="button" className="ot-key" aria-pressed={muted.has(speaker.id)} aria-label={`Skip ${speaker.name}`} title="Skip this speaker's turns" onClick={() => props.onToggleMute(speaker.id)}>Skip</button>
                </span>}
              </div>
              <div className="ot-track" style={gridStyle} onPointerDown={onTrack}>
                {track.width > 0 && !live && clips.length > 400 ? <DenseSpeechTrack
                  clips={clipsBySpeaker.get(speaker.id) ?? []} width={track.width} duration={span}
                  peaks={peaks} selectedId={selectedId} name={speaker.name}
                  onSelect={props.onSelectSpeech} onSeek={props.onSeek}
                /> : track.width > 0 && (clipsBySpeaker.get(speaker.id) ?? []).map((clip) => {
                  const left = x(clip.start);
                  const width = Math.max(6, x(clip.end) - left);
                  return (
                    <button
                      key={clip.id}
                      type="button"
                      className="ot-clip"
                      data-flat={peaks ? undefined : 'true'}
                      data-growing={live && Math.abs(clip.end - playClock.get()) < 4 ? 'true' : undefined}
                      style={{ left, width }}
                      aria-pressed={selectedId === clip.id}
                      aria-label={`${speaker.name}, ${clock(clip.start)} to ${clock(clip.end)}`}
                      onClick={() => props.onSelectSpeech(clip)}
                    >
                      <Wave start={clip.start} end={clip.end} width={width} peaks={peaks} />
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}
        {live && (unassigned.length > 0 || speakers.length === 0) && (
          <div className="ot-lane ot-lane-pending" data-live="true">
            <div className="ot-lane-head">
              <span className="ot-lane-name"><span className="ot-chip" aria-hidden="true" /><span>{speakers.length === 0 ? 'No voices yet' : 'New voice'}</span></span>
              <span className="ot-lane-sub">{unassigned.length ? 'Unassigned' : 'Lanes start on speech'}</span>
            </div>
            <div className="ot-track" style={gridStyle} onPointerDown={onTrack}>
              {track.width > 0 && unassigned.map((clip) => {
                const left = x(clip.start);
                return <span key={clip.id} className="ot-clip ot-clip-pending" style={{ left, width: Math.max(6, x(clip.end) - left) }} aria-hidden="true" />;
              })}
            </div>
          </div>
        )}
        {!live && noteLane('decision', decisions)}
        {!live && noteLane('action', actions)}
      </div>
    </section>
  );
}
