'use client';

import { memo, useId, useLayoutEffect, useRef } from 'react';
import { clock, type SpeechClip } from '@/lib/session';
import { PEAKS_PER_SECOND } from './hooks';

/** A long meeting has more passages than screen pixels. Paint the overview once,
 * retaining precise hit testing and keyboard access without thousands of DOM nodes. */
export const DenseSpeechTrack = memo(function DenseSpeechTrack({ clips, width, duration, peaks, selectedId, name, onSelect, onSeek }: {
  clips: SpeechClip[]; width: number; duration: number; peaks: Uint8Array | null;
  selectedId: string | null; name: string; onSelect: (clip: SpeechClip) => void; onSeek: (seconds: number) => void;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const hint = useId();
  const selected = clips.findIndex(clip => clip.id === selectedId);

  useLayoutEffect(() => {
    const el = canvas.current;
    const host = button.current;
    if (!el || !host || !width) return;
    const draw = () => {
      const height = host.clientHeight;
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      el.width = Math.ceil(width * ratio); el.height = Math.ceil(height * ratio);
      const ctx = el.getContext('2d');
      if (!ctx) return;
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      const style = getComputedStyle(host);
      const ink = style.color;
      const fill = style.getPropertyValue('--tone-fill').trim();
      const pps = width / duration;
      const top = 5; const barHeight = Math.max(4, height - 10);
      for (let i = 0; i < clips.length; i++) {
        const clip = clips[i];
        const x = clip.start * pps;
        const w = Math.max(1, (clip.end - clip.start) * pps);
        ctx.fillStyle = fill; ctx.fillRect(x, top, w, barHeight);
        ctx.strokeStyle = ink; ctx.globalAlpha = i === selected ? 1 : .35;
        ctx.lineWidth = i === selected ? 2 : .5;
        ctx.strokeRect(x, top, w, barHeight);
        ctx.globalAlpha = 1;
        if (peaks?.length && w >= 4) {
          ctx.fillStyle = ink;
          for (let px = 1; px < w - 1; px += 2) {
            const from = Math.floor((clip.start + px / pps) * PEAKS_PER_SECOND);
            const to = Math.min(peaks.length, Math.max(from + 1, Math.ceil((clip.start + (px + 2) / pps) * PEAKS_PER_SECOND)));
            let peak = 0;
            for (let sample = from; sample < to; sample++) peak = Math.max(peak, peaks[sample]);
            const h = Math.max(1, peak / 255 * (barHeight - 4));
            ctx.fillRect(x + px, (height - h) / 2, 1, h);
          }
        } else {
          ctx.fillStyle = ink; ctx.globalAlpha = .55;
          ctx.fillRect(x, height / 2, w, 1); ctx.globalAlpha = 1;
        }
      }
    };
    draw();
    const resize = new ResizeObserver(draw); resize.observe(host);
    const theme = new MutationObserver(draw);
    theme.observe(document.documentElement, { attributes: true, attributeFilter: ['class', 'data-theme'] });
    return () => { resize.disconnect(); theme.disconnect(); };
  }, [clips, width, duration, peaks, selected]);

  const at = (seconds: number) => {
    let lo = 0, hi = clips.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (clips[mid].start <= seconds) lo = mid + 1; else hi = mid;
    }
    const clip = clips[lo - 1];
    return clip && seconds <= clip.end ? clip : null;
  };
  return <>
    <button ref={button} type="button" className="ot-dense-clips" aria-describedby={hint}
      aria-label={`${name}: ${clips.length} passages${selected >= 0 ? `, selected ${clock(clips[selected].start)} to ${clock(clips[selected].end)}` : ''}`}
      onClick={event => {
        if (event.detail === 0) { if (clips.length) onSelect(clips[Math.max(0, selected)]); return; }
        const box = event.currentTarget.getBoundingClientRect();
        const seconds = Math.max(0, Math.min(duration, (event.clientX - box.left) / box.width * duration));
        const clip = at(seconds);
        if (clip) onSelect(clip); else onSeek(seconds);
      }}
      onKeyDown={event => {
        let index: number;
        if (event.key === 'ArrowRight') index = Math.min(clips.length - 1, selected + 1);
        else if (event.key === 'ArrowLeft') index = Math.max(0, selected - 1);
        else if (event.key === 'Home') index = 0;
        else if (event.key === 'End') index = clips.length - 1;
        else return;
        event.preventDefault();
        if (clips[index]) onSelect(clips[index]);
      }}><canvas ref={canvas} aria-hidden="true" /></button>
    <span id={hint} className="sr-only">Use left and right arrows to select passages, or Home and End for the first and last passage. Click a passage to review it.</span>
  </>;
});
