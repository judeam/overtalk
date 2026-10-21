'use client';

import { Suspense, useEffect, useState, useRef, type ReactNode } from 'react';
import { usePathname, useSearchParams } from 'next/navigation';
import { AppHeader } from './AppHeader';
import { MeetingRail } from './MeetingRail';
import { GuideProvider } from '@/components/guide/GuideProvider';

/** Header across the top, meeting list on the left, the current surface beside it. */
export function AppShell({ children }: { children: ReactNode }) {
  const [railOpen, setRailOpen] = useState(false);
  const [compact, setCompact] = useState(false);
  const rail = useRef<HTMLDivElement>(null);
  useEffect(() => {
    try { document.documentElement.dataset.readingSize = JSON.parse(localStorage.getItem('overtalk.reading-size') || '"comfortable"'); } catch { /* default text size */ }
    const media = window.matchMedia('(max-width: 1023px)');
    const update = () => setCompact(media.matches);
    update(); media.addEventListener('change', update);
    const show = () => setRailOpen(true);
    window.addEventListener('overtalk:show-meetings', show);
    return () => { media.removeEventListener('change', update); window.removeEventListener('overtalk:show-meetings', show); };
  }, []);
  useEffect(() => { rail.current?.toggleAttribute('inert', compact && !railOpen); }, [compact, railOpen]);
  useEffect(() => {
    if (!compact || !railOpen) return;
    const previous = document.activeElement as HTMLElement;
    const focusable = () => Array.from(rail.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input, select, a[href]') ?? []).filter(el => el.getClientRects().length);
    focusable()[0]?.focus();
    const trap = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const items = focusable(); const first = items[0], last = items[items.length-1];
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
    };
    window.addEventListener('keydown', trap);
    return () => { window.removeEventListener('keydown', trap); previous?.focus(); };
  }, [compact, railOpen]);
  useEffect(() => {
    if (!railOpen) return;
    const onKey = (event: KeyboardEvent) => { if (event.key === 'Escape') setRailOpen(false); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [railOpen]);

  return (
    <GuideProvider>
    <div className="ot-app" data-rail-open={railOpen ? 'true' : undefined}>
      <AppHeader railOpen={railOpen} onToggleRail={() => setRailOpen((open) => !open)} />
      <Suspense fallback={<nav className="ot-rail" aria-label="Meetings" />}>
        <div className="ot-rail-container" ref={rail} role={compact && railOpen ? 'dialog' : undefined} aria-modal={compact && railOpen ? true : undefined} aria-label={compact && railOpen ? 'Your meetings' : undefined}>
          {compact && <button type="button" className="ot-rail-close ot-tool" onClick={() => setRailOpen(false)}>Close meetings</button>}
          <MeetingRail />
        </div>
        <CloseOnNavigate onNavigate={() => setRailOpen(false)} />
      </Suspense>
      <div className="ot-scrim" aria-hidden="true" onClick={() => setRailOpen(false)} />
      <main className="ot-main">{children}</main>
    </div>
    </GuideProvider>
  );
}

/** In narrow windows the list slides over the session; it closes once a meeting opens. */
function CloseOnNavigate({ onNavigate }: { onNavigate: () => void }) {
  const pathname = usePathname();
  const searchParams = useSearchParams();
  useEffect(() => { onNavigate(); }, [pathname, searchParams]); // eslint-disable-line react-hooks/exhaustive-deps
  return null;
}
