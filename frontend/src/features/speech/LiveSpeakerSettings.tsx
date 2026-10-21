'use client';

import type { LocalSpeechConfig } from '@/features/speech/LocalSpeechSettings';
import { Input } from '@/components/ui/input';

export function LiveSpeakerSettings({ config, onChange }: {
  config: LocalSpeechConfig;
  onChange: (patch: Partial<LocalSpeechConfig>) => void;
}) {
  return <fieldset className="space-y-3 border-0 py-4">
    <legend className="px-1 font-medium">Live speaker transcription</legend>
    <label className="flex items-start gap-2 text-sm">
      <input type="checkbox" checked={config.live_diarization}
        onChange={event => onChange({ live_diarization: event.target.checked })} />
      Distinguish speakers during local recording
    </label>
    <p className="text-xs text-gray-600">Runs one continuous ASR and diarization session, including pauses. Supports up to eight distinct speakers per meeting. Labels are provisional during recording; you can name and merge speakers after saving.</p>
    {config.live_diarization && <>
      <label className="settings-field">Streaming speech model
        <select className="w-full" value={config.streaming_asr_model}
          onChange={event => onChange({ streaming_asr_model: event.target.value })}>
          <option value="nemotron-3.5">Nemotron 3.5 streaming ASR — multilingual</option>
          <option value="multitalker-parakeet">Multitalker Parakeet — English, overlapping speech</option>
        </select>
      </label>
      <label className="settings-field">Local streaming ASR .nemo checkpoint
        <Input value={config.streaming_asr_model_path} onChange={event => onChange({ streaming_asr_model_path: event.target.value })}
          placeholder="/path/to/nemotron-streaming.nemo" />
      </label>
      <p className="text-xs text-gray-600">Use the separate NeMo Python executable and a Nemotron-3 Diarization .nemo checkpoint in the fields above. Install the current NVIDIA NeMo Speech streaming helpers following speech-worker/README.md. Model checks run before audio capture starts.</p>
      <p className="text-xs text-gray-600">Select this local runtime for transcription to activate live speakers. If it cannot keep up, a warning appears and audio recording continues for later analysis. Use audio auto-save to retain that recovery option.</p>
      <p className="text-xs text-gray-600">Live transcripts can omit speech. Keep audio auto-save enabled and analyze the saved audio for a final transcript.</p>
    </>}
  </fieldset>;
}
