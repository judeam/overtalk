"use client";
import { LibraryResults } from "@/components/library/LibraryResults";
import { TrashPanel } from "@/components/TrashPanel";
import { useState, useEffect, useRef, FormEvent, Suspense } from "react";
import Link from "next/link";
import { useSearchParams } from "next/navigation";
import { useSidebar } from "@/components/Sidebar/SidebarProvider";
import { invoke } from "@tauri-apps/api/core";
import {
  Search,
  ArrowUpRight,
  Square,
  Plus,
  Check,
  Trash2,
  CalendarDays,
  ListChecks,
  BookOpen,
  X,
  Settings2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import {
  CitedAnswer,
  CitedClaim,
  MeetingSource,
  SearchRequest,
  SearchResponse,
  WorkspaceTask,
  CalendarEvent,
  sourceUrl,
  sourceTime,
  taskEvidence,
} from "@/lib/workspace";
import { toast } from "sonner";

type Draft = {
  id?: string;
  title: string;
  owner: string;
  due_date: string;
  meeting_id: string | null;
  evidence: MeetingSource[];
};
const emptyDraft: Draft = {
  title: "",
  owner: "",
  due_date: "",
  meeting_id: null,
  evidence: [],
};
function SourceLink({
  source,
  quote,
}: {
  source: MeetingSource;
  quote?: string;
}) {
  return (
    <Link className="library-citation" href={sourceUrl(source)}>
      <span>
        {source.title} · {sourceTime(source.start_time)}
      </span>
      {quote && <q>{quote}</q>}
      <ArrowUpRight size={15} aria-hidden />
    </Link>
  );
}
function LibraryContent() {
  const routeParams = useSearchParams();
  const meetingCount = useSidebar().meetings.length;
  const [tab, setTab] = useState("search");
  const [searchKind, setSearchKind] = useState<"search" | "ask">("search");
  const [scope, setScope] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [submittedQuery, setSubmittedQuery] = useState("");
  const [after, setAfter] = useState("");
  const [before, setBefore] = useState("");
  const [tag, setTag] = useState("");
  const [tags, setTags] = useState<{ tag: string; count: number }[]>([]);
  const [semantic, setSemantic] = useState(true);
  const [results, setResults] = useState<SearchResponse | null>(null);
  const [answer, setAnswer] = useState<CitedAnswer | null>(null);
  const [history, setHistory] = useState<CitedAnswer[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<"search" | "ask" | null>(null);
  const [error, setError] = useState("");
  const [tasks, setTasks] = useState<WorkspaceTask[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [calendar, setCalendar] = useState<CalendarEvent[]>([]);
  const [calendarBusy, setCalendarBusy] = useState(false);
  const job = useRef<string | null>(null);
  const generation = useRef(0);
  const mounted = useRef(true);
  useEffect(() => {
    const requested = routeParams.get('tab') || 'search';
    if (['search','actions','calendar','trash'].includes(requested)) setTab(requested);
  }, [routeParams]);
  const refreshTasks = async () =>
    setTasks(await invoke<WorkspaceTask[]>("workspace_tasks"));
  useEffect(() => {
    mounted.current = true;
    const params = new URLSearchParams(window.location.search);
    const meeting = params.get("meeting");
    setScope(meeting);
    if (params.get("tab")) setTab(params.get("tab")!);
    Promise.all([
      invoke<SearchResponse>("workspace_search", {
        request: { query: "", meeting_id: meeting, limit: 24 },
      }),
      invoke<{ tag: string; count: number }[]>("workspace_tags"),
      invoke<CitedAnswer[]>("workspace_answers"),
      invoke<WorkspaceTask[]>("workspace_tasks"),
      invoke<CalendarEvent[]>("workspace_calendar_events"),
    ])
      .then(([r, t, h, a, c]) => {
        if (mounted.current) {
          setResults(r);
          setTags(t);
          setHistory(h);
          setTasks(a);
          setCalendar(c);
        }
      })
      .catch((e) => {
        if (mounted.current) setError(String(e));
      })
      .finally(() => {
        if (mounted.current) setLoading(false);
      });
    return () => {
      mounted.current = false;
      generation.current++;
      if (job.current)
        void invoke("workspace_cancel_question", { jobId: job.current });
    };
  }, []);
  const request = (): SearchRequest => ({
    query,
    meeting_id: scope,
    after: after || null,
    before: before || null,
    tag: tag || null,
    semantic,
    limit: 24,
  });
  const run = async (kind: "search" | "ask") => {
    const seq = ++generation.current;
    setBusy(kind);
    setError("");
    setAnswer(null);
    try {
      if (kind === "search") {
        const value = await invoke<SearchResponse>("workspace_search", {
          request: request(),
        });
        if (seq === generation.current && mounted.current) { setResults(value); setSubmittedQuery(query); }
      } else {
        job.current = crypto.randomUUID();
        const value = await invoke<CitedAnswer>("workspace_ask", {
          request: request(),
          jobId: job.current,
        });
        if (seq === generation.current && mounted.current) {
          setAnswer(value);
          const saved = await invoke<CitedAnswer[]>("workspace_answers");
          if (seq === generation.current && mounted.current) setHistory(saved);
        }
      }
    } catch (e) {
      if (seq === generation.current && mounted.current) setError(String(e));
    } finally {
      if (seq === generation.current && mounted.current) {
        setBusy(null);
        job.current = null;
      }
    }
  };
  const cancel = async () => {
    if (job.current) {
      try {
        await invoke("workspace_cancel_question", { jobId: job.current });
      } catch (e) {
        setError(String(e));
      }
    }
  };
  const reviewAction = (claim: CitedClaim) => {
    if (!answer) return;
    const ids = new Set(claim.citations.map((c) => c.id));
    setDraft({
      ...emptyDraft,
      title: claim.text,
      meeting_id: scope,
      evidence: answer.sources.filter((s) => ids.has(s.id)),
    });
    setTab("actions");
  };
  const saveTask = async (e: FormEvent) => {
    e.preventDefault();
    if (!draft) return;
    setSaving(true);
    setError("");
    try {
      await invoke("workspace_save_task", {
        input: { ...draft, due_date: draft.due_date || null, status: "open" },
      });
      await refreshTasks();
      setDraft(null);
      toast.success("Action saved");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };
  const changeTask = async (task: WorkspaceTask) => {
    try {
      await invoke("workspace_task_status", {
        id: task.id,
        status: task.status === "open" ? "done" : "open",
      });
      await refreshTasks();
    } catch (e) {
      setError(String(e));
    }
  };
  const deleteTask = async (id: string) => {
    try {
      const original = tasks.find(task => task.id === id);
      await invoke("workspace_task_status", { id, status: "cancelled" });
      await refreshTasks();
      toast.success("Action moved to Trash", { action: { label: "Undo", onClick: () => { void invoke("workspace_task_status", { id, status: original?.status || "open" }).then(refreshTasks).catch(e => setError(String(e))); } } });
    } catch (e) {
      setError(String(e));
    }
  };
  const importCalendar = async () => {
    setCalendarBusy(true);
    setError("");
    try {
      const path = await invoke<string | null>("workspace_pick_file", {
        kind: "calendar",
      });
      if (!path) return;
      const result = await invoke<{ count: number; warnings: string[] }>(
        "workspace_import_calendar",
        { path },
      );
      setCalendar(await invoke<CalendarEvent[]>("workspace_calendar_events"));
      toast.success(`Imported ${result.count} calendar events`);
      if (result.warnings.length) setError(result.warnings.join(" "));
    } catch (e) {
      setError(String(e));
    } finally {
      setCalendarBusy(false);
    }
  };
  return (
    <main className="library-page custom-scrollbar" data-section={tab}>
      <div className="library-shell">
        <header className="library-header">
          <div>
            <h1>Library</h1>
            <p>
              Find a passage, revisit the context, and keep track of what comes next.
            </p>
          </div>
          <Link className="library-settings-link" data-tour="library-settings" href="/settings?tab=library">
            <Settings2 size={16} aria-hidden />
            Library settings
          </Link>
        </header>
        {scope && (
          <div className="library-scope">
            Searching one meeting
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setScope(null);
                setResults(null);
                setAnswer(null);
              }}
            >
              Search all meetings
              <X size={14} />
            </Button>
          </div>
        )}
        <Tabs value={tab} onValueChange={setTab} className="library-workspace">
          <TabsList className="library-tabs">
            <TabsTrigger value="search">
              <BookOpen size={16} aria-hidden />
              Browse &amp; search
            </TabsTrigger>
            <TabsTrigger value="actions">
              <ListChecks size={16} aria-hidden />
              Actions
            </TabsTrigger>
            <TabsTrigger value="trash"><Trash2 size={16} aria-hidden />Trash</TabsTrigger>
            <TabsTrigger value="calendar">
              <CalendarDays size={16} aria-hidden />
              Calendar
            </TabsTrigger>
          </TabsList>
          {error && (
            <p className="library-error" role="alert">
              {error}
            </p>
          )}
          <TabsContent value="search" className="library-search-tab">
            <form
              className="library-search"
              data-tour="library-search"
              onSubmit={(e) => {
                e.preventDefault();
                void run(searchKind);
              }}
            >
              <div className="library-search-kind" role="group" aria-label="Search mode"><button type="button" aria-pressed={searchKind === 'search'} onClick={() => setSearchKind('search')}>Find passages</button><button type="button" aria-pressed={searchKind === 'ask'} onClick={() => setSearchKind('ask')}>Ask a question</button></div>
              <p className="library-note">{searchKind === 'search' ? 'Search across transcripts and notes. Select a result to read it in context.' : 'Get an AI answer with sources. Uses the summary provider you selected in Settings.'}</p>
              <label htmlFor="library-query">
                {searchKind === "ask" ? "What would you like to know?" : "What are you looking for?"}
              </label>
              <div className="library-query-row">
                <Input
                  id="library-query"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={searchKind === "ask" ? "What did we decide about the launch?" : "Search names, topics or exact phrases"}
                  maxLength={2000}
                  disabled={!!busy}
                />
                <Button type="submit" disabled={!!busy || !query.trim()}>
                  <Search size={18} aria-hidden />
                  {searchKind === 'ask' ? 'Ask' : 'Search'}
                </Button>
                {busy === "ask" && (
                  <Button
                    variant="outline"
                    type="button"
                    onClick={() => void cancel()}
                  >
                    <Square size={14} aria-hidden />
                    Stop
                  </Button>
                )}
              </div>
              <details className="library-filters">
                <summary>Filter by date or tag</summary>
                <div className="library-filter-grid">
                  <label>
                    From
                    <Input
                      type="date"
                      value={after}
                      onChange={(e) => setAfter(e.target.value)}
                    />
                  </label>
                  <label>
                    Through
                    <Input
                      type="date"
                      value={before}
                      onChange={(e) => setBefore(e.target.value)}
                    />
                  </label>
                  <label>
                    Tag
                    <select
                      value={tag}
                      onChange={(e) => setTag(e.target.value)}
                    >
                      <option value="">All tags</option>
                      {tags.map((t) => (
                        <option key={t.tag} value={t.tag}>
                          {t.tag} ({t.count})
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="library-check">
                    <input
                      type="checkbox"
                      checked={semantic}
                      onChange={(e) => setSemantic(e.target.checked)}
                    />
                    Also find related meaning
                  </label>
                </div>
              </details>
            </form>
            {busy && (
              <p className="library-status" role="status">
                {busy === "ask"
                  ? "Reading the evidence…"
                  : "Searching your library…"}
              </p>
            )}
            {answer && (
              <section className="library-answer" aria-label="Answer">
                <h2>{answer.question}</h2>
                {answer.stale && (
                  <p className="library-error">
                    The transcript has changed since this answer. Ask again
                    before relying on it.
                  </p>
                )}
                {answer.abstain ? (
                  <p>
                    I couldn’t find enough evidence in the selected meetings to
                    answer that.
                  </p>
                ) : (
                  answer.claims.map((claim, i) => (
                    <article className="library-claim" key={i}>
                      <p>{claim.text}</p>
                      <div>
                        {claim.citations.map((citation, j) => {
                          const source = answer.sources.find(
                            (s) => s.id === citation.id,
                          );
                          return source ? (
                            <SourceLink
                              key={j}
                              source={source}
                              quote={citation.quote}
                            />
                          ) : null;
                        })}
                      </div>
                      <Button
                        variant="ghost"
                        size="sm"
                        disabled={answer.stale}
                        onClick={() => reviewAction(claim)}
                      >
                        Review as an action
                        <Plus size={14} aria-hidden />
                      </Button>
                    </article>
                  ))
                )}
                <p className="library-note">
                  Quotes are checked against the transcript. Review the source
                  before acting on an AI interpretation.
                </p>
              </section>
            )}
            {!answer && <>
              {results?.warnings.map((warning, index) => <p className="library-note" role="status" key={index}>{warning}</p>)}
              <LibraryResults sources={results?.sources ?? []} query={submittedQuery} loading={loading || busy === 'search'} meetingCount={meetingCount} />
            </>}
            {!!history.length && (
              <details className="library-history">
                <summary>Saved questions ({history.length})</summary>
                {history.map((item) => (
                  <div key={item.id}>
                    <button type="button" onClick={() => setAnswer(item)}>
                      {item.question}
                      {item.stale ? " · source changed" : ""}
                    </button>
                    <Button
                      variant="ghost"
                      size="icon"
                      aria-label={`Delete saved question: ${item.question}`}
                      onClick={async () => {
                        try {
                          await invoke("workspace_delete_answer", {
                            id: item.id,
                          });
                          setHistory((h) => h.filter((v) => v.id !== item.id));
                          if (answer?.id === item.id) setAnswer(null);
                        } catch (e) {
                          setError(String(e));
                        }
                      }}
                    >
                      <Trash2 size={16} />
                    </Button>
                  </div>
                ))}
              </details>
            )}
          </TabsContent>
          <TabsContent value="actions">
            <div className="library-section-heading">
              <div>
                <h2>Keep the next step clear.</h2>
                <p>
                  Review suggested commitments before you assign an owner or
                  date.
                </p>
              </div>
              <Button
                onClick={() => setDraft({ ...emptyDraft, meeting_id: scope })}
                disabled={!!draft}
              >
                <Plus size={16} aria-hidden />
                Add action
              </Button>
            </div>
            {draft && (
              <form className="library-task-editor" onSubmit={saveTask}>
                <h3>{draft.id ? "Edit action" : "Review action"}</h3>
                <label htmlFor="action-title">What needs to happen?</label>
                <textarea
                  id="action-title"
                  className="library-action-title"
                  autoFocus
                  required
                  value={draft.title}
                  maxLength={2000}
                  onChange={(e) =>
                    setDraft({ ...draft, title: e.target.value })
                  }
                />
                <div className="library-filter-grid">
                  <label>
                    Owner
                    <Input
                      value={draft.owner}
                      maxLength={200}
                      onChange={(e) =>
                        setDraft({ ...draft, owner: e.target.value })
                      }
                      placeholder="Leave blank if unassigned"
                    />
                  </label>
                  <label>
                    Due date
                    <Input
                      type="date"
                      value={draft.due_date}
                      onChange={(e) =>
                        setDraft({ ...draft, due_date: e.target.value })
                      }
                    />
                  </label>
                </div>
                {draft.evidence.map((s) => (
                  <SourceLink key={s.id} source={s} />
                ))}
                <div className="library-editor-buttons">
                  <Button type="submit" disabled={saving}>
                    {saving ? "Saving…" : "Save action"}
                  </Button>
                  <Button
                    variant="ghost"
                    type="button"
                    disabled={saving}
                    onClick={() => setDraft(null)}
                  >
                    Cancel
                  </Button>
                </div>
              </form>
            )}
            {tasks
              .filter((t) => t.status !== "cancelled" && (!scope || t.meeting_id === scope))
              .map((task) => (
                <article className="library-task" key={task.id}>
                  <Button
                    variant="outline"
                    size="icon"
                    aria-label={`${task.status === "open" ? "Complete" : "Reopen"} action: ${task.title}`}
                    onClick={() => void changeTask(task)}
                  >
                    {task.status === "done" ? (
                      <Check size={16} />
                    ) : (
                      <span className="library-task-dot" />
                    )}
                  </Button>
                  <div>
                    <p
                      className={
                        task.status === "done" ? "library-task-done" : ""
                      }
                    >
                      {task.title}
                    </p>
                    <span>
                      {task.owner || "Unassigned"}
                      {task.due_date ? ` · Due ${task.due_date}` : ""} ·{" "}
                      {task.status}
                    </span>
                    {taskEvidence(task).map((s) => (
                      <SourceLink key={s.id} source={s} />
                    ))}
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() =>
                      setDraft({
                        id: task.id,
                        title: task.title,
                        owner: task.owner,
                        due_date: task.due_date || "",
                        meeting_id: task.meeting_id,
                        evidence: taskEvidence(task),
                      })
                    }
                  >
                    Edit
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon"
                    aria-label={`Delete action: ${task.title}`}
                    onClick={() => void deleteTask(task.id)}
                  >
                    <Trash2 size={16} />
                  </Button>
                </article>
              ))}
            {!tasks.some(t => t.status !== "cancelled" && (!scope || t.meeting_id === scope)) && !draft && !loading && (
              <div className="library-empty">
                <h3>A clear next step beats a long to-do list.</h3>
                <p>
                  Add an action yourself, or turn a cited answer into one after
                  reviewing its source.
                </p>
              </div>
            )}
          </TabsContent>
          <TabsContent value="trash"><TrashPanel /></TabsContent>
          <TabsContent value="calendar">
            <div className="library-section-heading">
              <div>
                <h2>Before the next conversation.</h2>
                <p>
                  Import an ICS calendar to see upcoming meetings. Your calendar
                  stays local.
                </p>
              </div>
              <Button
                onClick={() => void importCalendar()}
                disabled={calendarBusy}
              >
                {calendarBusy ? "Importing…" : "Import calendar"}
              </Button>
            </div>
            {calendar.map((event) => (
              <article className="library-calendar-event" key={event.id}>
                <div className="library-calendar-date">
                  <strong>
                    {new Date(event.starts_at).toLocaleDateString(undefined, {
                      day: "numeric",
                    })}
                  </strong>
                  <span>
                    {new Date(event.starts_at).toLocaleDateString(undefined, {
                      month: "short",
                    })}
                  </span>
                </div>
                <div>
                  <h3>{event.title}</h3>
                  <p>
                    {event.all_day
                      ? "All day"
                      : new Date(event.starts_at).toLocaleTimeString(
                          undefined,
                          { hour: "2-digit", minute: "2-digit" },
                        )}
                    {event.location ? ` · ${event.location}` : ""}
                  </p>
                  {event.description && (
                    <details>
                      <summary>Meeting context</summary>
                      <p>{event.description}</p>
                    </details>
                  )}
                  <button
                    type="button"
                    className="library-text-link"
                    onClick={() => {
                      setQuery(event.title);
                      setScope(null);
                      setTab("search");
                    }}
                  >
                    Find related conversations
                    <ArrowUpRight size={14} aria-hidden />
                  </button>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label={`Remove calendar event: ${event.title}`}
                  onClick={async () => {
                    try {
                      await invoke("workspace_delete_calendar_event", {
                        id: event.id,
                      });
                      setCalendar((c) => c.filter((v) => v.id !== event.id));
                    } catch (e) {
                      setError(String(e));
                    }
                  }}
                >
                  <Trash2 size={16} />
                </Button>
              </article>
            ))}
            {!calendar.length && !loading && (
              <div className="library-empty">
                <h3>Bring the context with you.</h3>
                <p>
                  Export a calendar as .ics, then import it here. Recurring
                  events and exceptions are reviewed during import.
                </p>
              </div>
            )}
          </TabsContent>
        </Tabs>
      </div>
    </main>
  );
}

export default function LibraryPage() { return <Suspense fallback={<div className="library-page">Loading library…</div>}><LibraryContent /></Suspense>; }
