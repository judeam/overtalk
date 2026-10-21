//! Explicit external connections. Credentials live in the OS keychain; local
//! drafts are reviewed before publication. An uncertain write is never retried.
use crate::state::AppState;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{FromRow, SqlitePool};
use tauri::{AppHandle, State};
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
#[derive(Clone, Serialize, FromRow)]
pub struct Connection {
    pub id: String,
    pub provider: String,
    pub name: String,
    pub base_url: String,
    pub username: String,
    pub project_id: Option<String>,
    pub created_at: String,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ConnectionAction {
    List,
    Save {
        provider: String,
        name: String,
        base_url: String,
        username: String,
        secret: String,
        project_id: Option<String>,
    },
    Remove {
        id: String,
    },
    Read {
        id: String,
        target: String,
    },
    Browse {
        id: String,
    },
    Draft {
        id: String,
        target: String,
        update: bool,
        title: String,
        body: String,
        source_meeting_id: Option<String>,
    },
    Publish {
        id: String,
        expected_hash: String,
    },
}
#[derive(Serialize, Deserialize)]
pub(super) struct GoogleToken {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub client_id: String,
    pub client_secret: String,
}
pub(super) async fn secret(id: &str, value: Option<String>) -> Result<String, String> {
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("Overtalk connections", &id).map_err(err)?;
        if let Some(value) = value {
            entry.set_password(&value).map_err(|_| {
                "The OS keychain could not save this credential. Unlock it and try again."
                    .to_owned()
            })?;
            Ok(String::new())
        } else {
            entry.get_password().map_err(|_| {
                "Connection credential is unavailable. Unlock the OS keychain or reconnect."
                    .to_owned()
            })
        }
    })
    .await
    .map_err(err)?
}
pub(super) fn client() -> Result<reqwest::Client, String> {
    static CLIENT: std::sync::LazyLock<Result<reqwest::Client, String>> =
        std::sync::LazyLock::new(|| {
            reqwest::Client::builder()
                .tls_certs_only(crate::security::transport::tls_roots())
                .no_proxy()
                .user_agent("Overtalk/0.1")
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(40))
                .pool_max_idle_per_host(2)
                .build()
                .map_err(err)
        });
    (*CLIENT).clone()
}
pub(super) async fn response(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("The provider returned HTTP {}. Review permissions, destination and current content before retrying.",status.as_u16()));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Connection interrupted".to_string())?;
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
            return Err("Provider response exceeds 4 MiB".into());
        }
        bytes.extend(chunk);
    }
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).map_err(|_| "Provider returned invalid JSON".into())
}

async fn token(connection: &Connection) -> Result<String, String> {
    super::connection_oauth::access_token(connection).await
}
pub(super) async fn legacy_token(connection: &Connection, value: String) -> Result<String, String> {
    if connection.provider != "google" {
        return Ok(value);
    }
    let mut token: GoogleToken =
        serde_json::from_str(&value).map_err(|_| "Reconnect your Google account")?;
    if token.expires_at <= chrono::Utc::now().timestamp() + 90 {
        let data = response(
            client()?
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("client_id", token.client_id.as_str()),
                    ("client_secret", token.client_secret.as_str()),
                    ("refresh_token", token.refresh_token.as_str()),
                    ("grant_type", "refresh_token"),
                ])
                .send()
                .await
                .map_err(|_| "Could not refresh Google authorization")?,
        )
        .await?;
        token.access_token = data["access_token"]
            .as_str()
            .ok_or("Google did not return an access token")?
            .into();
        token.expires_at =
            chrono::Utc::now().timestamp() + data["expires_in"].as_i64().unwrap_or(3600);
        secret(
            &connection.id,
            Some(serde_json::to_string(&token).map_err(err)?),
        )
        .await?;
    }
    Ok(token.access_token)
}
fn validate_base(provider: &str, base: &str) -> Result<String, String> {
    match provider {
        "linear" => Ok("https://api.linear.app".into()),
        "google" => Ok("https://docs.googleapis.com".into()),
        "jira" | "confluence" => {
            let url =
                reqwest::Url::parse(base).map_err(|_| "Enter your HTTPS Atlassian site URL")?;
            let host = url.host_str().unwrap_or("");
            if url.scheme() != "https"
                || !host.ends_with(".atlassian.net")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.port().is_some()
                || !matches!(url.path(), "" | "/")
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "Use https://your-team.atlassian.net without a path or credentials".into(),
                );
            }
            Ok(format!("https://{host}"))
        }
        _ => Err("Unsupported provider".into()),
    }
}
fn target(value: &str, allow_empty: bool) -> Result<(), String> {
    if (value.is_empty() && !allow_empty)
        || value.len() > 200
        || value
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
    {
        return Err("Enter the resource ID, without a URL or path".into());
    }
    Ok(())
}
async fn connection(pool: &SqlitePool, id: &str) -> Result<Connection, String> {
    sqlx::query_as("SELECT * FROM workspace_connections WHERE id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(err)?
        .ok_or("Connection no longer exists".into())
}
async fn send(
    c: &Connection,
    method: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    crate::security::privacy::guard_process_destination(&c.base_url)?;
    let credential = token(c).await?;
    let mut request = client()?.request(method, format!("{}{path}", c.base_url));
    request = match c.provider.as_str() {
        "linear" => request.header(reqwest::header::AUTHORIZATION, credential),
        "google" => request.bearer_auth(credential),
        _ => request.basic_auth(&c.username, Some(credential)),
    };
    if let Some(body) = body {
        request = request.json(&body);
    }
    response(
        request
            .send()
            .await
            .map_err(|_| "The provider could not be reached; check the connection")?,
    )
    .await
}
async fn graphql(c: &Connection, query: &str, variables: Value) -> Result<Value, String> {
    let value = send(
        c,
        reqwest::Method::POST,
        "/graphql",
        Some(json!({"query":query,"variables":variables})),
    )
    .await?;
    if value["errors"].is_array() {
        return Err(
            "Linear rejected the request. Check the resource and connection permissions.".into(),
        );
    }
    Ok(value["data"].clone())
}
fn google_text(value: &Value, out: &mut String) {
    if let Some(content) = value.pointer("/textRun/content").and_then(Value::as_str) {
        out.push_str(content);
    }
    match value {
        Value::Array(a) => {
            for v in a {
                google_text(v, out)
            }
        }
        Value::Object(o) => {
            for (k, v) in o {
                if k != "textRun" {
                    google_text(v, out)
                }
            }
        }
        _ => {}
    }
}
pub async fn read(c: &Connection, id: &str) -> Result<Value, String> {
    if super::connection_services::supported(c) {
        return super::connection_services::read(c, id).await;
    }
    if super::connection_mcp::is_mcp(c) {
        return super::connection_mcp::read(c, id).await;
    }
    target(id, false)?;
    match c.provider.as_str() {
        "linear" => {
            let v = graphql(
                c,
                "query($id:String!){issue(id:$id){id identifier title description updatedAt url state{name}}}",
                json!({"id":id}),
            )
            .await?;
            let issue = &v["issue"];
            if issue.is_null() {
                return Err("Issue not found".into());
            }
            Ok(
                json!({"title":issue["title"],"text":issue["description"],"revision":issue["updatedAt"],"url":issue["url"],"status":issue["state"]["name"]}),
            )
        }
        "jira" => {
            let v = send(
                c,
                reqwest::Method::GET,
                &format!("/rest/api/3/issue/{id}?fields=summary,description,updated,status"),
                None,
            )
            .await?;
            Ok(
                json!({"title":v["fields"]["summary"],"text":v["fields"]["description"],"revision":v["fields"]["updated"],"url":format!("{}/browse/{id}",c.base_url),"status":v["fields"]["status"]["name"]}),
            )
        }
        "confluence" => {
            let v = send(
                c,
                reqwest::Method::GET,
                &format!("/wiki/api/v2/pages/{id}?body-format=storage"),
                None,
            )
            .await?;
            Ok(
                json!({"title":v["title"],"text":v["body"]["storage"]["value"],"revision":v["version"]["number"],"url":format!("{}/wiki/spaces/~/pages/{id}",c.base_url)}),
            )
        }
        "google" => {
            let v = send(
                c,
                reqwest::Method::GET,
                &format!("/v1/documents/{id}?includeTabsContent=true"),
                None,
            )
            .await?;
            let mut text = String::new();
            google_text(&v, &mut text);
            Ok(
                json!({"title":v["title"],"text":text,"revision":v["revisionId"],"url":format!("https://docs.google.com/document/d/{id}/edit")}),
            )
        }
        _ => Err("Unsupported connection".into()),
    }
}
fn adf(text: &str) -> Value {
    json!({"type":"doc","version":1,"content":text.lines().map(|line|json!({"type":"paragraph","content":if line.is_empty(){vec![]}else{vec![json!({"type":"text","text":line})]}})).collect::<Vec<_>>()})
}
fn html(text: &str) -> String {
    format!(
        "<p>{}</p>",
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('\n', "</p><p>")
    )
}
async fn publish(
    c: &Connection,
    target_id: &str,
    title: &str,
    body: &str,
    revision: &str,
    update: bool,
) -> Result<Value, String> {
    if update {
        let current = read(c, target_id).await?;
        if current["revision"] != revision {
            return Err(
                "Remote content changed after this draft was prepared. Read it again and prepare a \
                 new draft."
                    .into(),
            );
        }
    }
    if super::connection_services::supported(c) {
        return super::connection_services::publish(c, target_id, title, body, update, revision)
            .await;
    }
    if super::connection_mcp::is_mcp(c) {
        return super::connection_mcp::publish(c, target_id, title, body, update, revision).await;
    }
    match c.provider.as_str() {
        "linear" => {
            let input = if update {
                json!({"title": title, "description": body})
            } else {
                json!({"teamId": target_id, "title": title, "description": body})
            };
            graphql(
                c,
                if update {
                    "mutation($id:String!,$input:IssueUpdateInput!){issueUpdate(id:$id,\
                     input:$input){success issue{id url}}}"
                } else {
                    "mutation($input:IssueCreateInput!){issueCreate(input:$input){success issue{id \
                     url}}}"
                },
                json!({"id": target_id, "input": input}),
            )
            .await
        }
        "jira" => {
            send(
                c,
                if update {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::POST
                },
                &if update {
                    format!("/rest/api/3/issue/{target_id}")
                } else {
                    "/rest/api/3/issue".into()
                },
                Some(json!({
                    "fields": if update {
                        json!({"summary": title, "description": adf(body)})
                    } else {
                        json!({
                            "project": {"key": target_id},
                            "issuetype": {"name": "Task"},
                            "summary": title,
                            "description": adf(body)
                        })
                    }
                })),
            )
            .await
        }
        "confluence" => {
            let version = serde_json::from_str::<Value>(revision)
                .ok()
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let mut data = json!({
                "status": "current",
                "title": title,
                "body": {"representation": "storage", "value": html(body)}
            });
            if update {
                data["id"] = json!(target_id);
                data["version"] = json!({
                    "number": version + 1,
                    "message": "Reviewed update from Overtalk"
                });
            } else {
                data["spaceId"] = json!(target_id);
            }
            send(
                c,
                if update {
                    reqwest::Method::PUT
                } else {
                    reqwest::Method::POST
                },
                &if update {
                    format!("/wiki/api/v2/pages/{target_id}")
                } else {
                    "/wiki/api/v2/pages".into()
                },
                Some(data),
            )
            .await
        }
        "google" => {
            let id = if update {
                target_id.to_owned()
            } else {
                let created = send(
                    c,
                    reqwest::Method::POST,
                    "/v1/documents",
                    Some(json!({"title": title})),
                )
                .await?;
                created["documentId"]
                    .as_str()
                    .ok_or("Google did not return a document ID")?
                    .to_owned()
            };
            // Existing docs are appended to, not blindly overwritten. Revision
            // checks are enforced by Google atomically.
            let mut request = json!({
                "requests": [{
                    "insertText": {
                        "endOfSegmentLocation": {},
                        "text": format!("\n{body}\n")
                    }
                }]
            });
            if update {
                request["writeControl"] = json!({
                    "requiredRevisionId": serde_json::from_str::<Value>(revision).map_err(err)?
                });
            }
            send(
                c,
                reqwest::Method::POST,
                &format!("/v1/documents/{id}:batchUpdate"),
                Some(request),
            )
            .await
            .map_err(|_| {
                format!(
                    "Document {id} exists, but the text update was not confirmed. Inspect it \
                     before preparing another change."
                )
            })?;
            Ok(json!({
                "url": format!("https://docs.google.com/document/d/{id}/edit")
            }))
        }
        _ => Err("Unsupported connection".into()),
    }
}
#[tauri::command]
pub async fn workspace_connections(
    _app: AppHandle,
    state: State<'_, AppState>,
    action: ConnectionAction,
) -> Result<Value, String> {
    let pool = state.db_manager.pool();
    match action {
        ConnectionAction::List => {
            let rows: Vec<Connection> =
                sqlx::query_as("SELECT * FROM workspace_connections ORDER BY name LIMIT 100")
                    .fetch_all(pool)
                    .await
                    .map_err(err)?;
            Ok(json!(rows
                .into_iter()
                .map(|c| json!({
                    "id": c.id,
                    "name": c.name,
                    "description": c.provider,
                    "status": if super::connection_mcp::is_mcp(&c)
                        || super::connection_services::supported(&c)
                        || c.provider == "google"
                    {
                        "Account linked"
                    } else {
                        "API token"
                    },
                    "account": c.username
                }))
                .collect::<Vec<_>>()))
        }
        ConnectionAction::Save {
            provider,
            name,
            base_url,
            username,
            secret: credential,
            project_id,
        } => {
            if name.trim().is_empty()
                || name.len() > 120
                || credential.is_empty()
                || credential.len() > 8192
                || provider == "google"
            {
                return Err(
                    "Check the connection name and token. Google uses browser sign-in.".into(),
                );
            }
            let base_url = validate_base(&provider, &base_url)?;
            let id = uuid::Uuid::new_v4().to_string();
            secret(&id, Some(credential)).await?;
            let saved = sqlx::query("INSERT INTO workspace_connections VALUES(?,?,?,?,?,?,?)")
                .bind(&id)
                .bind(provider)
                .bind(name)
                .bind(base_url)
                .bind(username)
                .bind(project_id)
                .bind(chrono::Utc::now().to_rfc3339())
                .execute(pool)
                .await;
            if let Err(error) = saved {
                super::connection_oauth::delete_secret(&id).await?;
                return Err(err(error));
            }
            Ok(json!({"id":id}))
        }
        ConnectionAction::Remove { id } => {
            let operation = super::connection_oauth::operation_lock(&id).await;
            let _operation = operation.lock().await;
            let c = connection(pool, &id).await?;
            let revoked = super::connection_oauth::disconnect(&c).await?;
            sqlx::query("DELETE FROM workspace_connections WHERE id=?")
                .bind(id)
                .execute(pool)
                .await
                .map_err(err)?;
            Ok(json!({
                "revoked": revoked,
                "message": if revoked {
                    "Disconnected and revoked account access."
                } else {
                    "Disconnected on this device. You can also remove Overtalk from your \
                     provider's authorized apps."
                }
            }))
        }
        ConnectionAction::Read { id, target } => read(&connection(pool, &id).await?, &target).await,
        ConnectionAction::Browse { id } => {
            let c = connection(pool, &id).await?;
            if super::connection_services::supported(&c) {
                return super::connection_services::browse(&c).await;
            }
            if super::connection_mcp::is_mcp(&c) {
                return super::connection_mcp::browse(&c).await;
            }
            match c.provider.as_str() {
                "linear" => {
                    let v = graphql(&c, "query{teams(first:100){nodes{id name key}}}", json!({}))
                        .await?;
                    Ok(v["teams"]["nodes"].clone())
                }
                "jira" => {
                    let v = send(
                        &c,
                        reqwest::Method::GET,
                        "/rest/api/3/project/search?maxResults=100",
                        None,
                    )
                    .await?;
                    Ok(v["values"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|v| json!({"id":v["key"],"name":v["name"]}))
                        .collect::<Vec<_>>()
                        .into())
                }
                "confluence" => {
                    let v = send(
                        &c,
                        reqwest::Method::GET,
                        "/wiki/api/v2/spaces?limit=100",
                        None,
                    )
                    .await?;
                    Ok(v["results"].clone())
                }
                "google" => {
                    let credential = token(&c).await?;
                    let v = response(
                        client()?
                            .get("https://www.googleapis.com/drive/v3/files")
                            .bearer_auth(credential)
                            .query(&[
                                (
                                    "q",
                                    "trashed = false and mimeType = \
                                     'application/vnd.google-apps.document'",
                                ),
                                ("pageSize", "100"),
                                ("fields", "files(id,name,modifiedTime),nextPageToken"),
                                ("orderBy", "modifiedTime desc"),
                            ])
                            .send()
                            .await
                            .map_err(err)?,
                    )
                    .await?;
                    Ok(v["files"].clone())
                }
                _ => Err("Unsupported connection".into()),
            }
        }
        ConnectionAction::Draft {
            id,
            target: mut resource,
            update,
            title,
            body,
            source_meeting_id,
        } => {
            let operation = super::connection_oauth::operation_lock(&id).await;
            let _operation = operation.lock().await;
            let c = connection(pool, &id).await?;
            if super::connection_services::supported(&c) {
                resource = super::connection_services::resolve(&c, &resource).await?;
            } else if c.provider == "notion_mcp" {
                super::connection_mcp::notion_target(&resource)?;
            } else if c.provider == "atlassian_mcp" {
                super::connection_mcp::validate_target(&resource)?;
            } else {
                target(&resource, c.provider == "google" && !update)?;
            }
            if title.trim().is_empty() || title.len() > 300 || body.len() > 64000 {
                return Err("Use a title and a document under 64 KiB".into());
            }
            let revision = if update {
                read(&c, &resource).await?["revision"].to_string()
            } else {
                String::new()
            };
            let draft = uuid::Uuid::new_v4().to_string();
            let kind = if update { "update" } else { "create" };
            sqlx::query(
                "INSERT INTO outgoing_drafts(id,connection_id,kind,target,title,body,\
                 expected_revision,created_at,source_meeting_id) VALUES(?,?,?,?,?,?,?,?,?)",
            )
            .bind(&draft)
            .bind(id)
            .bind(kind)
            .bind(&resource)
            .bind(&title)
            .bind(&body)
            .bind(&revision)
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(source_meeting_id)
            .execute(pool)
            .await
            .map_err(err)?;
            let hash = super::projects::fingerprint(&format!(
                "{draft}\0{resource}\0{title}\0{body}\0{revision}"
            ));
            Ok(json!({
                "id": draft,
                "connection_name": c.name,
                "target": resource,
                "title": title,
                "body": body,
                "hash": hash,
                "effect": if (c.provider == "microsoft" || c.provider == "google") && update {
                    "Append to the document; preserve existing content."
                } else if c.provider == "notion_mcp" && update {
                    "Replace the existing page content with this reviewed text."
                } else {
                    ""
                }
            }))
        }
        ConnectionAction::Publish { id, expected_hash } => {
            let row: Option<(String, String, String, String, String, String, String)> =
                sqlx::query_as(
                    "SELECT connection_id,kind,target,title,body,expected_revision,status \
                     FROM outgoing_drafts WHERE id=?",
                )
                .bind(&id)
                .fetch_optional(pool)
                .await
                .map_err(err)?;
            let (connection_id, kind, resource, title, body, revision, status) =
                row.ok_or("Draft no longer exists")?;
            let operation = super::connection_oauth::operation_lock(&connection_id).await;
            let _operation = operation.lock().await;
            let hash = super::projects::fingerprint(&format!(
                "{id}\0{resource}\0{title}\0{body}\0{revision}"
            ));
            if hash != expected_hash || status != "draft" {
                return Err(
                    "This draft changed or was already submitted. Review its status before \
                     publishing."
                        .into(),
                );
            }
            let c = connection(pool, &connection_id).await?;
            crate::security::privacy::guard_destination(pool, &c.base_url).await?;
            crate::security::audit::append(pool, "external_publish_reviewed", &id, true).await?;
            let claim = sqlx::query(
                "UPDATE outgoing_drafts SET status='publishing' WHERE id=? AND status='draft'",
            )
            .bind(&id)
            .execute(pool)
            .await
            .map_err(err)?;
            if claim.rows_affected() != 1 {
                return Err("This draft is already being published".into());
            }
            let result = publish(&c, &resource, &title, &body, &revision, kind == "update").await;
            sqlx::query("UPDATE outgoing_drafts SET status=?,result=? WHERE id=?")
                .bind(if result.is_ok() {
                    "published"
                } else {
                    "uncertain"
                })
                .bind(
                    result
                        .as_ref()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|e| e.clone()),
                )
                .bind(id)
                .execute(pool)
                .await
                .map_err(err)?;
            result
        }
    }
}

pub(super) async fn sync_google_calendar(
    state: State<'_, AppState>,
    id: String,
) -> Result<usize, String> {
    let c = connection(state.db_manager.pool(), &id).await?;
    if c.provider != "google" {
        return Err("Choose a Google connection".into());
    }
    let credential = token(&c).await?;
    let now = chrono::Utc::now();
    let mut url =
        reqwest::Url::parse("https://www.googleapis.com/calendar/v3/calendars/primary/events")
            .unwrap();
    url.query_pairs_mut().extend_pairs([
        ("timeMin", (now - chrono::Duration::days(1)).to_rfc3339()),
        ("timeMax", (now + chrono::Duration::days(60)).to_rfc3339()),
        ("singleEvents", "true".into()),
        ("maxResults", "250".into()),
        ("orderBy", "startTime".into()),
    ]);
    let mut events = Vec::new();
    let mut page = String::new();
    for _ in 0..4 {
        let mut next = url.clone();
        if !page.is_empty() {
            next.query_pairs_mut().append_pair("pageToken", &page);
        }
        let data = response(
            client()?
                .get(next)
                .bearer_auth(&credential)
                .send()
                .await
                .map_err(err)?,
        )
        .await?;
        events.extend(data["items"].as_array().cloned().unwrap_or_default());
        page = data["nextPageToken"].as_str().unwrap_or("").into();
        if page.is_empty() {
            break;
        }
    }
    if !page.is_empty() {
        return Err(
            "Calendar has more than 1,000 events in this range. Narrow the calendar before syncing."
                .into(),
        );
    }
    let pool = state.db_manager.pool();
    let mut tx = pool.begin().await.map_err(err)?;
    let source = format!("google:{id}");
    sqlx::query("DELETE FROM workspace_calendar WHERE source=?")
        .bind(&source)
        .execute(&mut *tx)
        .await
        .map_err(err)?;
    let mut count = 0;
    let mut own: Option<String> = None;
    for event in events {
        if event["status"] == "cancelled" {
            continue;
        }
        let uid = event["id"].as_str().ok_or("Calendar event has no ID")?;
        let date = |key: &str| -> Result<String, String> {
            if let Some(s) = event[key]["dateTime"].as_str() {
                chrono::DateTime::parse_from_rfc3339(s)
                    .map(|v| v.with_timezone(&chrono::Utc).to_rfc3339())
                    .map_err(err)
            } else {
                let date = event[key]["date"]
                    .as_str()
                    .ok_or("Calendar event has no time")?;
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc().to_rfc3339())
                    .map_err(err)
            }
        };
        let join = event["hangoutLink"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                event["conferenceData"]["entryPoints"]
                    .as_array()
                    .and_then(|a| a.iter().find(|e| e["entryPointType"] == "video"))
                    .and_then(|e| e["uri"].as_str())
                    .map(str::to_owned)
            })
            .or_else(|| {
                super::calendar::meeting_link(&format!(
                    "{} {}",
                    event["description"].as_str().unwrap_or(""),
                    event["location"].as_str().unwrap_or("")
                ))
            });
        let participants = event["attendees"]
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .map(|p| {
                json!({
                    "name": p["displayName"].as_str().unwrap_or(""),
                    "email": p["email"].as_str().unwrap_or("").to_lowercase(),
                    "status": p["responseStatus"].as_str().unwrap_or("needsAction"),
                    "self": p["self"] == true,
                    "organizer": p["organizer"] == true
                })
            })
            .collect::<Vec<_>>();
        if own.is_none() {
            own = participants
                .iter()
                .find(|p| p["self"] == true)
                .and_then(|p| p["email"].as_str())
                .map(str::to_owned);
        }
        sqlx::query(
            "INSERT INTO workspace_calendar(id,source,uid,recurrence_id,title,description,\
             starts_at,ends_at,timezone,all_day,location,meeting_url,participants,organizer,\
             series_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(super::projects::fingerprint(&format!("{source}:{uid}")))
        .bind(&source)
        .bind(uid)
        .bind("")
        .bind(event["summary"].as_str().unwrap_or("Untitled event"))
        .bind(event["description"].as_str().unwrap_or(""))
        .bind(date("start")?)
        .bind(date("end")?)
        .bind(event["start"]["timeZone"].as_str().unwrap_or("UTC"))
        .bind(event["start"]["date"].is_string())
        .bind(event["location"].as_str().unwrap_or(""))
        .bind(join)
        .bind(serde_json::to_string(&participants).map_err(err)?)
        .bind(
            event["organizer"]["email"]
                .as_str()
                .unwrap_or("")
                .to_lowercase(),
        )
        .bind(event["recurringEventId"].as_str().unwrap_or(""))
        .execute(&mut *tx)
        .await
        .map_err(err)?;
        count += 1;
    }
    // Google sign-in does not name its account; the calendar's own entry does.
    if let Some(own) = own.filter(|email| email.contains('@')) {
        sqlx::query(
            "UPDATE workspace_connections SET username=? WHERE id=? \
             AND username IN ('','Browser sign-in')",
        )
        .bind(own)
        .bind(&id)
        .execute(&mut *tx)
        .await
        .map_err(err)?;
    }
    tx.commit().await.map_err(err)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_do_not_leak_credentials() {
        for url in [
            "http://team.atlassian.net",
            "https://team.atlassian.net.evil.test",
            "https://u:p@team.atlassian.net",
            "https://team.atlassian.net/path",
            "https://127.0.0.1",
        ] {
            assert!(validate_base("jira", url).is_err());
        }
        assert_eq!(
            validate_base("jira", "https://team.atlassian.net/").unwrap(),
            "https://team.atlassian.net"
        );
    }
    #[test]
    fn resource_ids_are_not_paths() {
        for id in ["../secret", "abc?x=1", "https://test", "a/b"] {
            assert!(target(id, false).is_err());
        }
        assert!(target("TEAM-4", false).is_ok());
    }
    #[test]
    fn confluence_content_is_escaped() {
        assert_eq!(html("<script>&"), "<p>&lt;script&gt;&amp;</p>");
    }
}
