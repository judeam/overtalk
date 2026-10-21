//! End to end against the engine's schema: learning from a person's
//! filing, filing upcoming events and finished meetings, and brief facts.
use super::signals::{Event, Own};
use serde_json::json;
use sqlx::SqlitePool;

async fn library() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    sqlx::raw_sql(
        "INSERT INTO projects(id,name,created_at,updated_at) VALUES
           ('orion','Orion','2026-10-01','2026-10-01'),('atlas','Atlas','2026-10-01','2026-10-01');
         INSERT INTO project_vocabulary(id,project_id,term) VALUES('v1','orion','Kestrel');
         INSERT INTO project_people(id,project_id,name,role) VALUES('p1','orion','Maya Chen','Product lead');
         INSERT INTO workspace_connections(id,provider,name,username,created_at) VALUES('work','google','Work','me@mine.example','2026-10-01');",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

const TEAM: &str = r#"[{"name":"Maya Chen","email":"maya@acme.example","status":"accepted"},{"name":"Jonas Weber","email":"jonas@acme.example","status":"accepted"},{"name":"Me","email":"me@mine.example","self":true,"status":"accepted"}]"#;

async fn recorded(pool: &SqlitePool, id: &str, title: &str, series: &str, people: &str) {
    sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,?,'2026-10-14 10:00:00','2026-10-14 10:00:00')")
        .bind(id).bind(title).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO meeting_events(meeting_id,event_id,source,series_id,title,starts_at,ends_at,participants,linked_by,created_at) VALUES(?,?,'google:work',?,?,'2026-10-14T10:00:00Z','2026-10-14T10:30:00Z',?,'join','2026-10-14')")
        .bind(id).bind(format!("event-{id}")).bind(series).bind(title).bind(people).execute(pool).await.unwrap();
}

async fn upcoming(pool: &SqlitePool, id: &str, title: &str, series: &str, people: &str) {
    let start = chrono::Utc::now() + chrono::Duration::minutes(45);
    sqlx::query("INSERT INTO workspace_calendar(id,source,uid,recurrence_id,title,description,starts_at,ends_at,timezone,all_day,location,meeting_url,participants,organizer,series_id) VALUES(?,'google:work',?,'',?,'',?,?,'UTC',0,'','https://meet.google.com/abc-defg-hij',?,'',?)")
        .bind(id).bind(id).bind(title).bind(start.to_rfc3339()).bind((start + chrono::Duration::minutes(30)).to_rfc3339()).bind(people).bind(series)
        .execute(pool).await.unwrap();
}

async fn event_project(pool: &SqlitePool, id: &str) -> Option<(Option<String>, String, String)> {
    sqlx::query_as("SELECT project_id,origin,reason FROM event_projects WHERE event_id=?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn filing_one_meeting_files_the_series_and_the_team_from_then_on() {
    let pool = library().await;
    recorded(&pool, "m1", "Orion release review", "series-1", TEAM).await;
    super::learn::filed(&pool, Some("m1"), None, Some("orion")).await;
    let rules: Vec<(String, String)> = sqlx::query_as(
        "SELECT kind,value FROM project_match_rules WHERE project_id='orion' ORDER BY kind,value",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rules,
        [
            ("account".into(), "google:work".into()),
            ("domain".into(), "acme.example".into()),
            ("email".into(), "jonas@acme.example".into()),
            ("email".into(), "maya@acme.example".into()),
            ("series".into(), "series-1".into()),
        ]
    );

    upcoming(&pool, "next-review", "Release review", "series-1", "[]").await;
    upcoming(&pool, "team-call", "Planning", "", TEAM).await;
    upcoming(
        &pool,
        "one-of-them",
        "Coffee",
        "",
        r#"[{"email":"maya@acme.example"}]"#,
    )
    .await;
    upcoming(
        &pool,
        "strangers",
        "Intro",
        "",
        r#"[{"email":"kim@else.example"}]"#,
    )
    .await;
    super::upcoming::refresh_with(&pool, "confident").await;
    let (project, origin, reason) = event_project(&pool, "next-review").await.unwrap();
    assert_eq!(
        (project.as_deref(), origin.as_str()),
        (Some("orion"), "automatic")
    );
    assert!(
        reason.starts_with("Part of the “Orion release review” series"),
        "{reason}"
    );
    assert_eq!(
        event_project(&pool, "team-call").await.unwrap().1,
        "automatic"
    );
    assert_eq!(
        event_project(&pool, "one-of-them").await.unwrap().1,
        "suggested"
    );
    assert!(event_project(&pool, "strangers").await.is_none());

    // The person's choice for an event is never replaced, and "off" clears
    // only what automatic filing decided.
    sqlx::query(
        "UPDATE event_projects SET project_id='atlas',origin='manual' WHERE event_id='team-call'",
    )
    .execute(&pool)
    .await
    .unwrap();
    super::upcoming::refresh_with(&pool, "confident").await;
    assert_eq!(
        event_project(&pool, "team-call")
            .await
            .unwrap()
            .0
            .as_deref(),
        Some("atlas")
    );
    super::upcoming::refresh_with(&pool, "off").await;
    assert!(event_project(&pool, "next-review").await.is_none());
    assert!(event_project(&pool, "team-call").await.is_some());

    // A switched-off signal stays off when the person files again.
    sqlx::query("UPDATE project_match_rules SET enabled=0 WHERE kind='domain'")
        .execute(&pool)
        .await
        .unwrap();
    super::learn::filed(&pool, Some("m1"), None, Some("orion")).await;
    let enabled: i64 =
        sqlx::query_scalar("SELECT enabled FROM project_match_rules WHERE kind='domain'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(enabled, 0);
}

#[tokio::test]
async fn finished_meetings_are_filed_once_from_their_transcript_and_never_against_a_person() {
    let pool = library().await;
    sqlx::raw_sql(
        "INSERT INTO meetings(id,title,created_at,updated_at) VALUES
           ('m2','Orion launch prep','2026-10-15','2026-10-15'),('m3','Chat','2026-10-15','2026-10-15'),('m4','Orion launch prep','2026-10-15','2026-10-15');
         INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES
           ('t1','m2','Kestrel builds are green for Orion.','00:00',0,4),('t2','m2','Orion ships on Friday.','00:05',5,8),
           ('t3','m3','Lunch was nice.','00:00',0,3),
           ('t4','m4','Kestrel builds are green for Orion. Orion ships.','00:00',0,4);
         INSERT INTO workspace_capture_workflows(meeting_id,project_id,project_origin,created_at,finalized) VALUES('m4',NULL,'manual','2026-10-15',1);",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        super::post::decide_meeting(&pool, "m2", "confident").await,
        Some(("filed", Some("orion".into())))
    );
    let (origin, reason): (String, String) =
        sqlx::query_as("SELECT origin,reason FROM project_meetings WHERE meeting_id='m2'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(origin, "automatic");
    assert!(reason.contains("Title names Orion"), "{reason}");
    // Decided once: moving it by hand is final.
    sqlx::query("DELETE FROM project_meetings WHERE meeting_id='m2'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        super::post::decide_meeting(&pool, "m2", "confident").await,
        None
    );
    assert_eq!(
        super::post::decide_meeting(&pool, "m3", "confident").await,
        Some(("unsorted", None))
    );
    // The person chose "No project" when recording.
    assert_eq!(
        super::post::decide_meeting(&pool, "m4", "confident").await,
        None
    );
}

#[tokio::test]
async fn briefs_quote_last_decisions_with_their_moment_and_list_who_is_coming() {
    let pool = library().await;
    recorded(&pool, "m1", "Orion release review", "series-1", TEAM).await;
    sqlx::raw_sql(
        "INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time) VALUES
           ('a','m1','Let us start.','00:00',0,3),('b','m1','We ship RC2 on Friday.','00:29',29,33),('c','m1','Who owns the rollback plan?','00:41',41,44);
         INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result) VALUES
           ('m1','completed','2026-10-14','2026-10-14','{\"markdown\":\"# Review\\n\\n## Key Decisions\\n- **Ship** RC2 on Friday [S2]\\n\\n## Open Questions\\n- Who owns the rollback plan? [S3](#source-3)\\n\"}');
         INSERT INTO workspace_tasks(id,meeting_id,title,owner,due_date,status,evidence_json,created_at,updated_at) VALUES
           ('task','m1','Draft the rollback plan','Maya','2026-10-01','open','[]','2026-10-14','2026-10-14');
         INSERT INTO project_meetings(meeting_id,project_id,origin) VALUES('m1','orion','manual');",
    )
    .execute(&pool)
    .await
    .unwrap();
    upcoming(&pool, "next", "Release review", "series-1", TEAM).await;
    let row: crate::extensions::calendar::CalendarEvent =
        sqlx::query_as("SELECT * FROM workspace_calendar WHERE id='next'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let own = Own::load(&pool).await;
    let event = Event::from_row(&json!(row), &own);
    let (summary, sections, related) =
        super::briefs::facts(&pool, &event, Some(&("orion".into(), "Orion".into())))
            .await
            .unwrap();
    assert_eq!(related.len(), 1);
    assert!(
        summary.starts_with("Orion · 2nd meeting in this series · last one"),
        "{summary}"
    );
    let section = |title: &str| {
        sections
            .iter()
            .find(|s| s.title.starts_with(title))
            .unwrap_or_else(|| panic!("{title} missing"))
    };
    let decided = &section("Decided last time").items[0];
    assert_eq!(decided.text, "Ship RC2 on Friday");
    assert_eq!(decided.source.as_ref().unwrap().start, 29.0);
    assert_eq!(
        section("Still open").items[0]
            .source
            .as_ref()
            .unwrap()
            .start,
        41.0
    );
    let action = &section("Open actions").items[0].text;
    assert!(
        action.contains("Draft the rollback plan — Maya · overdue since Thu 1 Oct"),
        "{action}"
    );
    let label = &decided.source.as_ref().unwrap().label;
    assert!(label.ends_with("Wed 14 Oct · 00:29"), "{label}");
    let coming: Vec<&str> = section("Who's coming")
        .items
        .iter()
        .map(|i| i.text.as_str())
        .collect();
    assert_eq!(
        coming.len(),
        2,
        "the person themself is not listed: {coming:?}"
    );
    assert!(
        coming[0].starts_with("Maya Chen (maya@acme.example) · Product lead · last met"),
        "{coming:?}"
    );
    let agenda = super::briefs::fallback_agenda(&sections);
    assert_eq!(agenda[0].text, "Who owns the rollback plan?");
    assert!(agenda[1].text.starts_with("Check: Draft the rollback plan"));
}

#[tokio::test]
async fn the_person_on_nearly_every_imported_invite_is_treated_as_themself() {
    let pool = library().await;
    sqlx::query("DELETE FROM workspace_connections")
        .execute(&pool)
        .await
        .unwrap();
    for (id, guest) in [
        ("a", "maya@acme.example"),
        ("b", "lena@north.example"),
        ("c", "kim@else.example"),
    ] {
        upcoming(
            &pool,
            id,
            "Call",
            "",
            &format!(r#"[{{"email":"{guest}"}},{{"email":"Me@Mine.example"}}]"#),
        )
        .await;
    }
    // A weekly one-to-one counts once, however many occurrences it has.
    for n in 0..6 {
        upcoming(
            &pool,
            &format!("w{n}"),
            "Weekly",
            "weekly",
            r#"[{"email":"maya@acme.example"}]"#,
        )
        .await;
    }
    sqlx::query(
        "UPDATE workspace_calendar SET organizer='me@mine.example' WHERE series_id='weekly'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let own = Own::load(&pool).await;
    assert!(own.is_own("me@mine.example"), "{own:?}");
    assert!(!own.is_own("maya@acme.example"));
    assert!(own.domains.contains("mine.example"));
}
