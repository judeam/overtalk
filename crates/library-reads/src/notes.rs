//! Saved notes as Markdown, and the decisions, actions and open questions in
//! them with the transcript lines they cite.
use serde_json::Value;

/// Seconds as 03:09, or 1:02:03 past the first hour.
pub fn clock(seconds: f64) -> String {
    let total = if seconds.is_finite() && seconds > 0.0 {
        seconds as u64
    } else {
        0
    };
    let (hours, minutes, seconds) = (total / 3600, total / 60 % 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

/// The Markdown of saved notes: the editor's Markdown, older block documents,
/// or the first app versions' titled sections.
pub fn notes_markdown(saved: &Value) -> String {
    if let Some(markdown) = saved.get("markdown").and_then(Value::as_str) {
        return markdown.to_owned();
    }
    if let Some(blocks) = saved.get("summary_json").and_then(Value::as_array) {
        return render_blocks(blocks, 0);
    }
    let Some(sections) = saved.as_object() else {
        return String::new();
    };
    let mut keys: Vec<&String> = saved["_section_order"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|key| sections.get_key_value(key).map(|(key, _)| key))
        .collect();
    for key in sections.keys() {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    keys.into_iter()
        .filter_map(|key| {
            let section = &sections[key];
            let title = section["title"].as_str()?;
            let body = section["blocks"]
                .as_array()?
                .iter()
                .filter_map(|block| {
                    let content = block["content"].as_str()?;
                    Some(if block["type"] == "paragraph" {
                        content.to_owned()
                    } else {
                        format!("- {content}")
                    })
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            (!body.is_empty()).then(|| format!("## {title}\n\n{body}"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn inline(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(inline).collect(),
        Value::Object(_) => match value["text"].as_str() {
            Some(text) => text.to_owned(),
            None => inline(&value["content"]),
        },
        _ => String::new(),
    }
}

fn render_blocks(blocks: &[Value], depth: usize) -> String {
    if depth > 32 {
        return String::new();
    }
    let indent = "  ".repeat(depth);
    blocks
        .iter()
        .filter_map(|block| {
            let kind = block["type"].as_str().unwrap_or("paragraph");
            let mut text = if kind == "table" {
                let rows: Vec<Vec<String>> = block["content"]["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| {
                        row["cells"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|cell| inline(cell).replace('|', "\\|").replace('\n', " "))
                            .collect()
                    })
                    .collect();
                let mut lines: Vec<String> = rows
                    .iter()
                    .map(|cells| format!("| {} |", cells.join(" | ")))
                    .collect();
                if let Some(first) = rows.first() {
                    lines.insert(1, format!("|{}", " --- |".repeat(first.len())));
                }
                lines.join("\n")
            } else {
                let content = inline(&block["content"]);
                match kind {
                    "heading" => {
                        let level = block["props"]["level"].as_u64().unwrap_or(2).clamp(1, 6);
                        format!("{} {content}", "#".repeat(level as usize))
                    }
                    "bulletListItem" | "checkListItem" => format!("{indent}- {content}"),
                    "numberedListItem" => format!("{indent}1. {content}"),
                    _ => format!("{indent}{content}"),
                }
            };
            if let Some(children) = block["children"].as_array().filter(|c| !c.is_empty()) {
                text = format!("{text}\n{}", render_blocks(children, depth + 1));
            }
            (!text.trim().is_empty()).then_some(text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Decision,
    Action,
    Question,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Item {
    pub kind: Kind,
    pub text: String,
    pub owner: Option<String>,
    pub due: Option<String>,
    /// Cited transcript lines, numbered from 1 as in [S12].
    pub sources: Vec<u64>,
    /// A time written in the notes, in seconds, for items that cite no line.
    pub written_time: Option<f64>,
}

/// Decisions, actions and open questions, from bullets or table rows under a
/// heading that names them. Other sections are summary and discussion.
pub(crate) fn items(markdown: &str) -> Vec<Item> {
    let mut kind = None;
    let mut header: Option<Vec<String>> = None;
    let mut items = Vec::new();
    for line in markdown.lines().map(str::trim) {
        if let Some(section) = heading(line) {
            kind = section;
            header = None;
            continue;
        }
        let Some(kind) = kind else {
            continue;
        };
        if line.starts_with('|') {
            let cells = cells(line);
            if cells
                .iter()
                .all(|cell| cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
            {
                continue;
            }
            match &header {
                None => {
                    header = Some(
                        cells
                            .iter()
                            .map(|cell| clean(cell).to_lowercase())
                            .collect(),
                    )
                }
                Some(columns) => items.extend(table_item(kind, columns, &cells)),
            }
            continue;
        }
        header = None;
        if let Some(body) = list_body(line) {
            items.extend(list_item(kind, body));
        }
    }
    items
}

/// One line of notes, as a search hit: readable text, cited lines and any written time.
pub(crate) fn line(line: &str) -> (String, Vec<u64>, Option<f64>) {
    let body = list_body(line).unwrap_or(line);
    let mut sources = references(body);
    let text = if body.starts_with('|') {
        let cells = cells(body);
        for cell in cells.iter().filter(|cell| is_citation(cell)) {
            sources.extend(
                cell.split(|c: char| !c.is_ascii_alphanumeric())
                    .filter_map(source_number),
            );
        }
        cells
            .iter()
            .filter(|cell| !is_citation(cell))
            .map(|cell| clean(cell))
            .filter(|cell| !cell.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
    } else {
        clean(body)
    };
    sources.dedup();
    let written = if sources.is_empty() {
        written_time(body)
    } else {
        None
    };
    (text, sources, written)
}

/// `Some(kind)` for a heading line; `Some(None)` is a section of another kind.
fn heading(line: &str) -> Option<Option<Kind>> {
    if let Some(text) = line.strip_prefix('#') {
        return Some(section(text.trim_start_matches('#')));
    }
    // Every accepted ending is ASCII, so these byte offsets are char boundaries.
    let bold = line.starts_with("**")
        && (line.ends_with("**") || line.ends_with("**:") || line.ends_with(":**"))
        && line.len() > 4
        && !line[2..line.len() - 2].contains("**");
    if bold {
        return Some(section(line));
    }
    // Compact notes write "Decisions:" on its own line. Only a short line that
    // names a kind counts, so "We agreed on the following:" stays content.
    if line.ends_with(':') && line.chars().count() <= 40 && list_body(line).is_none() {
        if let Some(kind) = section(line) {
            return Some(Some(kind));
        }
    }
    None
}

fn section(heading: &str) -> Option<Kind> {
    let heading = heading.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|word| heading.contains(word));
    if any(&[
        "decision",
        "decided",
        "decisões",
        "decisiones",
        "entscheidung",
        "beschl",
    ]) {
        Some(Kind::Decision)
    } else if any(&[
        "question",
        "unresolved",
        "still open",
        "open issue",
        "pergunta",
        "frage",
    ]) {
        Some(Kind::Question)
    } else if any(&[
        "action",
        "task",
        "to-do",
        "todo",
        "next step",
        "follow-up",
        "follow up",
        "tarefa",
        "aufgabe",
    ]) {
        Some(Kind::Action)
    } else {
        None
    }
}

fn list_body(line: &str) -> Option<&str> {
    let body = ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
        .or_else(|| {
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0)
                .then(|| &line[digits..])
                .and_then(|rest| rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")))
        })?;
    let body = ["[ ] ", "[x] ", "[X] "]
        .iter()
        .find_map(|check| body.strip_prefix(check))
        .unwrap_or(body);
    Some(body.trim())
}

fn cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let inner = line.strip_prefix('|').unwrap_or(line);
    let inner = inner.strip_suffix('|').unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut escaped = false;
    for c in inner.chars() {
        match c {
            '\\' if !escaped => escaped = true,
            '|' if !escaped => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => {
                if escaped && c != '|' {
                    cell.push('\\');
                }
                cell.push(c);
                escaped = false;
            }
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}

fn is_none(text: &str) -> bool {
    let text = text.trim().to_lowercase();
    text.is_empty()
        || text == "-"
        || text == "—"
        || text == "n/a"
        || (text.len() < 60 && (text.starts_with("none") || text.starts_with("no ")))
}

fn list_item(kind: Kind, body: &str) -> Option<Item> {
    let mut sources = references(body);
    let mut written_time = written_time(body);
    let (mut text, mut owner, mut due) = (body.to_owned(), None, None);
    if let Some((rest, found_owner, found_due)) = owner_and_due(&text) {
        (text, owner, due) = (rest, found_owner, found_due);
    }
    if kind == Kind::Action && owner.is_none() {
        if let Some((name, rest)) = bold_owner(&text) {
            owner = Some(name);
            text = rest;
        }
    }
    let text = clean(&text);
    if is_none(&text) {
        return None;
    }
    sources.dedup();
    if !sources.is_empty() {
        written_time = None;
    }
    Some(Item {
        kind,
        text,
        owner,
        due,
        sources,
        written_time,
    })
}

fn table_item(kind: Kind, columns: &[String], cells: &[String]) -> Option<Item> {
    let find = |words: &[&str]| {
        columns
            .iter()
            .position(|column| words.iter().any(|word| column.contains(word)))
    };
    let owner = find(&["owner", "who", "assignee", "responsible"]);
    let due = find(&["due", "deadline", "when"]);
    let text = find(&[
        "task",
        "action",
        "item",
        "decision",
        "question",
        "what",
        "description",
    ])
    .filter(|index| Some(*index) != owner && Some(*index) != due)
    .or_else(|| {
        (0..cells.len()).find(|index| {
            Some(*index) != owner && Some(*index) != due && !is_citation(&cells[*index])
        })
    })?;
    let cell = |index: Option<usize>| {
        index
            .and_then(|index| cells.get(index))
            .map(|cell| clean(cell))
            .filter(|cell| !cell.is_empty())
    };
    let body = cell(Some(text))?;
    if is_none(&body) {
        return None;
    }
    let mut sources = Vec::new();
    let mut written = None;
    for cell in cells {
        sources.extend(references(cell));
        if is_citation(cell) {
            sources.extend(
                cell.split(|c: char| !c.is_ascii_alphanumeric())
                    .filter_map(source_number),
            );
        }
        written = written
            .or_else(|| written_time(cell))
            .or_else(|| parse_clock(cell.trim()));
    }
    sources.dedup();
    Some(Item {
        kind,
        text: body,
        owner: cell(owner),
        due: cell(due),
        written_time: if sources.is_empty() { written } else { None },
        sources,
    })
}

/// A cell holding only line references, such as "S12, S13" or "[S12]".
fn is_citation(cell: &str) -> bool {
    let tokens: Vec<_> = cell
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    !tokens.is_empty() && tokens.iter().all(|token| source_number(token).is_some())
}

fn source_number(token: &str) -> Option<u64> {
    token
        .strip_prefix('S')
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
        .filter(|number| *number > 0)
}

/// Bracketed groups, e.g. "[S3][S5]" or "[S3, S5]".
fn brackets(text: &str) -> impl Iterator<Item = &str> {
    text.split('[')
        .skip(1)
        .filter_map(|part| part.split_once(']').map(|(inner, _)| inner))
}

fn references(text: &str) -> Vec<u64> {
    brackets(text)
        .filter(|inner| is_reference(inner))
        .flat_map(|inner| {
            inner
                .split([',', ' ', ';'])
                .filter_map(source_number)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn parse_clock(text: &str) -> Option<f64> {
    let parts: Vec<_> = text.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|part| {
            part.is_empty() || part.len() > 2 || !part.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return None;
    }
    Some(parts.iter().fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap_or(0.0)
    }))
}

/// Compact notes end an item with its first citation's time: "[S4] at 00:12".
fn trailing_time(text: &str) -> Option<(&str, f64)> {
    let trimmed = text.trim_end_matches(['.', ' ']);
    let (before, time) = trimmed.rsplit_once("] at ")?;
    Some((&trimmed[..=before.len()], parse_clock(time)?))
}

/// "[03:09]" anywhere, or a time after the citations.
fn written_time(text: &str) -> Option<f64> {
    brackets(text)
        .find_map(parse_clock)
        .or_else(|| trailing_time(text).map(|(_, time)| time))
}

fn is_reference(inner: &str) -> bool {
    parse_clock(inner).is_some()
        || (!inner.trim().is_empty()
            && inner
                .split([',', ' ', ';'])
                .filter(|token| !token.is_empty())
                .all(|token| source_number(token).is_some()))
}

/// "(owner: Priya; due: Friday)", as compact notes write actions.
fn owner_and_due(text: &str) -> Option<(String, Option<String>, Option<String>)> {
    let start = text
        .as_bytes()
        .windows(7)
        .position(|window| window.eq_ignore_ascii_case(b"(owner:"))?;
    let end = start + text[start..].find(')')?;
    let mut owner = None;
    let mut due = None;
    for part in text[start + 1..end].split(';') {
        let (key, value) = part.split_once(':')?;
        let value = value.trim();
        let value = (!value.is_empty()).then(|| value.to_owned());
        match key.trim().to_lowercase().as_str() {
            "owner" => owner = value,
            "due" => due = value,
            _ => {}
        }
    }
    Some((
        format!("{}{}", &text[..start], &text[end + 1..]),
        owner,
        due,
    ))
}

/// "**Priya:** Review tables" names its owner first.
fn bold_owner(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("**")?;
    let (name, rest) = rest.split_once("**")?;
    let (name, rest) = match name.strip_suffix(':') {
        Some(name) => (name, rest),
        None => (name, rest.strip_prefix(':')?),
    };
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= 60)
        .then(|| (name.to_owned(), rest.trim().to_owned()))
}

/// The readable text: citations, times and emphasis removed.
fn clean(text: &str) -> String {
    let mut rest = trailing_time(text).map_or(text, |(before, _)| before);
    let mut out = String::new();
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|close| open + close) else {
            break;
        };
        out.push_str(&rest[..open]);
        if !is_reference(&rest[open + 1..close]) {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    let out = out.replace("**", "").replace("__", "");
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out.trim_end_matches([' ', '—', '-', ',', ';'])
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clock_switches_to_hours_after_an_hour() {
        assert_eq!(clock(189.4), "03:09");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(f64::NAN), "00:00");
    }

    #[test]
    fn notes_come_from_markdown_blocks_or_legacy_sections() {
        assert_eq!(
            notes_markdown(&json!({"markdown":"## Decisions\n- Ship"})),
            "## Decisions\n- Ship"
        );
        let blocks = json!({"summary_json":[
            {"type":"heading","props":{"level":2},"content":[{"type":"text","text":"Decisions"}]},
            {"type":"bulletListItem","content":"Ship first [S1]","children":[{"type":"paragraph","content":"Nested"}]},
            {"type":"table","content":{"rows":[{"cells":[["Owner"],["Task"]]},{"cells":[["Priya"],["Tables | PDF"]]}]}}
        ]});
        assert_eq!(
            notes_markdown(&blocks),
            "## Decisions\n\n- Ship first [S1]\n  Nested\n\n| Owner | Task |\n| --- | --- |\n| Priya | Tables \\| PDF |"
        );
        let legacy = json!({"_section_order":["b","a"],
            "a":{"title":"Summary","blocks":[{"type":"paragraph","content":"Short"}]},
            "b":{"title":"Decisions","blocks":[{"type":"bullet","content":"Ship"}]}});
        assert_eq!(
            notes_markdown(&legacy),
            "## Decisions\n\n- Ship\n\n## Summary\n\nShort"
        );
        assert_eq!(notes_markdown(&json!("text")), "");
    }

    #[test]
    fn items_keep_their_kind_owner_due_and_citations() {
        let notes = "## Summary\n- Not an item [S9]\n\
            ## Key Decisions\nThe team agreed on the following:\n- Search ships first [S1][S3]\n- None noted.\n\
            **Action Items:**\n- **Priya:** Rework the PDF tables [01:40]\n\
            - Send the update (owner: Marco; due: Friday) [S4] at 00:12\n\
            Open questions:\n1. Who tells the customer? [S5, S6]\n\
            ## Action Items\n| **Owner** | Task | Due | Reference Transcript Segment | Segment Time stamp |\n\
            | --- | --- | --- | --- | --- |\n| Lena | Book the room | not stated | S7, S8 | 02:00 |\n\
            | Jonas | Draft the brief | Monday | — | 03:09 |\n| | None noted in this section. | | | |";
        let summary: Vec<_> = items(notes)
            .into_iter()
            .map(|item| {
                (
                    item.kind,
                    item.text,
                    item.owner,
                    item.due,
                    item.sources,
                    item.written_time,
                )
            })
            .collect();
        let some = |text: &str| Some(text.to_owned());
        assert_eq!(
            summary,
            vec![
                (
                    Kind::Decision,
                    "Search ships first".into(),
                    None,
                    None,
                    vec![1, 3],
                    None
                ),
                (
                    Kind::Action,
                    "Rework the PDF tables".into(),
                    some("Priya"),
                    None,
                    vec![],
                    Some(100.0)
                ),
                (
                    Kind::Action,
                    "Send the update".into(),
                    some("Marco"),
                    some("Friday"),
                    vec![4],
                    None
                ),
                (
                    Kind::Question,
                    "Who tells the customer?".into(),
                    None,
                    None,
                    vec![5, 6],
                    None
                ),
                (
                    Kind::Action,
                    "Book the room".into(),
                    some("Lena"),
                    some("not stated"),
                    vec![7, 8],
                    None
                ),
                (
                    Kind::Action,
                    "Draft the brief".into(),
                    some("Jonas"),
                    some("Monday"),
                    vec![],
                    Some(189.0)
                ),
            ]
        );
    }

    #[test]
    fn a_time_in_the_text_is_not_mistaken_for_a_citation_time() {
        let items = items("## Decisions\n- We meet at 10:30");
        assert_eq!(items[0].text, "We meet at 10:30");
        assert_eq!(items[0].written_time, None);
    }

    #[test]
    fn links_and_ordinary_brackets_survive_cleaning() {
        let items = items("## Decisions\n- Use [the plan](https://example.test) [draft] [S2]");
        assert_eq!(
            items[0].text,
            "Use [the plan](https://example.test) [draft]"
        );
        assert_eq!(items[0].sources, vec![2]);
    }
}
