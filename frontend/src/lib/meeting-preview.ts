import { invoke } from '@tauri-apps/api/core';
import type { MeetingMetadata, PaginatedTranscriptsResponse } from '@/types';

export interface MeetingPreview {
  metadata: MeetingMetadata;
  page: PaginatedTranscriptsResponse;
}
interface Entry {
  promise: Promise<MeetingPreview>;
  data?: MeetingPreview;
  expires: number;
  chars: number;
}
// Only first pages, never complete transcripts or audio. Limit both item count
// and text size, since one imported transcript line can itself be very large.
const previews = new Map<string, Entry>();
const MAX_ENTRIES = 6;
const MAX_CHARS = 500_000;
const TTL_MS = 30_000;

function trim() {
  let chars = [...previews.values()].reduce((sum, entry) => sum + entry.chars, 0);
  while (previews.size > MAX_ENTRIES || chars > MAX_CHARS) {
    const oldest = previews.keys().next().value as string;
    chars -= previews.get(oldest)!.chars;
    previews.delete(oldest);
  }
}
export function peekMeetingPreview(meetingId: string): MeetingPreview | null {
  const entry = previews.get(meetingId);
  if (!entry || entry.expires <= Date.now()) {
    if (entry) previews.delete(meetingId);
    return null;
  }
  return entry.data ?? null;
}
export function invalidateMeetingPreview(meetingId?: string, pendingOnly = false) {
  if (meetingId) {
    if (!pendingOnly || !previews.get(meetingId)?.data) previews.delete(meetingId);
  }
  else previews.clear();
}
export function preloadMeeting(meetingId: string, refresh = false): Promise<MeetingPreview> {
  const cached = previews.get(meetingId);
  if (!refresh && cached && cached.expires > Date.now()) {
    previews.delete(meetingId); previews.set(meetingId, cached);
    return cached.promise;
  }
  let entry: Entry;
  const promise = Promise.all([
    invoke<MeetingMetadata>('api_get_meeting_metadata', { meetingId }),
    invoke<PaginatedTranscriptsResponse>('api_get_meeting_transcripts', { meetingId, limit: 100, offset: 0 }),
  ]).then(([metadata, page]) => {
    if (metadata.id !== meetingId) throw new Error('Meeting response did not match the requested meeting');
    const data = { metadata, page };
    if (previews.get(meetingId) === entry) {
      entry.data = data;
      entry.chars = metadata.title.length + page.transcripts.reduce((sum, line) => sum + line.text.length, 0);
      entry.expires = Date.now() + TTL_MS;
      if (entry.chars > MAX_CHARS) previews.delete(meetingId);
      else trim();
    }
    return data;
  }).catch(error => {
    if (previews.get(meetingId) === entry) previews.delete(meetingId);
    throw error;
  });
  entry = { promise, expires: Date.now() + TTL_MS, chars: 0 };
  previews.delete(meetingId); previews.set(meetingId, entry); trim();
  return promise;
}
