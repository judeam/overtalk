'use client';

import { ClipboardList, FileText, MoreHorizontal, Play, Quote } from 'lucide-react';
import { Menu, MenuItem } from '@/components/shell/Menu';
import { clock, shortDate, speakerSummary, type TimelineSpeaker } from '@/lib/session';

export interface InspectorItem {
  id: string;
  heading: string;
  time: number;
  quote: string;
  person: { label: string; name: string; tone: string | null } | null;
  interpretation?: boolean;
  evidence?: { text: string; time: number }[];
  taskText?: string;
}

interface InspectorProps {
  item: InspectorItem | null;
  meetingTitle: string;
  createdAt?: string;
  speakers: TimelineSpeaker[];
  canPlay: boolean;
  onPlayFrom: (seconds: number) => void;
  onCopy: (item: InspectorItem) => void;
  onCopyWithContext: (item: InspectorItem) => void;
  onOpenNotes: () => void;
  onCreateTask?: (item: InspectorItem) => void;
}

export function Inspector({ item, meetingTitle, createdAt, speakers, canPlay, ...actions }: InspectorProps) {
  return (
    <aside className="ot-panel ot-inspector" aria-label="Selected moment" aria-live="polite" data-tour="inspector" data-kind={item?.taskText ? "action" : item?.interpretation ? "decision" : "transcript"}>
      {item ? (
        <>
          <div className="ot-inspector-head">
            <h3 className="ot-inspector-title">
              <span>{item.heading}</span>
              <span className="ot-dot-sep" aria-hidden="true" />
              <span className="ot-time">{clock(item.time)}</span>
            </h3>
            <Menu trigger={<button type="button" className="ot-icon-button ot-inspector-more" aria-label="More for this moment"><MoreHorizontal /></button>}>
              <MenuItem icon={<Quote />} onSelect={() => actions.onCopyWithContext(item)}>Copy with time and meeting</MenuItem>
              <MenuItem icon={<FileText />} onSelect={actions.onOpenNotes}>Open the notes</MenuItem>
            </Menu>
          </div>
          {item.interpretation ? <><p className="ot-interpretation-label">From the notes · review the source</p><p key={item.id} className="ot-quote">{item.quote}</p></> : <blockquote key={item.id} className="ot-quote">“{item.quote}”</blockquote>}
          {item.evidence?.map((source, i) => <div className="ot-evidence" key={i}><button type="button" className="ot-link" disabled={!canPlay} onClick={() => actions.onPlayFrom(source.time)}>Source at {clock(source.time)}</button><blockquote>“{source.text}”</blockquote></div>)}
          {item.person && (
            <div className="ot-owner">
              <span className="ot-owner-label">{item.person.label}</span>
              <span className={`ot-line-dot${item.person.tone ? ` ot-tone-${item.person.tone}` : ''}`} aria-hidden="true" style={item.person.tone ? undefined : { background: 'var(--ot-line-strong)' }} />
              <span className="ot-owner-name">{item.person.name}</span>
            </div>
          )}
          <hr className="ot-rule" />
          <div className="ot-actions">
            <button type="button" className="ot-button ot-button-primary" data-tour="play-from" disabled={!canPlay} onClick={() => actions.onPlayFrom(item.time)}>
              <Play fill="currentColor" strokeWidth={0} />
              <span>Play from <span className="ot-time">{clock(item.time)}</span></span>
            </button>
            <button type="button" className="ot-button ot-button-secondary" onClick={() => actions.onCopy(item)}>
              <ClipboardList />
              <span>Copy</span>
            </button>
          </div>
          {item.taskText && actions.onCreateTask && <button type="button" className="ot-button ot-button-secondary ot-promote-action" onClick={() => actions.onCreateTask?.(item)}>Review as an action</button>}
          <hr className="ot-rule" />
        </>
      ) : (
        <div className="ot-inspector-empty">
          <strong>Nothing selected</strong>
          Select a clip on the timeline or a line in the transcript to read it here and play from that moment.
        </div>
      )}
      <h4 className="ot-context-title">Context</h4>
      <dl className="ot-context">
        <dt>In the meeting</dt>
        <dd>{meetingTitle}</dd>
        {createdAt && (<><dt>Date</dt><dd>{shortDate(createdAt)}</dd></>)}
        <dt>Speakers</dt>
        <dd>{speakerSummary(speakers)}</dd>
      </dl>
    </aside>
  );
}
