//! Share a copy of a meeting. Nothing is uploaded: the person saves a file and decides
//! who receives it. A page is one self-contained HTML file that opens offline in any
//! browser; each note links to its source moments, which play embedded audio. An
//! .overtalk file is a zip a teammate opens in Overtalk with every source intact. Both
//! follow the export redaction rules.
//!
//! Opening an .overtalk file treats it as untrusted: a size limit, an allow-list of flat
//! entry names (no paths, links or directories), checksums, strict validation of every
//! field, and audio that only the sandboxed media worker decodes. Nothing in the file
//! is executed or interpreted as markup.
use crate::extensions::privacy::{Redactions, Timing, REDACTED_PASSAGE};
use crate::state::AppState;
use base64::Engine as _;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use overtalk_documents::{html::escape, MeetingDocument, Passage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::{
    collections::{HashMap, HashSet},
    io::{BufReader, Read, Seek, Write},
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};
use tauri::{AppHandle, State};

/// The only .overtalk layout this version reads and writes.
pub const FORMAT: &str = "overtalk-meeting-v1";
const MAX_BUNDLE: u64 = 2 * 1024 * 1024 * 1024;
const MAX_MANIFEST: u64 = 64 * 1024;
const MAX_MEETING: u64 = 32 * 1024 * 1024;
const MAX_NOTES: usize = 8 * 1024 * 1024;
const MAX_SEGMENTS: usize = 100_000;
const MAX_TRANSCRIPT: usize = 16 * 1024 * 1024;
const MAX_LINE: usize = 64 * 1024;
const MAX_SPEAKERS: usize = 500;
const MAX_SECONDS: f64 = 86_400.0;
/// A page with more audio than this belongs in source clips or an .overtalk file.
const MAX_PAGE: u64 = 200 * 1024 * 1024;
/// Recording containers an .overtalk file may carry; FFmpeg judges the content.
const AUDIO_TYPES: &[&str] = &["webm", "ogg", "m4a", "mp4", "mp3", "wav", "flac"];
/// Compact speech is mono Opus. Pages use the lower rate because base64 adds a third.
const PAGE_BITRATE: u32 = 24_000;
const FILE_BITRATE: u32 = 32_000;
/// Context around a source moment, and the longest stretch of one line a clip plays.
const CLIP_LEAD: f64 = 2.0;
const CLIP_TAIL: f64 = 2.0;
const CLIP_LINE: f64 = 26.0;
const MAX_CLIPS: usize = 200;
const NOT_OVERTALK: &str = "This is not an Overtalk meeting file";
const DAMAGED: &str = "This Overtalk file is damaged or incomplete and was not opened";
const UNEXPECTED: &str =
    "This file contains something other than an Overtalk meeting and was not opened";
const TOO_LARGE: &str = "This file is larger than Overtalk opens and was not opened";
const UNPLAYABLE: &str =
    "The audio in this file could not be played and the meeting was not opened";
// One copy at a time: encoding is heavy, and opening a file creates a meeting.
static SHARE_JOB: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A self-contained web page.
    Page,
    /// An .overtalk file for Overtalk.
    File,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Audio {
    /// Pages: a few seconds around each source moment.
    Clips,
    /// Pages: the whole recording, compactly encoded.
    Full,
    /// Files: the recording as saved.
    Original,
    /// Files: the whole recording, compactly encoded.
    Compact,
    None,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShareOptions {
    pub kind: Kind,
    pub audio: Audio,
    #[serde(default)]
    pub redactions: Redactions,
}
impl ShareOptions {
    fn validate(&self) -> Result<(), String> {
        let fits = match self.kind {
            Kind::Page => matches!(self.audio, Audio::Clips | Audio::Full | Audio::None),
            Kind::File => matches!(self.audio, Audio::Original | Audio::Compact | Audio::None),
        };
        if !fits {
            return Err("Choose an audio option for this kind of copy".into());
        }
        self.redactions.validate()
    }
    fn redacted(&self) -> bool {
        self.redactions.active()
    }
}

/// meeting.json in an .overtalk file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Meeting {
    title: String,
    created_at: String,
    /// Seconds, from the last timed transcript line.
    duration: f64,
    /// How participants were told, as one line, and the record it describes.
    notice_line: Option<String>,
    notice: Option<Notice>,
    project: Option<String>,
    speakers: Vec<Speaker>,
    /// Transcript lines in Overtalk's order, so line n is the notes' source [Sn].
    segments: Vec<Segment>,
    /// The saved notes exactly as stored.
    notes: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Segment {
    text: String,
    start: Option<f64>,
    end: Option<f64>,
    speaker_id: Option<String>,
    /// The capture that heard the line ("mic", "system") when it was recorded so.
    source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Speaker {
    id: String,
    name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Notice {
    method: String,
    route: String,
    text: String,
    required: bool,
    given_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    app_version: String,
    created_at: String,
    /// Whether redaction rules changed this copy.
    redacted: bool,
    files: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Serialize)]
pub struct ShareEstimate {
    pub bytes: u64,
    pub clips: usize,
    pub audio_seconds: f64,
    pub notes: bool,
}

#[derive(Debug, Serialize)]
pub struct ShareSaved {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct OpenedMeeting {
    pub meeting_id: String,
    pub title: String,
    pub audio: bool,
}

/// A file name for a copy of `title`, as the export dialog suggests one.
pub(crate) fn file_name(title: &str, extension: &str) -> String {
    let stem: String = title
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
    let stem = stem.trim();
    format!(
        "{}.{extension}",
        if stem.is_empty() { "Meeting" } else { stem }
    )
}

/// At most `max` characters, no NUL, and not blank when `required`.
fn bounded(value: &str, max: usize, required: bool) -> bool {
    value.chars().count() <= max && !value.contains('\0') && (!required || !value.trim().is_empty())
}

fn hex(digest: impl std::fmt::LowerHex) -> String {
    format!("{digest:x}")
}

fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    }
}

/// Every source number the notes cite, as written.
fn references(notes: &str) -> impl Iterator<Item = usize> + '_ {
    static REFERENCE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"\[S(\d{1,6})\]").unwrap());
    REFERENCE
        .captures_iter(notes)
        .filter_map(|c| c[1].parse().ok())
}

impl Notice {
    fn valid(&self) -> bool {
        matches!(
            self.method.as_str(),
            "copied" | "read_aloud" | "confirmed" | "skipped"
        ) && matches!(self.route.as_str(), "local" | "external")
            && bounded(&self.text, 4000, false)
            && DateTime::parse_from_rfc3339(&self.given_at).is_ok()
    }
}

impl Meeting {
    /// Every field of a file someone else made, before any of it is stored. Copies
    /// are checked the same way before they are saved, so Overtalk opens its own files.
    fn check(&self) -> Result<(), &'static str> {
        if !bounded(&self.title, 1000, false) {
            return Err("the meeting title is too long");
        }
        if DateTime::parse_from_rfc3339(&self.created_at).is_err() {
            return Err("the meeting date is invalid");
        }
        if !(self.duration.is_finite() && (0.0..=MAX_SECONDS).contains(&self.duration)) {
            return Err("the meeting length is invalid");
        }
        if self
            .notice_line
            .as_deref()
            .is_some_and(|line| !bounded(line, 500, false))
            || self.notice.as_ref().is_some_and(|notice| !notice.valid())
        {
            return Err("the participant notice record is invalid");
        }
        if self
            .project
            .as_deref()
            .is_some_and(|name| !bounded(name, 120, true))
        {
            return Err("the project name is invalid");
        }
        let mut speakers = HashSet::new();
        if self.speakers.len() > MAX_SPEAKERS
            || !self.speakers.iter().all(|s| {
                bounded(&s.id, 200, true)
                    && bounded(&s.name, 200, false)
                    && speakers.insert(s.id.as_str())
            })
        {
            return Err("a speaker is invalid");
        }
        if self.segments.len() > MAX_SEGMENTS {
            return Err("the transcript is too long");
        }
        let time =
            |t: Option<f64>| t.is_none_or(|t| t.is_finite() && (0.0..=MAX_SECONDS).contains(&t));
        let mut total = 0usize;
        let mut previous = None;
        for segment in &self.segments {
            total += segment.text.len();
            // Overtalk orders untimed lines first, then by start time; sources count in that order.
            let ordered = match (segment.start, previous) {
                (None, Some(_)) => false,
                (Some(start), Some(before)) => start >= before,
                _ => true,
            };
            if total > MAX_TRANSCRIPT
                || !bounded(&segment.text, MAX_LINE, false)
                || !time(segment.start)
                || !time(segment.end)
                || segment.start.zip(segment.end).is_some_and(|(s, e)| e < s)
                || !ordered
                || segment
                    .speaker_id
                    .as_deref()
                    .is_some_and(|id| !speakers.contains(id))
                || segment
                    .source
                    .as_deref()
                    .is_some_and(|source| !bounded(source, 32, true))
            {
                return Err("a transcript line is invalid or out of order");
            }
            previous = segment.start.or(previous);
        }
        // A citation of a line that no longer exists shows as unavailable, as it does
        // in the meeting it came from.
        if self.notes.as_ref().is_some_and(|notes| {
            !notes.is_object()
                || serde_json::to_vec(notes).is_ok_and(|bytes| bytes.len() > MAX_NOTES)
                || !crate::summary::commands::summary_is_renderable(notes)
                || overtalk_documents::notes_markdown(notes).is_err()
        }) {
            return Err("the notes are invalid");
        }
        Ok(())
    }
}

/// The meeting as it is saved, read in the order that numbers its sources.
async fn load(pool: &SqlitePool, id: &str) -> Result<Meeting, String> {
    let error = |e: sqlx::Error| e.to_string();
    let meeting =
        crate::database::repositories::meeting::MeetingsRepository::get_meeting_metadata(pool, id)
            .await
            .map_err(error)?
            .ok_or("This meeting no longer exists")?;
    let mut rows = sqlx::query_as::<_, (String, Option<f64>, Option<f64>, Option<String>, Option<String>)>(
        "SELECT transcript,audio_start_time,audio_end_time,speaker_id,speaker FROM transcripts WHERE meeting_id=? AND coalesce(is_partial,0)=0 ORDER BY audio_start_time,id",
    )
    .bind(id)
    .fetch(pool);
    let mut segments = Vec::new();
    let mut bytes = 0;
    while let Some((text, start, end, speaker_id, source)) = rows.try_next().await.map_err(error)? {
        bytes += text.len();
        if bytes > MAX_TRANSCRIPT || segments.len() >= MAX_SEGMENTS {
            return Err("This meeting is too large to share as one file".into());
        }
        segments.push(Segment {
            text,
            start,
            end: end.filter(|end| start.is_none_or(|start| *end >= start)),
            speaker_id,
            source: source.filter(|s| bounded(s, 32, true)),
        });
    }
    drop(rows);
    let mut speakers: Vec<Speaker> = sqlx::query_as::<_, (String, String)>(
        "SELECT id,name FROM meeting_speakers WHERE meeting_id=? ORDER BY id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(error)?
    .into_iter()
    .map(|(id, name)| Speaker { id, name })
    .collect();
    // A line's speaker without a saved name is shown by its id, here and once opened.
    let named: HashSet<String> = speakers.iter().map(|s| s.id.clone()).collect();
    let unnamed: std::collections::BTreeSet<&String> = segments
        .iter()
        .filter_map(|s| s.speaker_id.as_ref())
        .filter(|id| !named.contains(*id))
        .collect();
    speakers.extend(unnamed.into_iter().map(|id| Speaker {
        id: id.clone(),
        name: id.clone(),
    }));
    let notice = sqlx::query_as::<_, (String, String, String, bool, String)>(
        "SELECT method,route,notice,required,given_at FROM meeting_notices WHERE meeting_id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(error)?
    .map(|(method, route, text, required, given_at)| Notice {
        method,
        route,
        text,
        required,
        given_at,
    });
    let project = sqlx::query_scalar(
        "SELECT p.name FROM project_meetings m JOIN projects p ON p.id=m.project_id WHERE m.meeting_id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(error)?;
    let notes = crate::summary::commands::load_summary_response(pool, id.to_owned())
        .await?
        .data
        .filter(crate::summary::commands::summary_is_renderable);
    let duration = segments
        .iter()
        .filter_map(|s| s.end.or(s.start))
        .fold(0.0, f64::max);
    Ok(Meeting {
        title: meeting.title,
        created_at: meeting.created_at.0.to_rfc3339(),
        duration,
        notice_line: crate::security::notice::summary(pool, id).await?,
        notice,
        project,
        speakers,
        segments,
        notes,
    })
}

/// The meeting as an export document, with each passage's audio interval.
fn document(
    meeting: &Meeting,
    rules: &Redactions,
) -> Result<(MeetingDocument, Vec<Timing>), String> {
    let names: HashMap<&str, &str> = meeting
        .speakers
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();
    let notes = match &meeting.notes {
        Some(notes) if rules.keeps_notes() => overtalk_documents::notes_markdown(notes)?,
        _ => String::new(),
    };
    let passages = meeting
        .segments
        .iter()
        .map(|s| Passage {
            time: s.start.map(clock).unwrap_or_default(),
            speaker: s
                .speaker_id
                .as_deref()
                .map_or("Unassigned", |id| names.get(id).copied().unwrap_or(id))
                .to_owned(),
            text: s.text.clone(),
        })
        .collect();
    let timings = meeting.segments.iter().map(|s| (s.start, s.end)).collect();
    Ok((
        MeetingDocument {
            title: meeting.title.clone(),
            date: meeting.created_at.clone(),
            notice: meeting.notice_line.clone().unwrap_or_default(),
            notes,
            passages,
        },
        timings,
    ))
}

/// Apply the export redaction rules to the meeting an .overtalk file carries. Without
/// rules the notes stay exactly as stored. With any rule, only the redacted Markdown
/// travels: stored notes hold fields the Markdown never shows, so they cannot be
/// shown to be clean.
fn redact(meeting: &mut Meeting, rules: &Redactions) -> Result<(), String> {
    if !rules.active() {
        return Ok(());
    }
    let (mut document, timings) = document(meeting, rules)?;
    rules.apply(&mut document, &timings)?;
    let prepared = rules.prepare()?;
    meeting.title = document.title;
    meeting.notes = match meeting.notes.take() {
        Some(_) if rules.keeps_notes() => Some(json!({ "markdown": document.notes })),
        _ => None,
    };
    for (segment, passage) in meeting.segments.iter_mut().zip(document.passages) {
        segment.text = passage.text;
    }
    // Speaker ids can carry names (a remembered person's id); give each an opaque one,
    // mapped the same way in speakers and lines.
    let mut opaque: HashMap<String, String> = HashMap::new();
    let mut rename = |id: &str| {
        let next = format!("speaker-{}", opaque.len() + 1);
        opaque.entry(id.to_owned()).or_insert(next).clone()
    };
    for speaker in &mut meeting.speakers {
        speaker.id = rename(&speaker.id);
        speaker.name = prepared.text(&speaker.name);
    }
    for segment in &mut meeting.segments {
        segment.speaker_id = segment.speaker_id.as_deref().map(&mut rename);
    }
    meeting.project = meeting.project.as_deref().map(|name| prepared.text(name));
    meeting.notice_line = meeting
        .notice_line
        .as_deref()
        .map(|line| prepared.text(line));
    if let Some(notice) = &mut meeting.notice {
        notice.text = prepared.text(&notice.text);
    }
    Ok(())
}

/// Stretches of audio around the cited lines, merged where they meet or overlap.
fn clip_windows(timings: &[Timing], notes: &str, duration: f64) -> Vec<(f64, f64)> {
    let mut spans: Vec<(f64, f64)> = references(notes)
        .filter_map(|n| {
            let (start, end) = timings.get(n.checked_sub(1)?).copied()?;
            let start = start?;
            let end = end.unwrap_or(start).clamp(start, start + CLIP_LINE);
            Some((
                (start - CLIP_LEAD).max(0.0),
                (end + CLIP_TAIL).min(duration.max(start + 1.0)),
            ))
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (start, end) in spans {
        match merged.last_mut() {
            Some(last) if start <= last.1 + 0.5 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// Encode the recording, or one window of it, as compact mono Opus speech in WebM and
/// silence the redacted spans. FFmpeg runs in the worker sandbox, which may read only
/// the recording and write only `output`.
fn encode(
    input: &Path,
    output: &Path,
    window: Option<(f64, f64)>,
    mute: &[[f64; 2]],
    bitrate: u32,
) -> Result<(), String> {
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().ok_or("The audio encoder is missing")?;
    let (offset, length) = window.map_or((0.0, None), |(start, end)| (start, Some(end - start)));
    // Times restart at zero in a window; a short guard keeps a syllable from leaking.
    let silence: Vec<String> = mute
        .iter()
        .map(|s| ((s[0] - 0.1 - offset).max(0.0), s[1] + 0.1 - offset))
        .filter(|(start, end)| *end > 0.0 && length.is_none_or(|length| *start < length))
        .map(|(start, end)| format!("between(t,{start:.3},{end:.3})"))
        .collect();
    let mut filter = "asetpts=PTS-STARTPTS".to_owned();
    if !silence.is_empty() {
        filter.push_str(&format!(",volume=0:enable='{}'", silence.join("+")));
    }
    let mut sandbox = overtalk_worker_sandbox::Sandbox::new(false).map_err(|e| e.to_string())?;
    sandbox.read(input).map_err(|e| e.to_string())?;
    sandbox.output(output).map_err(|e| e.to_string())?;
    let mut command = sandbox.command(&ffmpeg).map_err(|e| e.to_string())?;
    command.args([
        "-nostdin",
        "-v",
        "error",
        "-y",
        "-protocol_whitelist",
        "file,pipe",
    ]);
    if let Some(length) = length {
        command
            .arg("-ss")
            .arg(format!("{offset:.3}"))
            .arg("-t")
            .arg(format!("{length:.3}"));
    }
    command
        .arg("-i")
        .arg(input)
        .args(["-map", "0:a:0", "-vn", "-ac", "1", "-af", &filter])
        .args(["-c:a", "libopus", "-b:a", &bitrate.to_string()])
        .args(["-application", "voip", "-f", "webm"])
        .arg(output);
    let timeout = Duration::from_secs(length.map_or(1800, |length| 60 + length as u64));
    let result =
        overtalk_worker_sandbox::run_bounded(command, 65536, timeout).map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("The audio could not be encoded; nothing was saved".into());
    }
    Ok(())
}

fn encode_bytes(
    input: &Path,
    window: Option<(f64, f64)>,
    mute: &[[f64; 2]],
) -> Result<Vec<u8>, String> {
    let work = tempfile::tempdir().map_err(|e| e.to_string())?;
    let output = work.path().join("audio.webm");
    encode(input, &output, window, mute, PAGE_BITRATE)?;
    crate::security::storage::read_limited(&output, MAX_PAGE as usize).map_err(|e| e.to_string())
}

/// Embedded audio: the whole recording, or one clip per window.
struct Track {
    start: f64,
    end: f64,
    data: Vec<u8>,
}

struct Page<'a> {
    document: &'a MeetingDocument,
    timings: &'a [Timing],
    duration: f64,
    tracks: &'a [Track],
    /// The tracks hold the whole recording rather than clips.
    full: bool,
    redacted: bool,
    shared: String,
}

const STYLE: &str = ":root{color-scheme:light dark;--bg:#f1f4f6;--panel:#fff;--ink:#151a21;\
--muted:#5f6977;--line:#e1e6ea;--accent:#087777;--on-accent:#fff;--mark:#e1f0ed}
@media (prefers-color-scheme:dark){:root{--bg:#0f1318;--panel:#161b22;--ink:#e8ecf0;\
--muted:#8d97a3;--line:#2a323c;--accent:#69d1ca;--on-accent:#0f1318;--mark:#1d2e32}}
*{box-sizing:border-box}html{-webkit-text-size-adjust:100%}
body{margin:0;background:var(--bg);color:var(--ink);\
font:17px/1.55 system-ui,-apple-system,\"Segoe UI\",Roboto,\"Helvetica Neue\",sans-serif}
main{max-width:46rem;margin:0 auto;padding:28px 16px 40px}
header{margin:0 0 20px}.kicker{margin:0;color:var(--accent);font-size:.8rem;font-weight:600;\
letter-spacing:.06em;text-transform:uppercase}
h1{font-size:1.8rem;line-height:1.2;margin:.3rem 0 .4rem;overflow-wrap:anywhere}
.meta,.notice,footer,.empty{color:var(--muted);font-size:.92rem;margin:.2rem 0}
section{background:var(--panel);border:1px solid var(--line);border-radius:12px;padding:16px 18px;\
margin:0 0 16px}
h2{font-size:1.15rem;margin:0 0 .6rem}h3{font-size:1.05rem;margin:1.1rem 0 .4rem}\
h4,h5,h6{font-size:1rem;margin:1rem 0 .3rem}
section>h3:first-of-type{margin-top:.2rem}p,ul,ol{margin:.4rem 0}ul,ol{padding-left:1.3rem}\
li{margin:.25rem 0}
blockquote{margin:.6rem 0;padding-left:12px;border-left:3px solid var(--line);color:var(--muted)}
pre{overflow-x:auto;background:var(--bg);padding:10px;border-radius:8px}code{font-size:.92em}
.table{overflow-x:auto;margin:.5rem 0}table{border-collapse:collapse;width:100%;font-size:.95rem}
th,td{border-bottom:1px solid var(--line);padding:6px 8px;text-align:left;vertical-align:top}\
th{font-weight:600}
.play,a.source{display:inline-flex;align-items:center;gap:4px;min-height:32px;margin:2px;\
padding:2px 10px;border:1px solid var(--line);border-radius:999px;background:transparent;\
color:var(--accent);font:inherit;font-size:.85rem;font-variant-numeric:tabular-nums;\
text-decoration:none;cursor:pointer;vertical-align:middle}
.play[aria-pressed=true]{background:var(--accent);border-color:var(--accent);color:var(--on-accent)}
.play:focus-visible,a:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
audio{width:100%}.transcript{list-style:none;margin:0;padding:0}
.line{margin:0;padding:10px 6px;border-top:1px solid var(--line);border-radius:6px}\
.line:first-child{border-top:0}.line.on,.line:target{background:var(--mark)}
.line-meta{display:flex;flex-wrap:wrap;align-items:center;gap:4px 10px;margin:0 0 2px;\
color:var(--muted);font-size:.85rem}
.time{font-variant-numeric:tabular-nums}.speaker{color:var(--ink);font-weight:600}\
.line-text{margin:0;white-space:pre-line;overflow-wrap:anywhere}
footer{text-align:center;margin-top:24px}footer p{margin:.2rem 0}
@media (max-width:480px){body{font-size:16px}main{padding:20px 12px 32px}section{padding:14px 12px}\
h1{font-size:1.5rem}}";

// Plays a source moment from its embedded track; a second press pauses it.
const SCRIPT: &str = r#"(()=>{let audio=null,button=null;
const reset=()=>{if(button)button.setAttribute('aria-pressed','false');document.querySelectorAll('.line.on').forEach(l=>l.classList.remove('on'));};
const stop=()=>{if(audio)audio.pause();reset();button=null;};
document.addEventListener('click',e=>{const b=e.target.closest('button.play');if(!b)return;
if(b===button&&audio&&!audio.paused){stop();return;}
stop();const a=document.getElementById(b.dataset.track);if(!a)return;audio=a;button=b;
b.setAttribute('aria-pressed','true');const line=document.getElementById(b.dataset.line);if(line)line.classList.add('on');
const at=parseFloat(b.dataset.at)||0;const go=()=>{a.currentTime=at;a.play().catch(stop);};
if(a.readyState>0)go();else{a.addEventListener('loadedmetadata',go,{once:true});a.load();}});
document.querySelectorAll('audio').forEach(a=>{a.addEventListener('ended',()=>{if(a===audio)stop();});a.addEventListener('pause',()=>{if(a===audio)reset();});});})();"#;

fn page(content: &Page) -> Result<String, String> {
    let document = content.document;
    let full = content.full;
    let playable = |index: usize| -> Option<(usize, f64)> {
        if document.passages.get(index)?.text == REDACTED_PASSAGE {
            return None;
        }
        let at = content.timings.get(index)?.0?;
        let track = content
            .tracks
            .iter()
            .position(|t| at >= t.start && at < t.end)?;
        Some((track, at - content.tracks[track].start))
    };
    let play = |index: usize, track: usize, offset: f64, label: &str| {
        format!(
            "<button type=\"button\" class=\"play\" data-track=\"audio-{}\" data-at=\"{offset:.2}\" data-line=\"s{}\" aria-pressed=\"false\" aria-label=\"Play the moment at {label}\"><span aria-hidden=\"true\">▶</span> {label}</button>",
            track + 1,
            index + 1,
            label = escape(label),
        )
    };
    let label = |index: usize| {
        content
            .timings
            .get(index)
            .and_then(|t| t.0)
            .map(clock)
            .unwrap_or_else(|| format!("line {}", index + 1))
    };
    let source = |n: usize| -> Option<String> {
        let index = n.checked_sub(1)?;
        document.passages.get(index)?;
        let label = label(index);
        Some(match playable(index) {
            Some((track, offset)) => play(index, track, offset, &label),
            None => format!(
                "<a class=\"source\" href=\"#s{n}\" aria-label=\"Read the source at {label}\">{label}</a>",
                label = escape(&label)
            ),
        })
    };
    let notes = overtalk_documents::html::notes_html(&document.notes, &source)?;
    let date = DateTime::parse_from_rfc3339(&document.date)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%-d %B %Y, %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| document.date.clone());
    let minutes = (content.duration / 60.0).round() as u64;
    let length = match minutes {
        _ if content.duration <= 0.0 => String::new(),
        0..=59 => format!(" · {} min", minutes.max(1)),
        _ => format!(" · {} h {} min", minutes / 60, minutes % 60),
    };
    let data = |track: &Track| {
        format!(
            "data:audio/webm;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&track.data)
        )
    };
    let mut out = String::with_capacity(
        64 * 1024
            + notes.len()
            + content
                .tracks
                .iter()
                .map(|t| t.data.len() * 4 / 3)
                .sum::<usize>(),
    );
    out.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    out.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; media-src data:; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">\n<meta name=\"referrer\" content=\"no-referrer\">\n");
    out.push_str(&format!(
        "<title>{}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n<header>\n<p class=\"kicker\">Shared meeting · a copy</p>\n<h1>{}</h1>\n<p class=\"meta\">{}{length}</p>\n",
        escape(&document.title),
        escape(&document.title),
        escape(&date),
    ));
    if !document.notice.is_empty() {
        out.push_str(&format!(
            "<p class=\"notice\">{}</p>\n",
            escape(&document.notice)
        ));
    }
    out.push_str("</header>\n");
    if let (true, Some(track)) = (full, content.tracks.first()) {
        out.push_str(&format!(
            "<section aria-labelledby=\"recording\">\n<h2 id=\"recording\">Recording</h2>\n<audio id=\"audio-1\" controls preload=\"metadata\" src=\"{}\"></audio>\n</section>\n",
            data(track)
        ));
    }
    out.push_str("<section aria-labelledby=\"notes\">\n<h2 id=\"notes\">Notes</h2>\n");
    if notes.trim().is_empty() {
        out.push_str("<p class=\"empty\">No notes are included in this copy.</p>\n");
    } else {
        out.push_str(&notes);
    }
    out.push_str("</section>\n<section aria-labelledby=\"transcript\">\n<h2 id=\"transcript\">Transcript</h2>\n<ol class=\"transcript\">\n");
    for (index, passage) in document.passages.iter().enumerate() {
        // A line with audio shows its time as the button that plays it.
        let time = match playable(index) {
            Some((track, offset)) => play(index, track, offset, &label(index)),
            None => format!("<span class=\"time\">{}</span>", escape(&passage.time)),
        };
        out.push_str(&format!(
            "<li class=\"line\" id=\"s{}\"><p class=\"line-meta\">{time}<span class=\"speaker\">{}</span></p><p class=\"line-text\">{}</p></li>\n",
            index + 1,
            escape(&passage.speaker),
            escape(&passage.text)
        ));
    }
    if document.passages.is_empty() {
        out.push_str("<li class=\"line empty\">No transcript is included in this copy.</li>\n");
    }
    out.push_str("</ol>\n</section>\n<footer>\n");
    out.push_str(&format!(
        "<p>Shared from Overtalk · {}</p>\n<p>This page is a copy. Later changes to the meeting in Overtalk do not appear here.</p>\n",
        escape(&content.shared)
    ));
    if content.redacted {
        out.push_str("<p>Some content was removed or silenced before sharing.</p>\n");
    }
    out.push_str("</footer>\n</main>\n");
    if !full {
        for (index, track) in content.tracks.iter().enumerate() {
            out.push_str(&format!(
                "<audio id=\"audio-{}\" preload=\"none\" src=\"{}\"></audio>\n",
                index + 1,
                data(track)
            ));
        }
    }
    if !content.tracks.is_empty() {
        out.push_str(&format!("<script>{SCRIPT}</script>\n"));
    }
    out.push_str("</body>\n</html>\n");
    Ok(out)
}

/// What a copy contains, before any audio is encoded.
struct Plan {
    meeting: Meeting,
    document: MeetingDocument,
    timings: Vec<Timing>,
    recording: Option<PathBuf>,
    windows: Vec<(f64, f64)>,
    audio_seconds: f64,
}

async fn plan(pool: &SqlitePool, id: &str, options: &ShareOptions) -> Result<Plan, String> {
    options.validate()?;
    let mut meeting = load(pool, id).await?;
    let rules = &options.redactions;
    let (mut document, timings) = document(&meeting, rules)?;
    rules.apply(&mut document, &timings)?;
    if options.kind == Kind::File {
        redact(&mut meeting, rules)?;
        meeting.check().map_err(|reason| {
            format!("This meeting cannot be saved as an Overtalk file: {reason}.")
        })?;
    }
    let recording = match options.audio {
        Audio::None => None,
        _ => Some(crate::extensions::media::recording(pool, id).await?),
    };
    let windows = match options.audio {
        Audio::Clips => clip_windows(&timings, &document.notes, meeting.duration),
        _ => Vec::new(),
    };
    if windows.len() > MAX_CLIPS {
        return Err(
            "The notes cite too many moments for clips; share the full recording instead".into(),
        );
    }
    let audio_seconds = match (&recording, options.audio) {
        (_, Audio::Clips) => windows.iter().map(|(start, end)| end - start).sum(),
        (Some(recording), _) => {
            let recording = recording.clone();
            let fallback = meeting.duration;
            tokio::task::spawn_blocking(move || {
                crate::audio::import::extract_duration_from_metadata(&recording).unwrap_or(fallback)
            })
            .await
            .map_err(|e| e.to_string())?
        }
        _ => 0.0,
    };
    Ok(Plan {
        meeting,
        document,
        timings,
        recording,
        windows,
        audio_seconds,
    })
}

fn estimate(plan: &Plan, options: &ShareOptions) -> Result<ShareEstimate, String> {
    let compact = |bitrate: u32| (plan.audio_seconds * f64::from(bitrate) / 8.0) as u64 + 1024;
    let bytes = match options.kind {
        Kind::Page => {
            let text = page(&Page {
                document: &plan.document,
                timings: &plan.timings,
                duration: plan.meeting.duration,
                tracks: &[],
                full: false,
                redacted: false,
                shared: String::new(),
            })?
            .len() as u64;
            let audio = match options.audio {
                Audio::None => 0,
                _ => compact(PAGE_BITRATE) + plan.windows.len() as u64 * 1024,
            };
            text + audio * 4 / 3 + 4096
        }
        Kind::File => {
            let json = serde_json::to_vec_pretty(&plan.meeting).map_err(|e| e.to_string())?;
            let mut deflate =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            deflate.write_all(&json).map_err(|e| e.to_string())?;
            let text = deflate.finish().map_err(|e| e.to_string())?.len() as u64;
            let audio = match (options.audio, &plan.recording) {
                (Audio::Original, Some(recording)) if original(recording, options).is_some() => {
                    std::fs::metadata(recording).map_or(0, |m| m.len())
                }
                (Audio::None, _) | (_, None) => 0,
                _ => compact(FILE_BITRATE),
            };
            text + audio + 2048
        }
    };
    Ok(ShareEstimate {
        bytes,
        clips: plan.windows.len(),
        audio_seconds: plan.audio_seconds,
        notes: !plan.document.notes.trim().is_empty(),
    })
}

/// The recording's own container when it can travel unchanged: no silenced spans,
/// and a type an .overtalk file may carry.
fn original(recording: &Path, options: &ShareOptions) -> Option<String> {
    let extension = recording.extension()?.to_str()?.to_ascii_lowercase();
    (options.redactions.spans.is_empty() && AUDIO_TYPES.contains(&extension.as_str()))
        .then_some(extension)
}

fn write_page(plan: &Plan, options: &ShareOptions, output: &mut impl Write) -> Result<(), String> {
    let mute = &options.redactions.spans;
    let tracks = match (&plan.recording, options.audio) {
        (Some(recording), Audio::Full) => vec![Track {
            start: 0.0,
            end: f64::INFINITY,
            data: encode_bytes(recording, None, mute)?,
        }],
        (Some(recording), Audio::Clips) => plan
            .windows
            .iter()
            .map(|&(start, end)| {
                Ok(Track {
                    start,
                    end,
                    data: encode_bytes(recording, Some((start, end)), mute)?,
                })
            })
            .collect::<Result<_, String>>()?,
        _ => Vec::new(),
    };
    let html = page(&Page {
        document: &plan.document,
        timings: &plan.timings,
        duration: plan.meeting.duration,
        tracks: &tracks,
        full: options.audio == Audio::Full,
        redacted: options.redacted(),
        shared: chrono::Local::now().format("%-d %B %Y").to_string(),
    })?;
    if html.len() as u64 > MAX_PAGE {
        return Err("This page would be too large to open comfortably; share source clips or an Overtalk file instead".into());
    }
    output.write_all(html.as_bytes()).map_err(|e| e.to_string())
}

fn write_bundle<W: Write + Seek>(
    writer: W,
    meeting: &Meeting,
    audio: Option<(&Path, &str)>,
    redacted: bool,
) -> Result<W, String> {
    let error = |e: zip::result::ZipError| e.to_string();
    let mut zip = zip::ZipWriter::new(writer);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let stored = deflated.compression_method(zip::CompressionMethod::Stored);
    let json = serde_json::to_vec_pretty(meeting).map_err(|e| e.to_string())?;
    let mut files = vec![Entry {
        name: "meeting.json".into(),
        bytes: json.len() as u64,
        sha256: hex(Sha256::digest(&json)),
    }];
    zip.start_file("meeting.json", deflated).map_err(error)?;
    zip.write_all(&json).map_err(|e| e.to_string())?;
    if let Some((path, extension)) = audio {
        let name = format!("audio.{extension}");
        let mut input =
            overtalk_worker_sandbox::open_regular(path, false, false).map_err(|e| e.to_string())?;
        zip.start_file(name.as_str(), stored).map_err(error)?;
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = vec![0; 256 * 1024];
        loop {
            let n = input.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            bytes += n as u64;
            if bytes > MAX_BUNDLE {
                return Err("The recording is too large to share as one file".into());
            }
            hash.update(&buffer[..n]);
            zip.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        }
        files.push(Entry {
            name,
            bytes,
            sha256: hex(hash.finalize()),
        });
    }
    let manifest = Manifest {
        format: FORMAT.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        created_at: Utc::now().to_rfc3339(),
        redacted,
        files,
    };
    zip.start_file("manifest.json", deflated).map_err(error)?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    zip.finish().map_err(error)
}

fn write_file(
    plan: &Plan,
    options: &ShareOptions,
    output: &mut (impl Write + Seek),
) -> Result<(), String> {
    let work = tempfile::tempdir().map_err(|e| e.to_string())?;
    let audio = match (&plan.recording, options.audio) {
        (Some(recording), Audio::Original | Audio::Compact) => {
            match original(recording, options).filter(|_| options.audio == Audio::Original) {
                Some(extension) => Some((recording.clone(), extension)),
                None => {
                    let compact = work.path().join("audio.webm");
                    encode(
                        recording,
                        &compact,
                        None,
                        &options.redactions.spans,
                        FILE_BITRATE,
                    )?;
                    Some((compact, "webm".to_owned()))
                }
            }
        }
        _ => None,
    };
    write_bundle(
        output,
        &plan.meeting,
        audio
            .as_ref()
            .map(|(path, extension)| (path.as_path(), extension.as_str())),
        options.redacted(),
    )?;
    Ok(())
}

/// The chosen file with the extension its kind needs; it must be in an existing folder.
fn destination(path: &str, kind: Kind) -> Result<PathBuf, String> {
    let extension = match kind {
        Kind::Page => "html",
        Kind::File => "overtalk",
    };
    let mut path = PathBuf::from(path);
    if !path.is_absolute() || path.file_name().is_none() {
        return Err("Choose where to save the copy".into());
    }
    if !path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(extension))
    {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{extension}"));
        path.set_file_name(name);
    }
    if !path.parent().is_some_and(Path::is_dir) {
        return Err("Choose an existing folder for the copy".into());
    }
    Ok(path)
}

/// Estimate the size of a copy before the person chooses where to save it.
#[tauri::command]
pub async fn workspace_share_estimate(
    state: State<'_, AppState>,
    meeting_id: String,
    options: ShareOptions,
) -> Result<ShareEstimate, String> {
    let plan = plan(state.db_manager.pool(), &meeting_id, &options).await?;
    estimate(&plan, &options)
}

/// Save a copy of the meeting to the file the person chose.
#[tauri::command]
pub async fn workspace_share_meeting(
    state: State<'_, AppState>,
    meeting_id: String,
    options: ShareOptions,
    path: String,
) -> Result<ShareSaved, String> {
    let _job = SHARE_JOB
        .try_lock()
        .map_err(|_| "Another copy is being prepared; try again when it finishes")?;
    if !super::media::take_chosen(Path::new(&path), super::media::Purpose::Save) {
        return Err("Choose where to save the copy".into());
    }
    let destination = destination(&path, options.kind)?;
    // The dialog confirmed replacing the chosen file, not one with an extension added.
    if destination != Path::new(&path) && destination.exists() {
        return Err("A file with that name already exists; choose another name".into());
    }
    let pool = state.db_manager.pool();
    let plan = plan(pool, &meeting_id, &options).await?;
    if plan.recording.is_some() && destination.canonicalize().ok() == plan.recording {
        return Err("Choose a new file; the original recording must be kept".into());
    }
    let target = destination.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<u64, String> {
        let parent = target.parent().ok_or("Choose a folder for the copy")?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        match options.kind {
            Kind::Page => write_page(&plan, &options, &mut file)?,
            Kind::File => write_file(&plan, &options, &mut file)?,
        }
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        let bytes = file.as_file().metadata().map_err(|e| e.to_string())?.len();
        file.persist(&target).map_err(|e| e.to_string())?;
        Ok(bytes)
    })
    .await
    .map_err(|e| e.to_string())??;
    crate::extensions::usage::count(pool, "meeting_shared").await;
    Ok(ShareSaved {
        path: destination.to_string_lossy().into_owned(),
        bytes,
    })
}

/// An .overtalk file whose manifest and meeting passed every check. Its audio entry,
/// if any, is verified when it is extracted.
#[derive(Debug)]
struct Bundle {
    meeting: Meeting,
    audio: Option<Entry>,
}

fn entry_limit(name: &str) -> Option<u64> {
    match name {
        "manifest.json" => Some(MAX_MANIFEST),
        "meeting.json" => Some(MAX_MEETING),
        _ => name
            .strip_prefix("audio.")
            .filter(|extension| AUDIO_TYPES.contains(extension))
            .map(|_| MAX_BUNDLE),
    }
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<BufReader<std::fs::File>>, String> {
    // The final path component must be a regular file, not a link or device.
    let file = overtalk_worker_sandbox::open_regular(path, false, false)
        .map_err(|_| "Choose an Overtalk meeting file")?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_BUNDLE {
        return Err(TOO_LARGE.into());
    }
    zip::ZipArchive::new(BufReader::new(file)).map_err(|_| NOT_OVERTALK.into())
}

/// Check every entry's name, type and declared size before reading any content.
fn entry_names<R: Read + Seek>(zip: &mut zip::ZipArchive<R>) -> Result<HashSet<String>, String> {
    if zip.len() > 3 {
        return Err(UNEXPECTED.into());
    }
    let mut names = HashSet::new();
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(|_| NOT_OVERTALK)?;
        let name = entry.name().map_err(|_| UNEXPECTED)?.into_owned();
        let limit = entry_limit(&name).ok_or(UNEXPECTED)?;
        let regular = entry
            .unix_mode()
            .is_none_or(|mode| matches!(mode & 0o170000, 0 | 0o100000));
        if entry.is_dir()
            || entry.is_symlink()
            || !regular
            || entry.encrypted()
            || !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
            || entry.enclosed_name().as_deref() != Some(Path::new(&name))
            || !names.insert(name)
        {
            return Err(UNEXPECTED.into());
        }
        if entry.size() > limit {
            return Err(TOO_LARGE.into());
        }
    }
    Ok(names)
}

/// Read a small entry; the limit holds however large the entry claims to be.
fn read_entry<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let entry = zip.by_name(name).map_err(|_| NOT_OVERTALK)?;
    let mut bytes = Vec::new();
    entry
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DAMAGED)?;
    if bytes.len() as u64 > limit {
        return Err(TOO_LARGE.into());
    }
    Ok(bytes)
}

fn manifest(bytes: &[u8]) -> Result<Manifest, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| NOT_OVERTALK)?;
    match value.get("format").and_then(Value::as_str) {
        Some(FORMAT) => {}
        Some(format) if format.starts_with("overtalk-meeting-") => {
            return Err(
                "This file was made by a newer version of Overtalk. Update Overtalk to open it."
                    .into(),
            )
        }
        _ => return Err(NOT_OVERTALK.into()),
    }
    let manifest: Manifest = serde_json::from_value(value).map_err(|_| DAMAGED)?;
    let valid = bounded(&manifest.app_version, 64, true)
        && DateTime::parse_from_rfc3339(&manifest.created_at).is_ok()
        && manifest.files.iter().all(|file| {
            file.name != "manifest.json"
                && entry_limit(&file.name).is_some_and(|limit| file.bytes <= limit)
                && file.sha256.len() == 64
                && file
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
    if !valid {
        return Err(DAMAGED.into());
    }
    Ok(manifest)
}

/// Read and validate an .overtalk file without extracting anything.
fn read_bundle(path: &Path) -> Result<Bundle, String> {
    let mut zip = open_zip(path)?;
    let names = entry_names(&mut zip)?;
    if !names.contains("manifest.json") {
        return Err(NOT_OVERTALK.into());
    }
    let manifest = manifest(&read_entry(&mut zip, "manifest.json", MAX_MANIFEST)?)?;
    // The manifest lists every other entry exactly once, and nothing else.
    let listed: HashSet<&str> = manifest.files.iter().map(|f| f.name.as_str()).collect();
    if listed.len() != manifest.files.len()
        || listed.len() + 1 != names.len()
        || !listed.iter().all(|name| names.contains(*name))
        || manifest
            .files
            .iter()
            .filter(|f| f.name.starts_with("audio."))
            .count()
            > 1
    {
        return Err(DAMAGED.into());
    }
    let listing = manifest
        .files
        .iter()
        .find(|f| f.name == "meeting.json")
        .ok_or(DAMAGED)?;
    let bytes = read_entry(&mut zip, "meeting.json", MAX_MEETING)?;
    if bytes.len() as u64 != listing.bytes || hex(Sha256::digest(&bytes)) != listing.sha256 {
        return Err(DAMAGED.into());
    }
    let meeting: Meeting = serde_json::from_slice(&bytes)
        .map_err(|_| "This file was not opened: the meeting in it is damaged.")?;
    meeting
        .check()
        .map_err(|reason| format!("This file was not opened: {reason}."))?;
    Ok(Bundle {
        meeting,
        audio: manifest
            .files
            .into_iter()
            .find(|f| f.name.starts_with("audio.")),
    })
}

/// Extract the audio entry into `folder` under its allow-listed flat name, checking
/// its size and checksum against the manifest as it streams.
fn extract_audio(source: &Path, listing: &Entry, folder: &Path) -> Result<PathBuf, String> {
    let mut zip = open_zip(source)?;
    let mut input = zip.by_name(&listing.name).map_err(|_| DAMAGED)?;
    let mut output = tempfile::NamedTempFile::new_in(folder).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let n = input.read(&mut buffer).map_err(|_| DAMAGED)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > listing.bytes {
            return Err(DAMAGED.into());
        }
        hash.update(&buffer[..n]);
        output.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
    }
    if bytes != listing.bytes || hex(hash.finalize()) != listing.sha256 {
        return Err(DAMAGED.into());
    }
    output.as_file().sync_all().map_err(|e| e.to_string())?;
    let path = folder.join(&listing.name);
    output.persist(&path).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Decode the start of the recording in the media sandbox. Anything FFmpeg cannot
/// read as audio is refused before the meeting exists.
fn verify_audio(path: &Path) -> Result<(), String> {
    let mut reader = crate::audio::decoder::IsolatedPcmReader::open(path, 16_000, 1, 0.0)
        .map_err(|_| UNPLAYABLE)?;
    match reader.read_samples() {
        Ok(Some(samples)) if !samples.is_empty() => Ok(()),
        _ => Err(UNPLAYABLE.into()),
    }
}

/// Create the meeting `key` from a validated file in one transaction. Line identifiers
/// keep the file's order, so the notes' sources resolve as they did.
async fn store(
    pool: &SqlitePool,
    meeting: &Meeting,
    folder: Option<&Path>,
    key: uuid::Uuid,
) -> Result<String, String> {
    let id = format!("meeting-{key}");
    let created = DateTime::parse_from_rfc3339(&meeting.created_at)
        .map_err(|e| e.to_string())?
        .with_timezone(&Utc);
    let now = Utc::now();
    let error = |e: sqlx::Error| e.to_string();
    let mut tx = pool.begin().await.map_err(error)?;
    sqlx::query(
        "INSERT INTO meetings(id,title,created_at,updated_at,folder_path) VALUES(?,?,?,?,?)",
    )
    .bind(&id)
    .bind(meeting.title.trim())
    .bind(created)
    .bind(now)
    .bind(folder.map(|f| f.to_string_lossy().into_owned()))
    .execute(&mut *tx)
    .await
    .map_err(error)?;
    for speaker in &meeting.speakers {
        sqlx::query("INSERT INTO meeting_speakers(meeting_id,id,name) VALUES(?,?,?)")
            .bind(&id)
            .bind(&speaker.id)
            .bind(&speaker.name)
            .execute(&mut *tx)
            .await
            .map_err(error)?;
    }
    for (index, segment) in meeting.segments.iter().enumerate() {
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time,duration,speaker,speaker_id) VALUES(?,?,?,?,?,?,?,?,?)")
            .bind(format!("transcript-{key}-{index:06}"))
            .bind(&id)
            .bind(&segment.text)
            .bind(&meeting.created_at)
            .bind(segment.start)
            .bind(segment.end)
            .bind(segment.start.zip(segment.end).map(|(start, end)| end - start))
            .bind(&segment.source)
            .bind(&segment.speaker_id)
            .execute(&mut *tx)
            .await
            .map_err(error)?;
    }
    if let Some(notes) = &meeting.notes {
        sqlx::query("INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result) VALUES(?,'completed',?,?,?)")
            .bind(&id)
            .bind(now)
            .bind(now)
            .bind(notes.to_string())
            .execute(&mut *tx)
            .await
            .map_err(error)?;
    }
    if let Some(notice) = &meeting.notice {
        // The sender's record, kept as theirs: it is not a notice given on this computer.
        sqlx::query("INSERT INTO meeting_notices(meeting_id,method,route,notice,required,given_at,source) VALUES(?,?,?,?,?,?,'shared-file')")
            .bind(&id)
            .bind(&notice.method)
            .bind(&notice.route)
            .bind(&notice.text)
            .bind(notice.required)
            .bind(&notice.given_at)
            .execute(&mut *tx)
            .await
            .map_err(error)?;
    }
    // A project of the same name in this library files the meeting; none is created.
    if let Some(project) = &meeting.project {
        sqlx::query("INSERT INTO project_meetings(meeting_id,project_id,origin,reason) SELECT ?,id,'manual','' FROM projects WHERE archived=0 AND name=? COLLATE NOCASE ORDER BY created_at LIMIT 1")
            .bind(&id)
            .bind(project.trim())
            .execute(&mut *tx)
            .await
            .map_err(error)?;
    }
    tx.commit().await.map_err(error)?;
    Ok(id)
}

/// Open an .overtalk file as a new meeting in this library.
#[tauri::command]
pub async fn workspace_open_shared(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<OpenedMeeting, String> {
    let _job = SHARE_JOB
        .try_lock()
        .map_err(|_| "Another copy is being prepared; try again when it finishes")?;
    // Only a file the person picked in the open dialog, as for saving a copy.
    if !super::media::take_chosen(Path::new(&path), super::media::Purpose::Open) {
        return Err("Choose an Overtalk meeting file".into());
    }
    let source = PathBuf::from(path);
    let read = source.clone();
    let bundle = tokio::task::spawn_blocking(move || read_bundle(&read))
        .await
        .map_err(|e| e.to_string())??;
    let key = uuid::Uuid::new_v4();
    let folder = match bundle.audio.clone() {
        Some(listing) => {
            let base = crate::audio::recording_preferences::load_recording_preferences(&app)
                .await
                .map_err(|e| e.to_string())?
                .save_folder;
            // Each opened meeting gets its own folder, even for one file opened twice.
            let title = format!(
                "{} {}",
                bundle.meeting.title.chars().take(80).collect::<String>(),
                &key.simple().to_string()[..8]
            );
            Some(
                tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
                    let folder =
                        crate::audio::audio_processing::create_meeting_folder(&base, &title, false)
                            .map_err(|e| e.to_string())?;
                    let extracted = extract_audio(&source, &listing, &folder)
                        .and_then(|audio| verify_audio(&audio));
                    if let Err(error) = extracted {
                        let _ = std::fs::remove_dir_all(&folder);
                        return Err(error);
                    }
                    Ok(folder)
                })
                .await
                .map_err(|e| e.to_string())??,
            )
        }
        None => None,
    };
    let pool = state.db_manager.pool();
    let stored = store(pool, &bundle.meeting, folder.as_deref(), key).await;
    if stored.is_err() {
        if let Some(folder) = &folder {
            let _ = std::fs::remove_dir_all(folder);
        }
    }
    let meeting_id = stored?;
    crate::extensions::usage::count(pool, "meeting_imported").await;
    Ok(OpenedMeeting {
        meeting_id,
        title: bundle.meeting.title,
        audio: folder.is_some(),
    })
}

#[cfg(test)]
mod tests;
