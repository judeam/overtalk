/**
 * Transcript Service
 *
 * Handles all transcription-related Tauri backend calls and events.
 * Owns live transcript events and complete saved-transcript reads.
 * Callers own user-facing errors; failed reads never return partial transcripts.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { TranscriptUpdate, Transcript, PaginatedTranscriptsResponse } from '@/types';
import { TranscriptSnapshot } from '@/lib/transcriptSnapshot';

export interface TranscriptionErrorPayload {
  error: string;
  userMessage: string;
  actionable: boolean;
  phase: 'startup' | 'active';
}

/**
 * Transcript Service
 * Singleton service for managing transcription operations and transcript history
 */
class TranscriptService {
  /** Fetch bounded pages, without a separate count-only request or a stale UI cache. */
  async getMeetingTranscripts(meetingId: string): Promise<Transcript[]> {
    const transcripts: Transcript[] = [];
    while (true) {
      const page = await invoke<PaginatedTranscriptsResponse>('api_get_meeting_transcripts', {
        meetingId, limit: 500, offset: transcripts.length,
      });
      if (page.has_more && page.transcripts.length === 0) {
        throw new Error('Transcript pagination made no progress');
      }
      transcripts.push(...page.transcripts);
      if (!page.has_more) return transcripts;
    }
  }

  async onTranscriptSnapshot(callback: (snapshot: TranscriptSnapshot) => void): Promise<UnlistenFn> {
    return listen<TranscriptSnapshot>('transcript-snapshot', event => callback(event.payload));
  }
  /**
   * Get transcript history from backend (for reload sync)
   * @returns Promise<Transcript[]>
   */
  async getTranscriptHistory(): Promise<Transcript[]> {
    return invoke<Transcript[]>('get_transcript_history');
  }

  // Event Listeners

  /**
   * Listen for real-time transcript updates
   * @param callback - Function to call when new transcript segment arrives
   * @returns Promise that resolves to unlisten function
   */
  async onTranscriptUpdate(callback: (update: TranscriptUpdate) => void): Promise<UnlistenFn> {
    return listen<TranscriptUpdate>('transcript-update', (event) => {
      callback(event.payload);
    });
  }
}

// Export singleton instance
export const transcriptService = new TranscriptService();
