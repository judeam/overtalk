//! Bounded Streamable HTTP client for the official Linear and Atlassian servers.
//! Remote tools are discovered for schema compatibility, never given authority
//! to choose actions. Only the explicit read/draft/publish workflows call them.
use super::{
    connection_oauth,
    connections::{client, Connection},
};
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

struct Session {
    id: Option<String>,
    version: String,
    tools: Vec<Value>,
    created: Instant,
}
static SESSIONS: LazyLock<Mutex<HashMap<String, Arc<Mutex<Session>>>>> =
    LazyLock::new(Default::default);
const MAX_SESSIONS: usize = 16;
const MAX_CATALOG_BYTES: usize = 2 * 1024 * 1024;

fn session_cell(
    sessions: &mut HashMap<String, Arc<Mutex<Session>>>,
    id: &str,
) -> Result<Arc<Mutex<Session>>, String> {
    if let Some(cell) = sessions.get(id) {
        return Ok(cell.clone());
    }
    if sessions.len() >= MAX_SESSIONS {
        // A caller can be between acquiring the Arc and locking it. Checking
        // only try_lock would detach that caller and create parallel sessions.
        let oldest = sessions
            .iter()
            .filter(|(_, cell)| Arc::strong_count(cell) == 1)
            .filter_map(|(id, cell)| cell.try_lock().ok().map(|s| (id.clone(), s.created)))
            .min_by_key(|(_, created)| *created)
            .map(|(id, _)| id);
        if let Some(id) = oldest {
            sessions.remove(&id);
        } else {
            return Err("Connected services are busy. Try again shortly.".into());
        }
    }
    let cell = Arc::new(Mutex::new(Session {
        id: None,
        version: "2025-06-18".into(),
        tools: Vec::new(),
        created: Instant::now() - Duration::from_secs(3600),
    }));
    sessions.insert(id.into(), cell.clone());
    Ok(cell)
}

fn extend_catalog(tools: &mut Vec<Value>, page: &[Value], bytes: &mut usize) -> Result<(), String> {
    let page_bytes = page
        .iter()
        .map(|tool| tool.to_string().len())
        .sum::<usize>();
    if tools.len() + page.len() > 1000 || page_bytes > MAX_CATALOG_BYTES.saturating_sub(*bytes) {
        return Err("Connected service tool catalogue is too large".into());
    }
    *bytes += page_bytes;
    tools.extend_from_slice(page);
    Ok(())
}
pub(super) fn is_mcp(c: &Connection) -> bool {
    matches!(
        c.provider.as_str(),
        "linear_mcp" | "atlassian_mcp" | "notion_mcp"
    )
}
pub(super) async fn forget(id: &str) {
    SESSIONS.lock().await.remove(id);
}
fn endpoint(c: &Connection) -> Result<&'static str, String> {
    match c.provider.as_str() {
        "linear_mcp" => Ok("https://mcp.linear.app/mcp"),
        "atlassian_mcp" => Ok("https://mcp.atlassian.com/v2/mcp?tools=all"),
        "notion_mcp" => Ok("https://mcp.notion.com/mcp"),
        _ => Err("Unsupported account connection".into()),
    }
}
fn decode_rpc(value: Value, id: &Value) -> Result<Option<Value>, String> {
    if value["id"] != *id {
        return Ok(None);
    }
    if !value["error"].is_null() {
        return Err("The connected service rejected this request. Check your permissions and destination before retrying.".into());
    }
    value
        .get("result")
        .cloned()
        .map(Some)
        .ok_or("Invalid response from connected service".into())
}
fn event_result(buffer: &[u8], id: &Value) -> Result<Option<Value>, String> {
    let text = match std::str::from_utf8(buffer) {
        Ok(text) => text,
        Err(error) if error.error_len().is_none() => return Ok(None),
        Err(_) => return Err("Invalid event encoding from connected service".into()),
    };
    let normalized = text.replace("\r\n", "\n");
    let text = normalized.as_str();
    for event in text.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str(&data) {
            if let Some(result) = decode_rpc(value, id)? {
                return Ok(Some(result));
            }
        }
    }
    Ok(None)
}

/// Parse completed SSE events once. Re-parsing the entire accumulated response
/// on every network fragment makes a large tool result quadratic in its size.
#[derive(Default)]
struct EventBuffer {
    bytes: Vec<u8>,
    scan: usize,
}
impl EventBuffer {
    fn push(&mut self, chunk: &[u8], id: &Value) -> Result<Option<Value>, String> {
        self.bytes.extend_from_slice(chunk);
        let mut consumed = 0;
        while self.scan + 1 < self.bytes.len() {
            if self.bytes[self.scan] != b'\n' {
                self.scan += 1;
                continue;
            }
            let end = match self.bytes[self.scan + 1] {
                b'\n' => self.scan + 2,
                b'\r' if self.scan + 2 == self.bytes.len() => break,
                b'\r' if self.bytes[self.scan + 2] == b'\n' => self.scan + 3,
                _ => {
                    self.scan += 1;
                    continue;
                }
            };
            if let Some(result) = event_result(&self.bytes[consumed..self.scan + 1], id)? {
                return Ok(Some(result));
            }
            consumed = end;
            self.scan = end;
        }
        if consumed > 0 {
            self.bytes.drain(..consumed);
            self.scan -= consumed;
        }
        Ok(None)
    }

    fn finish(&self, id: &Value) -> Result<Option<Value>, String> {
        event_result(&self.bytes, id)
    }
}
async fn rpc(
    c: &Connection,
    session: &mut Session,
    method: &str,
    params: Value,
    notification: bool,
) -> Result<Value, String> {
    let destination =
        reqwest::Url::parse(endpoint(c)?).map_err(|_| "Invalid connected service endpoint")?;
    crate::security::privacy::guard_process_destination(
        &destination.origin().ascii_serialization(),
    )?;
    let token = connection_oauth::access_token(c).await?;
    let id = json!(uuid::Uuid::new_v4().to_string());
    let mut body = json!({"jsonrpc":"2.0","method":method,"params":params});
    if !notification {
        body["id"] = id.clone();
    }
    let mut request = client()?
        .post(endpoint(c)?)
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", &session.version)
        .json(&body);
    if let Some(id) = &session.id {
        request = request.header("Mcp-Session-Id", id);
    }
    let response = request
        .send()
        .await
        .map_err(|_| "Connection interrupted. Review remote content before retrying a change.")?;
    read_response(response, session, &id, notification).await
}
async fn read_response(
    response: reqwest::Response,
    session: &mut Session,
    id: &Value,
    notification: bool,
) -> Result<Value, String> {
    if !response.status().is_success() {
        if response.status().as_u16() == 404 {
            session.created = Instant::now() - Duration::from_secs(3600);
        }
        return Err(format!(
            "Connected service returned HTTP {}. Reconnect if your session expired.",
            response.status().as_u16()
        ));
    }
    if let Some(id) = response
        .headers()
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
    {
        if id.len() > 1024 {
            return Err("Invalid service session ID".into());
        }
        session.id = Some(id.into());
    }
    if notification {
        return Ok(Value::Null);
    }
    let sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/event-stream"));
    let mut bytes = Vec::new();
    let mut events = EventBuffer::default();
    let mut received = 0usize;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Connected service response was interrupted")?;
        received = received.saturating_add(chunk.len());
        if received > 4 * 1024 * 1024 {
            return Err("Connected service response exceeds 4 MiB".into());
        }
        if sse {
            if let Some(result) = events.push(&chunk, id)? {
                return Ok(result);
            }
        } else {
            bytes.extend_from_slice(&chunk);
        }
    }
    if sse {
        return events
            .finish(id)?
            .ok_or("Connected service sent no matching response".into());
    }
    decode_rpc(
        serde_json::from_slice(&bytes).map_err(|_| "Connected service returned invalid JSON")?,
        id,
    )?
    .ok_or("Connected service returned a mismatched response".into())
}
async fn session(c: &Connection) -> Result<Arc<Mutex<Session>>, String> {
    endpoint(c)?;
    let cell = {
        let mut sessions = SESSIONS.lock().await;
        session_cell(&mut sessions, &c.id)?
    };
    let mut s = cell.lock().await;
    if s.created.elapsed() > Duration::from_secs(600) {
        s.id = None;
        s.tools.clear();
        s.version = "2025-06-18".into();
        let init=rpc(c,&mut s,"initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"Overtalk","version":env!("CARGO_PKG_VERSION")}}),false).await?;
        let version = init["protocolVersion"].as_str().unwrap_or("");
        if !matches!(
            version,
            "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25"
        ) {
            return Err("The service requires an unsupported MCP version".into());
        }
        s.version = version.into();
        rpc(c, &mut s, "notifications/initialized", json!({}), true).await?;
        let mut cursor = None;
        let mut seen = HashSet::new();
        let mut catalog_bytes = 0;
        for _ in 0..20 {
            let params = cursor
                .as_ref()
                .map(|c| json!({"cursor":c}))
                .unwrap_or(json!({}));
            let result = rpc(c, &mut s, "tools/list", params, false).await?;
            let tools = result["tools"]
                .as_array()
                .ok_or("Connected service omitted its tool catalogue")?;
            extend_catalog(&mut s.tools, tools, &mut catalog_bytes)?;
            cursor = result["nextCursor"].as_str().map(str::to_owned);
            if let Some(cursor) = &cursor {
                if !seen.insert(cursor.clone()) {
                    return Err("Connected service repeated a catalogue page".into());
                }
            } else {
                s.created = Instant::now();
                drop(s);
                return Ok(cell);
            }
        }
        return Err("Connected service catalogue has too many pages".into());
    }
    drop(s);
    Ok(cell)
}
fn args_for(schema: &Value, args: Value) -> Result<Value, String> {
    let properties = schema["properties"]
        .as_object()
        .ok_or("Connected tool has no supported input schema")?;
    let mut args = args.as_object().cloned().ok_or("Invalid tool arguments")?;
    args.retain(|k, _| properties.contains_key(k));
    for required in schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if !args.contains_key(required) {
            return Err(format!("The connected tool now requires {required}. Update Overtalk before using this action."));
        }
    }
    Ok(Value::Object(args))
}
async fn tool(c: &Connection, names: &[&str], args: Value) -> Result<Value, String> {
    let cell = session(c).await?;
    let mut session = cell.lock().await;
    let t = names
        .iter()
        .find_map(|name| session.tools.iter().find(|t| t["name"] == *name))
        .ok_or("This action is not available with the permissions granted to Overtalk")?;
    let name = t["name"]
        .as_str()
        .ok_or("Invalid remote tool name")?
        .to_owned();
    let args = args_for(&t["inputSchema"], args)?;
    let result = rpc(
        c,
        &mut session,
        "tools/call",
        json!({"name":name,"arguments":args}),
        false,
    )
    .await?;
    if result["isError"] == true {
        return Err("The service could not complete the action. Review the destination, required fields and account permissions.".into());
    }
    if let Some(value) = result.get("structuredContent") {
        return Ok(payload(value).clone());
    }
    let content = result["content"]
        .as_array()
        .ok_or("Connected service returned no content")?;
    let text = content
        .iter()
        .filter(|v| v["type"] == "text")
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return Err("Connected service returned no readable content".into());
    }
    let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    Ok(payload(&value).clone())
}
fn payload(mut value: &Value) -> &Value {
    for _ in 0..3 {
        match value.get("data") {
            Some(inner) if inner.is_object() || inner.is_array() => value = inner,
            _ => break,
        }
    }
    value
}
fn rows(v: &Value) -> &[Value] {
    let v = payload(v);
    if let Some(a) = v.as_array() {
        return a;
    }
    for key in [
        "results",
        "values",
        "nodes",
        "data",
        "teams",
        "projects",
        "spaces",
        "resources",
    ] {
        if let Some(a) = v[key].as_array() {
            return a;
        }
    }
    &[]
}
fn string<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|k| v[*k].as_str()).unwrap_or("")
}
pub(super) fn validate_target(target: &str) -> Result<(), String> {
    let parts = target.split(':').collect::<Vec<_>>();
    if parts.len() != 3
        || !matches!(parts[0], "jira" | "confluence")
        || parts[1..].iter().any(|p| {
            p.is_empty()
                || p.len() > 200
                || !p
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
    {
        return Err("Choose a destination first. Atlassian targets use jira:site-id:issue-key or confluence:site-id:page-id.".into());
    }
    Ok(())
}
pub(super) async fn browse(c: &Connection) -> Result<Value, String> {
    if c.provider == "notion_mcp" {
        let v = tool(
            c,
            &["notion-search", "search"],
            json!({"query":"","query_type":"internal","page_size":100}),
        )
        .await?;
        let mut items = vec![json!({"id":"","name":"Private pages · create a new page"})];
        items.extend(
            rows(&v)
                .iter()
                .filter(|r| r["object"] != "database")
                .map(|r| json!({"id":r["id"],"name":string(r,&["title","name","url"])})),
        );
        return Ok(items.into());
    }
    if c.provider == "linear_mcp" {
        let v = tool(c, &["list_teams"], json!({"limit":100})).await?;
        return Ok(rows(&v)
            .iter()
            .map(|v| json!({"id":v["id"],"name":v["name"]}))
            .collect::<Vec<_>>()
            .into());
    }
    let resources = tool(c, &["getAccessibleAtlassianResources"], json!({})).await?;
    let mut out = Vec::new();
    for site in rows(&resources).iter().take(20) {
        let cloud = string(site, &["id", "cloudId"]);
        if cloud.is_empty() {
            continue;
        }
        // Some sites provide only one product. Keep the available product's
        // destinations if the other is not included in the account grant.
        for (kind, names) in [
            ("jira", vec!["listJiraProjects"]),
            (
                "confluence",
                vec!["listConfluenceSpaces", "getConfluenceSpaces"],
            ),
        ] {
            let result = tool(
                c,
                &names,
                json!({"cloudId":cloud,"limit":100,"maxResults":100}),
            )
            .await;
            if let Ok(v) = result {
                for row in rows(&v).iter().take(100) {
                    let id = if kind == "jira" {
                        string(row, &["key", "id"])
                    } else {
                        string(row, &["id", "key"])
                    };
                    if !id.is_empty() {
                        out.push(json!({"id":format!("{kind}:{cloud}:{id}"),"name":format!("{} · {} · {}",string(site,&["name","url"]),if kind=="jira"{"Jira"}else{"Confluence"},string(row,&["name","title"]))}));
                    }
                }
            }
        }
    }
    if out.is_empty() {
        return Err("No Jira projects or Confluence spaces are available. Check the sites and permissions selected during sign-in.".into());
    }
    Ok(out.into())
}
pub(super) async fn read(c: &Connection, target: &str) -> Result<Value, String> {
    if c.provider == "notion_mcp" {
        let id = notion_target(target)?;
        if id.is_empty() {
            return Err("Choose a Notion page to read".into());
        }
        let v = tool(c, &["notion-fetch", "fetch"], json!({"id":id,"url":id})).await?;
        let body = v
            .get("text")
            .or_else(|| v.get("content"))
            .cloned()
            .unwrap_or_else(|| v.clone());
        let stable = body
            .as_str()
            .map(|s| s.find("<page").map(|i| &s[i..]).unwrap_or(s));
        let revision = v.get("last_edited_time").cloned().unwrap_or_else(|| {
            json!(super::projects::fingerprint(
                stable.unwrap_or(&body.to_string())
            ))
        });
        return Ok(
            json!({"title":v["title"].as_str().unwrap_or("Notion page"),"text":body,"revision":revision,"url":v["url"]}),
        );
    }
    let v = if c.provider == "linear_mcp" {
        tool(c, &["get_issue"], json!({"id":target})).await?
    } else {
        validate_target(target)?;
        let p = target.split(':').collect::<Vec<_>>();
        if p[0] == "jira" {
            tool(c,&["getJiraIssue"],json!({"cloudId":p[1],"issueIdOrKey":p[2],"fields":["summary","description","updated"],"responseContentFormat":"markdown"})).await?
        } else {
            tool(c,&["getConfluenceContent","getConfluencePage"],json!({"cloudId":p[1],"content_id":p[2],"pageId":p[2],"content_format":"markdown","bodyFormat":"storage","detail":"full"})).await?
        }
    };
    normalize_read(v)
}
fn normalize_read(v: Value) -> Result<Value, String> {
    // Hash the stable, complete resource when no provider revision is exposed.
    // This still detects intervening edits, but is not an atomic compare-and-set.
    let item = v.get("issue").or_else(|| v.get("page")).unwrap_or(&v);
    let title = string(item, &["title", "summary"]);
    let title = if title.is_empty() {
        item["fields"]["summary"]
            .as_str()
            .unwrap_or("Remote content")
    } else {
        title
    };
    let body = item
        .get("description")
        .or_else(|| item.pointer("/body/value"))
        .or_else(|| item.pointer("/body/storage/value"))
        .or_else(|| item.pointer("/fields/description"))
        .or_else(|| item.get("body"))
        .cloned()
        .unwrap_or_else(|| v.clone());
    let revision = item
        .get("updatedAt")
        .or_else(|| item.pointer("/fields/updated"))
        .or_else(|| item.get("version"))
        .cloned()
        .unwrap_or_else(|| json!(super::projects::fingerprint(&v.to_string())));
    Ok(json!({"title":title,"text":body,"revision":revision,"url":string(item,&["url","webUrl"])}))
}
pub(super) async fn publish(
    c: &Connection,
    target: &str,
    title: &str,
    body: &str,
    update: bool,
    revision: &str,
) -> Result<Value, String> {
    if c.provider == "notion_mcp" {
        let id = notion_target(target)?;
        let value = if update {
            if id.is_empty() {
                return Err("Choose a Notion page to update".into());
            }
            tool(
                c,
                &["notion-update-page"],
                json!({"page_id":id,"command":"replace_content","new_str":body,"allow_async":true}),
            )
            .await?
        } else {
            let mut args =
                json!({"pages":[{"properties":{"title":title},"content":body}],"allow_async":true});
            if id.is_empty() {
                args["creation_mode"] = json!("draft");
            } else {
                args["parent"] = json!({"page_id":id});
            }
            tool(c, &["notion-create-pages"], args).await?
        };
        return finish_notion(c, value).await;
    }
    if c.provider == "linear_mcp" {
        return tool(
            c,
            &["save_issue"],
            if update {
                json!({"id":target,"title":title,"description":body})
            } else {
                json!({"team":target,"title":title,"description":body})
            },
        )
        .await;
    }
    validate_target(target)?;
    let p = target.split(':').collect::<Vec<_>>();
    if p[0] == "jira" {
        if update {
            tool(c,&["editJiraIssue"],json!({"cloudId":p[1],"issueIdOrKey":p[2],"fields":{"summary":title,"description":body},"contentFormat":"markdown"})).await
        } else {
            tool(c,&["createJiraIssue"],json!({"cloudId":p[1],"projectKey":p[2],"issueType":"Task","summary":title,"description":body,"contentFormat":"markdown"})).await
        }
    } else if update {
        let rev: Value = serde_json::from_str(revision).unwrap_or(Value::Null);
        let version = rev.as_u64().or_else(|| rev["number"].as_u64());
        // The server's modern content tool handles its own revision validation.
        // Keep body updates explicit and never retry an uncertain write.
        tool(c,&["updateConfluenceContent","updateConfluencePage"],json!({"cloudId":p[1],"contentId":p[2],"content_id":p[2],"pageId":p[2],"title":title,"body":{"format":"markdown","value":body},"version":version.map(|v|v+1),"versionNumber":version.map(|v|v+1),"contentFormat":"markdown"})).await
    } else {
        tool(c,&["createConfluenceContent","createConfluencePage"],json!({"cloudId":p[1],"contentType":"page","parent":{"spaceId":p[2]},"spaceId":p[2],"title":title,"body":{"format":"markdown","value":body},"contentFormat":"markdown"})).await
    }
}
pub(super) fn notion_target(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let id = if value.starts_with("https://") {
        let url = reqwest::Url::parse(value).map_err(|_| "Invalid Notion link")?;
        if url.scheme() != "https"
            || url.username() != ""
            || url.password().is_some()
            || url.port().is_some()
            || !url.host_str().is_some_and(|h| {
                h == "notion.so" || h == "www.notion.so" || h.ends_with(".notion.site")
            })
        {
            return Err("Use a Notion page link".into());
        }
        let last = url
            .path_segments()
            .and_then(|mut p| p.rfind(|s| !s.is_empty()))
            .ok_or("Notion link has no page ID")?;
        let tail = last.get(last.len().saturating_sub(32)..).unwrap_or(last);
        if tail.bytes().all(|b| b.is_ascii_hexdigit()) && tail.len() == 32 {
            tail.to_owned()
        } else {
            last.to_owned()
        }
    } else {
        value.to_owned()
    };
    uuid::Uuid::parse_str(&id)
        .map(|id| id.to_string())
        .map_err(|_| "Choose a Notion page or paste its full link".into())
}
async fn finish_notion(c: &Connection, mut value: Value) -> Result<Value, String> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let task = value.get("async_task").unwrap_or(&value);
        if task["object"] != "async_task" && value.get("async_task").is_none() {
            return Ok(value);
        }
        match task["status"].as_str(){Some("succeeded")=>return Ok(task.get("result").cloned().unwrap_or(value)),Some("failed")=>return Err("Notion could not finish the queued change. Inspect the page before creating another draft.".into()),_=>{}}
        if Instant::now() >= deadline {
            return Err(
                "Notion is still processing the change. Inspect the page before retrying.".into(),
            );
        }
        let id = task["id"]
            .as_str()
            .filter(|s| s.len() < 200)
            .ok_or("Notion returned no task ID")?
            .to_owned();
        let seconds = task["poll_after_seconds"]
            .as_u64()
            .unwrap_or(2)
            .clamp(1, 10);
        tokio::time::sleep(Duration::from_secs(seconds)).await;
        value = tool(c, &["notion-get-async-task"], json!({"task_id":id})).await?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_cache_is_bounded_and_does_not_evict_active_callers() {
        let mut cells = HashMap::new();
        let active = (0..MAX_SESSIONS)
            .map(|i| session_cell(&mut cells, &i.to_string()).unwrap())
            .collect::<Vec<_>>();
        assert!(session_cell(&mut cells, "overflow").is_err());
        assert!(Arc::ptr_eq(
            &active[0],
            &session_cell(&mut cells, "0").unwrap()
        ));
        drop(active);
        assert!(session_cell(&mut cells, "overflow").is_ok());
        assert_eq!(cells.len(), MAX_SESSIONS);
    }
    #[test]
    fn catalogue_budget_applies_across_pages() {
        let page = [json!({"name":"large", "description":"x".repeat(MAX_CATALOG_BYTES / 2)})];
        let mut tools = Vec::new();
        let mut bytes = 0;
        extend_catalog(&mut tools, &page, &mut bytes).unwrap();
        assert!(extend_catalog(&mut tools, &page, &mut bytes).is_err());
        assert_eq!(tools.len(), 1);
    }
    #[test]
    fn incremental_events_handle_split_utf8_crlf_and_unrelated_messages() {
        let data = concat!(
            ": keepalive\r\n\r\n",
            "data: {\"id\":\"other\",\"result\":false}\n\n",
            "event: message\r\ndata: {\"id\":\"wanted\",\"result\":\"नमस्ते 日本語\"}\r\n\r\n"
        );
        for size in 1..18 {
            let mut events = EventBuffer::default();
            let mut result = None;
            for chunk in data.as_bytes().chunks(size) {
                if let Some(value) = events.push(chunk, &json!("wanted")).unwrap() {
                    result = Some(value);
                    break;
                }
            }
            assert_eq!(result, Some(json!("नमस्ते 日本語")), "fragment size {size}");
        }
        let mut events = EventBuffer::default();
        for _ in 0..1000 {
            assert!(events
                .push(b": keepalive\n\n", &json!(1))
                .unwrap()
                .is_none());
            assert!(
                events.bytes.is_empty(),
                "completed events must not accumulate"
            );
        }
        assert!(events
            .push(b"data: {\"id\":1,\"result\":true}", &json!(1))
            .unwrap()
            .is_none());
        assert_eq!(events.finish(&json!(1)).unwrap(), Some(json!(true)));
    }

    #[test]
    #[ignore = "manual performance comparison of fragmented tool responses"]
    fn benchmark_fragmented_tool_response() {
        let data = format!(
            "data: {{\"id\":\"other\",\"result\":\"{}\"}}\n\ndata: {{\"id\":1,\"result\":true}}\n\n",
            "x".repeat(256 * 1024)
        );
        let id = json!(1);
        let started = Instant::now();
        let mut buffer = Vec::new();
        let mut old_result = None;
        for chunk in data.as_bytes().chunks(256) {
            buffer.extend_from_slice(chunk);
            old_result = event_result(&buffer, &id).unwrap();
        }
        let before = started.elapsed();
        let started = Instant::now();
        let mut events = EventBuffer::default();
        let mut new_result = None;
        for chunk in data.as_bytes().chunks(256) {
            new_result = events.push(chunk, &id).unwrap();
        }
        let after = started.elapsed();
        assert_eq!(old_result, Some(json!(true)));
        assert_eq!(new_result, old_result);
        println!(
            "SSE_PERF before_ms={:.3} after_ms={:.3} bytes={} fragment_bytes=256",
            before.as_secs_f64() * 1000.0,
            after.as_secs_f64() * 1000.0,
            data.len()
        );
    }

    #[test]
    fn notion_targets_keep_page_identity_and_reject_foreign_links() {
        let id = "12345678-1234-1234-1234-123456789abc";
        assert_eq!(notion_target(id).unwrap(), id);
        assert_eq!(
            notion_target("https://www.notion.so/Project-12345678123412341234123456789abc")
                .unwrap(),
            id
        );
        assert_eq!(notion_target("").unwrap(), "");
        for target in [
            "https://notion.so.evil.test/12345678123412341234123456789abc",
            "https://user@www.notion.so/12345678123412341234123456789abc",
            "../../page",
            "https://notion.so/",
        ] {
            assert!(notion_target(target).is_err());
        }
    }
    #[tokio::test]
    async fn streamable_http_handles_json_sse_and_session_headers() {
        use tokio::io::AsyncWriteExt;
        for (mime, body) in [
            (
                "application/json",
                r#"{"jsonrpc":"2.0","id":"test","result":{"ok":true}}"#,
            ),
            (
                "text/event-stream",
                "event: message\r\ndata: {\"id\":\"test\",\"result\":{\"ok\":true}}\r\n\r\n",
            ),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let reply=format!("HTTP/1.1 200 OK\r\nContent-Type: {mime}\r\nMcp-Session-Id: session-test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
            tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                crate::test_support::read_http_headers(&mut stream).await;
                stream.write_all(reply.as_bytes()).await.unwrap();
            });
            let response = reqwest::Client::new()
                .get(format!("http://{address}"))
                .send()
                .await
                .unwrap();
            let mut session = Session {
                id: None,
                version: "2025-06-18".into(),
                tools: vec![],
                created: Instant::now(),
            };
            assert_eq!(
                read_response(response, &mut session, &json!("test"), false)
                    .await
                    .unwrap(),
                json!({"ok":true})
            );
            assert_eq!(session.id.as_deref(), Some("session-test"));
        }
    }
    #[test]
    fn sse_accepts_utf8_split_at_a_network_boundary() {
        let bytes = "data: {\"id\":1,\"result\":\"नमस्ते\"}\n\n".as_bytes();
        let split = bytes.iter().position(|b| *b >= 128).unwrap() + 1;
        assert!(event_result(&bytes[..split], &json!(1)).unwrap().is_none());
        assert!(event_result(bytes, &json!(1)).unwrap().is_some());
    }
    #[test]
    fn sse_matches_ids_and_propagates_rpc_errors() {
        let data=b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"ping\"}\n\nevent: message\ndata: {\"id\":\"x\",\"result\":{\"ok\":true}}\n\n";
        assert_eq!(
            event_result(data, &json!("x")).unwrap(),
            Some(json!({"ok":true}))
        );
        assert_eq!(event_result(data, &json!("other")).unwrap(), None);
        assert!(decode_rpc(
            json!({"id":1,"error":{"message":"secret must not escape"}}),
            &json!(1)
        )
        .unwrap_err()
        .find("secret")
        .is_none());
    }
    #[test]
    fn tool_schema_blocks_missing_fields_and_drops_unwanted_actions() {
        let schema = json!({"properties":{"id":{},"title":{}},"required":["id"]});
        assert!(args_for(&schema, json!({"title":"x"})).is_err());
        assert_eq!(
            args_for(&schema, json!({"id":"a","delete":true})).unwrap(),
            json!({"id":"a"})
        );
    }
    #[test]
    fn atlassian_target_is_not_a_url_or_injected_path() {
        assert!(validate_target("jira:abc-123:TEAM-1").is_ok());
        for target in [
            "jira:https://evil:TEAM-1",
            "confluence:site:../secret",
            "jira:site:TEAM-1:delete",
            "jira:site:",
        ] {
            assert!(validate_target(target).is_err());
        }
    }
    #[test]
    fn provider_data_envelopes_keep_the_resource() {
        let v = json!({"data":{"results":[{"id":"one"}]}});
        assert_eq!(rows(&v).len(), 1);
        assert_eq!(
            payload(&json!({"data":{"title":"Title"}}))["title"],
            "Title"
        );
    }
    #[test]
    fn remote_revision_changes_with_content() {
        let a = normalize_read(json!({"title":"A","description":"old"})).unwrap();
        let b = normalize_read(json!({"title":"A","description":"new"})).unwrap();
        assert_ne!(a["revision"], b["revision"]);
    }
}
