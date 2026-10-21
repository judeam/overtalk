use super::*;

/// A seeded transcript line: text, start, end and speaker.
type Line<'a> = (&'a str, Option<f64>, Option<f64>, Option<&'a str>);
/// One way to damage an otherwise valid meeting.
type Change<'a> = &'a dyn Fn(&mut Meeting);

const NOTES: &str = "## Decisions\n- Ship the reduced scope for Maya [S2][S3]\n\n## Action items\n| Owner | Task | Due |\n| --- | --- | --- |\n| Maya | Prepare the revised plan [S4] | 28 June |\n\n## Open questions\n- Does Orion need extra support? [S6]";

async fn database() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::query("INSERT INTO projects(id,name,created_at,updated_at) VALUES('orion','Orion','2026-10-01','2026-10-01')")
        .execute(&pool)
        .await
        .unwrap();
    pool
}

/// A meeting with an untimed first line, two speakers, notes citing sources, a
/// participant notice and a project.
async fn seed(pool: &SqlitePool) -> String {
    let id = "meeting-0d6c1a52-6f1e-4c55-9d1f-6f0b1c7e2a11";
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,'Orion review','2026-10-18T09:00:00+00:00','2026-10-18T10:00:00+00:00')")
        .bind(id).execute(pool).await.unwrap();
    for (speaker, name) in [("speaker-1", "Maya"), ("speaker-2", "Leila")] {
        sqlx::query("INSERT INTO meeting_speakers(meeting_id,id,name) VALUES(?,?,?)")
            .bind(id)
            .bind(speaker)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
    }
    let lines: [Line; 7] = [
        ("Recording started.", None, None, None),
        (
            "The previous target was 24 June.",
            Some(0.0),
            Some(9.6),
            Some("speaker-1"),
        ),
        (
            "We reduce the scope.",
            Some(10.0),
            Some(18.0),
            Some("speaker-2"),
        ),
        (
            "Agreed, with a cost ceiling.",
            Some(18.5),
            Some(25.0),
            Some("speaker-1"),
        ),
        (
            "Maya prepares the revised plan by 28 June.",
            Some(30.0),
            Some(38.0),
            Some("speaker-1"),
        ),
        (
            "Questions to questions@example.test, please.",
            Some(38.0),
            Some(45.0),
            Some("speaker-9"),
        ),
        (
            "Does Orion need extra support?",
            Some(120.0),
            Some(128.0),
            Some("speaker-2"),
        ),
    ];
    for (index, (text, start, end, speaker)) in lines.into_iter().enumerate() {
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time,speaker_id,speaker) VALUES(?,?,?,'2026-10-18T09:00:00Z',?,?,?,'mic')")
            .bind(format!("line-{index}"))
            .bind(id)
            .bind(text)
            .bind(start)
            .bind(end)
            .bind(speaker)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result) VALUES(?,'completed','2026-10-18T10:00:00+00:00','2026-10-18T10:00:00+00:00',?)")
        .bind(id)
        .bind(json!({ "markdown": NOTES }).to_string())
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO meeting_notices(meeting_id,method,route,notice,required,given_at) VALUES(?,'copied','local','I am recording this call.',1,'2026-10-18T09:00:00+00:00')")
        .bind(id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO project_meetings(meeting_id,project_id) VALUES(?,'orion')")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id.into()
}

fn options(kind: Kind, redactions: Redactions) -> ShareOptions {
    ShareOptions {
        kind,
        audio: Audio::None,
        redactions,
    }
}

fn render(plan: &Plan, tracks: &[Track], full: bool) -> String {
    page(&Page {
        document: &plan.document,
        timings: &plan.timings,
        duration: plan.meeting.duration,
        tracks,
        full,
        redacted: false,
        shared: "20 October 2026".into(),
    })
    .unwrap()
}

/// The part of the page for transcript line `n`.
fn line(html: &str, n: usize) -> &str {
    let start = html.find(&format!("id=\"s{n}\"")).unwrap();
    &html[start..start + html[start..].find("</li>").unwrap()]
}

/// Every attribute that could fetch or navigate stays inside the page.
fn assert_self_contained(html: &str) {
    for attribute in ["src=\"", "href=\"", "action=\"", "srcset=\"", "url("] {
        for (at, _) in html.match_indices(attribute) {
            let value = &html[at + attribute.len()..];
            assert!(
                value.starts_with("data:audio/webm;base64,") || value.starts_with('#'),
                "{attribute}{}",
                &value[..value.len().min(40)]
            );
        }
    }
    assert!(!html.contains("http://") && !html.contains("https://"));
    assert!(html.contains("default-src 'none'; media-src data:; style-src 'unsafe-inline'"));
}

#[test]
fn page_escapes_hostile_titles_and_text_and_requests_nothing() {
    let meeting = Meeting {
        title: "<script>alert('title')</script> & \"quotes\"".into(),
        created_at: "2026-10-18T09:00:00+00:00".into(),
        duration: 10.0,
        notice_line: Some("Participants were notified <b>at 09:00</b>".into()),
        speakers: vec![Speaker {
            id: "s".into(),
            name: "\"><svg onload=alert(3)>".into(),
        }],
        segments: vec![Segment {
            text: "</p><script>alert(2)</script><a href=\"#x\" onclick=alert(4)>x</a>".into(),
            start: Some(1.0),
            end: Some(4.0),
            speaker_id: Some("s".into()),
            source: None,
        }],
        notes: Some(
            json!({ "markdown": "## Decisions <iframe src=x>\n- <img src=x onerror=alert(1)> [click](https://evil.example/a) ![x](https://evil.example/i.png) [S1]" }),
        ),
        ..Default::default()
    };
    let (document, timings) = document(&meeting, &Redactions::default()).unwrap();
    let tracks = [Track {
        start: 0.0,
        end: 6.0,
        data: b"clip".to_vec(),
    }];
    let html = page(&Page {
        document: &document,
        timings: &timings,
        duration: meeting.duration,
        tracks: &tracks,
        full: false,
        redacted: false,
        shared: "20 October 2026".into(),
    })
    .unwrap();
    for hostile in [
        "<script>alert",
        "<svg",
        "<img",
        "<iframe",
        "<b>at",
        "evil.example",
    ] {
        assert!(!html.contains(hostile), "{hostile}");
    }
    assert!(html.contains("<title>&lt;script&gt;alert(&#39;title&#39;)&lt;/script&gt; &amp; &quot;quotes&quot;</title>"));
    assert!(html.contains("&quot;&gt;&lt;svg onload=alert(3)&gt;"));
    assert!(
        html.contains("&lt;/p&gt;&lt;script&gt;alert(2)&lt;/script&gt;&lt;a href=&quot;#x&quot;")
    );
    assert!(html.contains("Participants were notified &lt;b&gt;at 09:00&lt;/b&gt;"));
    assert_eq!(
        html.matches("<script>").count(),
        1,
        "only the player script"
    );
    assert!(html.contains("Shared from Overtalk · 20 October 2026"));
    assert!(html.contains("This page is a copy."));
    assert_self_contained(&html);
}

#[tokio::test]
async fn clips_cover_only_cited_moments_merged_where_they_overlap() {
    let pool = database().await;
    let id = seed(&pool).await;
    let mut copy = plan(&pool, &id, &options(Kind::Page, Redactions::default()))
        .await
        .unwrap();
    // [S2][S3][S4] are adjacent lines; [S6] stands alone; line 7 is not cited.
    copy.windows = clip_windows(&copy.timings, &copy.document.notes, copy.meeting.duration);
    assert_eq!(copy.windows, vec![(0.0, 27.0), (36.0, 47.0)]);
    let tracks: Vec<Track> = copy
        .windows
        .iter()
        .map(|&(start, end)| Track {
            start,
            end,
            data: vec![0; 16],
        })
        .collect();
    let html = render(&copy, &tracks, false);
    assert_eq!(html.matches("<audio ").count(), 2);
    assert!(html.contains("data-track=\"audio-1\" data-at=\"10.00\" data-line=\"s3\""));
    assert!(html.contains("data-track=\"audio-2\" data-at=\"2.00\" data-line=\"s6\""));
    for n in [2, 3, 4, 6] {
        assert!(line(&html, n).contains("class=\"play\""), "line {n}");
    }
    // Untimed lines and lines outside every clip have nothing to play.
    assert!(!line(&html, 1).contains("class=\"play\""));
    assert!(!line(&html, 5).contains("class=\"play\""));
    assert!(!line(&html, 7).contains("class=\"play\""));
    assert!(html.contains("<th>Owner</th>"));
    assert_self_contained(&html);
    // Without audio, sources link to their transcript line instead.
    let html = render(&copy, &[], false);
    assert!(!html.contains("<audio") && !html.contains("<script>"));
    assert!(html.contains("<a class=\"source\" href=\"#s4\""));
    // The whole recording plays every timed line from its own time.
    let full = [Track {
        start: 0.0,
        end: f64::INFINITY,
        data: vec![0; 16],
    }];
    let html = render(&copy, &full, true);
    assert_eq!(html.matches("<audio ").count(), 1);
    assert!(line(&html, 7).contains("data-at=\"120.00\""));
}

#[tokio::test]
async fn pages_apply_export_redactions() {
    let pool = database().await;
    let id = seed(&pool).await;
    let rules = Redactions {
        terms: vec!["maya".into()],
        contacts: true,
        ..Default::default()
    };
    let copy = plan(&pool, &id, &options(Kind::Page, rules)).await.unwrap();
    let html = render(&copy, &[], false);
    assert!(!html.to_lowercase().contains("maya"));
    assert!(!html.contains("example.test"));
    assert!(html.contains("[redacted] prepares the revised plan"));
    assert!(html.contains("Ship the reduced scope for [redacted]"));
    assert!(html.contains("Participants were notified"));
    // Silenced time leaves no notes, no title and no words from that span.
    let rules = Redactions {
        spans: vec![[30.0, 40.0]],
        ..Default::default()
    };
    let copy = plan(&pool, &id, &options(Kind::Page, rules)).await.unwrap();
    let html = render(&copy, &[], false);
    assert!(html.contains("<h1>Redacted meeting</h1>"));
    assert!(html.contains("No notes are included in this copy."));
    assert!(line(&html, 5).contains("[redacted passage]"));
    assert!(line(&html, 6).contains("[redacted passage]"));
    assert!(line(&html, 7).contains("Does Orion need extra support?"));
    assert!(!html.contains("revised plan"));
    // Leaving the notes out keeps everything else.
    let rules = Redactions {
        omit_notes: true,
        ..Default::default()
    };
    let copy = plan(&pool, &id, &options(Kind::Page, rules)).await.unwrap();
    let html = render(&copy, &[], false);
    assert!(html.contains("<h1>Orion review</h1>") && !html.contains("Decisions"));
}

fn bundle_of(plan: &Plan, audio: Option<(&Path, &str)>) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new()
        .suffix(".overtalk")
        .tempfile()
        .unwrap();
    write_bundle(file.as_file_mut(), &plan.meeting, audio, false).unwrap();
    file
}

#[tokio::test]
async fn overtalk_files_round_trip_transcript_notes_and_sources() {
    let source = database().await;
    let id = seed(&source).await;
    let copy = plan(&source, &id, &options(Kind::File, Redactions::default()))
        .await
        .unwrap();
    let file = bundle_of(&copy, None);
    let bundle = read_bundle(file.path()).unwrap();
    assert!(bundle.audio.is_none());
    let target = database().await;
    let first = store(&target, &bundle.meeting, None, uuid::Uuid::new_v4())
        .await
        .unwrap();
    // Opening the same file again makes a second, independent meeting.
    let second = store(&target, &bundle.meeting, None, uuid::Uuid::new_v4())
        .await
        .unwrap();
    assert_ne!(first, second);
    let original = load(&source, &id).await.unwrap();
    let mut opened = load(&target, &first).await.unwrap();
    // The notice record travels, but reads as the sender's report, not as given here.
    let line = opened.notice_line.take().unwrap();
    assert_eq!(
        line.strip_prefix(crate::security::notice::SHARED_FILE_PREFIX),
        original.notice_line.as_deref()
    );
    opened.notice_line = original.notice_line.clone();
    assert_eq!(opened, original);
    assert_eq!(opened.notes, Some(json!({ "markdown": NOTES })));
    assert_eq!(opened.project.as_deref(), Some("Orion"));
    // A speaker without a saved name keeps the id it is shown by.
    assert!(opened
        .speakers
        .iter()
        .any(|s| s.id == "speaker-9" && s.name == "speaker-9"));
    let sources = |timeline: crate::extensions::timeline::MeetingTimeline| timeline.sources;
    assert_eq!(
        sources(
            crate::extensions::timeline::load_timeline(&target, &first)
                .await
                .unwrap()
        ),
        sources(
            crate::extensions::timeline::load_timeline(&source, &id)
                .await
                .unwrap()
        ),
    );
    let searchable: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_documents WHERE meeting_id=? AND source_kind='transcript'",
    )
    .bind(&first)
    .fetch_one(&target)
    .await
    .unwrap();
    assert_eq!(searchable, 7);
}

#[tokio::test]
async fn overtalk_files_carry_redacted_copies_only() {
    let pool = database().await;
    let id = seed(&pool).await;
    let rules = Redactions {
        terms: vec!["Maya".into()],
        contacts: true,
        ..Default::default()
    };
    let copy = plan(&pool, &id, &options(Kind::File, rules)).await.unwrap();
    let file = bundle_of(&copy, None);
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file.path()).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("meeting.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    assert!(!text.contains("Maya") && !text.contains("example.test"));
    let meeting = read_bundle(file.path()).unwrap().meeting;
    assert_eq!(meeting.speakers[0].name, "[redacted]");
    assert!(meeting.notes.unwrap()["markdown"]
        .as_str()
        .unwrap()
        .contains("for [redacted] [S2][S3]"));
    let rules = Redactions {
        spans: vec![[0.0, 20.0]],
        ..Default::default()
    };
    let copy = plan(&pool, &id, &options(Kind::File, rules)).await.unwrap();
    assert!(copy.meeting.notes.is_none());
    assert_eq!(copy.meeting.segments[2].text, REDACTED_PASSAGE);
    assert_eq!(
        copy.meeting.segments[6].text,
        "Does Orion need extra support?"
    );
}

#[test]
fn redaction_leaves_no_stored_field_notice_or_speaker_id_behind() {
    let mut meeting = valid_meeting();
    meeting.speakers[0].id = "person-maya".into();
    meeting.segments[0].speaker_id = Some("person-maya".into());
    // A stored field the Markdown never shows still names Maya.
    meeting.notes = Some(json!({"markdown": "## Decisions\n- Ship [S2]", "owners": ["Maya"]}));
    meeting.notice_line = Some("Maya told everyone at 09:02".into());
    meeting.notice = Some(Notice {
        method: "chat".into(),
        route: "local".into(),
        text: "Maya is recording this call".into(),
        required: true,
        given_at: "2026-10-18T09:02:00+00:00".into(),
    });
    let rules = Redactions {
        terms: vec!["Maya".into()],
        ..Default::default()
    };
    redact(&mut meeting, &rules).unwrap();
    let text = serde_json::to_string(&meeting).unwrap();
    assert!(
        !text.contains("Maya") && !text.contains("person-maya"),
        "{text}"
    );
    assert_eq!(
        meeting.notes,
        Some(json!({"markdown": "## Decisions\n- Ship [S2]"}))
    );
    assert_eq!(meeting.speakers[0].id, "speaker-1");
    assert_eq!(meeting.segments[0].speaker_id.as_deref(), Some("speaker-1"));
    // Without rules the copy is exactly what was stored.
    let mut untouched = valid_meeting();
    redact(&mut untouched, &Redactions::default()).unwrap();
    assert_eq!(untouched, valid_meeting());
}

#[test]
fn only_a_file_chosen_for_this_purpose_is_accepted_once() {
    let folder = tempfile::tempdir().unwrap();
    let chosen = folder.path().join("copy.html");
    use crate::extensions::media::{choose_for_test, take_chosen, Purpose};
    assert!(!take_chosen(&chosen, Purpose::Save));
    choose_for_test(&chosen, Purpose::Save);
    assert!(take_chosen(&chosen, Purpose::Save));
    assert!(!take_chosen(&chosen, Purpose::Save));
    // A file picked to open is never a place to write, and the other way round.
    choose_for_test(&chosen, Purpose::Open);
    assert!(!take_chosen(&chosen, Purpose::Save));
    assert!(take_chosen(&chosen, Purpose::Open));
    choose_for_test(&chosen, Purpose::Save);
    assert!(!take_chosen(&chosen, Purpose::Open));
}

#[test]
fn audio_entries_are_checked_against_the_manifest_as_they_stream() {
    let meeting = Meeting {
        title: "Audio".into(),
        created_at: "2026-10-18T09:00:00+00:00".into(),
        ..Default::default()
    };
    let recording = tempfile::Builder::new().suffix(".webm").tempfile().unwrap();
    std::fs::write(recording.path(), b"pretend opus").unwrap();
    let mut file = tempfile::Builder::new()
        .suffix(".overtalk")
        .tempfile()
        .unwrap();
    write_bundle(
        file.as_file_mut(),
        &meeting,
        Some((recording.path(), "webm")),
        false,
    )
    .unwrap();
    let listing = read_bundle(file.path()).unwrap().audio.unwrap();
    assert_eq!(listing.name, "audio.webm");
    let folder = tempfile::tempdir().unwrap();
    let extracted = extract_audio(file.path(), &listing, folder.path()).unwrap();
    assert_eq!(extracted, folder.path().join("audio.webm"));
    assert_eq!(std::fs::read(&extracted).unwrap(), b"pretend opus");
    // Audio that does not match its manifest entry is refused.
    let forged = Entry {
        sha256: "0".repeat(64),
        ..listing
    };
    assert_eq!(
        extract_audio(file.path(), &forged, folder.path()).unwrap_err(),
        DAMAGED
    );
}

enum Item<'a> {
    File(&'a str, Vec<u8>),
    Link(&'a str, &'a str),
    Folder(&'a str),
}

fn archive(items: &[Item]) -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new()
        .suffix(".overtalk")
        .tempfile()
        .unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(file.path()).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for item in items {
        match item {
            Item::File(name, bytes) => {
                zip.start_file(*name, options).unwrap();
                zip.write_all(bytes).unwrap();
            }
            Item::Link(name, target) => zip.add_symlink(*name, *target, options).unwrap(),
            Item::Folder(name) => zip.add_directory(*name, options).unwrap(),
        }
    }
    zip.finish().unwrap();
    file
}

fn listing(name: &str, bytes: &[u8]) -> Value {
    json!({"name": name, "bytes": bytes.len(), "sha256": hex(Sha256::digest(bytes))})
}

fn manifest_of(format: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format": format,
        "app_version": "0.1.0",
        "created_at": "2026-10-20T10:00:00+00:00",
        "redacted": false,
        "files": files.iter().map(|(name, bytes)| listing(name, bytes)).collect::<Vec<_>>(),
    }))
    .unwrap()
}

fn valid_meeting() -> Meeting {
    Meeting {
        title: "Valid".into(),
        created_at: "2026-10-18T09:00:00+00:00".into(),
        duration: 20.0,
        speakers: vec![Speaker {
            id: "speaker-1".into(),
            name: "Maya".into(),
        }],
        segments: vec![
            Segment {
                text: "First".into(),
                start: Some(1.0),
                end: Some(5.0),
                speaker_id: Some("speaker-1".into()),
                source: None,
            },
            Segment {
                text: "Second".into(),
                start: Some(6.0),
                end: Some(9.0),
                speaker_id: None,
                source: Some("system".into()),
            },
        ],
        notes: Some(json!({"markdown": "## Decisions\n- Ship [S2]"})),
        ..Default::default()
    }
}

/// A well-formed file holding `meeting` exactly as given.
fn file_with(meeting: &[u8]) -> tempfile::NamedTempFile {
    archive(&[
        Item::File("meeting.json", meeting.to_vec()),
        Item::File(
            "manifest.json",
            manifest_of(FORMAT, &[("meeting.json", meeting)]),
        ),
    ])
}

#[test]
fn malicious_or_foreign_files_are_refused() {
    let meeting = serde_json::to_vec(&valid_meeting()).unwrap();
    let manifest = manifest_of(FORMAT, &[("meeting.json", &meeting)]);
    assert!(read_bundle(file_with(&meeting).path()).is_ok());
    let refused = |items: &[Item]| read_bundle(archive(items).path()).unwrap_err();
    // Paths that would leave the destination, links, folders and unknown entries.
    for name in [
        "../meeting.json",
        "/meeting.json",
        "audio/../../evil.webm",
        "run.sh",
    ] {
        let error = refused(&[
            Item::File(name, meeting.clone()),
            Item::File("manifest.json", manifest_of(FORMAT, &[(name, &meeting)])),
        ]);
        assert_eq!(error, UNEXPECTED, "{name}");
    }
    let link = refused(&[
        Item::File("meeting.json", meeting.clone()),
        Item::Link("audio.webm", "/etc/passwd"),
        Item::File("manifest.json", manifest.clone()),
    ]);
    assert_eq!(link, UNEXPECTED);
    let folder = refused(&[
        Item::File("meeting.json", meeting.clone()),
        Item::Folder("audio.webm"),
        Item::File("manifest.json", manifest.clone()),
    ]);
    assert_eq!(folder, UNEXPECTED);
    // Formats from elsewhere, and versions this Overtalk does not know.
    let newer = refused(&[
        Item::File("meeting.json", meeting.clone()),
        Item::File(
            "manifest.json",
            manifest_of("overtalk-meeting-v2", &[("meeting.json", &meeting)]),
        ),
    ]);
    assert!(newer.contains("newer version"), "{newer}");
    let foreign = refused(&[
        Item::File("meeting.json", meeting.clone()),
        Item::File(
            "manifest.json",
            manifest_of("other-meeting-v1", &[("meeting.json", &meeting)]),
        ),
    ]);
    assert_eq!(foreign, NOT_OVERTALK);
    let not_zip = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(not_zip.path(), b"#!/bin/sh\necho no").unwrap();
    assert_eq!(read_bundle(not_zip.path()).unwrap_err(), NOT_OVERTALK);
    // Damaged JSON, a checksum that does not match, and entries the manifest omits.
    assert!(read_bundle(file_with(b"{\"title\":").path())
        .unwrap_err()
        .contains("damaged"));
    let mismatch = refused(&[
        Item::File("meeting.json", b"{}".to_vec()),
        Item::File("manifest.json", manifest.clone()),
    ]);
    assert_eq!(mismatch, DAMAGED);
    let unlisted = refused(&[
        Item::File("meeting.json", meeting.clone()),
        Item::File("audio.webm", b"opus".to_vec()),
        Item::File("manifest.json", manifest.clone()),
    ]);
    assert_eq!(unlisted, DAMAGED);
    // Declared and actual sizes are bounded before anything is parsed.
    let oversize = vec![b' '; MAX_MEETING as usize + 1];
    assert_eq!(
        read_bundle(file_with(&oversize).path()).unwrap_err(),
        TOO_LARGE
    );
}

#[test]
fn every_meeting_field_is_validated() {
    let refused = |change: &dyn Fn(&mut Meeting)| {
        let mut meeting = valid_meeting();
        change(&mut meeting);
        read_bundle(file_with(&serde_json::to_vec(&meeting).unwrap()).path()).unwrap_err()
    };
    let cases: [(&str, Change); 10] = [
        ("title", &|m: &mut Meeting| m.title = "x".repeat(1001)),
        ("date", &|m: &mut Meeting| m.created_at = "yesterday".into()),
        ("length", &|m: &mut Meeting| m.duration = -1.0),
        ("order", &|m: &mut Meeting| m.segments.swap(0, 1)),
        ("untimed", &|m: &mut Meeting| m.segments[1].start = None),
        ("speaker", &|m: &mut Meeting| {
            m.segments[0].speaker_id = Some("nobody".into())
        }),
        ("time", &|m: &mut Meeting| m.segments[0].end = Some(0.5)),
        ("empty notes", &|m: &mut Meeting| {
            m.notes = Some(json!({"markdown": "   "}))
        }),
        ("notes", &|m: &mut Meeting| {
            m.notes = Some(json!(["not", "an", "object"]))
        }),
        ("notice", &|m: &mut Meeting| {
            m.notice = Some(Notice {
                method: "assumed".into(),
                route: "local".into(),
                text: String::new(),
                required: true,
                given_at: "2026-10-18T09:00:00+00:00".into(),
            })
        }),
    ];
    for (name, change) in cases {
        assert!(
            refused(change).starts_with("This file was not opened:"),
            "{name}"
        );
    }
    // Fields this version does not know are refused rather than silently dropped.
    let mut value = serde_json::to_value(valid_meeting()).unwrap();
    value["script"] = json!("rm -rf /");
    assert!(
        read_bundle(file_with(&serde_json::to_vec(&value).unwrap()).path())
            .unwrap_err()
            .contains("damaged")
    );
}

#[test]
fn copies_are_named_and_placed_safely() {
    assert_eq!(
        file_name("Q3 / plan: <final>", "html"),
        "Q3 _ plan_ _final_.html"
    );
    assert_eq!(file_name("///", "overtalk"), "___.overtalk");
    assert_eq!(file_name("", "overtalk"), "Meeting.overtalk");
    let folder = tempfile::tempdir().unwrap();
    let chosen = folder.path().join("notes");
    assert_eq!(
        destination(chosen.to_str().unwrap(), Kind::Page).unwrap(),
        folder.path().join("notes.html")
    );
    assert_eq!(
        destination(
            folder.path().join("a.OVERTALK").to_str().unwrap(),
            Kind::File
        )
        .unwrap(),
        folder.path().join("a.OVERTALK")
    );
    assert!(destination("relative.html", Kind::Page).is_err());
    assert!(destination(
        folder.path().join("missing/a.html").to_str().unwrap(),
        Kind::Page
    )
    .is_err());
    let audio_for_page = ShareOptions {
        kind: Kind::Page,
        audio: Audio::Original,
        redactions: Redactions::default(),
    };
    assert!(audio_for_page.validate().is_err());
}

#[test]
fn clips_are_opus_with_silenced_spans_and_only_audio_opens() {
    let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("FFmpeg test dependency");
    let folder = tempfile::tempdir().unwrap();
    let recording = folder.path().join("audio.wav");
    let status = std::process::Command::new(ffmpeg)
        .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
        .arg("sine=frequency=440:sample_rate=48000:duration=6")
        .arg(&recording)
        .status()
        .unwrap();
    assert!(status.success());
    // Seconds 1–3 of the recording, with 1.5–2.0 silenced.
    let clip = encode_bytes(&recording, Some((1.0, 3.0)), &[[1.5, 2.0]]).unwrap();
    assert!(clip.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]), "a WebM file");
    assert!(
        clip.len() < 16 * 1024,
        "{} bytes for two seconds",
        clip.len()
    );
    let path = folder.path().join("clip.webm");
    std::fs::write(&path, &clip).unwrap();
    verify_audio(&path).unwrap();
    let mut reader = crate::audio::decoder::IsolatedPcmReader::open(&path, 16_000, 1, 0.0).unwrap();
    let mut samples = Vec::new();
    while let Some(block) = reader.read_samples().unwrap() {
        samples.extend(block);
    }
    let rms = |from: f64, to: f64| {
        let part = &samples[(from * 16_000.0) as usize..(to * 16_000.0) as usize];
        (part.iter().map(|s| s * s).sum::<f32>() / part.len() as f32).sqrt()
    };
    assert!((1.9..2.1).contains(&(samples.len() as f64 / 16_000.0)));
    assert!(rms(0.55, 0.95) < 0.005, "silenced span");
    assert!(rms(1.3, 1.9) > 0.02, "audible after the span");
    // Anything that is not audio is refused before a meeting is created.
    let script = folder.path().join("audio.webm");
    std::fs::write(&script, b"#!/bin/sh\necho opened\n").unwrap();
    assert_eq!(verify_audio(&script).unwrap_err(), UNPLAYABLE);
}
