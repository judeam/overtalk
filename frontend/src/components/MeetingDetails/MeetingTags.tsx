"use client";
import { useEffect, useState, FormEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import Link from "next/link";
import { toast } from "sonner";

export function MeetingTags({ meetingId }: { meetingId: string }) {
  const [tags, setTags] = useState("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    invoke<string[]>("workspace_meeting_tags", { meetingId })
      .then((v) => { if (active) setTags((v ?? []).join(", ")); })
      .catch((e) => { if (active) setError(String(e)); });
    return () => { active = false; };
  }, [meetingId]);
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      await invoke("workspace_set_tags", { meetingId, tags: tags.split(",").map((t) => t.trim()).filter(Boolean) });
      toast.success("Tags saved");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="ot-section" aria-labelledby={`tags-title-${meetingId}`}>
      <h4 className="ot-section-title" id={`tags-title-${meetingId}`}>Tags</h4>
      <form className="ot-inline-form" onSubmit={save}>
        <label className="sr-only" htmlFor={`tags-${meetingId}`}>Tags, separated by commas</label>
        <input id={`tags-${meetingId}`} className="ot-input" value={tags} onChange={(e) => setTags(e.target.value)} placeholder="design, weekly review" />
        <button type="submit" className="ot-button ot-button-secondary ot-button-small" disabled={busy}>{busy ? "Saving…" : "Save tags"}</button>
      </form>
      <Link className="ot-link" href={`/library?meeting=${encodeURIComponent(meetingId)}`}>Ask a question about this meeting</Link>
      {error && <p role="alert" className="ot-danger-text">{error}</p>}
    </section>
  );
}
