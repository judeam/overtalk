export interface DesktopAudioDevice { name: string; device_type: 'Input' | 'Output'; display_name?: string; unavailable_reason?: string }
export const deviceValue = (device: DesktopAudioDevice) => `${device.name} (${device.device_type.toLowerCase()})`;
export function deviceLabel(devices: DesktopAudioDevice[], value: string | null, fallback = 'Default microphone'): string {
  if (!value || value === 'default') return fallback;
  const device = devices.find((d) => deviceValue(d) === value || d.name === value);
  const label = device?.display_name || device?.name || value;
  return label.startsWith('pulse:') ? 'Selected audio device' : label.replace(/ \((input|output)\)$/, '');
}
