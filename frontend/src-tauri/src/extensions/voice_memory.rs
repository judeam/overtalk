// SPDX-License-Identifier: Apache-2.0
//! Opt-in, local voice memory. No audio or embeddings are sent to a provider.
use crate::security::storage::hex;
use crate::state::AppState;
use futures_util::StreamExt;
use overtalk_voice_memory as voice;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static JOB: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, FromRow)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub created_at: String,
}
/// How a person agreed to have their voice remembered, given while enrolling them.
#[derive(serde::Deserialize)]
pub struct VoiceConsent {
    /// `in_meeting`, `in_writing` or `other`.
    pub method: String,
    /// Where their agreement is recorded; required unless they agreed in the meeting.
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize)]
pub struct Status {
    pub ready: bool,
    pub profiles: Vec<Profile>,
}

fn model_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("models/voice-memory/campplus.onnx"))
}
fn verified(path: &Path) -> bool {
    crate::security::storage::hash_regular_file(path, 32 * 1024 * 1024)
        .is_ok_and(|(digest, _)| hex(&digest.finalize()) == voice::MODEL_SHA256)
}

#[tauri::command]
pub async fn voice_memory_status<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<Status, String> {
    let profiles = sqlx::query_as("SELECT v.id,COALESCE((SELECT p.name FROM project_people p WHERE p.profile_id=v.id LIMIT 1),v.name) AS name,v.created_at FROM voice_profiles v ORDER BY name,id")
        .fetch_all(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    // No inference runtime is loaded just to inspect status.
    let path = model_path(&app)?;
    let ready = tokio::task::spawn_blocking(move || verified(&path))
        .await
        .map_err(|e| e.to_string())?;
    Ok(Status { ready, profiles })
}

#[tauri::command]
pub async fn voice_memory_setup<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let _job = JOB
        .try_lock()
        .map_err(|_| "Voice memory is busy; try again when it finishes")?;
    let path = model_path(&app)?;
    let check = path.clone();
    if tokio::task::spawn_blocking(move || verified(&check))
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }
    let parent = path.parent().ok_or("Invalid model directory")?;
    crate::security::storage::private_directory(parent).map_err(|e| e.to_string())?;
    let temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    let result = async {
        let client = reqwest::Client::builder()
            .tls_certs_only(crate::security::transport::tls_roots())
            .timeout(Duration::from_secs(180))
            .https_only(true)
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(voice::MODEL_URL)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        let mut stream = response.bytes_stream();
        let mut size = 0usize;
        let mut hash = Sha256::new();
        let mut file =
            tokio::fs::File::from_std(temporary.as_file().try_clone().map_err(|e| e.to_string())?);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| e.to_string())?;
            size += chunk.len();
            if size > 32 * 1024 * 1024 {
                return Err("Voice model download exceeds the size limit".into());
            }
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        if hex(&hash.finalize()) != voice::MODEL_SHA256 {
            return Err("Voice model checksum mismatch; download rejected".into());
        }
        file.sync_all().await.map_err(|e| e.to_string())?;
        drop(file);
        Ok::<(), String>(())
    }
    .await;
    result?;
    temporary.persist(&path).map_err(|e| e.to_string())?;
    Ok(())
}

async fn recording(pool: &SqlitePool, meeting: &str) -> Result<PathBuf, String> {
    let folder: Option<Option<String>> =
        sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
            .bind(meeting)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let folder = PathBuf::from(
        folder
            .flatten()
            .ok_or("This meeting has no saved recording")?,
    )
    .canonicalize()
    .map_err(|e| e.to_string())?;
    let path = crate::audio::retranscription::find_audio_file(&folder)
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !path.starts_with(folder) || !path.is_file() {
        return Err("Recording must be inside its meeting folder".into());
    }
    Ok(path)
}
async fn turns(pool: &SqlitePool, meeting: &str) -> Result<Vec<(String, f64, f64)>, String> {
    sqlx::query_as("SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time LIMIT 100000")
        .bind(meeting).fetch_all(pool).await.map_err(|e|e.to_string())
}

async fn decode(path: &Path, start: f64, end: f64) -> Result<Vec<f32>, String> {
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().ok_or("The audio decoder is missing")?;
    let mut sandbox = overtalk_worker_sandbox::Sandbox::new(false).map_err(|e| e.to_string())?;
    sandbox.read(path).map_err(|e| e.to_string())?;
    let mut cmd =
        tokio::process::Command::from(sandbox.command(&ffmpeg).map_err(|e| e.to_string())?);
    cmd.args(["-nostdin", "-v", "error", "-threads", "1", "-ss"])
        .arg(format!("{start:.6}"))
        .args(["-protocol_whitelist", "file,pipe", "-i"])
        .arg(path)
        .args([
            "-t",
            &format!("{:.6}", end - start),
            "-vn",
            "-threads",
            "1",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ac",
            "1",
            "-ar",
            "16000",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    let mut stdout = child
        .stdout
        .take()
        .ok_or("Audio decoder has no output")?
        .take((voice::MAX_SAMPLES * 4 + 1) as u64);
    tokio::time::timeout(Duration::from_secs(20), async {
        stdout
            .read_to_end(&mut output)
            .await
            .map_err(|e| e.to_string())?;
        if output.len() > voice::MAX_SAMPLES * 4 {
            return Err("Voice sample exceeds the size limit".to_string());
        }
        if !child.wait().await.map_err(|e| e.to_string())?.success() {
            return Err("Could not decode the voice sample".to_string());
        }
        Ok(())
    })
    .await
    .map_err(|_| "Audio sample decoding timed out")??;
    if output.len() % 4 != 0 {
        return Err("Incomplete audio sample".into());
    }
    Ok(output
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

async fn extract(
    model: &Path,
    audio: &Path,
    speaker: &str,
    turns: &[(String, f64, f64)],
) -> Result<Vec<Vec<f32>>, String> {
    let spans = voice::sample_spans(speaker, turns);
    if spans.len() < 2 {
        return Err(
            "Need two clear speech samples of at least 2 seconds without overlapping voices".into(),
        );
    }
    let mut clips = Vec::new();
    for (a, b) in spans {
        clips.push(decode(audio, a, b).await?);
    }
    let model = model.to_owned();
    tokio::task::spawn_blocking(move || {
        if !verified(&model) {
            return Err("Set up voice memory first; the verified voice model is missing".into());
        }
        let mut encoder = crate::native_model_worker::NativeModelWorker::launch(
            "voice",
            model.parent().ok_or("Invalid voice model path")?,
            model
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid voice model name")?,
            false,
        )
        .map_err(|e| e.to_string())?;
        clips
            .iter()
            .map(|clip| {
                encoder
                    .call::<Vec<f32>>("encode", clip, None)
                    .map_err(|e| e.to_string())
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reunite acoustic slots without rerunning ASR or enrolling anybody's voice.
pub(crate) async fn consolidate<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    meeting: &str,
    snapshot: bool,
) -> Result<usize, String> {
    let _job = JOB
        .try_lock()
        .map_err(|_| "Voice analysis is busy; try again when it finishes")?;
    let model = model_path(app)?;
    if !model.exists() {
        return Ok(0);
    }
    let all = turns(pool, meeting).await?;
    let speakers = crate::speakers::list_speakers(pool, meeting).await?;
    if !(2..=32).contains(&speakers.len()) {
        return Ok(0);
    }
    let audio = recording(pool, meeting).await?;
    let mut inputs = Vec::new();
    for speaker in &speakers {
        let mut clips = Vec::new();
        for (a, b) in voice::sample_spans_count(&speaker.id, &all, 4) {
            if let Ok(clip) = decode(&audio, a, b).await {
                clips.push(clip);
            }
        }
        inputs.push((speaker.id.clone(), clips));
    }
    let embeddings = tokio::task::spawn_blocking(move || {
        if !verified(&model) {
            return Err("The voice analysis model failed verification".to_string());
        }
        let mut encoder = crate::native_model_worker::NativeModelWorker::launch(
            "voice",
            model.parent().ok_or("Invalid voice model path")?,
            model
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("Invalid voice model name")?,
            false,
        )
        .map_err(|e| e.to_string())?;
        Ok(inputs
            .into_iter()
            .map(|(id, clips)| {
                (
                    id,
                    clips
                        .into_iter()
                        .filter_map(|clip| encoder.call::<Vec<f32>>("encode", &clip, None).ok())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>())
    })
    .await
    .map_err(|e| e.to_string())??;
    let clusters = speakers
        .iter()
        .map(|speaker| voice::consolidation::Cluster {
            id: speaker.id.clone(),
            confirmed_name: (!speaker.name.starts_with("Speaker ")).then(|| speaker.name.clone()),
            samples: embeddings.get(&speaker.id).cloned().unwrap_or_default(),
            intervals: all
                .iter()
                .filter(|t| t.0 == speaker.id)
                .map(|t| (t.1, t.2))
                .collect(),
        })
        .collect();
    let merges = voice::consolidation::consolidate(clusters);
    if merges.is_empty() {
        return Ok(0);
    }
    let mut tx = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    let current: Vec<(String, f64, f64)> = sqlx::query_as("SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time LIMIT 100000")
        .bind(meeting).fetch_all(&mut *tx).await.map_err(|e| e.to_string())?;
    let names: Vec<(String, String)> =
        sqlx::query_as("SELECT id,name FROM meeting_speakers WHERE meeting_id=? ORDER BY id")
            .bind(meeting)
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    if current != all
        || names
            != speakers
                .iter()
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect::<Vec<_>>()
    {
        return Err("Speakers changed during analysis. Try again.".into());
    }
    if snapshot {
        super::speech::snapshot_transcript(&mut tx, meeting).await?;
    }
    for merge in &merges {
        sqlx::query("UPDATE transcripts SET speaker_id=? WHERE meeting_id=? AND speaker_id=?")
            .bind(&merge.target)
            .bind(meeting)
            .bind(&merge.source)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE speaker_turns SET speaker_id=? WHERE meeting_id=? AND speaker_id=?")
            .bind(&merge.target)
            .bind(meeting)
            .bind(&merge.source)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        sqlx::query("UPDATE meeting_speakers SET name=(SELECT name FROM meeting_speakers WHERE meeting_id=? AND id=?) WHERE meeting_id=? AND id=? AND name LIKE 'Speaker %' AND (SELECT name FROM meeting_speakers WHERE meeting_id=? AND id=?) NOT LIKE 'Speaker %'")
            .bind(meeting).bind(&merge.source).bind(meeting).bind(&merge.target).bind(meeting).bind(&merge.source).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("INSERT OR IGNORE INTO meeting_voice_matches SELECT meeting_id,?,profile_id,similarity,confirmed FROM meeting_voice_matches WHERE meeting_id=? AND speaker_id=?")
            .bind(&merge.target).bind(meeting).bind(&merge.source).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        sqlx::query("DELETE FROM meeting_speakers WHERE meeting_id=? AND id=?")
            .bind(meeting)
            .bind(&merge.source)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    crate::speakers::fill_unassigned(&mut tx, meeting).await?;
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(merges.len())
}

#[tauri::command]
pub async fn consolidate_meeting_speakers<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<usize, String> {
    let count = consolidate(&app, state.db_manager.pool(), &meeting_id, true).await?;
    let _ = app.emit("meeting-speakers-updated", &meeting_id);
    Ok(count)
}

#[tauri::command]
pub async fn remember_speaker<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    meeting_id: String,
    speaker_id: String,
    name: String,
    person_id: Option<String>,
    consent: Option<VoiceConsent>,
) -> Result<(), String> {
    let _job = JOB
        .try_lock()
        .map_err(|_| "Voice memory is busy; try again when it finishes")?;
    let pool = state.db_manager.pool();
    let project_person: Option<(String, String)> = if let Some(id) = &person_id {
        Some(sqlx::query_as("SELECT p.name,p.id FROM project_people p JOIN project_meetings m ON m.project_id=p.project_id WHERE p.id=? AND m.meeting_id=?")
            .bind(id).bind(&meeting_id).fetch_optional(pool).await.map_err(|e|e.to_string())?.ok_or("Choose a person belonging to this meeting's project")?)
    } else {
        None
    };
    let display_name = project_person
        .as_ref()
        .map_or(name.as_str(), |p| p.0.as_str());
    let name = display_name.trim();
    if name.is_empty()
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || name.to_lowercase().starts_with("speaker ")
        || name.eq_ignore_ascii_case("unassigned")
    {
        return Err("Enter the person's name after checking their voice".into());
    }
    // A person's voice is remembered only with their explicit consent: one already recorded
    // in Privacy settings, or the one given here, which is recorded first.
    let authorization_id = match crate::security::privacy::voice_authorization(pool, name).await {
        Ok(id) => id,
        Err(missing) => match &consent {
            Some(consent) => {
                crate::security::privacy::record_voice_consent(
                    pool,
                    name,
                    &consent.method,
                    &consent.note,
                    &meeting_id,
                )
                .await?
            }
            None => return Err(missing),
        },
    };
    let storage_name = project_person.as_ref().map_or_else(
        || name.to_owned(),
        |p| format!("{} [{}]", name.chars().take(72).collect::<String>(), p.1),
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM voice_profiles")
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    if count >= 200 {
        return Err(
            "Voice memory holds up to 200 people. Forget an unused voice before adding another."
                .into(),
        );
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM voice_profiles WHERE name=? COLLATE NOCASE)",
    )
    .bind(&storage_name)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    if exists {
        return Err(
            "A voice already uses that name. Use a distinct name or forget the old voice first."
                .into(),
        );
    }
    let audio = recording(pool, &meeting_id).await?;
    let turns = turns(pool, &meeting_id).await?;
    let vectors = extract(&model_path(&app)?, &audio, &speaker_id, &turns).await?;
    if !voice::consistent(&vectors) {
        return Err("These samples do not sound like one consistent voice. Check or split this speaker before remembering them.".into());
    }
    let mut tx = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    let current_turns: Vec<(String,f64,f64)> = sqlx::query_as("SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? ORDER BY start_time LIMIT 100000")
        .bind(&meeting_id).fetch_all(&mut *tx).await.map_err(|e|e.to_string())?;
    if current_turns != turns {
        return Err("Speaker analysis changed while collecting samples. Try again.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let scope_project: Option<String> = if let Some((expected_name, person)) = &project_person {
        let current:Option<(String,String)>=sqlx::query_as("SELECT p.project_id,p.name FROM project_people p JOIN project_meetings m ON m.project_id=p.project_id WHERE p.id=? AND m.meeting_id=?")
            .bind(person).bind(&meeting_id).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;
        let (project, current_name) =
            current.ok_or("The person or project changed during enrollment. Try again.")?;
        if &current_name != expected_name {
            return Err("The person was renamed during enrollment. Try again.".into());
        }
        Some(project)
    } else {
        None
    };
    let changed = sqlx::query("UPDATE meeting_speakers SET name=? WHERE meeting_id=? AND id=?")
        .bind(name)
        .bind(&meeting_id)
        .bind(&speaker_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if changed.rows_affected() != 1 {
        return Err("The speaker changed during processing. Try again.".into());
    }
    sqlx::query(
        "INSERT INTO voice_profiles(id,name,model,embeddings,created_at,scope_project_id,authorization_id) VALUES(?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&storage_name)
    .bind(voice::MODEL_ID)
    .bind(serde_json::to_string(&vectors).map_err(|e| e.to_string())?)
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(scope_project)
    .bind(authorization_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    if let Some((_, person)) = &project_person {
        let changed =
            sqlx::query("UPDATE project_people SET profile_id=? WHERE id=? AND profile_id IS NULL")
                .bind(&id)
                .bind(person)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        if changed.rows_affected() != 1 {
            return Err("This person already has a remembered voice. Forget it before enrolling a replacement.".into());
        }
    }
    sqlx::query("INSERT OR REPLACE INTO meeting_voice_matches VALUES(?,?,?,?,1)")
        .bind(&meeting_id)
        .bind(&speaker_id)
        .bind(&id)
        .bind(1.)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())?;
    let _ = app.emit("meeting-speakers-updated", &meeting_id);
    Ok(())
}

pub async fn recognize<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    meeting: &str,
) -> Result<usize, String> {
    let original_project = super::projects::project_for(pool, meeting).await?;
    let profiles: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT v.id,COALESCE(p.name,v.name),v.embeddings FROM voice_profiles v \
         JOIN privacy_authorizations a ON a.id=v.authorization_id AND a.withdrawn_at IS NULL \
         AND (a.expires_at IS NULL OR julianday(a.expires_at)>julianday('now')) \
         LEFT JOIN project_people p ON p.profile_id=v.id AND p.project_id=(SELECT project_id \
         FROM project_meetings WHERE meeting_id=?) WHERE v.model=? AND ((p.id IS NOT NULL) \
         OR (NOT EXISTS(SELECT 1 FROM project_meetings WHERE meeting_id=?) \
         AND v.scope_project_id IS NULL)) ORDER BY v.id LIMIT 200",
    )
    .bind(meeting)
    .bind(voice::MODEL_ID)
    .bind(meeting)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    if profiles.is_empty() {
        return Ok(0);
    }
    let _job = JOB
        .try_lock()
        .map_err(|_| "Voice memory is busy; try again when it finishes")?;
    let vectors: Vec<Vec<Vec<f32>>> = profiles
        .iter()
        .map(|p| serde_json::from_str(&p.2).unwrap_or_default())
        .collect();
    let all = turns(pool, meeting).await?;
    let audio = recording(pool, meeting).await?;
    let model = model_path(app)?;
    let speakers = crate::speakers::list_speakers(pool, meeting).await?;
    let mut count = 0;
    for speaker in speakers {
        // Never overwrite a name confirmed by the user or an earlier match.
        if !speaker.name.starts_with("Speaker ") {
            continue;
        }
        let samples = match extract(&model, &audio, &speaker.id, &all).await {
            Ok(v) => v,
            Err(e) => {
                log::debug!("Voice memory skipped a sample: {e}");
                continue;
            }
        };
        let Some((index, score)) = voice::identify(&samples, &vectors) else {
            continue;
        };
        let (profile_id, name, _) = &profiles[index];
        let mut tx = crate::database::begin_write(pool)
            .await
            .map_err(|e| e.to_string())?;
        let current_project: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM project_meetings WHERE meeting_id=?")
                .bind(meeting)
                .fetch_optional(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        if current_project != original_project {
            return Err("The meeting changed projects during voice matching. Try again.".into());
        }
        let current_turns: Vec<(String, f64, f64)> = sqlx::query_as(
            "SELECT speaker_id,start_time,end_time FROM speaker_turns WHERE meeting_id=? \
             ORDER BY start_time LIMIT 100000",
        )
        .bind(meeting)
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        if current_turns != all {
            return Err("Speaker analysis changed while recognizing voices. Try again.".into());
        }
        let changed = sqlx::query(
            "UPDATE meeting_speakers SET name=? WHERE meeting_id=? AND id=? AND name=?",
        )
        .bind(name)
        .bind(meeting)
        .bind(&speaker.id)
        .bind(&speaker.name)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
        if changed.rows_affected() == 1 {
            sqlx::query("INSERT OR REPLACE INTO meeting_voice_matches VALUES(?,?,?,?,0)")
                .bind(meeting)
                .bind(&speaker.id)
                .bind(profile_id)
                .bind(score)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            count += 1;
        }
        tx.commit().await.map_err(|e| e.to_string())?;
    }
    Ok(count)
}

/// Recognition after transcription, when the person turned it on in Privacy settings. Asking
/// for it on one meeting ("Recognize voices in this meeting") always works.
pub async fn recognize_if_automatic<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    meeting: &str,
) -> Result<usize, String> {
    if !crate::security::privacy::policy(pool)
        .await?
        .voice_recognition_automatic
    {
        return Ok(0);
    }
    recognize(app, pool, meeting).await
}

#[tauri::command]
pub async fn recognize_meeting_speakers<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    meeting_id: String,
) -> Result<usize, String> {
    let count = recognize(&app, state.db_manager.pool(), &meeting_id).await?;
    let _ = app.emit("meeting-speakers-updated", &meeting_id);
    Ok(count)
}

#[tauri::command]
pub async fn forget_voice(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let _job = JOB
        .try_lock()
        .map_err(|_| "Voice memory is busy; try again when it finishes")?;
    sqlx::query("DELETE FROM voice_profiles WHERE id=?")
        .bind(profile_id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
