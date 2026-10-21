'use client';

import { useState, useEffect, useMemo } from 'react';
import { RecordingControls } from '@/components/RecordingControls';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import { useAudioDeviceLabels } from '@/hooks/useAudioDeviceLabels';
import { usePermissionCheck } from '@/hooks/usePermissionCheck';
import { useRecordingState, RecordingStatus } from '@/contexts/RecordingStateContext';
import { useTranscripts } from '@/contexts/TranscriptContext';
import { useConfig } from '@/contexts/ConfigContext';
import { StatusOverlays } from '@/app/_components/StatusOverlays';
import Analytics from '@/lib/analytics';
import { SettingsModals } from './_components/SettingsModal';
import { LiveSession } from '@/components/session/LiveSession';
import { PermissionWarning } from '@/components/PermissionWarning';
import { useIsLinux } from '@/hooks/usePlatform';
import { clock, defaultMeetingTitle, isDefaultMeetingTitle, toneOf } from '@/lib/session';
import { useModalState } from '@/hooks/useModalState';
import { useRecordingStateSync } from '@/hooks/useRecordingStateSync';
import { useRecordingStart } from '@/hooks/useRecordingStart';
import { useRecordingStop } from '@/hooks/useRecordingStop';
import { useTranscriptRecovery } from '@/hooks/useTranscriptRecovery';
import { TranscriptRecovery } from '@/components/TranscriptRecovery';
import { indexedDBService } from '@/services/indexedDBService';
import { toast } from 'sonner';
import { Copy, Globe, Mic, Upload } from 'lucide-react';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useGuide } from '@/components/guide/GuideProvider';
import { useRouter } from 'next/navigation';
import { RecordingReadiness } from '@/components/session/RecordingReadiness';

export default function Home() {
  // Local page state (not moved to contexts)
  const [isRecording, setIsRecordingState] = useState(false);
  const barHeights = ['12px', '22px', '16px'];
  const [showRecoveryDialog, setShowRecoveryDialog] = useState(false);

  // Use contexts for state management
  const { meetingTitle, setMeetingTitle, transcripts, copyTranscript } = useTranscripts();
  const { transcriptModelConfig, selectedDevices, betaFeatures } = useConfig();
  const { openImportDialog } = useImportDialog();
  const guide = useGuide();
  const recordingState = useRecordingState();
  const { microphoneLabel, systemAudioLabel } = useAudioDeviceLabels(selectedDevices.micDevice, selectedDevices.systemDevice);

  // Extract status from global state
  const { status, isProcessing } = recordingState;

  // Hooks
  const { hasMicrophone, hasSystemAudio, checkPermissions, isChecking } = usePermissionCheck();
  const isLinux = useIsLinux();
  const { setIsMeetingActive, isCollapsed: sidebarCollapsed, refetchMeetings } = useSidebar();
  const { modals, messages, showModal, hideModal } = useModalState(transcriptModelConfig);
  const { isRecordingDisabled, setIsRecordingDisabled } = useRecordingStateSync(isRecording, setIsRecordingState, setIsMeetingActive);
  const { handleRecordingStart } = useRecordingStart(isRecording, setIsRecordingState, showModal);

  // Get handleRecordingStop function and setIsStopping (state comes from global context)
  const { handleRecordingStop, setIsStopping } = useRecordingStop(
    setIsRecordingState,
    setIsRecordingDisabled
  );

  // Recovery hook
  const {
    recoverableMeetings,
    checkForRecoverableTranscripts,
    recoverMeeting,
    loadMeetingTranscripts,
    deleteRecoverableMeeting
  } = useTranscriptRecovery();

  const router = useRouter();

  useEffect(() => {
    // Track page view
    Analytics.trackPageView('home');
  }, []);

  // Before a recording, the title field shows the name the recording will get.
  useEffect(() => {
    if (!recordingState.isRecording && transcripts.length === 0 && isDefaultMeetingTitle(meetingTitle)) {
      setMeetingTitle(defaultMeetingTitle());
    }
  }, [recordingState.isRecording]); // eslint-disable-line react-hooks/exhaustive-deps

  // Startup recovery check
  useEffect(() => {
    const performStartupChecks = async () => {
      try {
        // Skip recovery check if currently recording or processing stop
        // This prevents the recovery dialog from showing when:
        if (recordingState.isRecording ||
          status === RecordingStatus.STOPPING ||
          status === RecordingStatus.PROCESSING_TRANSCRIPTS ||
          status === RecordingStatus.SAVING) {
          console.log('Skipping recovery check - recording in progress or processing');
          return;
        }

        // 1. Clean up old meetings (7+ days)
        try {
          await indexedDBService.deleteOldMeetings(7);
        } catch (error) {
          console.warn('⚠️ Failed to clean up old meetings:', error);
        }

        // 2. Clean up saved meetings (24+ hours after save)
        try {
          await indexedDBService.deleteSavedMeetings(24);
        } catch (error) {
          console.warn('⚠️ Failed to clean up saved meetings:', error);
        }

        // 3. Always check for recoverable meetings on startup
        // Don't skip based on sessionStorage - we need to check every time
        await checkForRecoverableTranscripts();
      } catch (error) {
        console.error('Failed to perform startup checks:', error);
      }
    };

    performStartupChecks();
  }, [checkForRecoverableTranscripts, recordingState.isRecording, status]);

  // Watch for recoverable meetings changes and show dialog once per session
  useEffect(() => {
    // Only show dialog if we have meetings and haven't shown it yet this session
    if (recoverableMeetings.length > 0) {
      const shownThisSession = sessionStorage.getItem('recovery_dialog_shown');
      if (!shownThisSession) {
        setShowRecoveryDialog(true);
        sessionStorage.setItem('recovery_dialog_shown', 'true');
      }
    }
  }, [recoverableMeetings]);

  // Handle recovery with toast notifications and navigation
  const handleRecovery = async (meetingId: string) => {
    try {
      const result = await recoverMeeting(meetingId);

      if (result.success) {
        toast.success('Meeting recovered successfully!', {
          description: result.audioRecoveryStatus?.status === 'success'
            ? 'Transcripts and audio recovered'
            : 'Transcripts recovered (no audio available)',
          action: result.meetingId ? {
            label: 'View Meeting',
            onClick: () => {
              router.push(`/meeting-details?id=${result.meetingId}`);
            }
          } : undefined,
          duration: 10000,
        });

        // Refresh sidebar to show the newly recovered meeting
        await refetchMeetings();

        // If no more recoverable meetings, clear session flag so dialog can show again
        if (recoverableMeetings.length === 0) {
          sessionStorage.removeItem('recovery_dialog_shown');
        }

        // Auto-navigate after a short delay
        if (result.meetingId) {
          setTimeout(() => {
            router.push(`/meeting-details?id=${result.meetingId}`);
          }, 2000);
        }
      }
    } catch (error) {
      toast.error('Failed to recover meeting', {
        description: error instanceof Error ? error.message : 'Unknown error occurred',
      });
      throw error;
    }
  };

  // Handle dialog close - clear session flag if no meetings left
  const handleDialogClose = () => {
    setShowRecoveryDialog(false);
    // If user closes dialog and there are no more meetings, clear the flag
    // This allows the dialog to show again next session if new meetings appear
    if (recoverableMeetings.length === 0) {
      sessionStorage.removeItem('recovery_dialog_shown');
    }
  };

  // Computed values using global status
  const isProcessingStop = status === RecordingStatus.PROCESSING_TRANSCRIPTS || isProcessing;

  const liveSpeakers = useMemo(() => {
    const order: { id: string; name: string; seconds: number }[] = [];
    for (const t of transcripts) {
      if (!t.speaker_id || t.audio_start_time == null) continue;
      let row = order.find((s) => s.id === t.speaker_id);
      if (!row) { row = { id: t.speaker_id, name: t.speaker_name || `Speaker ${order.length + 1}`, seconds: 0 }; order.push(row); }
      row.seconds += Math.max(0, (t.audio_end_time ?? t.audio_start_time) - t.audio_start_time);
    }
    return order;
  }, [transcripts]);

  const startPanel = (
    <div className="ot-start">
      <h2>Record a meeting</h2>
      <p>Capture your microphone and call audio, then review the transcript and turn it into useful notes.</p>
      {!isLinux && !isChecking && (
        <PermissionWarning hasMicrophone={hasMicrophone} hasSystemAudio={hasSystemAudio} onRecheck={checkPermissions} isRechecking={isChecking} />
      )}
      <RecordingReadiness onSoundCheck={() => { setMeetingTitle('Sound check'); window.dispatchEvent(new CustomEvent('start-recording-from-sidebar')); }} />
      <div className="ot-start-actions">
        <button type="button" className="ot-button ot-button-primary" onClick={() => window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'))} disabled={isRecordingDisabled || !hasMicrophone}>
          <span className="ot-record-dot ot-record-dot-small" aria-hidden="true" /><span>Record</span>
        </button>
        <button type="button" className="ot-button ot-button-secondary" onClick={() => showModal('deviceSettings')} disabled={isRecordingDisabled}>
          <Mic aria-hidden="true" /><span>Audio sources</span>
        </button>
        {betaFeatures.importAndRetranscribe && (
          <button type="button" className="ot-button ot-button-secondary" onClick={() => openImportDialog()}>
            <Upload aria-hidden="true" /><span>Import audio</span>
          </button>
        )}
      </div>
      <p className="ot-start-help">
        New here? <button type="button" className="ot-link" onClick={() => guide.startTour('home')}>Take the one-minute tour</button>
        {' '}or <button type="button" className="ot-link" onClick={() => guide.open()}>open the guide</button>.
      </p>
    </div>
  );

  return (
    <>
      <SettingsModals modals={modals} messages={messages} onClose={hideModal} />
      <TranscriptRecovery
        isOpen={showRecoveryDialog}
        onClose={handleDialogClose}
        recoverableMeetings={recoverableMeetings}
        onRecover={handleRecovery}
        onDelete={deleteRecoverableMeeting}
        onLoadPreview={loadMeetingTranscripts}
      />
      <LiveSession
        renderControls={(clock) => (
          <RecordingControls
            variant="transport"
            clock={clock}
            isRecording={recordingState.isRecording}
            onRecordingStop={(callApi = true) => handleRecordingStop(callApi)}
            onRecordingStart={handleRecordingStart}
            onTranscriptReceived={() => { }}
            onStopInitiated={() => setIsStopping(true)}
            barHeights={barHeights}
            onTranscriptionError={(message) => showModal('errorAlert', message)}
            isRecordingDisabled={isRecordingDisabled || (!hasMicrophone && !recordingState.isRecording)}
            isParentProcessing={isProcessingStop}
            selectedDevices={selectedDevices}
            meetingName={meetingTitle}
          />
        )}
        isRecording={recordingState.isRecording}
        isPaused={recordingState.isPaused}
        reportedSeconds={recordingState.activeDuration ?? recordingState.recordingDuration}
        title={meetingTitle}
        onRename={setMeetingTitle}
        transcripts={transcripts}
        microphoneLabel={microphoneLabel}
        systemLabel={systemAudioLabel}
        sourcesLocked={recordingState.isRecording}
        onSources={() => showModal('deviceSettings')}
        startPanel={startPanel}
        tools={(
          <>
            {transcripts.length > 0 && <button type="button" className="ot-tool" onClick={copyTranscript}><Copy aria-hidden="true" />Copy transcript</button>}
            {transcriptModelConfig.provider === 'localWhisper' && <button type="button" className="ot-tool" onClick={() => showModal('languageSettings')}><Globe aria-hidden="true" />Language</button>}
          </>
        )}
        speakersPanel={(
          <div className="ot-speakers-panel">
            {liveSpeakers.length === 0 ? (
              <p className="ot-note-text">Voices appear here as people speak. Labels during a call are provisional; you can name and merge speakers once the meeting is saved.</p>
            ) : (
              <>
                <ul className="ot-speaker-list">
                  {liveSpeakers.map((s, i) => (
                    <li key={s.id} className={`ot-speaker-row ot-tone-${toneOf(i)}`}>
                      <span className="ot-line-dot" aria-hidden="true" /><span>{s.name}</span><span className="ot-time">{clock(s.seconds)}</span>
                    </li>
                  ))}
                </ul>
                <p className="ot-note-text">Labels during a call are provisional. Name and merge speakers once the meeting is saved.</p>
              </>
            )}
          </div>
        )}
      />
      <StatusOverlays
        isProcessing={status === RecordingStatus.PROCESSING_TRANSCRIPTS && !recordingState.isRecording}
        isSaving={status === RecordingStatus.SAVING}
        sidebarCollapsed={sidebarCollapsed}
      />
    </>
  );
}
