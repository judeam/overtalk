use crate::notifications::{
    settings::{ConsentManager, NotificationSettings},
    system::SystemNotificationHandler,
    types::{Notification, NotificationType},
};
use anyhow::Result;
use log::{error as log_error, info as log_info, warn as log_warn};
use std::sync::Arc;
use tauri::{AppHandle, Runtime};
use tokio::sync::RwLock;

/// Central notification manager that coordinates all notification functionality
pub struct NotificationManager<R: Runtime> {
    system_handler: Arc<SystemNotificationHandler<R>>,
    consent_manager: Arc<ConsentManager>,
    settings: Arc<RwLock<NotificationSettings>>,
    initialized: Arc<RwLock<bool>>,
}

impl<R: Runtime> NotificationManager<R> {
    /// Create a new notification manager
    pub async fn new(app_handle: AppHandle<R>) -> Result<Self> {
        let system_handler = Arc::new(SystemNotificationHandler::new(app_handle.clone()));
        let consent_manager = Arc::new(ConsentManager::new()?);

        // Load initial settings
        let settings = consent_manager
            .get_settings_with_migration()
            .await
            .unwrap_or_else(|_| NotificationSettings::default());

        let manager = Self {
            system_handler,
            consent_manager,
            settings: Arc::new(RwLock::new(settings)),
            initialized: Arc::new(RwLock::new(false)),
        };

        log_info!("NotificationManager created successfully");
        Ok(manager)
    }

    /// Initialize the notification system
    pub async fn initialize(&self) -> Result<()> {
        let mut initialized = self.initialized.write().await;
        if *initialized {
            return Ok(());
        }

        log_info!("Initializing notification system...");

        // Check if this is the first launch
        if !self.consent_manager.has_consent().await {
            log_info!("First launch detected, notification consent will be requested by UI");
        }

        // Try to request system permission if not already granted
        if !self.consent_manager.has_system_permission().await {
            match self.system_handler.request_permission().await {
                Ok(granted) => {
                    self.consent_manager.set_system_permission(granted).await?;
                    if granted {
                        log_info!("System notification permission granted");
                    } else {
                        log_warn!("System notification permission was not granted");
                    }
                }
                Err(e) => {
                    log_error!("Failed to request notification permission: {}", e);
                }
            }
        }

        *initialized = true;
        log_info!("Notification system initialized successfully");
        Ok(())
    }

    /// Show a notification if all conditions are met
    pub async fn show_notification(&self, notification: Notification) -> Result<()> {
        if !self.consent_manager.has_consent().await {
            return Ok(());
        }
        // Ensure system is initialized
        if !*self.initialized.read().await {
            self.initialize().await?;
        }

        // Check if we should show notifications
        if !self.should_show_notification(&notification).await {
            log_info!(
                "Skipping notification due to settings: {}",
                notification.title
            );
            return Ok(());
        }

        // Log the notification attempt
        log_info!(
            "Showing notification: {} - {}",
            notification.title,
            notification.body
        );

        // Show the notification
        self.system_handler.show_notification(notification).await
    }

    /// Show a recording started notification
    pub async fn show_recording_started(&self, meeting_name: Option<String>) -> Result<()> {
        let settings = self.settings.read().await;
        log_info!(
            "🔔 Checking notification settings - show_recording_started: {}",
            settings.notification_preferences.show_recording_started
        );

        if !settings.notification_preferences.show_recording_started {
            log_info!("🚫 Recording started notification is disabled, skipping");
            return Ok(());
        }

        log_info!("✅ Recording started notification is enabled, showing notification");
        let notification = Notification::recording_started(meeting_name);
        self.show_notification(notification).await
    }

    /// Show a recording stopped notification
    pub async fn show_recording_stopped(&self) -> Result<()> {
        let settings = self.settings.read().await;
        if !settings.notification_preferences.show_recording_stopped {
            return Ok(());
        }

        let notification = Notification::recording_stopped();
        self.show_notification(notification).await
    }

    /// Get current notification settings
    pub async fn get_settings(&self) -> NotificationSettings {
        self.settings.read().await.clone()
    }

    /// Update notification settings
    pub async fn update_settings(&self, new_settings: NotificationSettings) -> Result<()> {
        log_info!("📝 Updating notification settings:");
        log_info!(
            "   show_recording_started: {}",
            new_settings.notification_preferences.show_recording_started
        );
        log_info!(
            "   show_recording_stopped: {}",
            new_settings.notification_preferences.show_recording_stopped
        );

        // Validate settings
        crate::notifications::settings::validate_settings(&new_settings)?;

        // Save to disk
        self.consent_manager.save_settings(&new_settings).await?;
        log_info!("💾 Settings saved to disk");

        // Update in-memory settings
        let mut settings = self.settings.write().await;
        *settings = new_settings;

        log_info!("✅ Notification settings updated successfully");
        Ok(())
    }

    /// Check if Do Not Disturb is active (system or manual)
    pub async fn is_dnd_active(&self) -> bool {
        let settings = self.settings.read().await;

        // Check manual DND first
        if settings.manual_dnd_mode {
            return true;
        }

        // Check system DND if user wants to respect it
        if settings.respect_do_not_disturb {
            self.system_handler.is_dnd_active().await
        } else {
            false
        }
    }

    /// Request notification permission from the system
    pub async fn request_permission(&self) -> Result<bool> {
        let granted = self.system_handler.request_permission().await?;
        self.consent_manager.set_system_permission(granted).await?;

        // Update in-memory settings
        let mut settings = self.settings.write().await;
        settings.system_permission_granted = granted;

        Ok(granted)
    }

    /// Set user consent for notifications
    pub async fn set_consent(&self, consent: bool) -> Result<()> {
        self.consent_manager.set_consent(consent).await?;

        // Update in-memory settings
        let mut settings = self.settings.write().await;
        settings.consent_given = consent;

        log_info!("User consent set to: {}", consent);
        Ok(())
    }

    /// Check if we should show a specific notification based on settings
    async fn should_show_notification(&self, notification: &Notification) -> bool {
        let settings = self.settings.read().await;

        // Check basic consent and permissions
        if !settings.consent_given || !settings.system_permission_granted {
            return false;
        }

        // Check DND status
        if self.is_dnd_active().await {
            // Only show critical notifications when DND is active
            match notification.priority {
                crate::notifications::types::NotificationPriority::Critical => {}
                _ => return false,
            }
        }

        // Check notification type specific settings
        match &notification.notification_type {
            NotificationType::RecordingStarted => {
                settings.notification_preferences.show_recording_started
            }
            NotificationType::RecordingStopped => {
                settings.notification_preferences.show_recording_stopped
            }
            NotificationType::RecordingPaused => {
                settings.notification_preferences.show_recording_paused
            }
            NotificationType::RecordingResumed => {
                settings.notification_preferences.show_recording_resumed
            }
            NotificationType::TranscriptionComplete => {
                settings
                    .notification_preferences
                    .show_transcription_complete
            }
            NotificationType::MeetingReminder(_) => {
                settings.notification_preferences.show_meeting_reminders
            }
            NotificationType::SystemError(_) => {
                settings.notification_preferences.show_system_errors
            }
            NotificationType::Test => true, // Always show test notifications
        }
    }
}
