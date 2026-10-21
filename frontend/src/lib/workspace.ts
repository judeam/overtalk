export interface MeetingSource {
  id: string;
  meeting_id: string;
  source_kind: string;
  source_id: string;
  title: string;
  body: string;
  start_time: number | null;
  end_time: number | null;
  recorded_at: string;
  fingerprint: string;
}
export interface SearchRequest {
  query: string;
  meeting_id?: string | null;
  after?: string | null;
  before?: string | null;
  tag?: string | null;
  limit?: number;
  semantic?: boolean;
}
export interface SearchResponse {
  sources: MeetingSource[];
  mode: string;
  warnings: string[];
}
export interface CitedClaim {
  text: string;
  citations: { id: string; quote: string }[];
}
export interface CitedAnswer {
  id: string;
  question: string;
  abstain: boolean;
  claims: CitedClaim[];
  sources: MeetingSource[];
  stale: boolean;
  provider: string;
}
export interface WorkspaceTask {
  id: string;
  meeting_id: string | null;
  title: string;
  owner: string;
  due_date: string | null;
  status: "open" | "done" | "cancelled";
  evidence_json: string;
  created_at: string;
  updated_at: string;
}
export interface CalendarEvent {
  id: string;
  title: string;
  description: string;
  starts_at: string;
  ends_at: string;
  all_day: boolean;
  timezone: string;
  location: string;
  meeting_url: string | null;
  source: string;
}
export function sourceUrl(source: MeetingSource): string {
  const params = new URLSearchParams({ id: source.meeting_id });
  if (source.source_kind === "transcript")
    params.set("segment", source.source_id);
  if (
    source.start_time != null &&
    Number.isFinite(source.start_time) &&
    source.start_time >= 0
  )
    params.set("at", String(source.start_time));
  return `/meeting-details?${params}`;
}
export function sourceTime(seconds: number | null): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0)
    return "Open notes";
  const total = Math.floor(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
export function taskEvidence(task: WorkspaceTask): MeetingSource[] {
  try {
    const parsed: unknown = JSON.parse(task.evidence_json);
    return Array.isArray(parsed)
      ? parsed.filter(
          (s) =>
            s && typeof s.id === "string" && typeof s.meeting_id === "string",
        )
      : [];
  } catch {
    return [];
  }
}
