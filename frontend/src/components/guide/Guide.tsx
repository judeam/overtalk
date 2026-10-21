import { useEffect, useMemo, useRef, useState } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { ArrowLeft, ArrowRight, BookOpen, Check, ExternalLink, Keyboard, Lightbulb, Play, Search, X } from 'lucide-react';
import { GROUPS, LESSONS, SHORTCUTS, type GuideAction, type GuideLesson, type TourId } from './lessons';
import markerData from './markers.json';

type Box = { x: number; y: number; w: number; h: number };
const MARKERS = markerData as Record<string, Record<string, Box>>;
const READ_KEY = 'overtalk.guide.read';

function useReadChapters() {
  const [read, setRead] = useState<string[]>([]);
  useEffect(() => {
    try {
      const value: unknown = JSON.parse(localStorage.getItem(READ_KEY) ?? '[]');
      setRead(Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : []);
    } catch { setRead([]); }
  }, []);
  const mark = (id: string) => setRead((now) => {
    const next = [...new Set([...now, id])];
    try { localStorage.setItem(READ_KEY, JSON.stringify(next)); } catch { /* progress is a per-device convenience */ }
    return next;
  });
  return { read, mark };
}

/** The real screen, with a numbered marker on each control the steps mention. */
function Shot({ lesson, active, onPick }: { lesson: GuideLesson; active: number; onPick: (i: number) => void }) {
  const boxes = MARKERS[lesson.shot] ?? {};
  return (
    <figure className="ot-guide-shot">
      <img src={`/guide/${lesson.shot}.webp`} alt={`The ${lesson.title.toLowerCase()} screen`} loading="lazy" />
      {lesson.steps.map((step, i) => {
        const box = step.target ? boxes[step.target] : undefined;
        if (!box) return null;
        const style = { left: `${box.x * 100}%`, top: `${box.y * 100}%`, width: `${box.w * 100}%`, height: `${box.h * 100}%` };
        return (
          <button
            key={step.title}
            type="button"
            className="ot-guide-mark"
            data-active={i === active ? 'true' : undefined}
            style={style}
            aria-label={`Step ${i + 1}: ${step.title}`}
            onClick={() => onPick(i)}
          >
            <span>{i + 1}</span>
          </button>
        );
      })}
    </figure>
  );
}

function Chapter({ lesson, read, onRead, onSelect, onAction }: {
  lesson: GuideLesson;
  read: boolean;
  onRead: () => void;
  onSelect: (id: string | null) => void;
  onAction: (action: GuideAction) => void;
}) {
  const [active, setActive] = useState(0);
  useEffect(() => setActive(0), [lesson.id]);
  const next = LESSONS[LESSONS.indexOf(lesson) + 1];
  return (
    <article className="ot-guide-chapter" key={lesson.id}>
      <button type="button" className="ot-guide-back" onClick={() => onSelect(null)}><ArrowLeft aria-hidden="true" />All chapters</button>
      <h1>{lesson.title}</h1>
      <p className="ot-guide-summary">{lesson.summary}</p>
      <Shot lesson={lesson} active={active} onPick={setActive} />
      <ol className="ot-guide-steps">
        {lesson.steps.map((step, i) => (
          <li key={step.title}>
            <button type="button" aria-current={i === active ? 'step' : undefined} onClick={() => setActive(i)}>
              <span className="ot-guide-num ot-time">{i + 1}</span>
              <span><strong>{step.title}</strong><span>{step.body}</span></span>
            </button>
          </li>
        ))}
      </ol>
      {lesson.keys && (
        <ul className="ot-guide-keys">
          {lesson.keys.map((key) => <li key={key}><Keyboard aria-hidden="true" />{key}</li>)}
        </ul>
      )}
      {lesson.tips?.map((tip) => <p key={tip} className="ot-guide-tip"><Lightbulb aria-hidden="true" />{tip}</p>)}
      <div className="ot-guide-actions">
        {lesson.actions?.map((action) => (
          <button key={action.label} type="button" className="ot-button ot-button-primary ot-button-small" onClick={() => onAction(action)}>
            {action.kind === 'tour' ? <Play aria-hidden="true" /> : null}{action.label}
          </button>
        ))}
        <button type="button" className="ot-button ot-button-secondary ot-button-small" onClick={onRead} disabled={read}>
          <Check aria-hidden="true" />{read ? 'Read' : 'Mark as read'}
        </button>
      </div>
      {next && (
        <button type="button" className="ot-guide-next" onClick={() => { onRead(); onSelect(next.id); }}>
          <span>Up next</span><strong>{next.title}</strong><ArrowRight aria-hidden="true" />
        </button>
      )}
    </article>
  );
}

export function Guide({ initialChapter = null, onClose, startTour, standalone = false }: { initialChapter?: string | null; onClose: () => void; startTour: (tour: TourId) => void; standalone?: boolean }) {
  const router = useRouter();
  const pathname = usePathname();
  const [selected, setSelected] = useState<string | null>(initialChapter);
  const [query, setQuery] = useState('');
  const { read, mark } = useReadChapters();
  const panel = useRef<HTMLElement>(null);
  const body = useRef<HTMLDivElement>(null);
  useEffect(() => { setSelected(initialChapter); }, [initialChapter]);
  useEffect(() => { if (body.current) body.current.scrollTop = 0; }, [selected]);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    if (!standalone) panel.current?.focus();
    return () => { if (!standalone) previous?.focus?.(); };
  }, [standalone]);

  const lesson = LESSONS.find((l) => l.id === selected);
  const results = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return LESSONS;
    return LESSONS.filter((l) => [l.title, l.summary, l.group, ...l.steps.map((s) => `${s.title} ${s.body}`), ...(l.keys ?? [])].join(' ').toLowerCase().includes(q));
  }, [query]);
  const tourHere = pathname?.startsWith('/meeting-details') ? 'review' : 'home';

  const act = (action: GuideAction) => {
    if (action.kind === 'tour') { if (standalone) router.push('/'); startTour(action.tour); return; }
    if (action.kind === 'go') { onClose(); router.push(action.href); return; }
    window.dispatchEvent(new CustomEvent(action.event));
  };

  const list = (
    <nav className="ot-guide-list" aria-label="Guide chapters">
      {(query ? ['Results'] : GROUPS).map((group) => {
        const rows = query ? results : results.filter((l) => l.group === group);
        if (!rows.length) return null;
        return (
          <section key={group}>
            <h3>{group === 'Privacy' ? 'Privacy and settings' : group}</h3>
            <ul>
              {rows.map((l) => (
                <li key={l.id}>
                  <button type="button" aria-current={selected === l.id ? 'page' : undefined} onClick={() => { setSelected(l.id); setQuery(''); }}>
                    <span className="ot-guide-row-text"><strong>{l.title}</strong><span>{l.summary}</span></span>
                    {read.includes(l.id) ? <Check className="ot-guide-read" aria-label="Read" /> : <ArrowRight aria-hidden="true" />}
                  </button>
                </li>
              ))}
            </ul>
          </section>
        );
      })}
      {query && !results.length && <p className="ot-note-text">No chapter mentions “{query}”. Try a word like record, speaker or export.</p>}
    </nav>
  );

  const overview = (
    <div className="ot-guide-overview">
      <h1>Learn Overtalk</h1>
      <p className="ot-guide-summary">Short chapters, each on the real screen. Read one when you need it, or take the tour.</p>
      <div className="ot-guide-actions">
        <button type="button" className="ot-button ot-button-primary ot-button-small" onClick={() => act({ kind: 'tour', tour: tourHere, label: '' })}>
          <Play aria-hidden="true" />Take the tour
        </button>
        <span className="ot-note-text ot-time">{read.filter((id) => LESSONS.some((l) => l.id === id)).length} of {LESSONS.length} chapters read</span>
      </div>
      {!standalone && list}
      <section className="ot-guide-shortcuts" aria-labelledby="ot-guide-keys-title">
        <h3 id="ot-guide-keys-title">Keyboard</h3>
        <dl>
          {SHORTCUTS.map(([key, what]) => (<div key={key}><dt><kbd>{key}</kbd></dt><dd>{what}</dd></div>))}
        </dl>
      </section>
    </div>
  );

  return (
    <aside
      ref={panel}
      className={`ot-guide${standalone ? ' ot-guide-page' : ''}`}
      aria-label="Overtalk guide"
      tabIndex={-1}
      onKeyDown={(e) => { if (e.key === 'Escape' && !standalone) { e.stopPropagation(); onClose(); } }}
    >
      <header className="ot-guide-head">
        <h2><BookOpen aria-hidden="true" />Guide</h2>
        <label className="ot-rail-search ot-guide-search">
          <Search aria-hidden="true" />
          <input type="search" placeholder="Find a feature" aria-label="Search the guide" value={query}
            onChange={(e) => { setQuery(e.target.value); setSelected(null); }} />
        </label>
        {!standalone && (
          <button type="button" className="ot-icon-button" aria-label="Open the guide as a page" title="Open as a page"
            onClick={() => { onClose(); router.push(`/guide${selected ? `?chapter=${selected}` : ''}`); }}>
            <ExternalLink />
          </button>
        )}
        <button type="button" className="ot-icon-button" aria-label="Close the guide" title="Close (Esc)" onClick={onClose}><X /></button>
      </header>
      <div className="ot-guide-body">
        {standalone && list}
        <div className="ot-guide-content" ref={body}>
          {lesson && !query ? (
            <Chapter lesson={lesson} read={read.includes(lesson.id)} onRead={() => mark(lesson.id)} onSelect={setSelected} onAction={act} />
          ) : query ? (!standalone ? list : overview) : overview}
        </div>
      </div>
    </aside>
  );
}
