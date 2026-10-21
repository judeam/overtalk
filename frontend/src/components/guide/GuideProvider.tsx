'use client';

import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from 'react';
import { usePathname } from 'next/navigation';
import { Guide } from './Guide';
import { Tour } from './Tour';
import { TOURS, type TourId } from './lessons';

interface GuideApi {
  open: (chapter?: string) => void;
  close: () => void;
  startTour: (tour: TourId) => void;
  isOpen: boolean;
}

const GuideContext = createContext<GuideApi | null>(null);

export const useGuide = () => {
  const api = useContext(GuideContext);
  if (!api) throw new Error('useGuide must be used inside GuideProvider');
  return api;
};

/** Set by setup so the first launch afterwards opens with the tour. */
export const PENDING_TOUR_KEY = 'overtalk.tour.pending';
const SEEN_KEY = 'overtalk.tour.seen';

const read = (key: string) => { try { return localStorage.getItem(key); } catch { return null; } };
const write = (key: string, value: string | null) => {
  try { if (value === null) localStorage.removeItem(key); else localStorage.setItem(key, value); } catch { /* per-device convenience only */ }
};
const tourSeen = (tour: TourId) => (read(SEEN_KEY) ?? '').split(',').includes(tour);
const markTourSeen = (tour: TourId) => {
  const seen = new Set((read(SEEN_KEY) ?? '').split(',').filter(Boolean));
  seen.add(tour);
  write(SEEN_KEY, [...seen].join(','));
};

export function GuideProvider({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [chapter, setChapter] = useState<string | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [tour, setTour] = useState<TourId | null>(null);
  const openRef = useRef(false);
  openRef.current = isOpen;

  const open = useCallback((next?: string) => { setChapter(next ?? null); setIsOpen(true); }, []);
  const close = useCallback(() => setIsOpen(false), []);
  const startTour = useCallback((next: TourId) => { setIsOpen(false); setTour(next); }, []);

  // F1, or ? outside a text field, opens the guide.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const typing = target?.closest('input, textarea, select, [contenteditable="true"]');
      if (event.key === 'F1' || (event.key === '?' && !typing)) {
        event.preventDefault();
        setIsOpen((now) => !now);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  // The first launch after setup starts with a short tour of the recorder.
  useEffect(() => {
    if (pathname !== '/' || read(PENDING_TOUR_KEY) !== 'home') return;
    write(PENDING_TOUR_KEY, null);
    const timer = window.setTimeout(() => setTour('home'), 900);
    return () => window.clearTimeout(timer);
  }, [pathname]);

  // The first saved meeting someone opens gets a five-step tour of the session, once.
  useEffect(() => {
    if (!pathname?.startsWith('/meeting-details') || tourSeen('review')) return;
    const timer = window.setTimeout(() => {
      // Never interrupt someone who already opened the guide; the tour waits for the next visit.
      if (!openRef.current && document.querySelector('[data-tour="lanes"] .ot-clip')) setTour('review');
    }, 1400);
    return () => window.clearTimeout(timer);
  }, [pathname]);

  return (
    <GuideContext.Provider value={{ open, close, startTour, isOpen }}>
      {children}
      {isOpen && <Guide initialChapter={chapter} onClose={close} startTour={startTour} />}
      {tour && (
        <Tour
          steps={TOURS[tour]}
          onDone={() => { markTourSeen(tour); setTour(null); }}
          onOpenGuide={() => { markTourSeen(tour); setTour(null); open(); }}
        />
      )}
    </GuideContext.Provider>
  );
}
