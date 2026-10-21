import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { ArrowLeft, ArrowRight, BookOpen } from 'lucide-react';
import type { TourStep } from './lessons';

const findTarget = (name: string) =>
  Array.from(document.querySelectorAll<HTMLElement>(`[data-tour="${name}"]`)).find((el) => {
    const box = el.getBoundingClientRect();
    return box.width > 0 && box.height > 0;
  }) ?? null;

/** Spotlights one real control at a time; steps whose control is not on screen are skipped. */
export function Tour({ steps, onDone, onOpenGuide }: { steps: TourStep[]; onDone: () => void; onOpenGuide: () => void }) {
  const [index, setIndex] = useState(0);
  const [box, setBox] = useState<DOMRect | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const [cardSize, setCardSize] = useState({ width: 360, height: 180 });
  const direction = useRef(1);

  const available = steps.map((step) => Boolean(findTarget(step.target)));
  const visible = steps.filter((_, i) => available[i]);
  const position = visible.indexOf(steps[index]);

  const measure = useCallback(() => {
    const step = steps[index];
    const el = step ? findTarget(step.target) : null;
    setBox(el ? el.getBoundingClientRect() : null);
  }, [steps, index]);

  // Skip steps whose target is missing, in the direction the reader is moving.
  useLayoutEffect(() => {
    const step = steps[index];
    if (!step) { onDone(); return; }
    const el = findTarget(step.target);
    if (!el) {
      const next = index + direction.current;
      if (next < 0 || next >= steps.length) onDone(); else setIndex(next);
      return;
    }
    el.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    measure();
    card.current?.focus({ preventScroll: true });
  }, [index, steps, measure, onDone]);

  useLayoutEffect(() => {
    if (card.current) setCardSize({ width: card.current.offsetWidth, height: card.current.offsetHeight });
  }, [index, box]);

  useEffect(() => {
    window.addEventListener('resize', measure);
    window.addEventListener('scroll', measure, true);
    return () => { window.removeEventListener('resize', measure); window.removeEventListener('scroll', measure, true); };
  }, [measure]);

  const go = useCallback((delta: number) => {
    direction.current = delta;
    setIndex((i) => {
      let next = i + delta;
      while (next >= 0 && next < steps.length && !findTarget(steps[next].target)) next += delta;
      if (next >= steps.length) { onDone(); return i; }
      return Math.max(0, next);
    });
  }, [steps, onDone]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') { event.preventDefault(); onDone(); }
      if (event.key === 'ArrowRight') { event.preventDefault(); go(1); }
      if (event.key === 'ArrowLeft') { event.preventDefault(); go(-1); }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [go, onDone]);

  if (!box || typeof document === 'undefined') return null;
  const step = steps[index];
  const pad = 8;
  const margin = 16;
  const below = box.bottom + pad + 12 + cardSize.height < window.innerHeight - margin;
  const above = box.top - pad - 12 - cardSize.height > margin;
  let top = below ? box.bottom + pad + 12 : above ? box.top - pad - 12 - cardSize.height : Math.max(margin, Math.min(window.innerHeight - cardSize.height - margin, box.top));
  let left = box.left + box.width / 2 - cardSize.width / 2;
  if (!below && !above) {
    // Beside a tall target: whichever side has room.
    left = box.right + pad + 12 + cardSize.width < window.innerWidth - margin ? box.right + pad + 12 : box.left - pad - 12 - cardSize.width;
  }
  left = Math.max(margin, Math.min(window.innerWidth - cardSize.width - margin, left));
  top = Math.max(margin, top);
  const last = position === visible.length - 1;

  return createPortal(
    <div className="ot-tour" role="presentation">
      <div className="ot-tour-scrim" onClick={onDone} aria-hidden="true" />
      <div
        className="ot-tour-spot"
        aria-hidden="true"
        style={{ top: box.top - pad, left: box.left - pad, width: box.width + pad * 2, height: box.height + pad * 2 }}
      />
      <div
        ref={card}
        className="ot-tour-card"
        role="dialog"
        aria-modal="false"
        aria-labelledby="ot-tour-title"
        aria-describedby="ot-tour-body"
        tabIndex={-1}
        style={{ top, left }}
      >
        <h3 id="ot-tour-title">{step.title}</h3>
        <p id="ot-tour-body">{step.body}</p>
        <div className="ot-tour-foot">
          <button type="button" className="ot-tool" onClick={onDone}>Skip tour</button>
          <span className="ot-tour-count ot-time" aria-label={`Step ${position + 1} of ${visible.length}`}>{position + 1} of {visible.length}</span>
          {position > 0 && (
            <button type="button" className="ot-button ot-button-secondary ot-button-small" onClick={() => go(-1)}><ArrowLeft aria-hidden="true" />Back</button>
          )}
          {last ? (
            <>
              <button type="button" className="ot-button ot-button-secondary ot-button-small" onClick={onOpenGuide}><BookOpen aria-hidden="true" />Open guide</button>
              <button type="button" className="ot-button ot-button-primary ot-button-small" onClick={onDone}>Done</button>
            </>
          ) : (
            <button type="button" className="ot-button ot-button-primary ot-button-small" onClick={() => go(1)}>Next<ArrowRight aria-hidden="true" /></button>
          )}
        </div>
      </div>
    </div>,
    document.body,
  );
}
