'use client';

import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { convertFileSrc, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { MeetingTimeline } from '@/lib/session';

/**
 * Where playback may continue from time `t`: return `t` to keep going, a later time to jump
 * over audio the listener muted, or `null` to stop.
 */
type PlaybackGate = (t: number) => number | null;

/** A time value that changes many times a second without re-rendering whoever holds it. */
export interface Clock { get: () => number; subscribe: (listener: () => void) => () => void }
export interface WritableClock extends Clock { set: (value: number) => void }

export function createClock(initial = 0): WritableClock {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set: (next) => { if (next === value) return; value = next; listeners.forEach((listener) => listener()); },
    subscribe: (listener) => { listeners.add(listener); return () => { listeners.delete(listener); }; },
  };
}

/** Re-renders only when `select(time)` changes, e.g. the whole second or the current line. */
export function useClock<T>(clock: Clock, select: (time: number) => T): T {
  return useSyncExternalStore(clock.subscribe, () => select(clock.get()), () => select(clock.get()));
}

export interface SessionAudio {
  clock: Clock;
  duration: number;
  playing: boolean;
  ready: boolean;
  error: string;
  play: () => void;
  pause: () => void;
  toggle: () => void;
  stop: () => void;
  seek: (seconds: number, play?: boolean) => void;
  setGate: (gate: PlaybackGate | null) => void;
  rate: number;
  setRate: (rate: number) => void;
  retry: () => void;
}

/** The meeting's recording behind the transport: one audio element, released on leave. */
export function useSessionAudio(meetingId: string, initialTime = 0): SessionAudio {
  const audio = useRef<HTMLAudioElement | null>(null);
  const pending = useRef<{ seconds: number; play: boolean } | null>(initialTime > 0 ? { seconds: initialTime, play: false } : null);
  const gate = useRef<PlaybackGate | null>(null);
  const frame = useRef(0);
  const clock = useRef<WritableClock>(createClock(initialTime)).current;
  const [duration, setDuration] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');
  const [rate, updateRate] = useState(1);
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let active = true;
    let capability = '';
    let sourceFrame = 0;
    const element = new Audio();
    element.playbackRate = rate;
    element.preload = 'metadata';
    audio.current = element;
    setReady(false);
    setError('');
    setPlaying(false);

    const sync = () => {
      const now = element.currentTime;
      const next = gate.current?.(now);
      if (next === null && !element.paused) {
        element.pause();
      } else if (next != null && next > now + 0.05) {
        element.currentTime = Math.min(next, element.duration || next);
      }
      clock.set(element.currentTime);
      if (!element.paused) frame.current = requestAnimationFrame(sync);
    };
    const onMeta = () => {
      setDuration(Number.isFinite(element.duration) ? element.duration : 0);
      setReady(true);
      const wanted = pending.current;
      pending.current = null;
      if (wanted) {
        element.currentTime = Math.min(wanted.seconds, element.duration || wanted.seconds);
        clock.set(element.currentTime);
        if (wanted.play) void element.play().catch(() => {});
      }
    };
    const onPlay = () => { setPlaying(true); cancelAnimationFrame(frame.current); frame.current = requestAnimationFrame(sync); };
    const onPause = () => { setPlaying(false); cancelAnimationFrame(frame.current); clock.set(element.currentTime); };
    const onSeeked = () => clock.set(element.currentTime);
    const onError = () => { if (element.src) setError('The recording could not be played.'); };
    element.addEventListener('loadedmetadata', onMeta);
    element.addEventListener('play', onPlay);
    element.addEventListener('pause', onPause);
    element.addEventListener('ended', onPause);
    element.addEventListener('seeked', onSeeked);
    element.addEventListener('error', onError);

    invoke<string>('workspace_audio_source', { meetingId })
      .then((source) => {
        capability = source;
        if (!active) {
          void invoke('workspace_release_audio', { source }).catch(() => {});
          return;
        }
        // Opening a decoder can synchronously occupy WebKit's main thread.
        // Let the transcript paint first, and skip obsolete media setup when
        // the user immediately moves to another meeting.
        sourceFrame = requestAnimationFrame(() => {
          sourceFrame = requestAnimationFrame(() => {
            if (active) element.src = source.startsWith('http://127.0.0.1:') ? source : convertFileSrc(source);
          });
        });
      })
      .catch((e) => { if (active) setError(String(e)); });

    return () => {
      active = false;
      cancelAnimationFrame(sourceFrame);
      cancelAnimationFrame(frame.current);
      element.pause();
      element.removeAttribute('src');
      element.load();
      audio.current = null;
      if (capability) void invoke('workspace_release_audio', { source: capability }).catch(() => {});
    };
  }, [meetingId, attempt]); // playback speed changes do not recreate the audio element

  const play = useCallback(() => {
    const element = audio.current;
    if (!element) return;
    if (!element.src || !ready) {
      pending.current = { seconds: pending.current?.seconds ?? clock.get(), play: true };
      return;
    }
    void element.play().catch(() => setError('Playback was blocked. Press play to try again.'));
  }, [ready, clock]);
  const pause = useCallback(() => audio.current?.pause(), []);
  const seek = useCallback((seconds: number, resume = false) => {
    if (!Number.isFinite(seconds) || seconds < 0) return;
    const element = audio.current;
    clock.set(seconds);
    if (!element || !ready) {
      pending.current = { seconds, play: resume };
      return;
    }
    element.currentTime = Math.min(seconds, element.duration || seconds);
    if (resume && element.paused) void element.play().catch(() => {});
  }, [ready]);
  const toggle = useCallback(() => (playing ? pause() : play()), [playing, pause, play]);
  const stop = useCallback(() => { audio.current?.pause(); seek(0); }, [seek]);
  const setGate = useCallback((next: PlaybackGate | null) => { gate.current = next; }, []);
  const setRate = useCallback((value: number) => {
    if (![0.75, 1, 1.25, 1.5, 1.75, 2].includes(value)) return;
    updateRate(value);
    if (audio.current) audio.current.playbackRate = value;
  }, []);
  const retry = useCallback(() => setAttempt(value => value + 1), []);

  return { clock, duration, playing, ready, error, play, pause, toggle, stop, seek, setGate, rate, setRate, retry };
}

const EMPTY_TIMELINE: MeetingTimeline = { duration: 0, speakers: [], turns: [], sources: [] };

export function useTimeline(meetingId: string) {
  const [timeline, setTimeline] = useState<MeetingTimeline>(EMPTY_TIMELINE);
  const [loaded, setLoaded] = useState(false);
  const request = useRef(0);
  const refetch = useCallback(async () => {
    const version = ++request.current;
    try {
      const result = await invoke<MeetingTimeline>('meeting_timeline', { meetingId });
      if (version === request.current) setTimeline(result);
    } catch (error) {
      if (version === request.current) console.error('Could not load the meeting timeline:', error);
    } finally {
      if (version === request.current) setLoaded(true);
    }
  }, [meetingId]);
  useEffect(() => {
    setLoaded(false);
    setTimeline(EMPTY_TIMELINE);
    void refetch();
    const unlisten = listen<string>('meeting-speakers-updated', (event) => {
      if (!event.payload || event.payload === meetingId) void refetch();
    });
    return () => { ++request.current; void unlisten.then((fn) => fn()); };
  }, [meetingId, refetch]);
  return { timeline, loaded, refetch };
}

/** Peaks at 10 per second (0–255), or null while loading or when there is no recording. */
export function useWaveform(meetingId: string, enabled = true) {
  const [peaks, setPeaks] = useState<Uint8Array | null>(null);
  useEffect(() => {
    let active = true;
    setPeaks(null);
    if (!enabled) return;
    invoke<number[]>('meeting_waveform', { meetingId })
      .then((values) => { if (active) setPeaks(Uint8Array.from(values)); })
      .catch(() => {});
    return () => { active = false; };
  }, [meetingId, enabled]);
  return peaks;
}

export const PEAKS_PER_SECOND = 10;
