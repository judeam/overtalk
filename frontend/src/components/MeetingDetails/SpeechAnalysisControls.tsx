'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';

interface Result { segments: number; warnings?: string[] }

export function SpeechAnalysisControls({ meetingId, hasRecording, onComplete }: {
  meetingId: string; hasRecording: boolean; onComplete?: () => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [diarize, setDiarize] = useState(true);
  const [count, setCount] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [backupDate, setBackupDate] = useState<string | null>(null);
  const job = useRef<string | null>(null);
  const generation = useRef(0);
  useEffect(() => {
    const owner = ++generation.current;
    setBusy(false); setError(null); setWarnings([]); setBackupDate(null);
    void invoke<string | null>('speech_backup_status', { meetingId }).then(date => {
      if (generation.current === owner) setBackupDate(date);
    }).catch(() => {});
    return () => {
      generation.current++;
      const oldJob = job.current;
      job.current = null;
      if (oldJob) void invoke('speech_cancel', { jobId: oldJob }).catch(() => {});
    };
  }, [meetingId]);
  const analyze = async () => {
    const owner = generation.current;
    const jobId = crypto.randomUUID();
    job.current = jobId; setBusy(true); setError(null); setWarnings([]);
    try {
      const result = await invoke<Result>('speech_analyze_meeting', { meetingId, jobId, diarize, numSpeakers: diarize && count ? Number(count) : null });
      if (generation.current !== owner) return;
      await onComplete?.();
      if (generation.current !== owner) return;
      setWarnings(result.warnings ?? []);
      setBackupDate(new Date().toISOString());
      toast.success(`Updated ${result.segments} transcript lines`);
    } catch (reason) { if (generation.current === owner) setError(String(reason)); }
    finally { if (generation.current === owner) { setBusy(false); job.current = null; } }
  };
  const restore = async () => {
    const owner = generation.current;
    setBusy(true); setError(null); setWarnings([]);
    try {
      await invoke('speech_restore_transcript', { meetingId });
      if (generation.current !== owner) return;
      await onComplete?.();
      if (generation.current === owner) toast.success('Previous transcript and speaker names restored');
    } catch (reason) { if (generation.current === owner) setError(String(reason)); }
    finally { if (generation.current === owner) setBusy(false); }
  };
  return (
    <section className="ot-section" data-tour="improve" aria-labelledby={`improve-${meetingId}`}>
      <h4 className="ot-section-title" id={`improve-${meetingId}`}>Improve the transcript</h4>
      <p className="ot-note-text">
        Runs speech recognition and speaker detection again on this computer, using your local speech settings. A recovery copy is saved first, so you can go back. New speaker labels need naming again, and notes should be generated again afterwards.
      </p>
      <div className="ot-inline-form">
        <label className="ot-check">
          <input type="checkbox" checked={diarize} disabled={busy} onChange={e => setDiarize(e.target.checked)} />
          <span>Identify speakers</span>
        </label>
        <label className="ot-field-inline">
          <span>Speakers</span>
          <input type="number" min={1} max={32} value={count} disabled={busy || !diarize} onChange={e => setCount(e.target.value)} placeholder="Auto" className="ot-input ot-input-narrow" />
        </label>
      </div>
      <div className="ot-inline-form">
        <button type="button" className="ot-button ot-button-primary ot-button-small" disabled={busy || !hasRecording} onClick={() => void analyze()}>
          {busy ? 'Working on the recording…' : 'Analyse recording'}
        </button>
        {busy && <button type="button" className="ot-button ot-button-secondary ot-button-small" onClick={async () => { if (job.current) await invoke('speech_cancel', { jobId: job.current }); }}>Cancel</button>}
        {backupDate && !busy && <button type="button" className="ot-button ot-button-secondary ot-button-small" onClick={() => void restore()}>Restore previous transcript</button>}
      </div>
      {!hasRecording && <p className="ot-note-text">This meeting has no saved recording, so it cannot be analysed again.</p>}
      {error && <p role="alert" className="ot-danger-text">{error}</p>}
      {warnings.map(warning => <p key={warning} role="status" className="ot-warn-text">{warning}</p>)}
    </section>
  );
}
