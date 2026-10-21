// Session view data: speaker lanes, note clips derived from the summary, and time formatting.

export interface TimelineSpeaker { id: string; name: string }
export interface TimelineTurn { speaker_id: string; start: number; end: number }
export interface MeetingTimeline {
  duration: number;
  speakers: TimelineSpeaker[];
  turns: TimelineTurn[];
  /** Audio interval of each transcript line, in the order summaries cite them as [S1], [S2], … */
  sources: ([number, number] | null)[];
}
export interface PatternBin { speaker: number; level: number }
export interface MeetingOverview {
  id: string;
  title: string;
  created_at: string;
  duration: number;
  speakers: TimelineSpeaker[];
  pattern: PatternBin[];
}

/** Fixed speaker tones, assigned in order of first appearance. Amber and violet stay with notes. */
const SPEAKER_TONES = ['teal', 'magenta', 'slate', 'moss', 'clay', 'plum'] as const;
export const toneOf = (index: number) => SPEAKER_TONES[((index % SPEAKER_TONES.length) + SPEAKER_TONES.length) % SPEAKER_TONES.length];

export interface SpeechClip { id: string; speakerId: string; start: number; end: number }

/** Merge one speaker's turns into clips, bridging pauses shorter than `gap` seconds. */
export function speechClips(turns: TimelineTurn[], gap = 1.5): SpeechClip[] {
  const bySpeaker = new Map<string, TimelineTurn[]>();
  for (const turn of turns) {
    if (!(turn.end > turn.start)) continue;
    const list = bySpeaker.get(turn.speaker_id) ?? [];
    list.push(turn);
    bySpeaker.set(turn.speaker_id, list);
  }
  const clips: SpeechClip[] = [];
  bySpeaker.forEach((list, speakerId) => {
    list.sort((a, b) => a.start - b.start);
    let current: SpeechClip | null = null;
    for (const turn of list) {
      if (current && turn.start - current.end <= gap) {
        current.end = Math.max(current.end, turn.end);
      } else {
        if (current) clips.push(current);
        current = { id: `${speakerId}@${turn.start}`, speakerId, start: turn.start, end: turn.end };
      }
    }
    if (current) clips.push(current);
  });
  return clips;
}

type NoteKind = 'decision' | 'action';

export interface NoteClip {
  id: string;
  kind: NoteKind;
  /** The whole item as written in the notes, without markup or source markers. */
  text: string;
  /** Short form for the clip face. */
  label: string;
  owner: string | null;
  sourceIds: number[];
  start: number;
  end: number;
  /** When it was said: the start of the last cited line, or the written timestamp. */
  moment: number;
}

const DECISION_HEADING = /decision|decided|agreement|beschluss|entscheid|decis[aã]o|decis[oõ]es|acordad|d[ée]cision/i;
const ACTION_HEADING = /action|to-?dos?|next steps?|follow[- ]?ups?|tasks?|aufgabe|ma[sß]nahme|tarefa|a[cç][aã]o|a[cç][oõ]es|pr[oó]ximos passos/i;
const LIST_ITEM = /^(\s{0,3})(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?(.+)$/;
const SOURCE = /\[S(\d+)\]/g;
const STAMP = /[[(]\s*(\d{1,2}):(\d{2})(?::(\d{2}))?\s*[\])]/;

function plain(text: string) {
  return text
    .replace(/\[S\d+\]\([^)]*\)/g, '')
    .replace(SOURCE, '')
    .replace(STAMP, '')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/(\*\*|__|`)/g, '')
    .replace(/(^|\s)[*_](\S[^*_]*\S|\S)[*_](?=\s|$|[.,;:!?])/g, '$1$2')
    .replace(/\s+([.,;:!?])/g, '$1')
    .replace(/\s{2,}/g, ' ')
    .trim();
}

/** "Priya: PDF tables", "Owner: Priya", "(owner: Priya)", "… — Priya" */
function splitOwner(text: string): { owner: string | null; task: string } {
  const tagged = text.match(/\(?\b(?:owner|assignee|verantwortlich|respons[aá]vel)\s*:\s*([^,;)]+)\)?/i);
  if (tagged) {
    return { owner: tagged[1].trim(), task: text.replace(tagged[0], '').replace(/\s*[,;–—-]\s*$/, '').trim() };
  }
  const lead = text.match(/^([A-ZÀ-Ý][\p{L}'’-]+(?:\s+[A-ZÀ-Ý][\p{L}'’-]+){0,2})\s*:\s+(.+)$/u);
  if (lead) return { owner: lead[1], task: lead[2] };
  const trail = text.match(/^(.+?)\s+[–—]\s+([A-ZÀ-Ý][\p{L}'’-]+(?:\s+[A-ZÀ-Ý][\p{L}'’-]+)?)\.?$/u);
  if (trail) return { owner: trail[2], task: trail[1] };
  return { owner: null, task: text };
}

function firstClause(text: string) {
  const clause = text.split(/[,;:(]|\s[–—]\s|\.\s/)[0].replace(/\.$/, '').trim();
  return clause.length >= 8 ? clause : text.replace(/\.$/, '');
}

/**
 * Decisions and actions from the notes, placed at the moments they cite.
 * Items with neither a source reference nor a written timestamp have no moment and are left out.
 */
export function noteClips(markdown: string, sources: MeetingTimeline['sources']): NoteClip[] {
  const clips: NoteClip[] = [];
  let section: NoteKind | null = null;
  markdown.split('\n').forEach((line, lineIndex) => {
    const heading = line.match(/^\s{0,3}#{1,6}\s+(.+?)\s*#*\s*$/) ?? line.match(/^\s*\*\*([^*]+)\*\*\s*:?\s*$/);
    if (heading) {
      section = DECISION_HEADING.test(heading[1]) ? 'decision' : ACTION_HEADING.test(heading[1]) ? 'action' : null;
      return;
    }
    if (!section) return;
    const item = line.match(LIST_ITEM);
    if (!item || item[1].length > 1) return;
    const raw = item[2];
    const sourceIds = Array.from(raw.matchAll(SOURCE), (m) => Number(m[1])).filter((n) => n >= 1);
    const spans = sourceIds
      .map((n) => sources[n - 1])
      .filter((span): span is [number, number] => Array.isArray(span));
    let start: number;
    let end: number;
    let moment: number;
    if (spans.length) {
      start = Math.min(...spans.map((s) => s[0]));
      end = Math.max(...spans.map((s) => s[1]));
      moment = spans.reduce((latest, s) => (s[0] > latest ? s[0] : latest), spans[0][0]);
    } else {
      const stamp = raw.match(STAMP);
      if (!stamp) return;
      const [h, m, s] = stamp[3] != null ? [Number(stamp[1]), Number(stamp[2]), Number(stamp[3])] : [0, Number(stamp[1]), Number(stamp[2])];
      start = moment = h * 3600 + m * 60 + s;
      end = start;
    }
    const text = plain(raw);
    if (!text) return;
    const { owner, task } = section === 'action' ? splitOwner(text) : { owner: null, task: text };
    const short = firstClause(task);
    clips.push({
      id: `${section}-${lineIndex}`,
      kind: section,
      text,
      label: owner ? `${owner}: ${short}` : short,
      owner,
      sourceIds,
      start,
      end,
      moment,
    });
  });
  return clips;
}

const pad = (n: number) => String(n).padStart(2, '0');

/** 00:03:09 — the transport clock. */
export function timecode(seconds: number) {
  const s = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}`;
}

/** 03:09, or 1:03:09 past the hour — lines, clips and durations. */
export function clock(seconds: number) {
  const s = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  const h = Math.floor(s / 3600);
  return h ? `${h}:${pad(Math.floor((s % 3600) / 60))}:${pad(s % 60)}` : `${pad(Math.floor(s / 60))}:${pad(s % 60)}`;
}

/** 12 Oct, or 12 Oct 2025 outside the current year. */
export function shortDate(iso: string, now = new Date()) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return '';
  const sameYear = date.getFullYear() === now.getFullYear();
  return date.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', ...(sameYear ? {} : { year: 'numeric' }) });
}

/** Ruler steps: major labelled ticks and the minor ticks between them. */
export function rulerSteps(duration: number, pixelsPerSecond: number) {
  const candidates = [5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600];
  const major = candidates.find((step) => step * pixelsPerSecond >= 110) ?? 3600;
  const minor = major / 2;
  const ticks: { at: number; major: boolean }[] = [];
  for (let at = 0; at <= duration + 0.001; at += minor) ticks.push({ at, major: Math.round(at / minor) % 2 === 0 });
  return { major, minor, ticks };
}

export function speakerSummary(speakers: TimelineSpeaker[]) {
  if (!speakers.length) return 'No speakers yet';
  const names = speakers.map((s) => s.name);
  const shown = names.length > 4 ? `${names.slice(0, 3).join(', ')} and ${names.length - 3} more` : names.join(', ');
  return `${speakers.length} speaker${speakers.length === 1 ? '' : 's'}: ${shown}`;
}

/** "Meeting on 14 Oct, 17:20": the title a recording gets until someone names it. */
export function defaultMeetingTitle(now = new Date()) {
  const day = now.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
  const time = now.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit' });
  return `Meeting on ${day}, ${time}`;
}

/** True for titles nobody chose: empty, generated, or the old underscore stamp. */
export function isDefaultMeetingTitle(title: string) {
  const t = title.trim();
  return !t || /^Meeting on \d{1,2} \S+, \d{2}:\d{2}$/.test(t) || /^Meeting \d{2}_\d{2}_\d{2}_/.test(t) || t === '+ New Call';
}
