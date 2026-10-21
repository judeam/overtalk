//! Literal, predictable export redaction. Never mutates a source document.
use overtalk_documents::MeetingDocument;
use serde::{Deserialize, Serialize};

/// What a time-redacted copy shows in place of its title and of a covered passage.
pub const REDACTED_TITLE: &str = "Redacted meeting";
pub const REDACTED_PASSAGE: &str = "[redacted passage]";
/// A passage's audio interval in seconds; either end may be unknown.
pub type Timing = (Option<f64>, Option<f64>);

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Redactions {
    #[serde(default)]
    pub terms: Vec<String>,
    #[serde(default)]
    pub contacts: bool,
    #[serde(default)]
    pub omit_notes: bool,
    #[serde(default)]
    pub spans: Vec<[f64; 2]>,
}
impl Redactions {
    pub fn validate(&self) -> Result<(), String> {
        if self.terms.len() > 200
            || self
                .terms
                .iter()
                .any(|t| t.trim().is_empty() || t.len() > 500)
            || self.spans.len() > 200
            || self.spans.iter().any(|s| {
                !s[0].is_finite() || !s[1].is_finite() || s[0] < 0. || s[1] <= s[0] || s[1] > 86400.
            })
        {
            return Err("Check redaction terms and time ranges".into());
        }
        Ok(())
    }

    /// Whether any rule changes the copy.
    pub fn active(&self) -> bool {
        !self.terms.is_empty() || self.contacts || self.omit_notes || !self.spans.is_empty()
    }

    /// Generated titles and notes can summarize redacted speech but have no reliable
    /// temporal alignment, so a time-filtered copy leaves the notes out.
    pub fn keeps_notes(&self) -> bool {
        !self.omit_notes && self.spans.is_empty()
    }

    /// Apply the rules to an exported copy. With time ranges, `timings` holds each
    /// passage's audio interval; a passage without one cannot be shown to be safe.
    pub fn apply(&self, document: &mut MeetingDocument, timings: &[Timing]) -> Result<(), String> {
        if !self.spans.is_empty() && timings.len() != document.passages.len() {
            return Err("Transcript timing is incomplete; export was not created".into());
        }
        let prepared = self.prepare()?;
        document.title = if self.spans.is_empty() {
            prepared.text(&document.title)
        } else {
            REDACTED_TITLE.into()
        };
        document.notes = if self.keeps_notes() {
            prepared.text(&document.notes)
        } else {
            String::new()
        };
        // The notice line can name the meeting or its participants.
        document.notice = prepared.text(&document.notice);
        for (index, passage) in document.passages.iter_mut().enumerate() {
            let (start, end) = timings.get(index).copied().unwrap_or_default();
            passage.speaker = prepared.text(&passage.speaker);
            passage.text = if prepared.overlaps(start, end) {
                REDACTED_PASSAGE.into()
            } else {
                prepared.text(&passage.text)
            };
        }
        Ok(())
    }

    pub fn prepare(&self) -> Result<PreparedRedactions, String> {
        self.validate()?;
        let mut terms: Vec<_> = self.terms.iter().collect();
        terms.sort_by_key(|s| std::cmp::Reverse(s.len()));
        let patterns = terms
            .into_iter()
            .map(|term| {
                regex::RegexBuilder::new(&regex::escape(term.trim()))
                    .case_insensitive(true)
                    .build()
                    .map_err(|_| "Could not prepare a redaction term".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PreparedRedactions {
            patterns,
            contacts: self.contacts,
            spans: self.spans.clone(),
        })
    }
}

/// Compile once per export, never once per speaker or transcript passage.
/// Pattern preparation fails closed instead of silently skipping a term.
pub struct PreparedRedactions {
    patterns: Vec<regex::Regex>,
    contacts: bool,
    spans: Vec<[f64; 2]>,
}
impl PreparedRedactions {
    pub fn text(&self, input: &str) -> String {
        use std::borrow::Cow;
        static CONTACTS: std::sync::LazyLock<[regex::Regex; 2]> = std::sync::LazyLock::new(|| {
            [
                regex::Regex::new(r"(?i)\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b").unwrap(),
                regex::Regex::new(r"\b(?:\+?[0-9][0-9 ()-]{7,}[0-9])\b").unwrap(),
            ]
        });
        let mut result = input.to_owned();
        let contacts = if self.contacts {
            CONTACTS.as_slice()
        } else {
            Default::default()
        };
        // Contacts first: a term replaced inside an address would stop the whole
        // address from matching.
        for pattern in contacts.iter().chain(&self.patterns) {
            if let Cow::Owned(redacted) = pattern.replace_all(&result, "[redacted]") {
                result = redacted;
            }
        }
        result
    }

    pub fn overlaps(&self, start: Option<f64>, end: Option<f64>) -> bool {
        if self.spans.is_empty() {
            return false;
        }
        let Some((start, end)) = start.zip(end).filter(|(start, end)| {
            start.is_finite() && end.is_finite() && *start >= 0. && *end >= *start
        }) else {
            // Unknown/inconsistent timing cannot establish that speech is safe.
            return true;
        };
        self.spans.iter().any(|s| {
            if end == start {
                start >= s[0] && start < s[1]
            } else {
                start < s[1] && end > s[0]
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use overtalk_documents::Passage;

    #[test]
    fn a_term_inside_an_address_does_not_save_the_rest_of_it() {
        let rules = Redactions {
            terms: vec!["maya".into()],
            contacts: true,
            ..Default::default()
        };
        let prepared = rules.prepare().unwrap();
        assert_eq!(
            prepared.text("Write to maya.k@example.test today"),
            "Write to [redacted] today"
        );
    }

    #[test]
    fn exports_redact_the_notice_line() {
        let rules = Redactions {
            terms: vec!["Maya".into()],
            ..Default::default()
        };
        let mut document = MeetingDocument {
            title: "Review".into(),
            date: "2026-10-18".into(),
            notice: "Maya told everyone at 09:02".into(),
            notes: String::new(),
            passages: Vec::new(),
        };
        rules.apply(&mut document, &[]).unwrap();
        assert_eq!(document.notice, "[redacted] told everyone at 09:02");
    }

    #[test]
    fn time_redaction_removes_overlapping_speech_and_unaligned_notes() {
        let mut doc = MeetingDocument {
            title: "Sensitive detail in generated title".into(),
            date: "2026-10-18".into(),
            notice: String::new(),
            notes: "Sensitive detail in generated notes".into(),
            passages: vec![Passage {
                time: "00:10".into(),
                speaker: "Alex".into(),
                text: "Sensitive detail at 00:21".into(),
            }],
        };
        let rules = Redactions {
            spans: vec![[20., 22.]],
            ..Default::default()
        };
        rules.apply(&mut doc, &[(Some(10.), Some(30.))]).unwrap();
        assert_eq!(doc.passages[0].text, "[redacted passage]");
        assert!(doc.notes.is_empty());
        assert_eq!(doc.title, "Redacted meeting");
        assert!(!String::from_utf8(doc.render("json").unwrap())
            .unwrap()
            .contains("Sensitive detail"));
    }

    #[test]
    #[ignore = "manual allocation and timing profile of redacted export"]
    fn benchmark_redacted_export() {
        let rules = Redactions {
            terms: (0..8).map(|n| format!("Private Person {n}")).collect(),
            contacts: true,
            ..Default::default()
        };
        let mut doc = MeetingDocument { title: "Synthetic project".into(), date: "2026-10-18".into(), notice: String::new(), notes: "Synthetic notes".into(),
            passages: (0..100).map(|_|Passage { time:"00:00".into(), speaker:"Private Person 1".into(), text:"Public project update from Private Person 2. Contact synthetic@example.test.".repeat(4) }).collect() };
        let start = std::time::Instant::now();
        rules
            .apply(&mut doc, &vec![(Some(0.), Some(1.)); 100])
            .unwrap();
        eprintln!("redacted_export_us={}", start.elapsed().as_micros());
        assert!(!doc.transcript().contains("Private Person"));
        assert!(!doc.transcript().contains("synthetic@example.test"));
    }

    #[test]
    fn literal_unicode_safe() {
        let r = Redactions {
            terms: vec!["Nikhil".into(), "secret.token".into()],
            contacts: true,
            ..Default::default()
        };
        assert_eq!(
            r.prepare()
                .unwrap()
                .text("NIKHIL: secret.token; a@b.com; नमस्ते"),
            "[redacted]: [redacted]; [redacted]; नमस्ते"
        );
        assert_eq!(r.prepare().unwrap().text("secretXtoken"), "secretXtoken");
    }
    #[test]
    fn ranges_reject_nan() {
        let r = Redactions {
            spans: vec![[f64::NAN, 1.]],
            ..Default::default()
        };
        assert!(r.validate().is_err());
    }

    #[test]
    fn time_ranges_use_overlap_and_fail_closed_without_reliable_timing() {
        let rules = Redactions {
            spans: vec![[20., 22.]],
            ..Default::default()
        }
        .prepare()
        .unwrap();
        for (start, end, expected) in [
            (Some(10.), Some(30.), true),
            (Some(10.), Some(20.), false),
            (Some(22.), Some(30.), false),
            (Some(21.), Some(21.), true),
            (Some(19.9), Some(20.1), true),
            (None, Some(30.), true),
            (Some(10.), None, true),
            (Some(f64::NAN), Some(30.), true),
            (Some(30.), Some(10.), true),
            (Some(-1.), Some(10.), true),
        ] {
            assert_eq!(rules.overlaps(start, end), expected, "{start:?}..{end:?}");
        }
        assert!(!Redactions::default()
            .prepare()
            .unwrap()
            .overlaps(None, None));
    }

    #[test]
    fn preparation_rejects_invalid_terms_and_preserves_literal_order() {
        assert!(
            serde_json::from_value::<Redactions>(serde_json::json!({
                "term": ["private name"]
            }))
            .is_err(),
            "A misspelled redaction option must not silently export original text"
        );
        assert!(Redactions {
            terms: vec![" ".into()],
            ..Default::default()
        }
        .prepare()
        .is_err());
        let rules = Redactions {
            terms: vec!["Ann".into(), "Ann Lee".into(), "a.b".into(), "Über".into()],
            ..Default::default()
        }
        .prepare()
        .unwrap();
        assert_eq!(
            rules.text("ANN LEE, Ann, a.b, axb, ÜBER, über"),
            "[redacted], [redacted], [redacted], axb, [redacted], [redacted]"
        );
    }
}

#[tauri::command]
pub async fn workspace_redacted_audio(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    meeting_id: String,
    spans: Vec<[f64; 2]>,
) -> Result<serde_json::Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let rules = Redactions {
        spans,
        ..Default::default()
    };
    rules.validate()?;
    if rules.spans.is_empty() {
        return Err("Choose at least one time range to mute".into());
    }
    let folder: Option<String> = sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
        .bind(&meeting_id)
        .fetch_optional(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?
        .flatten();
    let folder = std::path::PathBuf::from(folder.ok_or("This meeting has no recording")?)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let input = crate::audio::retranscription::find_audio_file(&folder)
        .map_err(|e| e.to_string())?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !input.starts_with(&folder) {
        return Err("Recording is outside the meeting folder".into());
    }
    let output = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name("redacted-meeting.wav")
            .add_filter("WAV audio", &["wav"])
            .blocking_save_file()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(output) = output else {
        return Ok(serde_json::Value::Null);
    };
    let output = output.into_path().map_err(|e| e.to_string())?;
    if output == input || output.canonicalize().ok().as_ref() == Some(&input) {
        return Err("Choose a new file; the original recording must be retained".into());
    }
    let temp = tempfile::Builder::new()
        .suffix(".wav")
        .tempfile_in(output.parent().ok_or("Choose an export folder")?)
        .map_err(|e| e.to_string())?;
    // Include a short guard at each edge so a frame boundary cannot expose a
    // clipped syllable. The user-selected recording is never modified.
    let condition = rules
        .spans
        .iter()
        .map(|s| format!("between(t,{:.3},{:.3})", (s[0] - 0.1).max(0.), s[1] + 0.1))
        .collect::<Vec<_>>()
        .join("+");
    let filter = format!("volume=0:enable='{condition}'");
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().ok_or("The audio encoder is missing")?;
    let mut sandbox = overtalk_worker_sandbox::Sandbox::new(false).map_err(|e| e.to_string())?;
    sandbox.read(&input).map_err(|e| e.to_string())?;
    sandbox.output(temp.path()).map_err(|e| e.to_string())?;
    let mut command = sandbox.command(&ffmpeg).map_err(|e| e.to_string())?;
    command
        .args([
            "-nostdin",
            "-v",
            "error",
            "-y",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(input)
        .args(["-vn", "-af", &filter, "-c:a", "pcm_s16le", "-f", "wav"])
        .arg(temp.path());
    let result = tokio::task::spawn_blocking(move || {
        let _sandbox = sandbox;
        overtalk_worker_sandbox::run_bounded(command, 65536, std::time::Duration::from_secs(300))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Audio export failed; the original recording is unchanged".into());
    }
    temp.persist(&output).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({"path":output}))
}
