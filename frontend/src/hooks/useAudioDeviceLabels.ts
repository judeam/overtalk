'use client';

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { deviceLabel, type DesktopAudioDevice } from '@/lib/audio-devices';

/** Resolve labels for display; keep the original route IDs for capture. */
export function useAudioDeviceLabels(microphone: string | null, systemAudio: string | null = null) {
  const [devices, setDevices] = useState<DesktopAudioDevice[]>([]);

  useEffect(() => {
    if ((!microphone || microphone === 'default') && (!systemAudio || systemAudio === 'default')) return;
    let active = true;
    void invoke<DesktopAudioDevice[]>('get_audio_devices')
      .then(value => { if (active) setDevices(value); })
      .catch(() => { if (active) setDevices([]); });
    return () => { active = false; };
  }, [microphone, systemAudio]);

  return {
    microphoneLabel: deviceLabel(devices, microphone),
    systemAudioLabel: deviceLabel(devices, systemAudio, 'Default system audio'),
  };
}
