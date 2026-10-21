use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[cfg(test)]
const SENSITIVE_ANALYTICS_KEYS: &[&str] = &[
    "meeting_title",
    "meetingTitle",
    "meeting_name",
    "meetingName",
    "file_name",
    "filename",
    "file_path",
    "folder_path",
    "path",
    "source_path",
    "meeting_folder_path",
    "device_name",
    "user_agent",
];

#[cfg(test)]
fn sanitize_analytics_properties(
    mut properties: HashMap<String, String>,
) -> HashMap<String, String> {
    properties.retain(|key, _| !SENSITIVE_ANALYTICS_KEYS.contains(&key.as_str()));
    properties
}

fn meeting_started_properties(meeting_id: &str) -> HashMap<String, String> {
    let mut properties = HashMap::new();
    properties.insert("meeting_id".to_string(), meeting_id.to_string());
    properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());
    properties
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AnalyticsConfig {
    pub api_key: String,
    pub host: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSession {
    pub session_id: String,
    pub user_id: String,
    pub start_time: DateTime<Utc>,
    pub is_active: bool,
}

impl UserSession {
    pub fn new(user_id: String) -> Self {
        let now = Utc::now();
        Self {
            session_id: format!("session_{}", Uuid::new_v4()),
            user_id,
            start_time: now,
            is_active: true,
        }
    }

    pub fn duration_seconds(&self) -> i64 {
        (Utc::now() - self.start_time).num_seconds()
    }
}

pub struct MeetingEnded<'a> {
    pub transcription_provider: &'a str,
    pub transcription_model: &'a str,
    pub summary_provider: &'a str,
    pub summary_model: &'a str,
    pub total_duration_seconds: Option<f64>,
    pub active_duration_seconds: f64,
    pub pause_duration_seconds: f64,
    pub microphone_device_type: &'a str,
    pub system_audio_device_type: &'a str,
    pub chunks_processed: u64,
    pub transcript_segments_count: u64,
    pub had_fatal_error: bool,
}

pub struct AnalyticsClient {
    user_id: Arc<Mutex<Option<String>>>,
    current_session: Arc<Mutex<Option<UserSession>>>,
}

impl AnalyticsClient {
    // Retain the legacy command API without linking or contacting a telemetry service.
    pub async fn new(_config: AnalyticsConfig) -> Self {
        Self {
            user_id: Arc::new(Mutex::new(None)),
            current_session: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn identify(
        &self,
        user_id: String,
        _properties: Option<HashMap<String, String>>,
    ) -> Result<(), String> {
        *self.user_id.lock().await = Some(user_id);
        Ok(())
    }

    pub async fn track_event(
        &self,
        _event_name: &str,
        _properties: Option<HashMap<String, String>>,
    ) -> Result<(), String> {
        Ok(())
    }

    // Enhanced user tracking methods
    pub async fn start_session(&self, user_id: String) -> Result<String, String> {
        let session = UserSession::new(user_id.clone());
        let session_id = session.session_id.clone();

        *self.current_session.lock().await = Some(session);

        let mut properties = HashMap::new();
        properties.insert("session_id".to_string(), session_id.clone());
        properties.insert("timestamp".to_string(), Utc::now().to_rfc3339());

        self.track_event("session_started", Some(properties))
            .await?;

        Ok(session_id)
    }

    pub async fn end_session(&self) -> Result<(), String> {
        let session = {
            let mut session_guard = self.current_session.lock().await;
            session_guard.take()
        };

        if let Some(session) = session {
            let mut properties = HashMap::new();
            properties.insert("session_id".to_string(), session.session_id.clone());
            properties.insert(
                "session_duration".to_string(),
                session.duration_seconds().to_string(),
            );
            properties.insert("timestamp".to_string(), Utc::now().to_rfc3339());

            self.track_event("session_ended", Some(properties)).await?;
        }

        Ok(())
    }

    pub async fn track_daily_active_user(&self) -> Result<(), String> {
        let user_id = match self.user_id.lock().await.clone() {
            Some(id) => id,
            None => {
                log::warn!("Attempted to track daily active user before user identification");
                return Ok(());
            }
        };

        let mut properties = HashMap::new();
        properties.insert("user_id".to_string(), user_id);
        properties.insert(
            "date".to_string(),
            Utc::now().format("%Y-%m-%d").to_string(),
        );
        properties.insert("timestamp".to_string(), Utc::now().to_rfc3339());

        self.track_event("daily_active_user", Some(properties))
            .await
    }

    pub async fn track_user_first_launch(&self) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("timestamp".to_string(), Utc::now().to_rfc3339());
        properties.insert(
            "app_version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );

        self.track_event("user_first_launch", Some(properties))
            .await
    }

    pub async fn get_current_session(&self) -> Option<UserSession> {
        self.current_session.lock().await.clone()
    }

    pub async fn is_session_active(&self) -> bool {
        self.current_session.lock().await.is_some()
    }

    // Meeting-specific event tracking methods
    pub async fn track_meeting_started(&self, meeting_id: &str) -> Result<(), String> {
        self.track_event(
            "meeting_started",
            Some(meeting_started_properties(meeting_id)),
        )
        .await
    }

    pub async fn track_recording_started(&self, meeting_id: &str) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("meeting_id".to_string(), meeting_id.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("recording_started", Some(properties))
            .await
    }

    pub async fn track_recording_stopped(
        &self,
        meeting_id: &str,
        duration_seconds: Option<u64>,
    ) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("meeting_id".to_string(), meeting_id.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        if let Some(duration) = duration_seconds {
            properties.insert("duration_seconds".to_string(), duration.to_string());
        }

        self.track_event("recording_stopped", Some(properties))
            .await
    }

    pub async fn track_meeting_deleted(&self, meeting_id: &str) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("meeting_id".to_string(), meeting_id.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("meeting_deleted", Some(properties)).await
    }

    pub async fn track_settings_changed(
        &self,
        setting_type: &str,
        new_value: &str,
    ) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("setting_type".to_string(), setting_type.to_string());
        properties.insert("new_value".to_string(), new_value.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("settings_changed", Some(properties)).await
    }

    pub async fn track_app_started(&self, version: &str) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("app_version".to_string(), version.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("app_started", Some(properties)).await
    }

    pub async fn track_feature_used(&self, feature_name: &str) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("feature_name".to_string(), feature_name.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("feature_used", Some(properties)).await
    }

    // Summary generation analytics
    pub async fn track_summary_generation_started(
        &self,
        model_provider: &str,
        model_name: &str,
        transcript_length: usize,
    ) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("model_provider".to_string(), model_provider.to_string());
        properties.insert("model_name".to_string(), model_name.to_string());
        properties.insert(
            "transcript_length".to_string(),
            transcript_length.to_string(),
        );
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("summary_generation_started", Some(properties))
            .await
    }

    pub async fn track_summary_generation_completed(
        &self,
        model_provider: &str,
        model_name: &str,
        success: bool,
        duration_seconds: Option<u64>,
        error_message: Option<&str>,
    ) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("model_provider".to_string(), model_provider.to_string());
        properties.insert("model_name".to_string(), model_name.to_string());
        properties.insert("success".to_string(), success.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        if let Some(duration) = duration_seconds {
            properties.insert("duration_seconds".to_string(), duration.to_string());
        }

        if let Some(error) = error_message {
            properties.insert("error_message".to_string(), error.to_string());
        }

        self.track_event("summary_generation_completed", Some(properties))
            .await
    }

    pub async fn track_summary_regenerated(
        &self,
        model_provider: &str,
        model_name: &str,
    ) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("model_provider".to_string(), model_provider.to_string());
        properties.insert("model_name".to_string(), model_name.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("summary_regenerated", Some(properties))
            .await
    }

    pub async fn track_model_changed(
        &self,
        old_provider: &str,
        old_model: &str,
        new_provider: &str,
        new_model: &str,
    ) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("old_provider".to_string(), old_provider.to_string());
        properties.insert("old_model".to_string(), old_model.to_string());
        properties.insert("new_provider".to_string(), new_provider.to_string());
        properties.insert("new_model".to_string(), new_model.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("model_changed", Some(properties)).await
    }

    pub async fn track_custom_prompt_used(&self, prompt_length: usize) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("prompt_length".to_string(), prompt_length.to_string());
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("custom_prompt_used", Some(properties))
            .await
    }

    pub async fn track_meeting_ended(&self, event: MeetingEnded<'_>) -> Result<(), String> {
        let MeetingEnded {
            transcription_provider,
            transcription_model,
            summary_provider,
            summary_model,
            total_duration_seconds,
            active_duration_seconds,
            pause_duration_seconds,
            microphone_device_type,
            system_audio_device_type,
            chunks_processed,
            transcript_segments_count,
            had_fatal_error,
        } = event;
        let mut properties = HashMap::new();

        // Model information
        properties.insert(
            "transcription_provider".to_string(),
            transcription_provider.to_string(),
        );
        properties.insert(
            "transcription_model".to_string(),
            transcription_model.to_string(),
        );
        properties.insert("summary_provider".to_string(), summary_provider.to_string());
        properties.insert("summary_model".to_string(), summary_model.to_string());

        // Duration metrics
        if let Some(duration) = total_duration_seconds {
            properties.insert("total_duration_seconds".to_string(), duration.to_string());
        }
        properties.insert(
            "active_duration_seconds".to_string(),
            active_duration_seconds.to_string(),
        );
        properties.insert(
            "pause_duration_seconds".to_string(),
            pause_duration_seconds.to_string(),
        );

        // Privacy-safe device types
        properties.insert(
            "microphone_device_type".to_string(),
            microphone_device_type.to_string(),
        );
        properties.insert(
            "system_audio_device_type".to_string(),
            system_audio_device_type.to_string(),
        );

        // Processing stats
        properties.insert("chunks_processed".to_string(), chunks_processed.to_string());
        properties.insert(
            "transcript_segments_count".to_string(),
            transcript_segments_count.to_string(),
        );
        properties.insert("had_fatal_error".to_string(), had_fatal_error.to_string());

        // Timestamp
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("meeting_ended", Some(properties)).await
    }

    // Analytics consent tracking
    pub async fn track_analytics_enabled(&self) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("analytics_enabled", Some(properties))
            .await
    }

    pub async fn track_analytics_disabled(&self) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("analytics_disabled", Some(properties))
            .await
    }

    pub async fn track_analytics_transparency_viewed(&self) -> Result<(), String> {
        let mut properties = HashMap::new();
        properties.insert("timestamp".to_string(), chrono::Utc::now().to_rfc3339());

        self.track_event("analytics_transparency_viewed", Some(properties))
            .await
    }

    pub fn is_enabled(&self) -> bool {
        false
    }

    pub async fn set_user_properties(
        &self,
        _properties: HashMap<String, String>,
    ) -> Result<(), String> {
        Ok(())
    }
}

// Helper function to create analytics client from config
pub async fn create_analytics_client(config: AnalyticsConfig) -> AnalyticsClient {
    AnalyticsClient::new(config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[test]
    fn analytics_properties_drop_sensitive_meeting_metadata() {
        let mut properties = HashMap::new();
        properties.insert("meeting_title".to_string(), "Board Strategy".to_string());
        properties.insert("meetingTitle".to_string(), "Board Strategy".to_string());
        properties.insert("meeting_name".to_string(), "Client Call".to_string());
        properties.insert("meetingName".to_string(), "Client Call".to_string());
        properties.insert("file_name".to_string(), "acquisition.wav".to_string());
        properties.insert("filename".to_string(), "acquisition.wav".to_string());
        properties.insert(
            "file_path".to_string(),
            "C:\\meetings\\acquisition.wav".to_string(),
        );
        properties.insert("folder_path".to_string(), "C:\\meetings".to_string());
        properties.insert(
            "path".to_string(),
            "C:\\meetings\\acquisition.wav".to_string(),
        );
        properties.insert(
            "source_path".to_string(),
            "C:\\imports\\source.wav".to_string(),
        );
        properties.insert(
            "meeting_folder_path".to_string(),
            "C:\\meetings\\private".to_string(),
        );
        properties.insert("device_name".to_string(), "Jane's AirPods".to_string());
        properties.insert("user_agent".to_string(), "Mozilla/5.0".to_string());
        properties.insert("meeting_id".to_string(), "meeting-123".to_string());
        properties.insert("duration_seconds".to_string(), "125".to_string());
        properties.insert("segments_count".to_string(), "42".to_string());
        properties.insert("model_name".to_string(), "parakeet".to_string());
        properties.insert("platform".to_string(), "Windows".to_string());

        let sanitized = sanitize_analytics_properties(properties);

        for key in [
            "meeting_title",
            "meetingTitle",
            "meeting_name",
            "meetingName",
            "file_name",
            "filename",
            "file_path",
            "folder_path",
            "path",
            "source_path",
            "meeting_folder_path",
            "device_name",
            "user_agent",
        ] {
            assert!(
                !sanitized.contains_key(key),
                "sensitive key remained: {}",
                key
            );
        }

        assert_eq!(
            sanitized.get("meeting_id"),
            Some(&"meeting-123".to_string())
        );
        assert_eq!(sanitized.get("duration_seconds"), Some(&"125".to_string()));
        assert_eq!(sanitized.get("segments_count"), Some(&"42".to_string()));
        assert_eq!(sanitized.get("model_name"), Some(&"parakeet".to_string()));
        assert_eq!(sanitized.get("platform"), Some(&"Windows".to_string()));
    }

    #[tokio::test]
    async fn ending_session_completes_without_telemetry() {
        let analytics = AnalyticsClient::new(AnalyticsConfig {
            enabled: true,
            api_key: "unused".into(),
            host: Some("https://example.invalid".into()),
        })
        .await;
        assert!(!analytics.is_enabled());
        analytics.start_session("user".into()).await.unwrap();
        timeout(Duration::from_secs(1), analytics.end_session())
            .await
            .expect("ending a session must not wait on its own session mutex")
            .unwrap();
        assert!(!analytics.is_session_active().await);
    }

    #[test]
    fn meeting_started_properties_do_not_include_title() {
        let properties = meeting_started_properties("meeting-123");

        assert_eq!(
            properties.get("meeting_id"),
            Some(&"meeting-123".to_string())
        );
        assert!(properties.contains_key("timestamp"));
        assert!(!properties.contains_key("meeting_title"));
    }
}
