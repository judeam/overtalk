"use client"
import { ReadingSettings } from "./settings/ReadingSettings"

import { useEffect, useState } from "react"
import { Switch } from "./ui/switch"
import { FolderOpen, ShieldCheck } from "lucide-react"
import { invoke } from "@tauri-apps/api/core"
import { Button } from "./ui/button"
import { SettingsSection, SettingsRow, SettingsLoading } from "./settings/SettingsSection"
import { useConfig, NotificationSettings } from "@/contexts/ConfigContext"

export function PreferenceSettings() {
  const {
    notificationSettings,
    storageLocations,
    isLoadingPreferences,
    loadPreferences,
    updateNotificationSettings
  } = useConfig();

  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean | null>(null);
  const [isInitialLoad, setIsInitialLoad] = useState(true);
  const [previousNotificationsEnabled, setPreviousNotificationsEnabled] = useState<boolean | null>(null);
  const [notificationError, setNotificationError] = useState('');

  // Lazy load preferences on mount (only loads if not already cached)
  useEffect(() => {
    loadPreferences();
  }, [loadPreferences]);

  // Update notificationsEnabled when notificationSettings are loaded from global state
  useEffect(() => {
    if (notificationSettings) {
      // Notification enabled means both started and stopped notifications are enabled
      const enabled =
        notificationSettings.notification_preferences.show_recording_started &&
        notificationSettings.notification_preferences.show_recording_stopped;
      setNotificationsEnabled(enabled);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(enabled);
        setIsInitialLoad(false);
      }
    } else if (!isLoadingPreferences) {
      // If not loading and no settings, use default
      setNotificationsEnabled(true);
      if (isInitialLoad) {
        setPreviousNotificationsEnabled(true);
        setIsInitialLoad(false);
      }
    }
  }, [notificationSettings, isLoadingPreferences, isInitialLoad])

  useEffect(() => {
    // Skip update on initial load or if value hasn't actually changed
    if (isInitialLoad || notificationsEnabled === null || notificationsEnabled === previousNotificationsEnabled) return;
    if (!notificationSettings) return;

    const handleUpdateNotificationSettings = async () => {
      setNotificationError('');

      try {
        // Update the notification preferences
        const updatedSettings: NotificationSettings = {
          ...notificationSettings,
          notification_preferences: {
            ...notificationSettings.notification_preferences,
            show_recording_started: notificationsEnabled,
            show_recording_stopped: notificationsEnabled,
          }
        };

        console.log("Calling updateNotificationSettings with:", updatedSettings);
        await updateNotificationSettings(updatedSettings);
        setPreviousNotificationsEnabled(notificationsEnabled);
        console.log("Successfully updated notification settings to:", notificationsEnabled);

      } catch (error) {
        setNotificationError('Could not save notification preferences. Please try again.');
        setNotificationsEnabled(previousNotificationsEnabled);
      }
    };

    handleUpdateNotificationSettings();
  }, [notificationsEnabled, notificationSettings, isInitialLoad, previousNotificationsEnabled, updateNotificationSettings])

  const handleOpenFolder = async (folderType: 'database' | 'models' | 'recordings') => {
    try {
      switch (folderType) {
        case 'database':
          await invoke('open_database_folder');
          break;
        case 'models':
          await invoke('open_models_folder');
          break;
        case 'recordings':
          await invoke('open_recordings_folder');
          break;
      }

    } catch (error) {
      console.error(`Failed to open ${folderType} folder:`, error);
    }
  };

  // Show loading only if we're actually loading and don't have cached data
  if (isLoadingPreferences && !notificationSettings && !storageLocations) {
    return <SettingsLoading />
  }

  // Show loading if notificationsEnabled hasn't been determined yet
  if (notificationsEnabled === null && !isLoadingPreferences) {
    return <SettingsLoading />
  }

  // Ensure we have a boolean value for the Switch component
  const notificationsEnabledValue = notificationsEnabled ?? false;

  return <div>
    <SettingsSection title="Reading & display" description="See more at a glance, or give the words more room."><ReadingSettings /></SettingsSection>
    <SettingsSection title="Notifications">
      <SettingsRow title="Meeting notifications" description="Let me know when a recording starts or stops." htmlFor="meeting-notifications">
        <Switch id="meeting-notifications" checked={notificationsEnabledValue} onCheckedChange={setNotificationsEnabled} />
      </SettingsRow>
      {notificationError && <p role="alert" className="settings-inline-error">{notificationError}</p>}
    </SettingsSection>
    <SettingsSection title="Storage" description="Your meeting files are saved on this device.">
      <div className="settings-row">
        <div className="settings-row-copy"><span className="settings-row-title">Meeting recordings</span><code className="settings-path">{storageLocations?.recordings || 'Loading location…'}</code></div>
        <Button variant="outline" onClick={() => handleOpenFolder('recordings')}><FolderOpen size={16} />Open folder</Button>
      </div>
      <SettingsRow title="Transcripts & notes" description="Open the application folder containing your meeting database."><Button variant="outline" onClick={() => handleOpenFolder('database')}><FolderOpen size={16} />Open folder</Button></SettingsRow>
      <SettingsRow title="Downloaded models" description="Speech and notes models stored on this computer."><Button variant="outline" onClick={() => handleOpenFolder('models')}><FolderOpen size={16} />Open folder</Button></SettingsRow>
    </SettingsSection>
    <SettingsSection title="Privacy">
      <p className="settings-note"><ShieldCheck /><span>Overtalk does not collect usage telemetry. External AI providers and connections run only when you configure them.</span></p>
    </SettingsSection>
  </div>;
}
