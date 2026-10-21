//! A whole meeting as Markdown or JSON: notes, people and the numbered transcript.
use overtalk_library_reads::Grant;
use serde_json::{json, Value};

use crate::args::Format;
use crate::config::Access;
use crate::library::read;

/// The meeting and every transcript line, read a page at a time.
pub(crate) fn transcript(access: &Access, meeting_id: &str) -> Result<(Value, Vec<Value>), String> {
    let mut lines = Vec::new();
    loop {
        let mut page = read(
            access,
            "read_transcript",
            &json!({"meeting_id": meeting_id, "limit": 500, "offset": lines.len()}),
        )?;
        let page_lines = page["lines"].take();
        let page_lines = page_lines.as_array().cloned().unwrap_or_default();
        let more = page["has_more"] == true && !page_lines.is_empty();
        lines.extend(page_lines);
        if !more {
            if let Value::Object(fields) = &mut page {
                fields.remove("has_more");
            }
            return Ok((page, lines));
        }
    }
}

pub(crate) fn export(access: &Access, meeting_id: &str, format: Format) -> Result<String, String> {
    access
        .grants()?
        .require(&[Grant::Meetings, Grant::Notes, Grant::Transcripts])?;
    let meeting = read(access, "show_meeting", &json!({"meeting_id": meeting_id}))?;
    let (_, lines) = transcript(access, meeting_id)?;
    Ok(match format {
        Format::Json => {
            serde_json::to_string_pretty(&json!({"meeting": meeting, "transcript": lines}))
                .unwrap_or_default()
                + "\n"
        }
        Format::Markdown => markdown(&meeting, &lines),
    })
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

/// One line, with no Markdown structure leaking in from a title or a name.
fn flat(value: &Value) -> String {
    text(value).replace(['\n', '\r'], " ")
}

pub(crate) fn markdown(meeting: &Value, lines: &[Value]) -> String {
    let mut out = format!("# {}\n\n", flat(&meeting["title"]));
    let when = [
        flat(&meeting["local_time"]),
        flat(&meeting["project"]["name"]),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" · ");
    if !when.is_empty() {
        out += &format!("{when}\n\n");
    }
    let people: Vec<String> = meeting["people"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|person| flat(&person["name"]))
        .collect();
    if !people.is_empty() {
        out += &format!("People: {}\n\n", people.join(", "));
    }
    let notes = text(&meeting["notes"]["markdown"]).trim();
    out += "## Notes\n\n";
    out += if notes.is_empty() {
        "No notes yet."
    } else {
        notes
    };
    out += "\n\n## Transcript\n\n";
    for line in lines {
        let speaker = flat(&line["speaker"]);
        let speaker = if speaker.is_empty() {
            String::new()
        } else {
            format!(" {speaker}:")
        };
        out += &format!(
            "[{} · {}]{speaker} {}\n\n",
            text(&line["source"]),
            text(&line["time"]),
            text(&line["text"]).trim()
        );
    }
    out.trim_end().to_owned() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_keeps_notes_and_numbers_each_line() {
        let meeting = json!({"title":"Sync\n# not a heading","local_time":"2026-10-18 11:00",
            "project":{"name":"Launch"},"people":[{"name":"Lena"},{"name":"Priya"}],
            "notes":{"markdown":"## Decisions\n- Ship [S1]"}});
        let lines = [
            json!({"source":"S1","time":"00:04","speaker":"Lena","text":"We ship."}),
            json!({"source":"S2","time":"00:09","speaker":null,"text":"Agreed."}),
        ];
        assert_eq!(
            markdown(&meeting, &lines),
            "# Sync # not a heading\n\n2026-10-18 11:00 · Launch\n\nPeople: Lena, Priya\n\n\
             ## Notes\n\n## Decisions\n- Ship [S1]\n\n## Transcript\n\n\
             [S1 · 00:04] Lena: We ship.\n\n[S2 · 00:09] Agreed.\n"
        );
    }
}
