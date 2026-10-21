//! Projects, open actions and briefs for upcoming meetings.
use std::collections::HashMap;

use serde_json::{json, Value};

use crate::args::Args;
use crate::meetings::{cite, item_json, project, project_id};
use crate::notes::{self, clock, notes_markdown, Kind};
use crate::{Arg, Grants, Store};

/// Meetings filed in project `p` that are not in the trash.
macro_rules! project_meetings {
    ($select:literal) => {
        concat!(
            "(SELECT ",
            $select,
            " FROM project_meetings q JOIN meetings n ON n.id=q.meeting_id \
             WHERE q.project_id=p.id AND NOT EXISTS(SELECT 1 FROM meeting_workspace_state w \
             WHERE w.meeting_id=n.id AND w.trashed_at IS NOT NULL))"
        )
    };
}

const PROJECTS: &str = concat!(
    "SELECT json_object('id',p.id,'name',p.name,\
     'archived',json(iif(p.archived,'true','false')),'meetings',",
    project_meetings!("count(*)"),
    ",'last_meeting_at',",
    project_meetings!("max(n.created_at)"),
    ",'open_actions',(SELECT count(*) FROM workspace_tasks t \
     JOIN project_meetings q ON q.meeting_id=t.meeting_id \
     WHERE q.project_id=p.id AND t.status='open'),\
     'next_event',json((SELECT json_object('title',c.title,'starts_at',c.starts_at,\
     'local_time',strftime('%Y-%m-%d %H:%M',c.starts_at,'localtime')) \
     FROM event_projects e JOIN workspace_calendar c ON c.id=e.event_id \
     WHERE e.project_id=p.id AND julianday(c.starts_at)>=julianday('now') \
     ORDER BY julianday(c.starts_at) LIMIT 1))) \
     FROM projects p WHERE ?1 OR p.archived=0 ORDER BY p.archived,lower(p.name),p.id"
);

/// Open tracked actions; `?3` narrows to overdue, due today or due within a week.
const TRACKED: &str = "SELECT json_object('id',t.id,'title',t.title,'owner',t.owner,\
    'due_date',t.due_date,'overdue',json(iif(t.due_date<date('now','localtime'),'true','false')),\
    'meeting',json(CASE WHEN m.id IS NULL THEN NULL ELSE json_object('id',m.id,'title',m.title,\
    'created_at',m.created_at,'local_time',strftime('%Y-%m-%d %H:%M',m.created_at,'localtime')) END),\
    'project',json(CASE WHEN p.id IS NULL THEN NULL ELSE json_object('id',p.id,'name',p.name) END),\
    'evidence',json(iif(json_valid(t.evidence_json),t.evidence_json,'[]'))) \
    FROM workspace_tasks t LEFT JOIN meetings m ON m.id=t.meeting_id \
    LEFT JOIN project_meetings pm ON pm.meeting_id=t.meeting_id \
    LEFT JOIN projects p ON p.id=pm.project_id \
    WHERE t.status='open' AND NOT EXISTS(SELECT 1 FROM meeting_workspace_state w \
    WHERE w.meeting_id=t.meeting_id AND w.trashed_at IS NOT NULL) \
    AND (?1 IS NULL OR p.id=?1) AND (?2 IS NULL OR instr(lower(t.owner),lower(?2))>0) \
    AND (?3 IS NULL OR t.due_date<=CASE ?3 WHEN 'overdue' THEN date('now','localtime','-1 day') \
    WHEN 'today' THEN date('now','localtime') ELSE date('now','localtime','+7 days') END) \
    ORDER BY t.due_date IS NULL,t.due_date,t.updated_at DESC LIMIT ?4";

/// Recent meetings with notes, newest first.
const NOTED: &str = concat!(
    "SELECT json_object(",
    meeting_fields!(),
    ",'notes',json(s.result))",
    meeting_rows!("JOIN summary_processes s ON s.meeting_id=m.id"),
    " AND json_valid(s.result) \
     AND date(m.created_at,'localtime')>=date('now','localtime',?1) \
     AND (?2 IS NULL OR p.id=?2) \
     ORDER BY julianday(m.created_at) DESC,m.id LIMIT 30"
);

/// Calendar events in the window, with their project and brief, plus briefs
/// whose event is no longer in the synced calendar.
const BRIEFS: &str = "SELECT event FROM (\
    SELECT json_object('event_id',c.id,'title',c.title,'starts_at',c.starts_at,\
    'local_time',strftime('%Y-%m-%d %H:%M',c.starts_at,'localtime'),\
    'ends_at',c.ends_at,'meeting_url',c.meeting_url,\
    'project',json(CASE WHEN p.id IS NULL THEN NULL ELSE json_object('id',p.id,'name',p.name,\
    'chosen',e.origin,'reason',e.reason) END),\
    'brief',json(CASE WHEN b.event_id IS NULL THEN NULL ELSE json_object('status',b.status,\
    'updated_at',b.updated_at,'body',json(iif(json_valid(b.body),b.body,'{}'))) END)) AS event,\
    julianday(c.starts_at) AS at \
    FROM workspace_calendar c LEFT JOIN event_projects e ON e.event_id=c.id \
    LEFT JOIN meeting_briefs b ON b.event_id=c.id \
    LEFT JOIN projects p ON p.id=coalesce(e.project_id,b.project_id) \
    WHERE julianday(c.ends_at)>=julianday('now') \
    AND julianday(c.starts_at)<=julianday('now','+'||?1||' days') \
    AND (?2 IS NULL OR p.id=?2) \
    UNION ALL \
    SELECT json_object('event_id',b.event_id,'title',b.title,'starts_at',b.starts_at,\
    'local_time',strftime('%Y-%m-%d %H:%M',b.starts_at,'localtime'),\
    'ends_at',NULL,'meeting_url',NULL,\
    'project',json(CASE WHEN p.id IS NULL THEN NULL ELSE json_object('id',p.id,'name',p.name,\
    'chosen','brief','reason','') END),\
    'brief',json_object('status',b.status,'updated_at',b.updated_at,\
    'body',json(iif(json_valid(b.body),b.body,'{}')))),julianday(b.starts_at) \
    FROM meeting_briefs b LEFT JOIN projects p ON p.id=b.project_id \
    WHERE NOT EXISTS(SELECT 1 FROM workspace_calendar c WHERE c.id=b.event_id) \
    AND julianday(b.starts_at)>=julianday('now','-1 hour') \
    AND julianday(b.starts_at)<=julianday('now','+'||?1||' days') \
    AND (?2 IS NULL OR p.id=?2)) ORDER BY at LIMIT 50";

const TITLES: &str = "SELECT json_object('id',id,'title',title,'created_at',created_at,\
    'local_time',strftime('%Y-%m-%d %H:%M',created_at,'localtime')) \
    FROM meetings WHERE id IN (SELECT value FROM json_each(?1))";

pub(crate) async fn projects<S: Store>(store: &mut S, args: &Value) -> Result<Value, String> {
    let args = Args::new(args, &["include_archived"])?;
    let archived = args.flag("include_archived", false)?;
    let projects = store.rows(PROJECTS, &[Arg::Int(archived.into())]).await?;
    Ok(json!({"projects": projects}))
}

/// Tracked actions that are still open, and action items written in recent notes.
pub(crate) async fn actions<S: Store>(
    store: &mut S,
    grants: Grants,
    args: &Value,
) -> Result<Value, String> {
    let args = Args::new(
        args,
        &["project", "owner", "due", "include_notes", "days", "limit"],
    )?;
    let owner = args.text("owner")?;
    let due = args.choice("due", &["overdue", "today", "week"])?;
    let include_notes = args.flag("include_notes", true)?;
    let days = args.number("days", 14, 1..=365)?;
    let limit = args.number("limit", 100, 1..=500)?;
    let project = project(store, args.text("project")?).await?;
    let mut tracked = store
        .rows(
            TRACKED,
            &[
                project_id(&project),
                owner.clone().into(),
                due.clone().into(),
                Arg::Int(limit),
            ],
        )
        .await?;
    for action in &mut tracked {
        let evidence = action["evidence"].take();
        action["sources"] = json!(evidence
            .as_array()
            .into_iter()
            .flatten()
            .map(|source| evidence_json(source, grants))
            .collect::<Vec<_>>());
        if let Value::Object(fields) = action {
            fields.remove("evidence");
        }
    }
    let mut noted = Vec::new();
    if include_notes {
        let meetings = store
            .rows(
                NOTED,
                &[Arg::Text(format!("-{days} days")), project_id(&project)],
            )
            .await?;
        let wanted = owner.as_deref().map(str::to_lowercase);
        for mut meeting in meetings {
            let markdown = notes_markdown(&meeting["notes"]);
            if let Value::Object(fields) = &mut meeting {
                fields.remove("notes");
            }
            let items: Vec<_> = notes::items(&markdown)
                .into_iter()
                .filter(|item| item.kind == Kind::Action)
                .filter(|item| {
                    wanted.as_ref().is_none_or(|wanted| {
                        let owner = item.owner.as_deref().unwrap_or_default();
                        format!("{owner} {}", item.text)
                            .to_lowercase()
                            .contains(wanted)
                    })
                })
                .collect();
            let id = meeting["id"].as_str().unwrap_or_default().to_owned();
            let numbers = items.iter().flat_map(|item| item.sources.iter().copied());
            let cited = cite(store, &id, numbers, grants.read_transcripts).await?;
            for item in &items {
                let mut action = item_json(item, &cited);
                action["meeting"] = json!({
                    "id": meeting["id"], "title": meeting["title"],
                    "created_at": meeting["created_at"], "local_time": meeting["local_time"],
                });
                action["project"] = meeting["project"].clone();
                noted.push(action);
            }
        }
    }
    Ok(json!({"tracked": tracked, "from_notes": noted, "notes_days": days}))
}

/// A tracked action's evidence as a source; its words only with transcript access.
fn evidence_json(source: &Value, grants: Grants) -> Value {
    let start = source["start_time"].as_f64();
    let mut value = json!({
        "meeting_id": source["meeting_id"],
        "start": start,
        "end": source["end_time"],
        "time": start.map(clock),
    });
    if grants.read_transcripts && source["source_kind"] == "transcript" {
        value["quote"] = source["body"].clone();
    }
    value
}

pub(crate) async fn briefs<S: Store>(store: &mut S, args: &Value) -> Result<Value, String> {
    let args = Args::new(args, &["days", "project"])?;
    let days = args.number("days", 7, 1..=60)?;
    let project = project(store, args.text("project")?).await?;
    let mut events = store
        .rows(BRIEFS, &[Arg::Int(days), project_id(&project)])
        .await?;
    let mut cited: Vec<String> = events
        .iter()
        .flat_map(|event| brief_items(&event["brief"]["body"]))
        .filter_map(|item| item["source"]["meeting_id"].as_str().map(str::to_owned))
        .collect();
    cited.sort();
    cited.dedup();
    let meetings: HashMap<String, Value> = if cited.is_empty() {
        HashMap::new()
    } else {
        store
            .rows(TITLES, &[Arg::Text(json!(cited).to_string())])
            .await?
            .into_iter()
            .filter_map(|row| Some((row["id"].as_str()?.to_owned(), row)))
            .collect()
    };
    for event in &mut events {
        // Indexing into null with [] would create an object; leave a missing brief null.
        if event["brief"].is_null() {
            continue;
        }
        let body = event["brief"]["body"].take();
        let item = |item: &Value| brief_item(item, &meetings);
        event["brief"]["summary"] = body["summary"].clone();
        event["brief"]["sections"] = json!(body["sections"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|section| json!({
                "title": section["title"],
                "items": section["items"].as_array().into_iter().flatten().map(item).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>());
        event["brief"]["agenda"] = json!(body["agenda"]
            .as_array()
            .into_iter()
            .flatten()
            .map(item)
            .collect::<Vec<_>>());
        event["brief"]["note"] = body["note"].clone();
        if let Value::Object(fields) = &mut event["brief"] {
            fields.remove("body");
        }
    }
    Ok(json!({"events": events, "days": days}))
}

fn brief_items(body: &Value) -> impl Iterator<Item = &Value> {
    body["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|section| section["items"].as_array().into_iter().flatten())
        .chain(body["agenda"].as_array().into_iter().flatten())
}

/// A brief item; a sourced one names the earlier meeting and the time in it.
fn brief_item(item: &Value, meetings: &HashMap<String, Value>) -> Value {
    let source = &item["source"];
    let Some(id) = source["meeting_id"].as_str().filter(|id| !id.is_empty()) else {
        return json!({"text": item["text"], "source": null});
    };
    let start = source["start"].as_f64();
    let meeting = meetings.get(id);
    json!({"text": item["text"], "source": {
        "meeting_id": id,
        "meeting_title": meeting.map(|meeting| meeting["title"].clone()),
        "meeting_time": meeting.map(|meeting| meeting["local_time"].clone()),
        "start": start,
        "time": start.map(clock),
        "label": source["label"],
    }})
}
