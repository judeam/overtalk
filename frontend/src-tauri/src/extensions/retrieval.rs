//! Exact local dense retrieval + BM25 reciprocal-rank fusion + optional reranking.
use super::knowledge::{fingerprint, search, SearchRequest, SearchResponse, Source};
use crate::state::AppState;
use futures_util::TryStreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
static INDEX_JOB: Lazy<Mutex<Option<CancellationToken>>> = Lazy::new(|| Mutex::new(None));

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RetrievalSettings {
    pub enabled: bool,
    pub python: String,
    pub device: String,
    pub size: String,
    pub family: String,
    pub reranker_size: String,
    pub embedding_dir: String,
    pub reranker_dir: String,
    pub rerank: bool,
}
impl Default for RetrievalSettings {
    fn default() -> Self {
        let profile = super::quality_profile::detected();
        Self {
            enabled: true,
            python: if cfg!(windows) { "python" } else { "python3" }.into(),
            device: profile.device.into(),
            size: profile.embedding_size.into(),
            family: profile.embedding_family.into(),
            reranker_size: profile.reranker_size.into(),
            embedding_dir: String::new(),
            reranker_dir: String::new(),
            rerank: true,
        }
    }
}
fn path<R: Runtime>(app: &AppHandle<R>) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("library-runtime.json"))
}
pub(crate) fn read_settings<R: Runtime>(app: &AppHandle<R>) -> Result<RetrievalSettings, String> {
    match crate::security::storage::read_limited(&path(app)?, 64 * 1024) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(RetrievalSettings::default()),
        Err(e) => Err(e.to_string()),
    }
}
#[tauri::command]
pub fn library_get_settings(app: AppHandle) -> Result<RetrievalSettings, String> {
    read_settings(&app)
}
#[tauri::command]
pub async fn library_save_settings(
    app: AppHandle,
    settings: RetrievalSettings,
) -> Result<(), String> {
    if settings.python.trim().is_empty()
        || settings.python.len() > 4096
        || !matches!(settings.device.as_str(), "cpu" | "cuda" | "auto")
        || !matches!(settings.size.as_str(), "0.6B" | "4B" | "8B")
        || !matches!(settings.family.as_str(), "qwen3" | "nemotron3")
        || !matches!(settings.reranker_size.as_str(), "0.6B" | "4B" | "8B")
        || (settings.family == "nemotron3" && settings.size != "8B")
    {
        return Err("Choose a supported model size, Python runtime and device".into());
    }
    if INDEX_JOB.lock().await.is_some() {
        return Err("Finish or cancel indexing before changing settings".into());
    }
    super::speech::shutdown_knowledge().await;
    crate::integrations::write_private_json(&path(&app)?, &settings)
}
fn request(config: &RetrievalSettings, operation: &str) -> Value {
    json!({"operation":operation,"size":if operation=="knowledge_rerank"{&config.reranker_size}else{&config.size},"family":config.family,"reranker_size":config.reranker_size,"device":config.device,"embedding_dir":config.embedding_dir,"reranker_dir":config.reranker_dir})
}
async fn health<R: Runtime>(
    app: &AppHandle<R>,
    config: &RetrievalSettings,
) -> Result<Value, String> {
    if !config.enabled {
        return Err("Enable semantic search in Library settings first".into());
    }
    super::speech::request(
        app,
        request(config, "knowledge_health"),
        &CancellationToken::new(),
    )
    .await
}
#[tauri::command]
pub async fn library_health(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let config = read_settings(&app)?;
    let status = health(&app, &config).await?;
    let revision = status["embedding"]["revision"].as_str().unwrap_or("");
    let total: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_documents WHERE length(body)>0")
            .fetch_one(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
    let indexed: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workspace_vectors WHERE model_revision=?")
            .bind(revision)
            .fetch_one(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?;
    Ok(
        json!({"runtime":status,"total":total,"indexed":indexed,"enabled":config.enabled,"size":config.size}),
    )
}
async fn infer<R: Runtime>(
    app: &AppHandle<R>,
    config: &RetrievalSettings,
    operation: &str,
    texts: Vec<String>,
    query: Value,
    token: &CancellationToken,
) -> Result<Value, String> {
    let mut value = request(config, operation);
    value["texts"] = json!(texts);
    value["query"] = query;
    value["directory"] = json!(if operation == "knowledge_rerank" {
        &config.reranker_dir
    } else {
        &config.embedding_dir
    });
    super::speech::request(app, value, token).await
}
fn vectors(response: &Value, expected: usize) -> Result<Vec<Vec<f32>>, String> {
    let result: Vec<Vec<f32>> = serde_json::from_value(response["vectors"].clone())
        .map_err(|_| "Invalid embedding response")?;
    if result.len() != expected
        || result
            .iter()
            .any(|v| v.is_empty() || v.len() > 4096 || v.iter().any(|x| !x.is_finite()))
    {
        return Err("Invalid embedding dimensions or numbers".into());
    }
    Ok(result)
}
fn text(source: &Source) -> String {
    format!(
        "{}\n{}",
        source.title,
        source.body.chars().take(5500).collect::<String>()
    )
    .chars()
    .take(6000)
    .collect()
}
#[tauri::command]
pub async fn library_index(app: AppHandle, state: State<'_, AppState>) -> Result<Value, String> {
    let token = CancellationToken::new();
    {
        let mut job = INDEX_JOB.lock().await;
        if job.is_some() {
            return Err("Indexing is already running".into());
        }
        *job = Some(token.clone());
    }
    let result = index_inner(&app, state.db_manager.pool(), &token).await;
    *INDEX_JOB.lock().await = None;
    result
}
#[tauri::command]
pub async fn library_cancel_index() {
    if let Some(token) = INDEX_JOB.lock().await.as_ref() {
        token.cancel();
    }
}
async fn index_inner(
    app: &AppHandle,
    pool: &SqlitePool,
    token: &CancellationToken,
) -> Result<Value, String> {
    let config = read_settings(app)?;
    let status = health(app, &config).await?;
    let revision = status["embedding"]["revision"]
        .as_str()
        .ok_or("Embedding model is not cached; download it separately first")?;
    let mut last = String::new();
    let mut updated = 0u64;
    let mut inspected = 0u64;
    loop {
        let sources: Vec<Source> = sqlx::query_as(
            "SELECT id,meeting_id,source_kind,source_id,title,body,start_time,end_time,recorded_at \
             FROM workspace_documents WHERE id>? AND length(body)>0 AND (source_kind!='transcript' \
             OR NOT EXISTS(SELECT 1 FROM transcripts t WHERE t.id=source_id AND t.is_partial=1)) \
             ORDER BY id LIMIT 1",
        )
        .bind(&last)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        if sources.is_empty() {
            break;
        }
        last = sources.last().unwrap().id.clone();
        inspected += sources.len() as u64;
        let mut changed = Vec::new();
        for source in sources {
            let stored: Option<String> = sqlx::query_scalar(
                "SELECT text_hash FROM workspace_vectors WHERE document_id=? AND model_revision=?",
            )
            .bind(&source.id)
            .bind(revision)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
            if stored.as_deref() != Some(&fingerprint(&source)) {
                changed.push(source);
            }
        }
        if !changed.is_empty() {
            let response = infer(
                app,
                &config,
                "knowledge_embed",
                changed.iter().map(text).collect(),
                json!(false),
                token,
            )
            .await?;
            let embeddings = vectors(&response, changed.len())?;
            if response["revision"].as_str() != Some(revision) {
                return Err("Embedding revision changed during indexing".into());
            }
            let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
            for (source, vector) in changed.iter().zip(embeddings) {
                let current: Option<Source> = sqlx::query_as(
                    "SELECT id,meeting_id,source_kind,source_id,title,body,start_time,end_time,\
                     recorded_at FROM workspace_documents WHERE id=?",
                )
                .bind(&source.id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
                if current
                    .as_ref()
                    .is_some_and(|s| fingerprint(s) == fingerprint(source))
                {
                    sqlx::query(
                        "INSERT INTO workspace_vectors VALUES(?,?,?,?) \
                         ON CONFLICT(document_id,model_revision) DO UPDATE \
                         SET text_hash=excluded.text_hash,vector_json=excluded.vector_json",
                    )
                    .bind(&source.id)
                    .bind(revision)
                    .bind(fingerprint(source))
                    .bind(serde_json::to_string(&vector).map_err(|e| e.to_string())?)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
                    updated += 1;
                }
            }
            if token.is_cancelled() {
                return Err("Indexing cancelled; completed batches remain usable".into());
            }
            tx.commit().await.map_err(|e| e.to_string())?;
        }
        let _ = app.emit(
            "library-index-progress",
            json!({"inspected":inspected,"updated":updated}),
        );
        if token.is_cancelled() {
            return Err("Indexing cancelled; completed batches remain usable".into());
        }
    }
    Ok(json!({"inspected":inspected,"updated":updated,"revision":revision}))
}
/// Rank fusion uses positions; BM25 and cosine scores have incompatible scales.
pub fn fuse(lexical: &[String], dense: &[String]) -> Vec<String> {
    let mut score: BTreeMap<String, f64> = BTreeMap::new();
    for list in [lexical, dense] {
        for (rank, id) in list.iter().enumerate() {
            *score.entry(id.clone()).or_default() += 1.0 / (60.0 + rank as f64 + 1.0);
        }
    }
    let mut sorted: Vec<_> = score.into_iter().collect();
    sorted.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    sorted.into_iter().map(|(id, _)| id).collect()
}

const DENSE_SQL: &str = "SELECT d.id,d.meeting_id,d.source_kind,d.source_id,d.title,d.body,\
                         d.start_time,d.end_time,d.recorded_at,v.vector_json,v.text_hash \
                         FROM workspace_documents d JOIN workspace_vectors v \
                         ON v.document_id=d.id WHERE NOT EXISTS(SELECT 1 \
                         FROM meeting_workspace_state ws WHERE ws.meeting_id=d.meeting_id \
                         AND ws.trashed_at IS NOT NULL) AND v.model_revision=? AND (? IS NULL \
                         OR d.meeting_id=?) AND (? IS NULL OR date(d.recorded_at)>=?) \
                         AND (? IS NULL OR date(d.recorded_at)<=?) AND (? IS NULL \
                         OR EXISTS(SELECT 1 FROM meeting_tags mt WHERE mt.meeting_id=d.meeting_id \
                         AND mt.tag=?)) AND (? IS NULL OR EXISTS(SELECT 1 \
                         FROM project_meetings pm WHERE pm.meeting_id=d.meeting_id \
                         AND pm.project_id=?)) AND (d.source_kind!='transcript' \
                         OR NOT EXISTS(SELECT 1 FROM transcripts t WHERE t.id=d.source_id \
                         AND t.is_partial=1)) LIMIT 10001";

fn score_row(
    row: &sqlx::sqlite::SqliteRow,
    vector: &[f32],
) -> Result<Option<(f32, Source)>, String> {
    let source = Source::from_row(row).map_err(|e| e.to_string())?;
    if row.get::<&str, _>("text_hash") != fingerprint(&source) {
        return Ok(None);
    }
    let candidate: Vec<f32> = serde_json::from_str(row.get::<&str, _>("vector_json"))
        .map_err(|_| "Corrupt local embedding; rebuild the index")?;
    if candidate.len() != vector.len() || candidate.iter().any(|x| !x.is_finite()) {
        return Err("Embedding dimensions changed; rebuild the index".into());
    }
    let similarity = candidate.iter().zip(vector).map(|(a, b)| a * b).sum();
    Ok(Some((similarity, source)))
}

/// Exact ranking with deterministic ties, retaining only the 100 candidates
/// consumed by rank fusion. The JSON vectors are released one row at a time.
fn retain_best(best: &mut Vec<(f32, Source)>, candidate: (f32, Source)) {
    let position = best.partition_point(|item| {
        item.0.total_cmp(&candidate.0).is_gt()
            || (item.0.total_cmp(&candidate.0).is_eq() && item.1.id < candidate.1.id)
    });
    if position < 100 {
        best.insert(position, candidate);
        best.truncate(100);
    }
}

async fn dense_sources(
    pool: &SqlitePool,
    input: &SearchRequest,
    revision: &str,
    vector: &[f32],
    token: &CancellationToken,
) -> Result<Vec<(f32, Source)>, String> {
    let tag = input.tag.as_ref().map(|s| s.to_lowercase());
    let mut rows = sqlx::query(DENSE_SQL)
        .bind(revision)
        .bind(&input.meeting_id)
        .bind(&input.meeting_id)
        .bind(&input.after)
        .bind(&input.after)
        .bind(&input.before)
        .bind(&input.before)
        .bind(&tag)
        .bind(&tag)
        .bind(&input.project_id)
        .bind(&input.project_id)
        .fetch(pool);
    let mut best = Vec::with_capacity(101);
    let mut count = 0;
    while let Some(row) = rows.try_next().await.map_err(|e| e.to_string())? {
        count += 1;
        if count > 10000 {
            return Err("This search contains more than 10,000 indexed passages; narrow the date or tag filter".into());
        }
        if token.is_cancelled() {
            return Err("Search cancelled".into());
        }
        if let Some(candidate) = score_row(&row, vector)? {
            retain_best(&mut best, candidate);
        }
    }
    Ok(best)
}
pub async fn hybrid<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    input: &SearchRequest,
    token: &CancellationToken,
) -> Result<SearchResponse, String> {
    let limit = if input.limit == 0 {
        24
    } else {
        input.limit.min(100)
    } as usize;
    let mut request = input.clone();
    request.limit = if input.semantic && !input.query.trim().is_empty() {
        100
    } else {
        limit as u32
    };
    let mut result = search(pool, &request).await?;
    result
        .sources
        .truncate(if input.semantic { 100 } else { limit });
    if !input.semantic || input.query.trim().is_empty() {
        return Ok(result);
    }
    let config = read_settings(app)?;
    if !config.enabled {
        result.sources.truncate(limit);
        result
            .warnings
            .push("Semantic search is off; these are keyword results".into());
        return Ok(result);
    }
    let semantic = async {
        let status = health(app, &config).await?;
        let revision = status["embedding"]["revision"]
            .as_str()
            .ok_or("Embedding model is not cached")?;
        let query = infer(
            app,
            &config,
            "knowledge_embed",
            vec![input.query.clone()],
            json!(true),
            token,
        )
        .await?;
        let vector = vectors(&query, 1)?.remove(0);
        if query["revision"].as_str() != Some(revision) {
            return Err("Embedding model revision changed".to_string());
        }
        let dense = dense_sources(pool, input, revision, &vector, token).await?;
        if dense.is_empty() {
            return Err("No current semantic index for this scope; rebuild the index".into());
        }
        let lexical = result
            .sources
            .iter()
            .map(|s| s.id.clone())
            .collect::<Vec<_>>();
        let ids = dense.iter().map(|(_, s)| s.id.clone()).collect::<Vec<_>>();
        let ranked = fuse(&lexical, &ids);
        let mut map: BTreeMap<String, Source> = result
            .sources
            .iter()
            .cloned()
            .map(|s| (s.id.clone(), s))
            .collect();
        for (_, mut source) in dense {
            source.fingerprint = fingerprint(&source);
            map.insert(source.id.clone(), source);
        }
        let mut sources: Vec<_> = ranked
            .into_iter()
            .take(24)
            .filter_map(|id| map.remove(&id))
            .collect();
        let mut mode = "hybrid".to_string();
        let mut warnings = Vec::new();
        if config.rerank {
            let reranked = async {
                let mut scores = Vec::new();
                for batch in sources.chunks(4) {
                    let response = infer(
                        app,
                        &config,
                        "knowledge_rerank",
                        batch
                            .iter()
                            .map(|s| text(s).chars().take(3000).collect())
                            .collect(),
                        json!(input.query),
                        token,
                    )
                    .await?;
                    let values: Vec<f64> = serde_json::from_value(response["scores"].clone())
                        .map_err(|_| "Invalid reranker response")?;
                    if values.len() != batch.len() || values.iter().any(|s| !s.is_finite()) {
                        return Err("Invalid reranking scores".to_string());
                    }
                    scores.extend(values);
                }
                Ok::<_, String>(scores)
            }
            .await;
            match reranked {
                Ok(scores) => {
                    let mut ranked: Vec<_> = sources.into_iter().zip(scores).collect();
                    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
                    sources = ranked.into_iter().map(|(s, _)| s).collect();
                    mode = "hybrid-reranked".into();
                }
                Err(error) => {
                    if token.is_cancelled() {
                        return Err(error);
                    }
                    warnings.push(format!("Reranking unavailable: {error}"));
                }
            }
        }
        sources.truncate(limit);
        Ok::<_, String>(SearchResponse {
            sources,
            mode,
            warnings,
        })
    }
    .await;
    match semantic {
        Ok(response) => Ok(response),
        Err(error) => {
            if token.is_cancelled() {
                return Err(error);
            }
            result.sources.truncate(limit);
            result.warnings.push(format!(
                "Semantic search unavailable: {error}. Showing keyword results."
            ));
            Ok(result)
        }
    }
}
use sqlx::FromRow;
#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_source(i: usize) -> Source {
        Source {
            id: format!("doc-{i:05}"),
            meeting_id: "m1".into(),
            source_kind: "summary".into(),
            source_id: format!("s{i}"),
            title: "Synthetic meeting".into(),
            body: "Synthetic passage. ".repeat(50),
            start_time: None,
            end_time: None,
            recorded_at: "2026-10-14".into(),
            fingerprint: String::new(),
        }
    }

    #[test]
    fn bounded_ranking_is_identical_to_full_sort_including_ties() {
        let mut full: Vec<_> = (0..2000)
            .rev()
            .map(|i| ((i % 17) as f32, synthetic_source(i)))
            .collect();
        let mut best = Vec::new();
        for candidate in &full {
            retain_best(&mut best, candidate.clone());
            assert!(best.len() <= 100);
        }
        full.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
        assert_eq!(
            best.iter().map(|s| &s.1.id).collect::<Vec<_>>(),
            full[..100].iter().map(|s| &s.1.id).collect::<Vec<_>>()
        );
    }

    async fn dense_fixture(count: usize, dimensions: usize) -> (tempfile::TempDir, SqlitePool) {
        let directory = tempfile::tempdir().unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(directory.path().join("search.sqlite"))
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE workspace_documents(id TEXT PRIMARY KEY,meeting_id TEXT,source_kind \
             TEXT,source_id TEXT,title TEXT,body TEXT,start_time REAL,end_time REAL,recorded_at \
             TEXT); CREATE TABLE workspace_vectors(document_id TEXT,model_revision TEXT,text_hash \
             TEXT,vector_json TEXT); CREATE TABLE meeting_workspace_state(meeting_id TEXT PRIMARY \
             KEY,trashed_at TEXT); CREATE TABLE meeting_tags(meeting_id TEXT,tag TEXT); \
             CREATE TABLE transcripts(id TEXT PRIMARY KEY,is_partial INTEGER); \
             CREATE TABLE project_meetings(meeting_id TEXT PRIMARY KEY,project_id TEXT);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let vector = serde_json::to_string(&vec![0.01f32; dimensions]).unwrap();
        for i in 0..count {
            let source = synthetic_source(i);
            sqlx::query("INSERT INTO workspace_documents VALUES(?,?,?,?,?,?,NULL,NULL,?)")
                .bind(&source.id)
                .bind(&source.meeting_id)
                .bind(&source.source_kind)
                .bind(&source.source_id)
                .bind(&source.title)
                .bind(&source.body)
                .bind(&source.recorded_at)
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("INSERT INTO workspace_vectors VALUES(?,'test',?,?)")
                .bind(&source.id)
                .bind(fingerprint(&source))
                .bind(&vector)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        (directory, pool)
    }

    #[tokio::test]
    async fn streamed_search_preserves_filters_revisions_and_cancellation() {
        let (_directory, pool) = dense_fixture(12, 2).await;
        sqlx::raw_sql(
            "UPDATE workspace_vectors SET text_hash='stale' WHERE document_id='doc-00000'; \
             UPDATE workspace_documents SET meeting_id='trashed' WHERE id='doc-00001'; \
             INSERT INTO meeting_workspace_state VALUES('trashed','today'); \
             UPDATE workspace_documents SET source_kind='transcript' WHERE id='doc-00002'; \
             INSERT INTO transcripts VALUES('s2',1);",
        )
        .execute(&pool)
        .await
        .unwrap();
        let token = CancellationToken::new();
        let result = dense_sources(
            &pool,
            &SearchRequest::default(),
            "test",
            &[1.0, 1.0],
            &token,
        )
        .await
        .unwrap();
        assert_eq!(result.len(), 9);
        assert_eq!(result[0].1.id, "doc-00003");
        sqlx::query("INSERT INTO project_meetings VALUES('m1','project-a')")
            .execute(&pool)
            .await
            .unwrap();
        let project_result = dense_sources(
            &pool,
            &SearchRequest {
                project_id: Some("project-a".into()),
                ..Default::default()
            },
            "test",
            &[1.0, 1.0],
            &token,
        )
        .await
        .unwrap();
        assert_eq!(project_result.len(), 9);
        let other_project = dense_sources(
            &pool,
            &SearchRequest {
                project_id: Some("project-b".into()),
                ..Default::default()
            },
            "test",
            &[1.0, 1.0],
            &token,
        )
        .await
        .unwrap();
        assert!(other_project.is_empty());
        let filtered = SearchRequest {
            meeting_id: Some("absent".into()),
            ..Default::default()
        };
        assert!(dense_sources(&pool, &filtered, "test", &[1.0, 1.0], &token)
            .await
            .unwrap()
            .is_empty());
        token.cancel();
        assert!(dense_sources(
            &pool,
            &SearchRequest::default(),
            "test",
            &[1.0, 1.0],
            &token
        )
        .await
        .unwrap_err()
        .contains("cancelled"));
    }

    /// Run each mode in a fresh process under `/usr/bin/time -v`; the collected
    /// mode reproduces the former algorithm with the same synthetic database.
    #[tokio::test]
    #[ignore = "Synthetic 10,000-embedding memory benchmark; run explicitly"]
    async fn dense_memory_probe() {
        let (_directory, pool) = dense_fixture(10000, 1024).await;
        let vector = vec![0.01; 1024];
        let start = std::time::Instant::now();
        let mode = std::env::var("SONEFOLD_DENSE_BENCH").unwrap_or_default();
        let best = if mode == "collected" {
            let rows = sqlx::query(DENSE_SQL)
                .bind("test")
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(None::<String>)
                .bind(None::<String>)
                .fetch_all(&pool)
                .await
                .unwrap();
            let mut all: Vec<_> = rows
                .into_iter()
                .filter_map(|r| score_row(&r, &vector).unwrap())
                .collect();
            all.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)));
            all.truncate(100);
            all
        } else {
            dense_sources(
                &pool,
                &SearchRequest::default(),
                "test",
                &vector,
                &CancellationToken::new(),
            )
            .await
            .unwrap()
        };
        assert_eq!(best.len(), 100);
        assert_eq!(best[0].1.id, "doc-00000");
        assert_eq!(best[99].1.id, "doc-00099");
        println!(
            "dense mode={mode:?}, elapsed_ms={}, result_count={}",
            start.elapsed().as_millis(),
            best.len()
        );
    }

    #[test]
    fn fusion_uses_rank_and_keeps_deterministic_ties() {
        assert_eq!(
            fuse(
                &["exact".into(), "both".into()],
                &["meaning".into(), "both".into()]
            ),
            vec!["both", "exact", "meaning"]
        );
    }
}
