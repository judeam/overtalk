-- Paginated transcripts and per-meeting duration lookups must not scan/sort a library.
CREATE INDEX transcripts_meeting_time ON transcripts(meeting_id, audio_start_time, id);
CREATE INDEX transcripts_meeting_end ON transcripts(meeting_id, audio_end_time);
CREATE INDEX speaker_turns_meeting_end ON speaker_turns(meeting_id, end_time);
