//! Plain text for people at a terminal. Scripts and assistants use --json.
use serde_json::Value;

use crate::args::View;

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn list(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// Local time when the library could convert it, else the stored time.
fn when(value: &Value) -> &str {
    match text(&value["local_time"]) {
        "" => match text(&value["created_at"]) {
            "" => text(&value["starts_at"]),
            stored => stored,
        },
        local => local,
    }
}

/// " · Product launch", or nothing outside a project.
fn in_project(value: &Value) -> String {
    match text(&value["project"]["name"]) {
        "" => String::new(),
        name => format!(" · {name}"),
    }
}

/// "2026-10-18 11:00  Product sync · Product launch  [id]"
fn meeting_line(meeting: &Value) -> String {
    format!(
        "{}  {}{}  [{}]",
        when(meeting),
        text(&meeting["title"]),
        in_project(meeting),
        text(&meeting["id"])
    )
}

/// "  • Rework the tables (Priya, due Friday)  03:09 S12"
fn item_line(item: &Value) -> String {
    let mut details = Vec::new();
    if let Some(owner) = item["owner"].as_str().filter(|owner| !owner.is_empty()) {
        details.push(owner.to_owned());
    }
    if let Some(due) = item["due"].as_str().filter(|due| !due.is_empty()) {
        details.push(format!("due {due}"));
    }
    let details = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    let sources: Vec<&str> = list(&item["sources"])
        .iter()
        .map(|source| text(&source["id"]))
        .collect();
    let place = [text(&item["time"]).to_owned(), sources.join(" ")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let place = if place.is_empty() {
        String::new()
    } else {
        format!("  {place}")
    };
    format!("  • {}{details}{place}", text(&item["text"]))
}

pub(crate) fn render(view: View, value: &Value) -> String {
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    match view {
        View::Meetings => {
            for meeting in list(&value["meetings"]) {
                line(meeting_line(meeting));
            }
            if list(&value["meetings"]).is_empty() {
                line("No meetings match.".into());
            }
            if value["has_more"] == true {
                line("More meetings match: use --offset or --limit to see them.".into());
            }
        }
        View::Show => {
            line(text(&value["title"]).to_owned());
            line(format!("{}{}", when(value), in_project(value)));
            let people: Vec<String> = list(&value["people"])
                .iter()
                .map(|person| text(&person["name"]).to_owned())
                .collect();
            if !people.is_empty() {
                line(format!("People: {}", people.join(", ")));
            }
            for (key, title) in [
                ("decisions", "Decisions"),
                ("actions", "Actions"),
                ("open_questions", "Open questions"),
            ] {
                if !list(&value[key]).is_empty() {
                    line(String::new());
                    line(title.into());
                    for item in list(&value[key]) {
                        line(item_line(item));
                    }
                }
            }
            line(String::new());
            match value["notes"]["markdown"].as_str() {
                Some(notes) if !notes.trim().is_empty() => {
                    line("Notes".into());
                    line(notes.trim().to_owned());
                }
                _ => line("No notes yet.".into()),
            }
        }
        View::Search => {
            for hit in list(&value["results"]) {
                let place = [text(&hit["kind"]), text(&hit["time"]), text(&hit["source"])]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                line(format!("{}  ({place})", meeting_line(hit)));
                if !text(&hit["text"]).is_empty() {
                    let speaker = match text(&hit["speaker"]) {
                        "" => String::new(),
                        speaker => format!("{speaker}: "),
                    };
                    line(format!("    {speaker}{}", text(&hit["text"])));
                }
            }
            if list(&value["results"]).is_empty() {
                line("Nothing matches.".into());
            }
            for skipped in list(&value["not_searched"]) {
                line(format!(
                    "Not searched: {}. {}",
                    text(&skipped["content"]),
                    text(&skipped["reason"])
                ));
            }
        }
        View::Projects => {
            for project in list(&value["projects"]) {
                let mut details = vec![
                    format!("{} meetings", project["meetings"]),
                    format!("{} open actions", project["open_actions"]),
                ];
                if let Some(last) = project["last_meeting_at"].as_str() {
                    details.push(format!("last {}", last.get(..10).unwrap_or(last)));
                }
                if let Some(next) = project["next_event"]["title"].as_str() {
                    details.push(format!("next “{next}” {}", when(&project["next_event"])));
                }
                let archived = if project["archived"] == true {
                    " (archived)"
                } else {
                    ""
                };
                line(format!(
                    "{}{archived}  {}",
                    text(&project["name"]),
                    details.join(", ")
                ));
            }
            if list(&value["projects"]).is_empty() {
                line("No projects yet.".into());
            }
        }
        View::Actions => {
            line("Tracked actions".into());
            for action in list(&value["tracked"]) {
                let mut details = Vec::new();
                if !text(&action["owner"]).is_empty() {
                    details.push(text(&action["owner"]).to_owned());
                }
                if let Some(due) = action["due_date"].as_str() {
                    details.push(format!("due {due}"));
                }
                if action["overdue"] == true {
                    details.push("overdue".into());
                }
                let from = match text(&action["meeting"]["title"]) {
                    "" => String::new(),
                    title => format!("  from “{title}”"),
                };
                line(format!(
                    "  • {} ({}){from}",
                    text(&action["title"]),
                    details.join(", ")
                ));
            }
            if list(&value["tracked"]).is_empty() {
                line("  None open.".into());
            }
            if value
                .get("from_notes")
                .is_some_and(|notes| !list(notes).is_empty())
            {
                line(String::new());
                line(format!(
                    "Action items in notes from the last {} days",
                    value["notes_days"]
                ));
                for action in list(&value["from_notes"]) {
                    line(format!(
                        "{}  from “{}”",
                        item_line(action),
                        text(&action["meeting"]["title"])
                    ));
                }
            }
        }
        View::Briefs => {
            for event in list(&value["events"]) {
                line(format!(
                    "{}  {}{}",
                    when(event),
                    text(&event["title"]),
                    in_project(event)
                ));
                let brief = &event["brief"];
                if brief.is_null() {
                    line("  No brief yet.".into());
                    line(String::new());
                    continue;
                }
                if !text(&brief["summary"]).is_empty() {
                    line(format!("  {}", text(&brief["summary"])));
                }
                let sourced = |item: &Value| {
                    let source = &item["source"];
                    if source.is_null() {
                        return format!("    • {}", text(&item["text"]));
                    }
                    let from = match text(&source["meeting_title"]) {
                        "" => text(&source["label"]).to_owned(),
                        title => format!("{title}, {}", text(&source["time"])),
                    };
                    format!("    • {}  [{from}]", text(&item["text"]))
                };
                for section in list(&brief["sections"]) {
                    line(format!("  {}", text(&section["title"])));
                    for item in list(&section["items"]) {
                        line(sourced(item));
                    }
                }
                if !list(&brief["agenda"]).is_empty() {
                    line("  Suggested agenda".into());
                    for item in list(&brief["agenda"]) {
                        line(sourced(item));
                    }
                }
                if !text(&brief["note"]).is_empty() {
                    line(format!("  {}", text(&brief["note"])));
                }
                line(String::new());
            }
            if list(&value["events"]).is_empty() {
                line(format!("No meetings in the next {} days.", value["days"]));
            }
        }
    }
    out
}

pub(crate) fn transcript(meeting: &Value, lines: &[Value]) -> String {
    let mut out = format!("{}\n\n", meeting_line(meeting));
    for line in lines {
        let speaker = match text(&line["speaker"]) {
            "" => String::new(),
            speaker => format!("{speaker}: "),
        };
        out += &format!(
            "{:>5}  {}  {speaker}{}\n",
            text(&line["source"]),
            text(&line["time"]),
            text(&line["text"]).trim()
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn items_show_owner_due_time_and_source() {
        let shown = json!({"title":"Sync","id":"m1","local_time":"2026-10-18 11:00",
            "project":{"name":"Launch"},"people":[{"name":"Lena"}],
            "decisions":[{"text":"Ship first","time":"00:10","sources":[{"id":"S1"}]}],
            "actions":[{"text":"Rework tables","owner":"Priya","due":"Friday","time":null,"sources":[]}],
            "open_questions":[],"notes":{"markdown":"## Decisions\n- Ship first [S1]"}});
        assert_eq!(
            render(View::Show, &shown),
            "Sync\n2026-10-18 11:00 · Launch\nPeople: Lena\n\nDecisions\n  • Ship first  00:10 S1\n\n\
             Actions\n  • Rework tables (Priya, due Friday)\n\nNotes\n## Decisions\n- Ship first [S1]\n"
        );
    }

    #[test]
    fn empty_results_say_so() {
        assert_eq!(
            render(View::Meetings, &json!({"meetings":[],"has_more":false})),
            "No meetings match.\n"
        );
        assert_eq!(
            render(
                View::Search,
                &json!({"results":[],"not_searched":[{"content":"notes","reason":"Turn on X."}]})
            ),
            "Nothing matches.\nNot searched: notes. Turn on X.\n"
        );
    }
}
