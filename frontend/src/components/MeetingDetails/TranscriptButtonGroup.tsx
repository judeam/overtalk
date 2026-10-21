"use client";

import { useState, useCallback } from 'react';
import { Copy, FolderOpen, RefreshCw } from 'lucide-react';
import { RetranscribeDialog } from './RetranscribeDialog';
import { useConfig } from '@/contexts/ConfigContext';


interface TranscriptButtonGroupProps {
  transcriptCount: number;
  onCopyTranscript: () => void;
  onOpenMeetingFolder: () => Promise<void>;
  meetingId?: string;
  meetingFolderPath?: string | null;
  onRefetchTranscripts?: () => Promise<void>;
}


export function TranscriptButtonGroup({
  transcriptCount,
  onCopyTranscript,
  onOpenMeetingFolder,
  meetingId,
  meetingFolderPath,
  onRefetchTranscripts,
}: TranscriptButtonGroupProps) {
  const { betaFeatures } = useConfig();
  const [showRetranscribeDialog, setShowRetranscribeDialog] = useState(false);

  const handleRetranscribeComplete = useCallback(async () => {
    // Refetch transcripts to show the updated data
    if (onRefetchTranscripts) {
      await onRefetchTranscripts();
    }
  }, [onRefetchTranscripts]);

  return (
    <div className="ot-tool-row">
      <button type="button" className="ot-tool" disabled={transcriptCount === 0}
        onClick={() => { onCopyTranscript(); }}>
        <Copy aria-hidden="true" />Copy transcript
      </button>
      <button type="button" className="ot-tool"
        onClick={() => { void onOpenMeetingFolder(); }}>
        <FolderOpen aria-hidden="true" />Show recording folder
      </button>
      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <button type="button" className="ot-tool" title="Transcribe the recording again with the current model"
          onClick={() => { setShowRetranscribeDialog(true); }}>
          <RefreshCw aria-hidden="true" />Transcribe again
        </button>
      )}
      {betaFeatures.importAndRetranscribe && meetingId && meetingFolderPath && (
        <RetranscribeDialog
          open={showRetranscribeDialog}
          onOpenChange={setShowRetranscribeDialog}
          meetingId={meetingId}
          meetingFolderPath={meetingFolderPath}
          onComplete={handleRetranscribeComplete}
        />
      )}
    </div>
  );
}
