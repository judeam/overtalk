'use client';

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { Loader2, Wand2 } from 'lucide-react';
import { useConfig } from '@/contexts/ConfigContext';
import { SpeechNotice, type SpeechNoticeData } from './SpeechNotice';

/** Set while a recording runs without a live transcript; the saved meeting picks it up. */
export const TRANSCRIBE_AFTER_CALL_KEY = 'overtalk.transcribeAfterCall';

type State = { kind: 'idle' } | { kind: 'running' } | { kind: 'failed'; notice: SpeechNoticeData };

/** A saved meeting with audio but no transcript: make one from the recording. */
export function TranscribeLater({ meetingId, folderPath, onDone }: { meetingId: string; folderPath?: string | null; onDone?: () => Promise<void> | void }) {
  const { transcriptModelConfig } = useConfig();
  const [state, setState] = useState<State>({ kind: 'idle' });
  const started = useRef(false);

  const run = async () => {
    if (!folderPath) return;
    setState({ kind: 'running' });
    try {
      if (transcriptModelConfig.provider === 'localSpeech') {
        await invoke('speech_analyze_meeting', { meetingId, jobId: crypto.randomUUID(), diarize: true, numSpeakers: null });
      } else {
        await new Promise<void>((resolve, reject) => {
          let stops: (() => void)[] = [];
          const done = (fn: () => void) => { stops.forEach((s) => s()); fn(); };
          void Promise.all([
            listen<{ meeting_id: string }>('retranscription-complete', (e) => { if (e.payload.meeting_id === meetingId) done(resolve); }),
            listen<{ meeting_id: string; error: string }>('retranscription-error', (e) => { if (e.payload.meeting_id === meetingId) done(() => reject(new Error(e.payload.error))); }),
          ]).then((s) => { stops = s; });
          invoke('start_retranscription_command', {
            meetingId, meetingFolderPath: folderPath, language: null,
            model: transcriptModelConfig.model || null, provider: transcriptModelConfig.provider || null,
          }).catch((e) => done(() => reject(e)));
        });
      }
      toast.success('The transcript is ready');
      setState({ kind: 'idle' });
      await onDone?.();
    } catch (error) {
      const notice = await invoke<SpeechNoticeData>('gpu_explain_error', { error: String(error) }).catch(() => ({ reason: 'engine', message: 'The transcript could not be made.', detail: String(error), can_free: false }));
      setState({ kind: 'failed', notice });
    }
  };

  // A recording that ran without a live transcript gets its transcript as soon as it is saved.
  useEffect(() => {
    if (started.current || !folderPath) return;
    let pending = false;
    try { pending = sessionStorage.getItem(TRANSCRIBE_AFTER_CALL_KEY) === '1'; sessionStorage.removeItem(TRANSCRIBE_AFTER_CALL_KEY); } catch { /* manual start stays available */ }
    if (pending) { started.current = true; void run(); }
  }, [folderPath]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!folderPath) {
    return <div className="ot-sheet-empty"><strong>No transcript</strong><span>This meeting has no saved recording to transcribe.</span></div>;
  }
  if (state.kind === 'running') {
    return (
      <div className="ot-sheet-empty" role="status">
        <Loader2 className="ot-spin ot-empty-icon" aria-hidden="true" />
        <strong>Making the transcript</strong>
        <span>Overtalk is transcribing the recording on this computer. Long meetings take a few minutes; you can keep using the app.</span>
      </div>
    );
  }
  if (state.kind === 'failed') {
    return (
      <div className="ot-sheet-empty">
        <SpeechNotice title="The transcript could not be made" notice={state.notice} onFreed={() => setState({ kind: 'idle' })}>
          <button type="button" className="ot-button ot-button-primary ot-button-small" onClick={() => void run()}>Try again</button>
        </SpeechNotice>
      </div>
    );
  }
  return (
    <div className="ot-sheet-empty">
      <strong>No transcript yet</strong>
      <span>The recording is saved. Make the transcript from it now; speakers are identified at the same time.</span>
      <button type="button" className="ot-button ot-button-primary ot-button-small ot-empty-action" onClick={() => void run()}>
        <Wand2 aria-hidden="true" />Make the transcript
      </button>
    </div>
  );
}
