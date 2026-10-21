import { transcriptService } from '@/services/transcriptService';
import { useCallback, RefObject } from 'react';
import { MeetingSummary, Transcript } from '@/types';
import { BlockNoteSummaryViewRef } from '@/components/AISummary/BlockNoteSummaryView';
import { toast } from 'sonner';
import { hasVisibleSummaryContent } from '@/lib/summary-content';
import { transcriptExportText } from '@/lib/meeting-export';
import { persistedSummaryMarkdown } from '@/lib/summary-export';

interface UseCopyOperationsProps {
  meeting: any;
  transcripts: Transcript[];
  meetingTitle: string;
  aiSummary: MeetingSummary | null;
  blockNoteSummaryRef: RefObject<BlockNoteSummaryViewRef>;
}

export function useCopyOperations({
  meeting,
  meetingTitle,
  aiSummary,
  blockNoteSummaryRef,
}: UseCopyOperationsProps) {

  const fetchAllTranscripts = useCallback(async (meetingId: string): Promise<Transcript[]> => {
    try {
      return await transcriptService.getMeetingTranscripts(meetingId);
    } catch (error) {
      console.error('Failed to fetch transcripts:', error);
      toast.error('Failed to fetch transcripts for copying');
      return [];
    }
  }, []);

  // Copy transcript to clipboard
  const handleCopyTranscript = useCallback(async () => {
    // CHANGE: Fetch ALL transcripts from database, not from pagination state
    console.log('📊 Fetching all transcripts for copying...');
    const allTranscripts = await fetchAllTranscripts(meeting.id);

    if (!allTranscripts.length) {
      const error_msg = 'No transcripts available to copy';
      console.log(error_msg);
      toast.error(error_msg);
      return;
    }

    console.log(`✅ Copying ${allTranscripts.length} transcripts to clipboard`);

    const header = `# Transcript of the Meeting: ${meeting.id} - ${meetingTitle ?? meeting.title}\n\n`;
    const date = `## Date: ${new Date(meeting.created_at).toLocaleDateString()}\n\n`;
    const fullTranscript = allTranscripts
      .map(transcriptExportText)
      .join('\n');

    await navigator.clipboard.writeText(header + date + fullTranscript);
    toast.success("Transcript copied to clipboard");
  }, [meeting, meetingTitle, fetchAllTranscripts]);

  // Copy summary to clipboard
  const handleCopySummary = useCallback(async () => {
    if (!hasVisibleSummaryContent(aiSummary)) {
      toast.error('No summary content available to copy');
      return;
    }
    try {
      let summaryMarkdown = '';

      console.log('🔍 Copy Summary - Starting...');

      // Try to get markdown from BlockNote editor first
      if (blockNoteSummaryRef.current?.getMarkdown) {
        console.log('📝 Trying to get markdown from ref...');
        summaryMarkdown = await blockNoteSummaryRef.current.getMarkdown();
        console.log('📝 Got markdown from ref, length:', summaryMarkdown.length);
      }

      if (!summaryMarkdown && !blockNoteSummaryRef.current?.isDirty) {
        summaryMarkdown = await persistedSummaryMarkdown(aiSummary);
      }

      // If still no summary content, show message
      if (!summaryMarkdown.trim()) {
        console.error('❌ No summary content available to copy');
        toast.error('No summary content available to copy');
        return;
      }

      // Build metadata header
      const header = `# Meeting Summary: ${meetingTitle}\n\n`;
      const metadata = `**Meeting ID:** ${meeting.id}\n**Date:** ${new Date(meeting.created_at).toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
      })}\n**Copied on:** ${new Date().toLocaleDateString('en-US', {
        year: 'numeric',
        month: 'long',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
      })}\n\n---\n\n`;

      const fullMarkdown = header + metadata + summaryMarkdown;
      await navigator.clipboard.writeText(fullMarkdown);

      console.log('✅ Successfully copied to clipboard!');
      toast.success("Summary copied to clipboard");
    } catch (error) {
      console.error('❌ Failed to copy summary:', error);
      toast.error("Failed to copy summary");
    }
  }, [aiSummary, meetingTitle, meeting, blockNoteSummaryRef]);

  return {
    handleCopyTranscript,
    handleCopySummary,
  };
}
