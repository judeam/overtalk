import { useState, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { SettingsSection, SettingsRow, SettingsLoading } from '@/components/settings/SettingsSection';
import { Switch } from '@/components/ui/switch';
import { FolderOpen, Info } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { DeviceSelection, SelectedDevices } from '@/components/DeviceSelection';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useConfig } from '@/contexts/ConfigContext';

export interface RecordingPreferences {
  save_folder: string;
  auto_save: boolean;
  file_format: string;
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
}

interface RecordingSettingsProps {
  onSave?: (preferences: RecordingPreferences) => void;
}

export function RecordingSettings({ onSave }: RecordingSettingsProps) {
  const [preferences, setPreferences] = useState<RecordingPreferences>({
    save_folder: '',
    auto_save: true,
    file_format: 'mp4',
    preferred_mic_device: null,
    preferred_system_device: null
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showRecordingNotification, setShowRecordingNotification] = useState(true);
  const { isRecording } = useRecordingState();
  const { setSelectedDevices } = useConfig();

  // Load recording preferences on component mount
  useEffect(() => {
    const loadPreferences = async () => {
      try {
        const prefs = await invoke<RecordingPreferences>('get_recording_preferences');
        setPreferences(prefs);
      } catch (error) {
        console.error('Failed to load recording preferences:', error);
        // If loading fails, get default folder path
        try {
          const defaultPath = await invoke<string>('get_default_recordings_folder_path');
          setPreferences(prev => ({ ...prev, save_folder: defaultPath }));
        } catch (defaultError) {
          console.error('Failed to get default folder path:', defaultError);
        }
      } finally {
        setLoading(false);
      }
    };

    loadPreferences();
  }, []);

  // Load recording notification preference
  useEffect(() => {
    const loadNotificationPref = async () => {
      try {
        const { Store } = await import('@tauri-apps/plugin-store');
        const store = await Store.load('preferences.json');
        const show = await store.get<boolean>('show_recording_notification') ?? true;
        setShowRecordingNotification(show);
      } catch (error) {
        console.error('Failed to load notification preference:', error);
      }
    };
    loadNotificationPref();
  }, []);

  const handleAutoSaveToggle = async (enabled: boolean) => {
    const newPreferences = { ...preferences, auto_save: enabled };
    setPreferences(newPreferences);
    await savePreferences(newPreferences);

  };

  const handleDeviceChange = async (devices: SelectedDevices) => {
    const newPreferences = {
      ...preferences,
      preferred_mic_device: devices.micDevice,
      preferred_system_device: devices.systemDevice
    };
    setPreferences(newPreferences);
    // Sync the in-memory selection ConfigContext exposes to the start path.
    // Without this, the picker only writes to disk (loaded into context once at
    // app mount), so a newly-chosen mic isn't honored until the next launch —
    // start keeps sending the stale launch-time device.
    setSelectedDevices(devices);
    await savePreferences(newPreferences);

    // Track default device preference changes
  };

  const handleOpenFolder = async () => {
    try {
      await invoke('open_recordings_folder');
    } catch (error) {
      console.error('Failed to open recordings folder:', error);
    }
  };

  const handleNotificationToggle = async (enabled: boolean) => {
    try {
      setShowRecordingNotification(enabled);
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('preferences.json');
      await store.set('show_recording_notification', enabled);
      await store.save();
      toast.success('Preference saved');
    } catch (error) {
      console.error('Failed to save notification preference:', error);
      toast.error('Failed to save preference');
    }
  };

  const savePreferences = async (prefs: RecordingPreferences) => {
    setSaving(true);
    try {
      await invoke('set_recording_preferences', { preferences: prefs });
      onSave?.(prefs);

      // Show success toast with device details
      toast.success("Audio device preferences saved");
    } catch (error) {
      console.error('Failed to save recording preferences:', error);
      toast.error("Failed to save device preferences", {
        description: error instanceof Error ? error.message : String(error)
      });
    } finally {
      setSaving(false);
    }
  };

  if (loading) return <SettingsLoading />;
  return <div>
    <SettingsSection title="Audio files">
      <SettingsRow title="Save audio recordings" description="Keep the audio after a meeting so you can listen again or re-transcribe it." htmlFor="save-recordings">
        <Switch id="save-recordings" checked={preferences.auto_save} onCheckedChange={handleAutoSaveToggle} disabled={saving} />
      </SettingsRow>
      {preferences.auto_save ? <>
        <div className="settings-row"><div className="settings-row-copy"><span className="settings-row-title">Save location</span><code className="settings-path">{preferences.save_folder || 'Default folder'}</code></div><Button variant="outline" onClick={handleOpenFolder}><FolderOpen size={16} />Open folder</Button></div>
        <p className="settings-note"><Info /><span>Saved as {preferences.file_format.toUpperCase()}. Each filename includes the recording date and time.</span></p>
      </> : <p role="status" className="settings-note">Audio files will not be kept. Transcripts remain available after the meeting.</p>}
    </SettingsSection>
    <SettingsSection title="Recording reminder">
      <SettingsRow title="Remind me to inform participants" description="Show a reminder when a recording starts." htmlFor="recording-reminder">
        <Switch id="recording-reminder" checked={showRecordingNotification} onCheckedChange={handleNotificationToggle} />
      </SettingsRow>
    </SettingsSection>
    <SettingsSection title="Audio sources" description="These devices will be selected when you start a new recording.">
      {isRecording && <p role="status" className="settings-note">Stop the current recording to change devices. Connecting a device will not switch the active source.</p>}
      <div className="settings-inline-panel"><DeviceSelection selectedDevices={{ micDevice: preferences.preferred_mic_device, systemDevice: preferences.preferred_system_device }} onDeviceChange={handleDeviceChange} disabled={saving || isRecording} /></div>
    </SettingsSection>
  </div>;
}
