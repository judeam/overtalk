import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Loader2, Mic, Volume2 } from 'lucide-react';
import { toast } from 'sonner';
import { OnboardingContainer } from '../OnboardingContainer';
import { useOnboarding } from '@/contexts/OnboardingContext';
import { useConfig } from '@/contexts/ConfigContext';
import { PENDING_TOUR_KEY } from '@/components/guide/GuideProvider';

interface Device { name: string; device_type: 'Input' | 'Output'; display_name?: string }
interface Level { device_name: string; rms_level: number; peak_level: number; is_active: boolean }

/** Pick the microphone and call audio, and see the microphone respond before the first meeting. */
export function AudioCheckStep({ step, totalSteps }: { step: number; totalSteps: number }) {
  const { completeOnboarding, goPrevious } = useOnboarding();
  const { selectedDevices, setSelectedDevices } = useConfig();
  const [devices, setDevices] = useState<Device[]>([]);
  const [mic, setMic] = useState<string | null>(selectedDevices.micDevice);
  const [system, setSystem] = useState<string | null>(selectedDevices.systemDevice);
  const [level, setLevel] = useState(0);
  const [heard, setHeard] = useState(false);
  const [finishing, setFinishing] = useState(false);
  const decay = useRef(0);

  const inputs = useMemo(() => devices.filter((d) => d.device_type === 'Input'), [devices]);
  const outputs = useMemo(() => devices.filter((d) => d.device_type === 'Output'), [devices]);

  useEffect(() => {
    void invoke<Device[]>('get_audio_devices').then(setDevices).catch(() => setDevices([]));
  }, []);

  // Listen to the chosen microphone (or every microphone, for the default) while this step is open.
  useEffect(() => {
    if (!inputs.length) return;
    let active = true;
    const names = mic ? [mic.replace(/ \((input|output)\)$/, '')] : inputs.filter(d => d.name === 'default').map(d => d.name);
    if (!names.length) return;
    const unlisten = listen<{ levels: Level[] }>('audio-levels', ({ payload }) => {
      if (!active) return;
      const relevant = payload.levels.filter((l) => names.includes(l.device_name));
      const peak = relevant.reduce((m, l) => Math.max(m, l.rms_level * 2.2, l.peak_level * 0.8), 0);
      decay.current = Math.max(peak, decay.current * 0.82);
      setLevel(Math.min(1, decay.current));
      if (peak > 0.08) setHeard(true);
    });
    void invoke('start_audio_level_monitoring', { deviceNames: names }).catch(() => {});
    return () => {
      active = false;
      void unlisten.then((stop) => stop());
      void invoke('stop_audio_level_monitoring').catch(() => {});
    };
  }, [mic, inputs]);

  const finish = async () => {
    setFinishing(true);
    try {
      const prefs = await invoke<Record<string, unknown>>('get_recording_preferences').catch(() => ({}));
      await invoke('set_recording_preferences', { preferences: { ...prefs, preferred_mic_device: mic, preferred_system_device: system } });
      setSelectedDevices({ micDevice: mic, systemDevice: system });
      await completeOnboarding();
      try { localStorage.setItem(PENDING_TOUR_KEY, 'home'); } catch { /* the tour stays one click away in the guide */ }
      await new Promise((resolve) => setTimeout(resolve, 100));
      window.location.reload();
    } catch (error) {
      toast.error('Could not finish setup', { description: String(error) });
      setFinishing(false);
    }
  };

  const label = (d: Device) => d.display_name || d.name;
  const bars = 24;

  return (
    <OnboardingContainer
      title="Check your sound"
      description="Overtalk records your microphone and the call you are in. Say something to see the microphone respond."
      step={step}
      totalSteps={totalSteps}
    >
      <div className="ot-setup-card">
        <label className="ot-setup-field">
          <span><Mic aria-hidden="true" />Microphone</span>
          <select className="ot-input" value={mic ?? ''} onChange={(e) => { setMic(e.target.value || null); setHeard(false); }}>
            <option value="">Default microphone</option>
            {inputs.map((d) => <option key={d.name} value={d.name}>{label(d)}</option>)}
          </select>
        </label>
        <div className="ot-setup-meter" role="meter" aria-label="Microphone level" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(level * 100)}>
          {Array.from({ length: bars }, (_, i) => <i key={i} data-on={level * bars > i ? 'true' : undefined} />)}
        </div>
        <p className="ot-setup-status" role="status">
          {heard ? 'Microphone activity detected. Make a short sound-check recording to verify your selected sources.' : inputs.length ? 'Waiting to hear you…' : 'No microphone found. Connect one, or continue and choose it later in Settings.'}
        </p>
        <label className="ot-setup-field">
          <span><Volume2 aria-hidden="true" />Call audio</span>
          <select className="ot-input" value={system ?? ''} onChange={(e) => setSystem(e.target.value || null)}>
            <option value="">Default speakers or headphones</option>
            {outputs.map((d) => <option key={d.name} value={d.name}>{label(d)}</option>)}
          </select>
        </label>
        <p className="ot-note-text">Choose where you hear calls. Overtalk records what plays there, so the other people are in the meeting too.</p>
      </div>
      <div className="ot-setup-nav">
        <button type="button" className="ot-button ot-button-secondary" onClick={goPrevious} disabled={finishing}>Back</button>
        <button type="button" className="ot-button ot-button-primary" onClick={() => void finish()} disabled={finishing}>
          {finishing ? <Loader2 className="ot-spin" aria-hidden="true" /> : null}Start using Overtalk
        </button>
      </div>
    </OnboardingContainer>
  );
}
