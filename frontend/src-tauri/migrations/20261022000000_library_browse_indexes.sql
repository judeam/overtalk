-- Browsing recent passages should stop after the requested page, rather than
-- sorting the whole library. Keep the full-text ranking path unchanged.
CREATE INDEX workspace_documents_recent
    ON workspace_documents(recorded_at DESC, start_time, id)
    WHERE length(body)>0;
CREATE INDEX workspace_documents_meeting_recent
    ON workspace_documents(meeting_id, recorded_at DESC, start_time, id)
    WHERE length(body)>0;
