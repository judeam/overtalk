//! Bound export work to the private engine and write only to a user-chosen path.
use super::argument;
use futures_util::TryStreamExt;
use overtalk_documents::{MeetingDocument, Passage};
use serde_json::Value;
use std::io::Write;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

type PassageTiming = (Option<f64>, Option<f64>);

pub(super) async fn template_file(app: &AppHandle, args: Value) -> Result<Value, String> {
    let app = app.clone();
    let action: String = argument(&args, "action")?;
    tokio::task::spawn_blocking(move || match action.as_str() {
        "import" => {
            let Some(file) = app
                .dialog()
                .file()
                .add_filter("Summary template", &["json", "md"])
                .blocking_pick_file()
            else {
                return Ok(Value::Null);
            };
            let path = file.into_path().map_err(|e| e.to_string())?;
            let contents = String::from_utf8(
                crate::security::storage::read_limited(&path, 256 * 1024)
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|_| "Template is not valid UTF-8")?;
            let value = if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            {
                template_markdown(&contents)?
            } else {
                let mut value: Value =
                    serde_json::from_str(&contents).map_err(|e| e.to_string())?;
                if value["template"].is_object() {
                    value = value["template"].take();
                }
                value
            };
            let template =
                crate::summary::templates::validate_and_parse_template(&value.to_string())?;
            serde_json::to_value(template).map_err(|e| e.to_string())
        }
        "export" => {
            let template = crate::summary::templates::validate_and_parse_template(
                &args["template"].to_string(),
            )?;
            let Some(file) = app
                .dialog()
                .file()
                .add_filter("Summary template", &["json"])
                .set_file_name("overtalk-template.json")
                .blocking_save_file()
            else {
                return Ok(Value::Null);
            };
            let path = file.into_path().map_err(|e| e.to_string())?;
            let parent = path.parent().ok_or("Choose a valid directory")?;
            let mut output = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
            serde_json::to_writer_pretty(&mut output, &template).map_err(|e| e.to_string())?;
            output.as_file().sync_all().map_err(|e| e.to_string())?;
            output.persist(path).map_err(|e| e.to_string())?;
            Ok(Value::Bool(true))
        }
        _ => Err("Unsupported template file operation".into()),
    })
    .await
    .map_err(|e| e.to_string())?
}

fn template_markdown(text: &str) -> Result<Value, String> {
    let mut name = String::new();
    let mut description = Vec::new();
    let mut sections = Vec::<Value>::new();
    for line in text.lines() {
        if let Some(title) = line.strip_prefix("# ") {
            if name.is_empty() {
                name = title.trim().into();
            }
        } else if let Some(title) = line.strip_prefix("## ") {
            sections
                .push(serde_json::json!({"title":title.trim(),"instruction":"","format":"list"}));
        } else if let Some(section) = sections.last_mut() {
            let prior = section["instruction"].as_str().unwrap_or_default();
            section["instruction"] = format!("{prior}\n{line}").trim().into();
        } else if !line.trim().is_empty() {
            description.push(line.trim());
        }
    }
    if name.is_empty() || sections.is_empty() {
        return Err("Use a # template name and ## section headings in Markdown templates".into());
    }
    Ok(
        serde_json::json!({"name":name,"description":if description.is_empty() {"Imported Markdown template".into()} else {description.join(" ")},"sections":sections}),
    )
}

async fn document(
    app: &AppHandle,
    id: String,
    include_notes: bool,
    include_timing: bool,
) -> Result<(MeetingDocument, Vec<PassageTiming>), String> {
    let state = app.state::<crate::state::AppState>();
    let metadata =
        crate::api::api_get_meeting_metadata(app.clone(), id.clone(), app.state()).await?;
    let mut query = sqlx::query_as::<_, (String, Option<f64>, Option<f64>, Option<String>)>(
        "SELECT t.transcript,t.audio_start_time,t.audio_end_time,coalesce(s.name,t.speaker_id) \
         FROM transcripts t LEFT JOIN meeting_speakers s ON s.meeting_id=t.meeting_id \
         AND s.id=t.speaker_id WHERE t.meeting_id=? AND coalesce(t.is_partial,0)=0 \
         ORDER BY t.audio_start_time,t.id",
    )
    .bind(&id)
    .fetch(state.db_manager.pool());
    let mut passages = Vec::new();
    let mut timings = Vec::new();
    let mut bytes = 0;
    while let Some((text, seconds, end, speaker)) =
        query.try_next().await.map_err(|e| e.to_string())?
    {
        bytes += text.len() + speaker.as_ref().map_or(10, String::len) + 48;
        if bytes > 8 * 1024 * 1024 || passages.len() >= 100_000 {
            return Err("This meeting is too large for a single export".into());
        }
        if include_timing {
            timings.push((seconds, end));
        }
        let second = seconds.unwrap_or(0.0).max(0.0) as u64;
        passages.push(Passage {
            time: format!(
                "{:02}:{:02}:{:02}",
                second / 3600,
                second / 60 % 60,
                second % 60
            ),
            speaker: speaker.unwrap_or_else(|| "Unassigned".into()),
            text,
        });
    }
    drop(query);
    let notes = if include_notes {
        let summary =
            crate::summary::commands::api_get_summary(app.clone(), app.state(), id, None).await?;
        summary
            .data
            .as_ref()
            .map(notes)
            .transpose()?
            .unwrap_or_default()
    } else {
        String::new()
    };
    Ok((
        MeetingDocument {
            title: metadata.title,
            date: metadata.created_at,
            notice: metadata.recording_notice.unwrap_or_default(),
            notes,
            passages,
        },
        timings,
    ))
}

fn notes(value: &Value) -> Result<String, String> {
    overtalk_documents::notes_markdown(value)
}

pub(super) async fn transcript(app: &AppHandle, args: Value) -> Result<Value, String> {
    let id = argument(&args, "meetingId")?;
    Ok(document(app, id, false, false).await?.0.transcript().into())
}

pub(super) async fn export(app: &AppHandle, args: Value) -> Result<Value, String> {
    let id = argument(&args, "meetingId")?;
    let format: String = argument(&args, "format")?;
    let (label, extension) = match format.as_str() {
        "markdown" => ("Markdown", "md"),
        "text" => ("Plain text", "txt"),
        "json" => ("Structured meeting", "json"),
        "docx" => ("Word document", "docx"),
        "pdf" => ("PDF document", "pdf"),
        _ => return Err("Unsupported export format".into()),
    };
    let redactions = args
        .get("redactions")
        .map(|value| {
            serde_json::from_value::<crate::extensions::privacy::Redactions>(value.clone())
                .map_err(|_| "Invalid redaction settings".to_owned())
        })
        .transpose()?;
    if let Some(rules) = &redactions {
        rules.validate()?;
    }
    let include_notes = redactions
        .as_ref()
        .is_none_or(crate::extensions::privacy::Redactions::keeps_notes);
    let include_timing = redactions
        .as_ref()
        .is_some_and(|rules| !rules.spans.is_empty());
    let (mut document, timings) = document(app, id, include_notes, include_timing).await?;
    let pool = app
        .state::<crate::state::AppState>()
        .db_manager
        .pool()
        .clone();
    let app = app.clone();
    let saved = tokio::task::spawn_blocking(move || {
        if let Some(rules) = &redactions {
            rules.apply(&mut document, &timings)?;
        }
        let title: String = document
            .title
            .chars()
            .take(100)
            .map(|c| {
                if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let picked = app
            .dialog()
            .file()
            .add_filter(label, &[extension])
            .set_file_name(format!("{title}.{extension}"))
            .blocking_save_file();
        let Some(picked) = picked else {
            return Ok(Value::Null);
        };
        let path = picked.into_path().map_err(|e| e.to_string())?;
        let parent = path.parent().ok_or("Choose a valid export directory")?;
        let contents = document.render(&format)?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(&contents).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(&path).map_err(|e| e.to_string())?;
        Ok(Value::String(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?;
    if saved.as_ref().is_ok_and(Value::is_string) {
        crate::extensions::usage::count(&pool, "export_saved").await;
    }
    saved
}
