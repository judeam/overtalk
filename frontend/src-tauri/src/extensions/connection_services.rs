//! Explicit, reviewed operations for Microsoft 365, Slack and GitHub. HTTP
//! authority is fixed per provider; document redirects never receive OAuth tokens.
use super::{
    connection_oauth,
    connections::{client, response, Connection},
};
use crate::state::AppState;
use base64::Engine;
use futures_util::StreamExt;
use reqwest::Method;
use serde_json::{json, Value};
use tauri::State;

pub(super) fn supported(c: &Connection) -> bool {
    matches!(c.provider.as_str(), "microsoft" | "slack" | "github")
}
fn api_url(c: &Connection, path: &str) -> Result<reqwest::Url, String> {
    let base = match c.provider.as_str() {
        "github" => "https://api.github.com",
        "slack" => "https://slack.com/api",
        "microsoft" => "https://graph.microsoft.com/v1.0",
        _ => return Err("Unsupported connected service".into()),
    };
    let mut url = reqwest::Url::parse(base).unwrap();
    let mut parts = url
        .path_segments_mut()
        .map_err(|_| "Invalid service endpoint")?;
    for part in path.split('/').filter(|p| !p.is_empty()) {
        parts.push(part);
    }
    drop(parts);
    Ok(url)
}
async fn request(
    c: &Connection,
    method: Method,
    path: &str,
    query: &[(&str, String)],
    body: Option<Value>,
) -> Result<reqwest::Response, String> {
    crate::security::privacy::guard_process_destination(
        &api_url(c, path)?.origin().ascii_serialization(),
    )?;
    let token = connection_oauth::access_token(c).await?;
    let mut req = client()?
        .request(method, api_url(c, path)?)
        .bearer_auth(token)
        .query(query)
        .header("Accept", "application/json");
    if c.provider == "github" {
        req = req.header("X-GitHub-Api-Version", "2022-11-28");
    }
    if let Some(body) = body {
        req = req.json(&body);
    }
    req.send().await.map_err(|_|"The connected service could not be reached. Review remote content before retrying a change.".into())
}
async fn api(
    c: &Connection,
    method: Method,
    path: &str,
    query: &[(&str, String)],
    body: Option<Value>,
) -> Result<Value, String> {
    let v = response(request(c, method, path, query, body).await?).await?;
    if c.provider == "slack" && v["ok"] != true {
        return Err(match v["error"].as_str() {
            Some("invalid_auth" | "token_revoked" | "token_expired") => {
                "Slack sign-in expired. Use Sign in again."
            }
            Some("missing_scope") => {
                "Slack needs additional permissions. Sign in again after updating the app's user \
                 scopes."
            }
            Some("channel_not_found" | "not_in_channel") => {
                "This Slack channel is unavailable to your account."
            }
            Some("cant_update_message") => {
                "Slack only allows you to edit messages your account can update."
            }
            _ => "Slack did not complete the request. Check the channel and your permissions.",
        }
        .into());
    }
    Ok(v)
}
fn safe_part(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_!.".contains(&b))
}
pub(super) fn normalize(c: &Connection, value: &str) -> Result<String, String> {
    let value = value.trim();
    let normalized = if value.starts_with("https://") {
        let u = reqwest::Url::parse(value).map_err(|_| "Paste a valid resource link")?;
        if !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
            return Err("The resource link must not include credentials or a custom port".into());
        }
        let p = u
            .path_segments()
            .map(|p| p.filter(|p| !p.is_empty()).collect::<Vec<_>>())
            .unwrap_or_default();
        match c.provider.as_str() {
            "github"
                if u.host_str() == Some("github.com")
                    && p.len() == 4
                    && matches!(p[2], "issues" | "pull") =>
            {
                format!("{}/{}/{}", p[0], p[1], p[3])
            }
            "github" if u.host_str() == Some("github.com") && p.len() == 2 => p.join("/"),
            "slack"
                if u.host_str()
                    .is_some_and(|h| h == "slack.com" || h.ends_with(".slack.com"))
                    && p.len() == 3
                    && p[0] == "archives" =>
            {
                let raw = p[2].strip_prefix('p').ok_or("Not a Slack message link")?;
                if raw.len() < 7 || !raw.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("Not a Slack message timestamp".into());
                }
                format!(
                    "{}/{}.{}",
                    p[1],
                    &raw[..raw.len() - 6],
                    &raw[raw.len() - 6..]
                )
            }
            _ => {
                return Err(
                    "Use a link from this connected service or choose Browse destinations".into(),
                )
            }
        }
    } else {
        value.to_owned()
    };
    let parts = normalized.split('/').collect::<Vec<_>>();
    let valid = match c.provider.as_str() {
        "github" => {
            (parts.len() == 2 || parts.len() == 3)
                && parts.iter().all(|p| safe_part(p))
                && (parts.len() != 3 || parts[2].bytes().all(|b| b.is_ascii_digit()))
        }
        "slack" => {
            (parts.len() == 1 || parts.len() == 2)
                && parts[0].bytes().all(|b| b.is_ascii_alphanumeric())
                && !parts[0].is_empty()
                && (parts.len() != 2
                    || parts[1].split('.').count() == 2
                        && parts[1].bytes().all(|b| b.is_ascii_digit() || b == b'.'))
        }
        "microsoft" => parts.len() == 2 && parts.iter().all(|p| safe_part(p)),
        _ => false,
    };
    if !valid {
        return Err(
            "Choose a destination from Browse, or paste a supported ticket/message link".into(),
        );
    }
    Ok(normalized)
}
pub(super) async fn resolve(c: &Connection, value: &str) -> Result<String, String> {
    if c.provider == "microsoft" && value.starts_with("https://") {
        let url = reqwest::Url::parse(value).map_err(|_| "Invalid Microsoft document link")?;
        if !microsoft_download_host(&url) {
            return Err("Use a OneDrive or SharePoint document link".into());
        }
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.as_bytes());
        let item = api(
            c,
            Method::GET,
            &format!("shares/u!{encoded}/driveItem"),
            &[],
            None,
        )
        .await?;
        let target = format!(
            "{}/{}",
            item["parentReference"]["driveId"]
                .as_str()
                .ok_or("Microsoft did not identify the drive")?,
            item["id"]
                .as_str()
                .ok_or("Microsoft did not identify the document")?
        );
        return normalize(c, &target);
    }
    normalize(c, value)
}
pub(super) async fn browse(c: &Connection) -> Result<Value, String> {
    match c.provider.as_str() {
        "github" => {
            let v = api(
                c,
                Method::GET,
                "user/repos",
                &[("per_page", "100".into()), ("sort", "pushed".into())],
                None,
            )
            .await?;
            Ok(v.as_array()
                .into_iter()
                .flatten()
                .map(|v| json!({"id":v["full_name"],"name":v["full_name"]}))
                .collect::<Vec<_>>()
                .into())
        }
        "slack" => {
            let mut out = Vec::new();
            let mut cursor = String::new();
            for _ in 0..5 {
                let v = api(
                    c,
                    Method::GET,
                    "conversations.list",
                    &[
                        ("types", "public_channel,private_channel".into()),
                        ("exclude_archived", "true".into()),
                        ("limit", "200".into()),
                        ("cursor", cursor.clone()),
                    ],
                    None,
                )
                .await?;
                out.extend(v["channels"].as_array().into_iter().flatten().filter(|v|v["is_member"]==true).map(|v|json!({"id":v["id"],"name":format!("#{}",v["name"].as_str().unwrap_or("channel"))})));
                cursor = v["response_metadata"]["next_cursor"]
                    .as_str()
                    .unwrap_or("")
                    .into();
                if cursor.is_empty() {
                    return Ok(out.into());
                }
            }
            Err(
                "Too many Slack channels. Use a channel ID or narrow your workspace membership."
                    .into(),
            )
        }
        "microsoft" => {
            let root = api(
                c,
                Method::GET,
                "me/drive/root",
                &[("$select", "id,name,parentReference".into())],
                None,
            )
            .await?;
            let drive = root["parentReference"]["driveId"]
                .as_str()
                .ok_or("Microsoft did not return your drive ID")?;
            let mut out = vec![
                json!({"id":format!("{}/{}",drive,root["id"].as_str().ok_or("Microsoft did not return the root folder")?),"name":"OneDrive · top-level folder"}),
            ];
            let children = api(
                c,
                Method::GET,
                "me/drive/root/children",
                &[
                    ("$top", "200".into()),
                    ("$select", "id,name,folder,file,parentReference".into()),
                ],
                None,
            )
            .await?;
            out.extend(children["value"].as_array().into_iter().flatten().filter(|v|v["folder"].is_object()).map(|v|json!({"id":format!("{}/{}",drive,v["id"].as_str().unwrap_or("")),"name":format!("OneDrive · {}",v["name"].as_str().unwrap_or("Folder"))})));
            Ok(out.into())
        }
        _ => Err("Unsupported connected service".into()),
    }
}
pub(super) async fn read(c: &Connection, target: &str) -> Result<Value, String> {
    let target = resolve(c, target).await?;
    let p = target.split('/').collect::<Vec<_>>();
    match c.provider.as_str() {
        "github" => {
            if p.len() != 3 {
                return Err("Paste a GitHub issue or pull-request link to read it".into());
            }
            let v = api(
                c,
                Method::GET,
                &format!("repos/{}/{}/issues/{}", p[0], p[1], p[2]),
                &[],
                None,
            )
            .await?;
            Ok(
                json!({"title":v["title"],"text":v["body"],"revision":v["updated_at"],"url":v["html_url"]}),
            )
        }
        "slack" => {
            let v = if p.len() == 2 {
                api(
                    c,
                    Method::GET,
                    "conversations.replies",
                    &[
                        ("channel", p[0].into()),
                        ("ts", p[1].into()),
                        ("limit", "100".into()),
                    ],
                    None,
                )
                .await?
            } else {
                api(
                    c,
                    Method::GET,
                    "conversations.history",
                    &[("channel", p[0].into()), ("limit", "50".into())],
                    None,
                )
                .await?
            };
            let messages = v["messages"].as_array().cloned().unwrap_or_default();
            let first = messages
                .first()
                .ok_or("This Slack conversation has no readable messages")?;
            let text = messages
                .iter()
                .map(|v| v["text"].as_str().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n\n");
            Ok(
                json!({"title":"Slack conversation","text":text,"revision":first["edited"]["ts"].as_str().or_else(||first["ts"].as_str()).unwrap_or(""),"url":""}),
            )
        }
        "microsoft" => {
            let metadata = graph_metadata(c, &target).await?;
            let name = metadata["name"].as_str().unwrap_or("").to_lowercase();
            if ![".docx", ".txt", ".md"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
            {
                return Err("Choose a Word (.docx), Markdown or text document".into());
            }
            let bytes = graph_download(c, &target).await?;
            let text = if name.ends_with(".docx") {
                tokio::task::spawn_blocking(move || overtalk_documents::read_docx(&bytes))
                    .await
                    .map_err(|_| "Word reader stopped")??
            } else if name.ends_with(".txt") || name.ends_with(".md") {
                String::from_utf8(bytes).map_err(|_| "The text document is not valid UTF-8")?
            } else {
                return Err("Choose a Word (.docx), Markdown or text document".into());
            };
            Ok(
                json!({"title":metadata["name"],"text":text,"revision":metadata["eTag"],"url":metadata["webUrl"]}),
            )
        }
        _ => Err("Unsupported connected service".into()),
    }
}
pub(super) async fn publish(
    c: &Connection,
    target: &str,
    title: &str,
    body: &str,
    update: bool,
    revision: &str,
) -> Result<Value, String> {
    let target = resolve(c, target).await?;
    let p = target.split('/').collect::<Vec<_>>();
    match c.provider.as_str() {
        "github" => {
            if update && p.len() != 3 || !update && p.len() != 2 {
                return Err(
                    "Choose a repository for a new issue, or an issue link to update one".into(),
                );
            }
            api(
                c,
                if update { Method::PATCH } else { Method::POST },
                &format!(
                    "repos/{}/{}/issues{}",
                    p[0],
                    p[1],
                    if update {
                        format!("/{}", p[2])
                    } else {
                        String::new()
                    }
                ),
                &[],
                Some(json!({"title":title,"body":body})),
            )
            .await
        }
        "slack" => {
            let text = format!("{title}\n\n{body}");
            if text.len() > 40000 {
                return Err("Slack messages must be shorter than 40,000 bytes".into());
            }
            if update && p.len() != 2 {
                return Err("Paste the Slack message link you want to update".into());
            }
            let mut payload = json!({"channel":p[0],"text":text,"mrkdwn":false,"parse":"none","link_names":false,"unfurl_links":false,"unfurl_media":false});
            if update {
                payload["ts"] = json!(p[1]);
            }
            api(
                c,
                Method::POST,
                if update {
                    "chat.update"
                } else {
                    "chat.postMessage"
                },
                &[],
                Some(payload),
            )
            .await
        }
        "microsoft" => {
            let metadata = graph_metadata(c, &target).await?;
            let name = metadata["name"].as_str().unwrap_or("").to_owned();
            let (path, bytes, mime) = if update {
                if !metadata["file"].is_object() {
                    return Err("Choose a document to update, not a folder".into());
                }
                let previous = graph_download(c, &target).await?;
                let title = title.to_owned();
                let body = body.to_owned();
                let lower = name.to_lowercase();
                let bytes = if lower.ends_with(".docx") {
                    tokio::task::spawn_blocking(move || {
                        overtalk_documents::append_docx(&previous, &title, &body)
                    })
                    .await
                    .map_err(|_| "Word writer stopped")??
                } else if lower.ends_with(".md") || lower.ends_with(".txt") {
                    let mut old =
                        String::from_utf8(previous).map_err(|_| "The text file is not UTF-8")?;
                    old.push_str(&format!("\n\n## {title}\n\n{body}\n"));
                    old.into_bytes()
                } else {
                    return Err("Updates support Word (.docx), Markdown and text documents".into());
                };
                (
                    format!("drives/{}/items/{}/content", p[0], p[1]),
                    bytes,
                    "application/octet-stream",
                )
            } else {
                if !metadata["folder"].is_object() {
                    return Err("Choose a destination folder for the new Word document".into());
                }
                let filename = word_filename(title)?;
                let document = overtalk_documents::MeetingDocument {
                    title: title.into(),
                    date: String::new(),
                    notice: String::new(),
                    notes: body.into(),
                    passages: vec![],
                };
                let bytes = tokio::task::spawn_blocking(move || document.render("docx"))
                    .await
                    .map_err(|_| "Word writer stopped")??;
                (
                    format!("drives/{}/items/{}:/{filename}:/content", p[0], p[1]),
                    bytes,
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                )
            };
            let token = connection_oauth::access_token(c).await?;
            let mut req = client()?
                .put(api_url(c, &path)?)
                .bearer_auth(token)
                .header("Content-Type", mime)
                .body(bytes);
            if update {
                let tag: Value =
                    serde_json::from_str(revision).map_err(|_| "Missing document revision")?;
                req = req.header("If-Match", tag.as_str().ok_or("Missing document revision")?);
            } else {
                req = req.query(&[("@microsoft.graph.conflictBehavior", "fail")]);
            }
            response(
                req.send().await.map_err(|_| {
                    "Document upload was interrupted; check OneDrive before retrying"
                })?,
            )
            .await
        }
        _ => Err("Unsupported connected service".into()),
    }
}
fn word_filename(title: &str) -> Result<String, String> {
    let name = title
        .chars()
        .filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c))
        .take(100)
        .collect::<String>();
    let name = name.trim().trim_end_matches('.');
    if name.is_empty() {
        return Err("Enter a valid document title".into());
    }
    Ok(if name.to_lowercase().ends_with(".docx") {
        name.into()
    } else {
        format!("{name}.docx")
    })
}
async fn graph_metadata(c: &Connection, target: &str) -> Result<Value, String> {
    api(
        c,
        Method::GET,
        &format!(
            "drives/{}/items/{}",
            target.split('/').next().unwrap(),
            target.split('/').nth(1).unwrap()
        ),
        &[("$select", "id,name,eTag,webUrl,file,folder,size".into())],
        None,
    )
    .await
}
fn microsoft_download_host(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
        && url.host_str().is_some_and(|h| {
            h.ends_with(".sharepoint.com")
                || h.ends_with(".1drv.com")
                || h.ends_with(".onedrive.com")
                || h.ends_with(".storage.live.com")
                || h == "1drv.ms"
                || h == "onedrive.live.com"
        })
}
async fn limited_bytes(response: reqwest::Response) -> Result<Vec<u8>, String> {
    if !response.status().is_success() {
        return Err("Microsoft could not return the document".into());
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Document download was interrupted")?;
        if bytes.len() + chunk.len() > 16 * 1024 * 1024 {
            return Err("Document exceeds the 16 MiB reading limit".into());
        }
        bytes.extend(chunk);
    }
    Ok(bytes)
}
async fn graph_download(c: &Connection, target: &str) -> Result<Vec<u8>, String> {
    let mut response = request(
        c,
        Method::GET,
        &format!(
            "drives/{}/items/{}/content",
            target.split('/').next().unwrap(),
            target.split('/').nth(1).unwrap()
        ),
        &[],
        None,
    )
    .await?;
    for _ in 0..3 {
        if !response.status().is_redirection() {
            return limited_bytes(response).await;
        }
        let url = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| reqwest::Url::parse(s).ok())
            .ok_or("Microsoft omitted the document download link")?;
        if !microsoft_download_host(&url) {
            return Err("Microsoft returned an unsupported document download host".into());
        }
        response = client()?
            .get(url)
            .send()
            .await
            .map_err(|_| "Document download failed")?;
    }
    Err("Too many document download redirects".into())
}

#[tauri::command]
pub async fn workspace_sync_calendar(
    state: State<'_, AppState>,
    id: String,
) -> Result<usize, String> {
    let c: Connection = sqlx::query_as("SELECT * FROM workspace_connections WHERE id=?")
        .bind(&id)
        .fetch_optional(state.db_manager.pool())
        .await
        .map_err(|_| "Could not load calendar connection")?
        .ok_or("Calendar connection no longer exists")?;
    if c.provider == "google" {
        return super::connections::workspace_sync_google_calendar(state, id).await;
    }
    if c.provider != "microsoft" {
        return Err("Choose a Google or Microsoft calendar account".into());
    }
    let now = chrono::Utc::now();
    let mut next = api_url(&c, "me/calendarView")?;
    next.query_pairs_mut().extend_pairs([
        (
            "startDateTime",
            (now - chrono::Duration::days(1)).to_rfc3339(),
        ),
        (
            "endDateTime",
            (now + chrono::Duration::days(60)).to_rfc3339(),
        ),
        ("$top", "250".into()),
    ]);
    let mut events = Vec::new();
    for page in 0..4 {
        let token = connection_oauth::access_token(&c).await?;
        let v = response(
            client()?
                .get(next.clone())
                .bearer_auth(token)
                .header("Prefer", "outlook.timezone=\"UTC\"")
                .send()
                .await
                .map_err(|_| "Microsoft calendar could not be reached")?,
        )
        .await?;
        events.extend(v["value"].as_array().cloned().unwrap_or_default());
        let Some(link) = v["@odata.nextLink"].as_str() else {
            break;
        };
        next = reqwest::Url::parse(link).map_err(|_| "Invalid calendar page")?;
        if next.scheme() != "https"
            || next.host_str() != Some("graph.microsoft.com")
            || !next.username().is_empty()
            || next.password().is_some()
            || next.port().is_some()
        {
            return Err("Unexpected calendar pagination host".into());
        }
        if events.len() >= 1000 || page == 3 {
            return Err("Calendar has more than 1,000 events in this range".into());
        }
    }
    let source = format!("microsoft:{id}");
    // Microsoft lists other attendees only; the account is the signed-in user.
    let own = c.username.to_lowercase();
    let mut tx = state
        .db_manager
        .pool()
        .begin()
        .await
        .map_err(|_| "Could not save calendar")?;
    sqlx::query("DELETE FROM workspace_calendar WHERE source=?")
        .bind(&source)
        .execute(&mut *tx)
        .await
        .map_err(|_| "Could not refresh calendar")?;
    let mut count = 0;
    for v in events {
        if v["isCancelled"] == true {
            continue;
        }
        let uid = v["id"].as_str().ok_or("Calendar event has no ID")?;
        let start = graph_time(&v["start"])?;
        let end = graph_time(&v["end"])?;
        let organizer = v["organizer"]["emailAddress"]["address"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let participants = v["attendees"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .map(|p| {
                let email = p["emailAddress"]["address"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase();
                json!({
                    "name": p["emailAddress"]["name"],
                    "organizer": !email.is_empty() && email == organizer,
                    "self": email == own,
                    "email": email,
                    "status": p["status"]["response"]
                })
            })
            .collect::<Vec<_>>();
        let join = v["onlineMeeting"]["joinUrl"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| super::calendar::meeting_link(v["bodyPreview"].as_str().unwrap_or("")));
        sqlx::query(
            "INSERT INTO workspace_calendar(id,source,uid,recurrence_id,title,description,\
             starts_at,ends_at,timezone,all_day,location,meeting_url,participants,organizer,\
             series_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(super::projects::fingerprint(&format!("{source}:{uid}")))
        .bind(&source)
        .bind(uid)
        .bind("")
        .bind(v["subject"].as_str().unwrap_or("Untitled event"))
        .bind(v["bodyPreview"].as_str().unwrap_or(""))
        .bind(start)
        .bind(end)
        .bind("UTC")
        .bind(v["isAllDay"] == true)
        .bind(v["location"]["displayName"].as_str().unwrap_or(""))
        .bind(join)
        .bind(
            serde_json::to_string(&participants)
                .map_err(|_| "Could not save calendar participants")?,
        )
        .bind(organizer)
        .bind(v["seriesMasterId"].as_str().unwrap_or(""))
        .execute(&mut *tx)
        .await
        .map_err(|_| "Could not save calendar event")?;
        count += 1;
    }
    tx.commit().await.map_err(|_| "Could not save calendar")?;
    Ok(count)
}
fn graph_time(value: &Value) -> Result<String, String> {
    let raw = value["dateTime"]
        .as_str()
        .ok_or("Calendar event has no time")?;
    let time = chrono::DateTime::parse_from_rfc3339(raw)
        .or_else(|_| chrono::DateTime::parse_from_rfc3339(&format!("{raw}Z")))
        .map_err(|_| "Calendar event has an invalid time")?;
    Ok(time.with_timezone(&chrono::Utc).to_rfc3339())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn c(provider: &str) -> Connection {
        Connection {
            id: "test".into(),
            provider: provider.into(),
            name: "Test".into(),
            base_url: String::new(),
            username: String::new(),
            project_id: None,
            created_at: String::new(),
        }
    }
    #[test]
    fn provider_paths_cannot_escape_the_fixed_api_authority() {
        assert_eq!(
            api_url(&c("slack"), "conversations.list").unwrap().as_str(),
            "https://slack.com/api/conversations.list"
        );
        assert_eq!(
            api_url(&c("microsoft"), "me/calendarView")
                .unwrap()
                .as_str(),
            "https://graph.microsoft.com/v1.0/me/calendarView"
        );
        let url = api_url(&c("github"), "repos/team/repo?access_token=leak").unwrap();
        assert_eq!(url.host_str(), Some("api.github.com"));
        assert!(url.query().is_none());
        assert!(normalize(&c("microsoft"), "drive/../file").is_err());
    }
    #[test]
    fn resource_links_preserve_provider_and_reject_injection() {
        assert_eq!(
            normalize(&c("github"), "https://github.com/team/repo/pull/12").unwrap(),
            "team/repo/12"
        );
        assert_eq!(
            normalize(
                &c("slack"),
                "https://team.slack.com/archives/C123/p1234567890123456"
            )
            .unwrap(),
            "C123/1234567890.123456"
        );
        for value in [
            "https://github.com.evil.test/team/repo/issues/1",
            "team/../1",
            "https://x:y@github.com/team/repo/issues/1",
            "team/repo/1?delete=yes",
        ] {
            assert!(normalize(&c("github"), value).is_err());
        }
    }
    #[test]
    fn microsoft_redirects_never_accept_arbitrary_hosts() {
        for u in [
            "https://evil.test/file",
            "http://team.sharepoint.com/file",
            "https://team.sharepoint.com.evil.test/file",
            "https://user:secret@team.sharepoint.com/file",
        ] {
            assert!(!microsoft_download_host(&reqwest::Url::parse(u).unwrap()));
        }
        assert!(microsoft_download_host(
            &reqwest::Url::parse("https://team.sharepoint.com/file").unwrap()
        ));
    }
    #[test]
    fn calendar_times_are_normalized_to_utc() {
        assert_eq!(
            graph_time(&json!({"dateTime":"2026-10-17T12:00:00.0000000","timeZone":"UTC"}))
                .unwrap(),
            "2026-10-17T12:00:00+00:00"
        );
    }
    #[test]
    fn word_titles_cannot_be_paths() {
        assert_eq!(
            word_filename("Plan / Sprint: 1").unwrap(),
            "Plan  Sprint 1.docx"
        );
        assert!(word_filename("../").is_err());
    }
}
