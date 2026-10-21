import { Transcript } from '@/types';

export interface TranscriptSnapshot {
  session_id: string;
  revision: number;
  is_partial: boolean;
  segments: Array<{
    id: string; text: string; display_time: string;
    audio_start_time: number; audio_end_time: number; duration: number;
    sequence_id: number; confidence?: number | null;
    speaker_id?: string | null; speaker_name?: string | null;
    speaker_confidence?: number | null; overlap?: boolean; is_partial?: boolean;
  }>;
}

/** A cumulative model revision replaces its predecessors, including removed
 * provisional words. Never append an entire snapshot to the previous text. */
export function transcriptsFromSnapshot(snapshot: TranscriptSnapshot): Transcript[] {
  const seen = new Set<string>();
  return snapshot.segments.map(segment => {
    if (!segment.id || seen.has(segment.id) || !Number.isFinite(segment.audio_start_time)
      || !Number.isFinite(segment.audio_end_time) || segment.audio_start_time < 0
      || segment.audio_end_time <= segment.audio_start_time) {
      throw new Error('Invalid live transcript snapshot');
    }
    seen.add(segment.id);
    return {
      id: segment.id, text: segment.text, timestamp: segment.display_time,
      sequence_id: segment.sequence_id, chunk_start_time: segment.audio_start_time,
      audio_start_time: segment.audio_start_time, audio_end_time: segment.audio_end_time,
      duration: segment.audio_end_time - segment.audio_start_time,
      confidence: segment.confidence ?? undefined,
      speaker_id: segment.speaker_id ?? undefined, speaker_name: segment.speaker_name ?? undefined,
      speaker_confidence: segment.speaker_confidence ?? undefined, overlap: segment.overlap ?? false,
      is_partial: snapshot.is_partial,
    };
  });
}
