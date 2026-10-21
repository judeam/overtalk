"use client";

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { MeetingSpeaker } from '@/types';

interface SpeakerManagerProps {
  meetingId: string;
  onChange?: () => Promise<void>;
  /** Lane colour per speaker id, so each name carries the colour of its lane. */
  tones?: Record<string, string>;
}

export function SpeakerManager({ meetingId, onChange, tones = {} }: SpeakerManagerProps) {
  const [speakers, setSpeakers] = useState<MeetingSpeaker[]>([]);
  const [editing, setEditing] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [targetId, setTargetId] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(async () => {
    const rows = await invoke<MeetingSpeaker[]>('list_meeting_speakers', { meetingId });
    setSpeakers(rows);
  }, [meetingId]);

  useEffect(() => {
    let active = true;
    setSpeakers([]);
    setEditing(null);
    setError(null);
    void invoke<MeetingSpeaker[]>('list_meeting_speakers', { meetingId })
      .then(rows => { if (active) setSpeakers(rows); })
      .catch(reason => { if (active) setError(String(reason)); });
    const unlisten = listen<string>('meeting-speakers-updated', event => {
      if (active && event.payload === meetingId) {
        void reload().catch(reason => { if (active) setError(String(reason)); });
      }
    });
    return () => { active = false; void unlisten.then(stop => stop()).catch(() => {}); };
  }, [meetingId, reload]);

  async function save(merge: boolean) {
    if (!editing || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (merge) {
        await invoke('merge_meeting_speakers', { meetingId, sourceId: editing, targetId });
        toast.success('Speakers merged', { description: 'The previous transcript and speaker names are recoverable.', action: { label: 'Undo', onClick: () => { void invoke('speech_restore_transcript', { meetingId }).then(reload).catch(reason => toast.error('Could not undo merge', { description: String(reason) })); } } });
      } else {
        await invoke('rename_meeting_speaker', { meetingId, speakerId: editing, name });
      }
      await reload();
      await onChange?.();
      setEditing(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="ot-section" data-tour="speaker-list" aria-labelledby={`speakers-${meetingId}`}>
      <h4 className="ot-section-title" id={`speakers-${meetingId}`}>Speakers</h4>
      {speakers.length === 0 ? (
        <p className="ot-note-text">No speakers were identified in this meeting yet. Improve the transcript below to find them.</p>
      ) : (
        <ul className="ot-speaker-list">
          {speakers.map(speaker => (
            <li key={speaker.id} className={`ot-speaker-item${tones[speaker.id] ? ` ot-tone-${tones[speaker.id]}` : ''}`}>
              {editing === speaker.id ? (
                <div className="ot-speaker-edit">
                  <form className="ot-inline-form" onSubmit={event => { event.preventDefault(); void save(false); }}>
                    <label className="sr-only" htmlFor={`speaker-name-${speaker.id}`}>Name for {speaker.name}</label>
                    <input id={`speaker-name-${speaker.id}`} className="ot-input" value={name} autoFocus maxLength={120}
                      disabled={busy} onChange={event => setName(event.target.value)} />
                    <button type="submit" className="ot-button ot-button-primary ot-button-small" disabled={busy || !name.trim()}>Save name</button>
                    <button type="button" className="ot-button ot-button-secondary ot-button-small" disabled={busy} onClick={() => setEditing(null)}>Cancel</button>
                  </form>
                  {speakers.length > 1 && (
                    <div className="ot-inline-form">
                      <label className="sr-only" htmlFor={`merge-${speaker.id}`}>Merge {speaker.name} into</label>
                      <select id={`merge-${speaker.id}`} className="ot-input" value={targetId} disabled={busy} onChange={event => setTargetId(event.target.value)}>
                        <option value="">Same person as…</option>
                        {speakers.filter(other => other.id !== speaker.id).map(other => (
                          <option key={other.id} value={other.id}>{other.name}</option>
                        ))}
                      </select>
                      <button type="button" className="ot-button ot-button-secondary ot-button-small" disabled={busy || !targetId} onClick={() => void save(true)}>Merge</button>
                    </div>
                  )}
                  <p className="ot-note-text">Names apply to this meeting only. Merging moves every turn into the chosen speaker.</p>
                </div>
              ) : (
                <>
                  <span className="ot-line-dot" aria-hidden="true" style={tones[speaker.id] ? undefined : { background: 'var(--ot-line-strong)' }} />
                  <span className="ot-speaker-name">{speaker.name}</span>
                  <span className="ot-speaker-count ot-time">{speaker.segment_count} {speaker.segment_count === 1 ? 'line' : 'lines'}</span>
                  <button type="button" className="ot-tool" disabled={busy}
                    onClick={() => { setEditing(speaker.id); setName(speaker.name); setTargetId(''); setError(null); }}>
                    Rename or merge
                  </button>
                </>
              )}
            </li>
          ))}
        </ul>
      )}
      {error && <p role="alert" className="ot-danger-text">{error}</p>}
    </section>
  );
}
