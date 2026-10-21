'use client';
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useRouter } from 'next/navigation';
import { Mic, HardDrive, AudioLines } from 'lucide-react';
import { useConfig } from '@/contexts/ConfigContext';
import { useAudioDeviceLabels } from '@/hooks/useAudioDeviceLabels';

export function RecordingReadiness({ onSoundCheck }: { onSoundCheck: () => void }) {
  const { selectedDevices, transcriptModelConfig } = useConfig();
  const { microphoneLabel, systemAudioLabel } = useAudioDeviceLabels(selectedDevices.micDevice, selectedDevices.systemDevice);
  const [saveAudio, setSaveAudio] = useState<boolean | null>(null);
  const router = useRouter();
  useEffect(() => { let active = true; void invoke<{auto_save: boolean}>('get_recording_preferences').then(p => { if (active) setSaveAudio(p.auto_save); }).catch(() => {}); return () => { active = false; }; }, []);
  return <div className="ot-readiness">
    <dl><div><dt><Mic size={16} />Audio sources</dt><dd>{microphoneLabel}<br />{systemAudioLabel}</dd></div><div><dt><HardDrive size={16} />Saved audio</dt><dd>{saveAudio == null ? 'Check recording settings' : saveAudio ? 'Kept for playback and recovery' : 'Not kept after the meeting'}</dd></div><div><dt><AudioLines size={16} />Transcription</dt><dd>{transcriptModelConfig.model || 'Choose a speech model'}<button type="button" className="ot-link" onClick={() => router.push('/settings?tab=Transcriptionmodels')}>Change speech model</button></dd></div></dl>
    <div className="ot-sound-check"><p>Before an important call, record a few seconds while speaking and playing call audio. Stop, then listen to check both sources.</p><button type="button" className="ot-tool" onClick={onSoundCheck}>Record a sound check</button></div>
  </div>;
}
