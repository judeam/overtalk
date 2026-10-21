import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { invalidateMeetingPreview, peekMeetingPreview, preloadMeeting } from '@/lib/meeting-preview';
import {
  Transcript,
  MeetingMetadata,
  PaginatedTranscriptsResponse,
  TranscriptSegmentData,
} from "@/types";

const DEFAULT_PAGE_SIZE = 100;

interface UsePaginatedTranscriptsProps {
  meetingId: string | null;
  /** Optional initial timestamp (in seconds) from URL for loading the correct page */
  initialTimestamp?: number;
  initialSegment?: string | null;
}

interface UsePaginatedTranscriptsReturn {
  metadata: MeetingMetadata | null;
  segments: TranscriptSegmentData[];
  transcripts: Transcript[];
  isLoading: boolean;
  isLoadingMore: boolean;
  hasMore: boolean;
  totalCount: number;
  loadedCount: number;
  error: string | null;

  // Actions
  loadMore: () => Promise<void>;
  reset: () => void;
  refetch: () => Promise<void>;
}

/**
 * Convert Transcript array to TranscriptSegmentData for virtualized display
 */
function convertTranscriptsToSegments(
  transcripts: Transcript[],
): TranscriptSegmentData[] {
  return transcripts.map((t) => ({
    id: t.id,
    timestamp: t.audio_start_time ?? 0,
    hasTiming:
      t.audio_start_time != null &&
      Number.isFinite(t.audio_start_time) &&
      t.audio_start_time >= 0,
    endTime: t.audio_end_time,
    text: t.text,
    confidence: t.confidence,
    speaker_id: t.speaker_id,
    speaker_name: t.speaker_name,
    speaker_confidence: t.speaker_confidence,
    overlap: t.overlap,
    is_partial: t.is_partial,
  }));
}

export function usePaginatedTranscripts({
  meetingId,
  initialSegment,
}: UsePaginatedTranscriptsProps): UsePaginatedTranscriptsReturn {
  const [metadata, setMetadata] = useState<MeetingMetadata | null>(null);
  const [transcripts, setTranscripts] = useState<Transcript[]>([]);
  const [totalCount, setTotalCount] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [isLoadingMore, setIsLoadingMore] = useState(false);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cached = meetingId && !initialSegment ? peekMeetingPreview(meetingId) : null;
  const currentMetadata = useRef(metadata);
  currentMetadata.current = metadata;

  const offsetRef = useRef(0);
  const activeMeetingIdRef = useRef<string | null>(null);
  const requestIdRef = useRef(0);
  const isLoadingRef = useRef(false);
  const lastLoadTimeRef = useRef(0); // Debounce protection

  const isCurrentRequest = useCallback(
    (requestId: number) =>
      requestIdRef.current === requestId &&
      activeMeetingIdRef.current === meetingId,
    [meetingId],
  );

  // Reset invalidates pending reads, including same-meeting refetches.
  const reset = useCallback((keepData = false, keepPending = false) => {
    if (!keepPending && activeMeetingIdRef.current) invalidateMeetingPreview(activeMeetingIdRef.current, true);
    requestIdRef.current += 1;
    isLoadingRef.current = false;
    lastLoadTimeRef.current = 0;
    if (!keepData) {
      setMetadata(null);
      setTranscripts([]);
      setTotalCount(0);
      setHasMore(false);
      offsetRef.current = 0;
    }
    setIsLoading(true);
    setIsLoadingMore(false);
    setError(null);
  }, []);

  // Load meeting metadata
  const loadMetadata = useCallback(
    async (requestId: number): Promise<MeetingMetadata | null> => {
      if (!meetingId || !isCurrentRequest(requestId)) return null;

      try {
        const data = await invoke<MeetingMetadata>("api_get_meeting_metadata", {
          meetingId,
        });
        if (!isCurrentRequest(requestId)) return null;
        setMetadata(data);
        return data;
      } catch (err) {
        if (!isCurrentRequest(requestId)) return null;
        console.error("Failed to load meeting metadata:", err);
        setError("Failed to load meeting details");
        return null;
      }
    },
    [meetingId, isCurrentRequest],
  );

  // Load transcripts at specific offset
  const loadTranscriptsAtOffset = useCallback(
    async (
      requestId: number,
      offset: number,
      append: boolean = true,
    ): Promise<Transcript[]> => {
      if (!meetingId || !isCurrentRequest(requestId)) return [];

      try {
        const response = await invoke<PaginatedTranscriptsResponse>(
          "api_get_meeting_transcripts",
          {
            meetingId,
            limit: DEFAULT_PAGE_SIZE,
            offset,
          },
        );

        if (!isCurrentRequest(requestId)) return [];
        const newTranscripts = response.transcripts;

        if (append) {
          setTranscripts((prev) => {
            if (!isCurrentRequest(requestId)) return prev;
            // Deduplicate by id
            const existingIds = new Set(prev.map((t) => t.id));
            const uniqueNew = newTranscripts.filter(
              (t) => !existingIds.has(t.id),
            );
            // Sort by audio_start_time
            return [...prev, ...uniqueNew].sort(
              (a, b) => (a.audio_start_time ?? 0) - (b.audio_start_time ?? 0),
            );
          });
        } else {
          setTranscripts(newTranscripts);
        }

        setHasMore(response.has_more);
        setTotalCount(response.total_count);
        offsetRef.current = offset + newTranscripts.length;

        return newTranscripts;
      } catch (err) {
        if (!isCurrentRequest(requestId)) return [];
        console.error("Failed to load transcripts:", err);
        setError("Failed to load transcripts");
        return [];
      }
    },
    [meetingId, isCurrentRequest],
  );

  // Load next page with debounce protection
  const loadMore = useCallback(async () => {
    const requestId = requestIdRef.current;
    if (!isCurrentRequest(requestId)) return;
    const now = Date.now();
    // Debounce: require at least 100ms between calls
    if (now - lastLoadTimeRef.current < 100) {
      return;
    }

    if (isLoadingRef.current || !hasMore || !meetingId || isLoading) return;

    lastLoadTimeRef.current = now;
    isLoadingRef.current = true;
    setIsLoadingMore(true);
    try {
      await loadTranscriptsAtOffset(requestId, offsetRef.current, true);
    } finally {
      if (isCurrentRequest(requestId)) {
        setIsLoadingMore(false);
        isLoadingRef.current = false;
      }
    }
  }, [
    hasMore,
    meetingId,
    loadTranscriptsAtOffset,
    isLoading,
    isCurrentRequest,
  ]);

  // Force refetch of data (e.g., after retranscription)
  const refetch = useCallback(async (allowCached = false) => {
    if (!meetingId || activeMeetingIdRef.current !== meetingId) return;

    reset(currentMetadata.current?.id === meetingId, true);
    const requestId = requestIdRef.current;
    try {
      if (!initialSegment) {
        const preview = await preloadMeeting(meetingId, !allowCached);
        if (!isCurrentRequest(requestId)) return;
        setMetadata(preview.metadata);
        setTranscripts(preview.page.transcripts);
        setTotalCount(preview.page.total_count);
        setHasMore(preview.page.has_more);
        offsetRef.current = preview.page.transcripts.length;
        return;
      }
      const metadataRequest = loadMetadata(requestId);
      let offset = 0;
      if (initialSegment) {
        offset = await invoke<number>("workspace_transcript_offset", {
          meetingId,
          segmentId: initialSegment,
        });
        if (!isCurrentRequest(requestId)) return;
      }
      await Promise.all([metadataRequest, loadTranscriptsAtOffset(requestId, offset, false)]);
    } catch (err) {
      if (isCurrentRequest(requestId)) setError(String(err));
    } finally {
      if (isCurrentRequest(requestId)) setIsLoading(false);
    }
  }, [
    meetingId,
    initialSegment,
    reset,
    loadMetadata,
    loadTranscriptsAtOffset,
    isCurrentRequest,
  ]);

  // A new meeting or effect lifetime owns its own requests.
  useEffect(() => {
    activeMeetingIdRef.current = meetingId;
    if (meetingId) {
      void refetch(true);
    } else {
      reset();
    }

    return () => {
      if (meetingId) invalidateMeetingPreview(meetingId, true);
      requestIdRef.current += 1;
      activeMeetingIdRef.current = null;
    };
  }, [meetingId, reset, refetch]);

  // Convert to segments (memoized)
  const visibleCached = metadata?.id !== meetingId ? cached : null;
  const visibleTranscripts = visibleCached?.page.transcripts ?? transcripts;
  const segments = useMemo(
    () => convertTranscriptsToSegments(visibleTranscripts),
    [visibleTranscripts],
  );

  return {
    metadata: visibleCached?.metadata ?? metadata,
    segments,
    transcripts: visibleTranscripts,
    isLoading: visibleCached ? false : isLoading,
    isLoadingMore,
    hasMore: visibleCached?.page.has_more ?? hasMore,
    totalCount: visibleCached?.page.total_count ?? totalCount,
    loadedCount: visibleTranscripts.length,
    error,
    loadMore,
    reset,
    refetch,
  };
}
