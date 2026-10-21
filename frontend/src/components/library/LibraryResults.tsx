"use client";
import { useEffect, useMemo, useRef, useState } from "react";
import Link from "next/link";
import {
  ArrowLeft,
  ArrowUpRight,
  Check,
  Copy,
  FileText,
  MessageSquare,
  Search,
} from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { toast } from "sonner";
import { HighlightedText } from "@/components/session/HighlightedText";
import { sourceTime, sourceUrl, type MeetingSource } from "@/lib/workspace";

const labelFor = (source: MeetingSource) =>
  source.source_kind === "transcript"
    ? "Transcript"
    : source.source_kind === "meeting"
      ? "Meeting"
      : "Notes";
const dateFor = (date: string) => {
  const value = new Date(date);
  return Number.isNaN(value.getTime())
    ? ""
    : value.toLocaleDateString(undefined, {
        day: "numeric",
        month: "short",
        year: "numeric",
      });
};
const excerpt = (text: string) =>
  text
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/\*\*|__/g, "")
    .replace(/\[S\d+\]/g, "")
    .replace(/\s+/g, " ")
    .trim();

/** Group context once per meeting; keep the selected passage readable alongside results. */
export function LibraryResults({
  sources,
  query,
  loading,
  meetingCount,
}: {
  sources: MeetingSource[];
  query: string;
  loading: boolean;
  meetingCount: number;
}) {
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const preview = useRef<HTMLElement>(null);
  const groups = useMemo(() => {
    const map = new Map<
      string,
      { title: string; date: string; sources: MeetingSource[] }
    >();
    for (const source of sources) {
      const group = map.get(source.meeting_id) ?? {
        title: source.title,
        date: source.recorded_at,
        sources: [],
      };
      group.sources.push(source);
      map.set(source.meeting_id, group);
    }
    return [...map.entries()].map(([id, group]) => ({
      id,
      ...group,
      sources: group.sources.some((s) => s.source_kind !== "meeting")
        ? group.sources.filter((s) => s.source_kind !== "meeting")
        : group.sources,
    }));
  }, [sources]);
  const passages = groups.flatMap((group) => group.sources);
  const selected =
    passages.find((source) => source.id === selectedId) ?? passages[0];
  useEffect(() => {
    setCopied(false);
  }, [selected?.id]);
  const showReading =
    selectedId !== null && passages.some((source) => source.id === selectedId);
  const select = (source: MeetingSource) => {
    setSelectedId(source.id);
    setCopied(false);
    requestAnimationFrame(() => {
      if (window.innerWidth < 1100) preview.current?.focus();
    });
  };
  if (loading && !sources.length)
    return (
      <div className="library-loading" role="status">
        <span className="ot-spinner" />
        Loading your meetings…
      </div>
    );
  if (!sources.length)
    return (
      <div className="library-empty-state">
        <Search size={28} aria-hidden />
        <h2>
          {query
            ? "No matching passages"
            : meetingCount
              ? "Your library is ready to search"
              : "Your meetings will live here"}
        </h2>
        <p>
          {query
            ? "Try a shorter phrase, another spelling, or clear your filters."
            : meetingCount
              ? "Search for a name, a decision, or a topic from your meetings."
              : "Record or import a meeting. Its transcript and notes will be searchable here."}
        </p>
        {!query && (
          <Link className="ot-button ot-button-primary" href="/">
            {meetingCount ? "Open meetings" : "Record a meeting"}
            <ArrowUpRight size={17} />
          </Link>
        )}
      </div>
    );
  return (
    <section
      className="library-browser"
      data-reading={showReading}
      aria-label="Search results"
    >
      <div className="library-results-list">
        <header className="library-results-heading">
          <h2>{query ? "Matching passages" : "Recent meetings"}</h2>
          <span>
            {groups.length} {groups.length === 1 ? "meeting" : "meetings"} ·{" "}
            {passages.length} {passages.length === 1 ? "passage" : "passages"}
          </span>
        </header>
        <div className="library-results-scroll">
          {groups.map((group) => (
            <section className="library-meeting-group" key={group.id}>
              <header>
                <h3>{group.title}</h3>
                <time>{dateFor(group.date)}</time>
              </header>
              {group.sources.map((source) => (
                <button
                  type="button"
                  key={source.id}
                  className="library-passage"
                  data-kind={source.source_kind}
                  aria-pressed={selected.id === source.id}
                  onClick={() => select(source)}
                >
                  <span className="library-passage-meta">
                    {source.source_kind === "transcript" ? (
                      <MessageSquare size={15} />
                    ) : (
                      <FileText size={15} />
                    )}
                    {labelFor(source)}
                    {source.start_time != null && (
                      <span className="ot-time">
                        {sourceTime(source.start_time)}
                      </span>
                    )}
                    <ArrowUpRight className="library-passage-arrow" size={16} />
                  </span>
                  <span className="library-passage-excerpt">
                    <HighlightedText
                      text={
                        source.source_kind === "meeting"
                          ? "Open this meeting to review its transcript and notes."
                          : excerpt(source.body)
                      }
                      query={query}
                    />
                  </span>
                </button>
              ))}
            </section>
          ))}
        </div>
      </div>
      <section
        className="library-preview"
        data-kind={selected.source_kind}
        ref={preview}
        tabIndex={-1}
        aria-label="Selected passage"
        onKeyDown={(event) => {
          if (event.key === "Escape") setSelectedId(null);
        }}
      >
        <button
          type="button"
          className="library-back-results ot-tool"
          onClick={() => setSelectedId(null)}
        >
          <ArrowLeft size={16} />
          Back to results
        </button>
        <header className="library-preview-header">
          <span className="library-preview-type">
            {labelFor(selected)}
            {selected.start_time != null && (
              <>
                {" "}
                ·{" "}
                <span className="ot-time">
                  {sourceTime(selected.start_time)}
                </span>
              </>
            )}
          </span>
          <h2>{selected.title}</h2>
          <p>{dateFor(selected.recorded_at)}</p>
        </header>
        <div className="library-preview-body">
          {selected.source_kind === "transcript" ? (
            <p className="library-transcript-passage">
              <HighlightedText text={selected.body} query={query} />
            </p>
          ) : selected.source_kind === "meeting" ? (
            <p>
              This meeting is saved in your workspace. Open it to listen, review
              the transcript, or write notes.
            </p>
          ) : (
            <ReactMarkdown remarkPlugins={[remarkGfm]}>
              {selected.body}
            </ReactMarkdown>
          )}
        </div>
        <footer className="library-preview-actions">
          <Link
            className="ot-button ot-button-primary"
            href={sourceUrl(selected)}
          >
            {selected.start_time != null
              ? `Open at ${sourceTime(selected.start_time)}`
              : "Open meeting"}
            <ArrowUpRight size={17} />
          </Link>
          <button
            type="button"
            className="ot-button ot-button-secondary"
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(selected.body);
                setCopied(true);
              } catch {
                toast.error("Could not copy the passage");
              }
            }}
          >
            {copied ? <Check size={17} /> : <Copy size={17} />}
            {copied ? "Copied" : "Copy passage"}
          </button>
        </footer>
      </section>
    </section>
  );
}
