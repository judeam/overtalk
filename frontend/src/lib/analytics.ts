import { invoke } from '@tauri-apps/api/core';

export interface AnalyticsProperties {
  [key: string]: string;
}

export interface DeviceInfo {
  platform: string;
  os_version: string;
  architecture: string;
}

export interface UserSession {
  session_id: string;
  user_id: string;
  start_time: string;
  last_heartbeat: string;
  is_active: boolean;
}

export class Analytics {
  private static initialized = false;
  private static initializationPromise: Promise<void> | null = null;
  private static meetingsInSession: number = 0;
  private static deviceInfo: DeviceInfo | null = null;
  private static readonly reportedTranscriptionErrorCodes = new Set<string>();

  static async init(): Promise<void> {
    // Prevent duplicate initialization
    if (this.initialized) {
      return;
    }

    // If already initializing, wait for it to complete
    if (this.initializationPromise) {
      return this.initializationPromise;
    }

    this.initializationPromise = this.doInit();
    return this.initializationPromise;
  }

  private static async doInit(): Promise<void> {
    try {
      await invoke('init_analytics');
      this.initialized = true;
      console.log('Analytics initialized successfully');
    } catch (error) {
      console.error('Failed to initialize analytics:', error);
      throw error;
    } finally {
      this.initializationPromise = null;
    }
  }

  static async track(eventName: string, properties?: AnalyticsProperties): Promise<void> {
    if (!this.initialized) {
      console.warn('Analytics not initialized');
      return;
    }

    try {
      await invoke('track_event', { eventName, properties });
    } catch (error) {
      console.error(`Failed to track event ${eventName}:`, error);
    }
  }

  // Enhanced user tracking methods for Phase 1
  static async startSession(userId: string): Promise<string | null> {
    if (!this.initialized) {
      console.warn('Analytics not initialized');
      return null;
    }

    try {
      const sessionId = await invoke('start_analytics_session', { userId });
      this.reportedTranscriptionErrorCodes.clear();

      return sessionId as string;
    } catch (error) {
      console.error('Failed to start analytics session:', error);
      return null;
    }
  }

  // Platform/Device detection methods
  static async getPlatform(): Promise<string> {
    try {
      // Use browser's user agent as fallback
      const userAgent = navigator.userAgent.toLowerCase();
      if (userAgent.includes('mac')) return 'macOS';
      if (userAgent.includes('win')) return 'Windows';
      if (userAgent.includes('linux')) return 'Linux';
      return 'unknown';
    } catch (error) {
      console.error('Failed to get platform:', error);
      return 'unknown';
    }
  }

  static async getOSVersion(): Promise<string> {
    try {
      const platform = await this.getPlatform();
      // Use navigator.userAgent for version info
      const userAgent = navigator.userAgent;
      return `${platform} (${userAgent})`;
    } catch (error) {
      console.error('Failed to get OS version:', error);
      return 'unknown';
    }
  }

  static async getDeviceInfo(): Promise<DeviceInfo> {
    if (this.deviceInfo) return this.deviceInfo;

    try {
      const platform = await this.getPlatform();
      const osVersion = await this.getOSVersion();

      // Detect architecture from user agent
      const userAgent = navigator.userAgent.toLowerCase();
      let architecture = 'unknown';
      if (userAgent.includes('arm') || userAgent.includes('aarch64')) {
        architecture = 'aarch64';
      } else if (userAgent.includes('x86_64') || userAgent.includes('x64')) {
        architecture = 'x86_64';
      } else if (userAgent.includes('x86')) {
        architecture = 'x86';
      }

      this.deviceInfo = {
        platform: platform,
        os_version: osVersion,
        architecture: architecture
      };

      return this.deviceInfo;
    } catch (error) {
      console.error('Failed to get device info:', error);
      return {
        platform: 'unknown',
        os_version: 'unknown',
        architecture: 'unknown'
      };
    }
  }

  // Helper methods for analytics.json store
  static async calculateDaysSince(dateKey: string): Promise<number | null> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const dateStr = await store.get<string>(dateKey);
      if (!dateStr) return null;
      const diffMs = Date.now() - new Date(dateStr).getTime();
      return Math.floor(diffMs / (1000 * 60 * 60 * 24));
    } catch (error) {
      console.error(`Failed to calculate days since ${dateKey}:`, error);
      return null;
    }
  }

  static async updateMeetingCount(): Promise<void> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');

      const totalMeetings = (await store.get<number>('total_meetings') || 0) + 1;
      await store.set('total_meetings', totalMeetings);
      await store.set('last_meeting_date', new Date().toISOString());

      // Update daily count
      const today = new Date().toISOString().split('T')[0];
      const dailyCounts = await store.get<Record<string, number>>('daily_meeting_counts') || {};
      dailyCounts[today] = (dailyCounts[today] || 0) + 1;
      await store.set('daily_meeting_counts', dailyCounts);
      await store.save();
    } catch (error) {
      console.error('Failed to update meeting count:', error);
    }
  }

  static async getMeetingsCountToday(): Promise<number> {
    try {
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');
      const today = new Date().toISOString().split('T')[0];
      const dailyCounts = await store.get<Record<string, number>>('daily_meeting_counts') || {};
      return dailyCounts[today] || 0;
    } catch (error) {
      console.error('Failed to get meetings count today:', error);
      return 0;
    }
  }

  // Enhanced meeting completion tracking
  static async trackMeetingCompleted(meetingId: string, metrics: {
    duration_seconds: number;
    transcript_segments: number;
    transcript_word_count: number;
    words_per_minute: number;
    meetings_today: number;
  }): Promise<void> {
    if (!this.initialized) return;

    try {
      const deviceInfo = await this.getDeviceInfo();

      await this.track('meeting_completed', {
        meeting_id: meetingId,
        duration_seconds: metrics.duration_seconds.toString(),
        transcript_segments: metrics.transcript_segments.toString(),
        transcript_word_count: metrics.transcript_word_count.toString(),
        words_per_minute: metrics.words_per_minute.toFixed(2),
        meetings_today: metrics.meetings_today.toString(),
        day_of_week: new Date().getDay().toString(),
        hour_of_day: new Date().getHours().toString(),
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version
      });

      this.meetingsInSession++;
    } catch (error) {
      console.error('Failed to track meeting completed:', error);
    }
  }

  // Copy tracking with frequency
  static async trackCopy(copyType: 'transcript' | 'summary', properties?: Record<string, any>): Promise<void> {
    if (!this.initialized) return;

    try {
      const deviceInfo = await this.getDeviceInfo();
      const { Store } = await import('@tauri-apps/plugin-store');
      const store = await Store.load('analytics.json');

      // Get today's date
      const today = new Date().toISOString().split('T')[0];
      const copyCounts = await store.get<Record<string, any>>('copy_counts') || {};
      const todayCounts = copyCounts[today] || {};
      const copyCount = todayCounts[copyType] || 0;

      // Update copy count
      todayCounts[copyType] = copyCount + 1;
      copyCounts[today] = todayCounts;
      await store.set('copy_counts', copyCounts);
      await store.save();

      const trackingProperties: AnalyticsProperties = {
        copy_type: copyType,
        copy_count_today: (copyCount + 1).toString(),
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version
      };

      // Add additional properties if provided
      if (properties) {
        Object.entries(properties).forEach(([key, value]) => {
          trackingProperties[key] = String(value);
        });
      }

      await this.track(`${copyType}_copied`, trackingProperties);
    } catch (error) {
      console.error(`Failed to track ${copyType} copy:`, error);
    }
  }

  static async trackFeatureUsed(featureName: string): Promise<void> {
    if (!this.initialized) return;

    try {
      await invoke('track_feature_used', { featureName });
    } catch (error) {
      console.error('Failed to track feature used:', error);
    }
  }

  // Convenience methods for common events
  static async trackPageView(pageName: string): Promise<void> {
    await this.track(`page_view_${pageName}`, { page: pageName });
  }

  static async trackButtonClick(buttonName: string, location?: string): Promise<void> {
    const properties: AnalyticsProperties = { button: buttonName };
    if (location) properties.location = location;
    await this.track(`button_click_${buttonName}`, properties);
  }

  static async trackError(errorType: string, errorMessage: string): Promise<void> {
    await this.track('error', {
      error_type: errorType,
      error_message: errorMessage
    });
  }

  // Reset initialization state (useful for testing)
  static reset(): void {
    this.initialized = false;
    this.initializationPromise = null;
    this.reportedTranscriptionErrorCodes.clear();
  }

  // Wait for analytics to be initialized
  static async waitForInitialization(timeout: number = 5000): Promise<boolean> {
    if (this.initialized) {
      return true;
    }

    const startTime = Date.now();
    while (!this.initialized && (Date.now() - startTime) < timeout) {
      await new Promise(resolve => setTimeout(resolve, 100));
    }

    return this.initialized;
  }

  // Track backend connection success/failure
  static async trackBackendConnection(success: boolean, error?: string) {
    // Wait for analytics to be initialized
    const isInitialized = await this.waitForInitialization();
    if (!isInitialized) {
      console.warn('Analytics not initialized within timeout, skipping backend connection tracking');
      return;
    }

    try {
      console.log('Tracking backend connection event:', { success, error });
      await invoke('track_event', {
        eventName: 'backend_connection',
        properties: {
          success: success.toString(),
          error: error || '',
          timestamp: new Date().toISOString()
        }
      });
      console.log('Backend connection event tracked successfully');
    } catch (error) {
      console.error('Failed to track backend connection:', error);
    }
  }

  private static getTranscriptionErrorCode(errorMessage: string): 'auth_rejected' | 'ort_failed' | 'transcription_failed' | null {
    const message = errorMessage.toLowerCase();
    if (/\bcool(?:ing)?[\s-]?down\b/.test(message)) return null;
    if (/http 401|unauthorized|invalid provider token|invalid api key|authentication failed/.test(message)) {
      return 'auth_rejected';
    }
    if (/ort error|onnxruntime|onnx runtime/.test(message)) {
      return 'ort_failed';
    }
    // ponytail: unknown failures share one bucket; split only when evidence requires another actionable class.
    return 'transcription_failed';
  }

  // Track each transcription error class at most once per analytics session.
  static async trackTranscriptionError(errorMessage: string) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping transcription error tracking');
      return;
    }

    const errorCode = this.getTranscriptionErrorCode(errorMessage);
    if (!errorCode || this.reportedTranscriptionErrorCodes.has(errorCode)) return;
    this.reportedTranscriptionErrorCodes.add(errorCode);

    try {
      console.log('Tracking transcription error event:', { errorCode });
      await invoke('track_event', {
        eventName: 'transcription_error',
        properties: {
          error_code: errorCode,
          timestamp: new Date().toISOString()
        }
      });
      console.log('Transcription error event tracked successfully');
    } catch (error) {
      console.error('Failed to track transcription error:', error);
    }
  }

  // Track transcription success
  static async trackTranscriptionSuccess(duration?: number) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping transcription success tracking');
      return;
    }

    try {
      console.log('Tracking transcription success event:', { duration });
      await invoke('track_event', {
        eventName: 'transcription_success',
        properties: {
          duration: duration ? duration.toString() : '',
          timestamp: new Date().toISOString()
        }
      });
      console.log('Transcription success event tracked successfully');
    } catch (error) {
      console.error('Failed to track transcription success:', error);
    }
  }

  // Summary generation analytics
  static async trackSummaryGenerationStarted(
    modelProvider: string,
    modelName: string,
    transcriptLength: number,
    timeSinceRecordingMinutes?: number
  ) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping summary generation started tracking');
      return;
    }

    try {
      const deviceInfo = await this.getDeviceInfo();
      console.log('Tracking summary generation started event:', {
        modelProvider,
        modelName,
        transcriptLength,
        timeSinceRecordingMinutes
      });

      const properties: AnalyticsProperties = {
        model_provider: modelProvider,
        model_name: modelName,
        transcript_length: transcriptLength.toString(),
        platform: deviceInfo.platform,
        os_version: deviceInfo.os_version
      };

      if (timeSinceRecordingMinutes !== undefined) {
        properties.time_since_recording_minutes = timeSinceRecordingMinutes.toFixed(2);
      }

      await this.track('summary_generation_started', properties);
      console.log('Summary generation started event tracked successfully');
    } catch (error) {
      console.error('Failed to track summary generation started:', error);
    }
  }

  static async trackSummaryGenerationCompleted(
    modelProvider: string,
    modelName: string,
    success: boolean,
    durationSeconds?: number,
    errorMessage?: string
  ) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping summary generation completed tracking');
      return;
    }

    try {
      console.log('Tracking summary generation completed event:', { modelProvider, modelName, success, durationSeconds, errorMessage });
      await invoke('track_summary_generation_completed', {
        modelProvider,
        modelName,
        success,
        durationSeconds: durationSeconds === undefined ? undefined : Math.floor(durationSeconds),
        errorMessage
      });
      console.log('Summary generation completed event tracked successfully');
    } catch (error) {
      console.error('Failed to track summary generation completed:', error);
    }
  }

  static async trackCustomPromptUsed(promptLength: number) {
    if (!this.initialized) {
      console.warn('Analytics not initialized, skipping custom prompt used tracking');
      return;
    }

    try {
      console.log('Tracking custom prompt used event:', { promptLength });
      await invoke('track_custom_prompt_used', {
        promptLength
      });
      console.log('Custom prompt used event tracked successfully');
    } catch (error) {
      console.error('Failed to track custom prompt used:', error);
    }
  }
}

export default Analytics;
