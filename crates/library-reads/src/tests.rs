//! Every operation against the app's own migrations and synthetic meetings.
use super::*;
use rusqlite::{types::Value as Sql, Connection};
use serde_json::json;

struct Database(Connection);

impl Store for Database {
    fn rows(
        &mut self,
        sql: &'static str,
        args: &[Arg],
    ) -> impl Future<Output = Result<Vec<Value>, String>> + Send {
        let result = (|| {
            let mut statement = self.0.prepare(sql).map_err(|e| e.to_string())?;
            let args = args.iter().map(|arg| match arg {
                Arg::Null => Sql::Null,
                Arg::Int(value) => Sql::Integer(*value),
                Arg::Text(value) => Sql::Text(value.clone()),
            });
            let rows = statement
                .query_map(rusqlite::params_from_iter(args), |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|e| e.to_string())?;
            rows.map(|row| {
                serde_json::from_str(&row.map_err(|e| e.to_string())?).map_err(|e| e.to_string())
            })
            .collect()
        })();
        std::future::ready(result)
    }
}

const NOTES: &str = "## Summary\nThe launch review.\n\n## Decisions\n- Search ships first [S1]\n\n\
    ## Action Items\n| Owner | Task | Due | Reference |\n| --- | --- | --- | --- |\n\
    | Priya | Rework the tables | Friday | [S2] |\n\n## Open questions\n- Who tells the customer? [S3]";

fn library() -> Database {
    let connection = Connection::open_in_memory().unwrap();
    let directory = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/src-tauri/migrations"
    );
    let mut migrations: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    migrations.sort();
    for migration in migrations {
        connection
            .execute_batch(&std::fs::read_to_string(&migration).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", migration.display()));
    }
    let now = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";
    connection
        .execute_batch(&format!(
            "INSERT INTO meetings(id,title,created_at,updated_at) VALUES
              ('m1','Product sync',strftime('%Y-%m-%dT%H:%M:%SZ','now','-2 days'),{now}),
              ('m2','Launch prep',strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 days'),{now}),
              ('gone','Trashed sync',strftime('%Y-%m-%dT%H:%M:%SZ','now','-3 days'),{now});
             INSERT INTO meeting_workspace_state(meeting_id,trashed_at) VALUES('gone',{now});
             INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,\
                 audio_end_time,speaker_id,is_partial) VALUES
              ('tp','m1','unfinished words','00:00',5,6,'A',1),
              ('t1','m1','We ship search first.','00:10',10,14,'A',0),
              ('t2','m1','Priya will rework the tables by Friday.','00:15',15,20,'B',0),
              ('t3','m1','Who tells the customer about search?','00:30',30,33,'A',0),
              ('u1','m2','Search is on the test machines.','00:05',5,8,NULL,0),
              ('g1','gone','Search in the trash.','00:01',1,2,NULL,0);
             INSERT INTO meeting_speakers(meeting_id,id,name) VALUES('m1','A','Lena'),('m1','B',\
                 'Priya');
             INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result)
              VALUES('m1','completed',{now},{now},json_object('markdown','{notes}'));
             INSERT INTO projects(id,name,archived,created_at,updated_at) VALUES
              ('launch','Product launch',0,{now},{now}),('old','Old project',1,{now},{now});
             INSERT INTO project_meetings(meeting_id,project_id) VALUES('m1','launch'),('gone',\
                 'launch');
             INSERT INTO meeting_events(meeting_id,event_id,source,title,starts_at,ends_at,\
                 participants,linked_by,created_at)
              VALUES('m1','e0','google:test','Product sync',{now},{now},
              '[{{\"name\":\"lena\",\"email\":\"lena@example.test\"}},{{\"name\":\"Marco Rossi\",\
                  \"email\":\"marco@example.test\"}}]','join',{now});
             INSERT INTO workspace_tasks(id,meeting_id,title,owner,due_date,status,evidence_json,\
                 created_at,updated_at) VALUES
              ('k1','m1','Rework the tables','Priya',date('now','localtime','-1 day'),'open',
               '[{{\"meeting_id\":\"m1\",\"source_kind\":\"transcript\",\
                   \"body\":\"Priya will rework the tables by Friday.\",\"start_time\":15.0,\
                   \"end_time\":20.0}}]',{now},{now}),
              ('k2','m1','Send the update','Marco',date('now','localtime','+20 days'),'open','[]',\
                  {now},{now}),
              ('k3','m1','Book the room','Lena',NULL,'done','[]',{now},{now});
             INSERT INTO workspace_calendar(id,source,uid,title,starts_at,ends_at,timezone) VALUES
              ('e1','google:test','u1','Launch review',strftime('%Y-%m-%dT%H:%M:%SZ','now',\
                  '+1 day'),
               strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 day','+30 minutes'),'UTC'),
              ('e2','google:test','u2','Quarterly planning',strftime('%Y-%m-%dT%H:%M:%SZ','now',\
                  '+20 days'),
               strftime('%Y-%m-%dT%H:%M:%SZ','now','+20 days','+1 hour'),'UTC');
             INSERT INTO event_projects(event_id,project_id,origin,reason,updated_at)
              VALUES('e1','launch','automatic','Invitees from example.test',{now});
             INSERT INTO meeting_briefs(event_id,project_id,title,starts_at,body,status,created_at,\
                 updated_at)
              VALUES('e1','launch','Launch review',strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 day'),
              '{{\"summary\":\"Third review.\",\"sections\":[{{\"title\":\"Decided last time\",\
                  \"items\":[{{\"text\":\"Search ships first.\",\"source\":{{\"meeting_id\":\"m1\",\
                  \"start\":10,\"label\":\"Product sync · 00:10\"}}}}]}}],\
                  \"agenda\":[{{\"text\":\"Agree who sends the update\",\"source\":null}}],\
                  \"note\":\"\"}}','ready',{now},{now});
             PRAGMA query_only=ON;",
            notes = NOTES.replace('\'', "''"),
        ))
        .unwrap();
    Database(connection)
}

fn all() -> Grants {
    Grants {
        read_meetings: true,
        read_transcripts: true,
        read_summaries: true,
        ..Default::default()
    }
}

fn run(db: &mut Database, grants: Grants, name: &str, args: Value) -> Result<Value, String> {
    complete(call(db, grants, name, &args))
}

#[test]
fn queries_run_on_the_vendored_sqlite_engine() {
    // crates/vendor/libsqlite3-sys bundles SQLite 3.54.1 for rusqlite and SQLx alike.
    let version: String = Connection::open_in_memory()
        .unwrap()
        .query_row("SELECT sqlite_version()", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, rusqlite::version());
    assert!(rusqlite::version_number() >= 3_054_001, "{version}");
}

#[test]
fn grants_are_checked_before_anything_is_read() {
    let mut db = library();
    let meetings_only = Grants {
        read_meetings: true,
        ..Default::default()
    };
    let error = run(
        &mut db,
        meetings_only,
        "show_meeting",
        json!({"meeting_id":"m1"}),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "Turn on “Allow reading notes” in Overtalk → Settings → Automation → Connections."
    );
    assert!(run(&mut db, Grants::default(), "find_meetings", json!({}))
        .unwrap_err()
        .contains("Allow reading meetings"));
    assert!(run(
        &mut db,
        meetings_only,
        "read_transcript",
        json!({"meeting_id":"m1"})
    )
    .unwrap_err()
    .contains("Allow reading transcripts"));
    assert_eq!(
        run(&mut db, all(), "drop_everything", json!({})).unwrap_err(),
        "Unknown library operation"
    );
    assert!(run(&mut db, all(), "find_meetings", json!({"limit": 0})).is_err());
    assert!(run(
        &mut db,
        all(),
        "find_meetings",
        json!({"since": "yesterday"})
    )
    .is_err());
    assert!(run(&mut db, all(), "find_meetings", json!({"surprise": 1}))
        .unwrap_err()
        .starts_with("Unknown argument “surprise”"));
}

#[test]
fn meetings_filter_by_project_date_and_title_and_skip_the_trash() {
    let mut db = library();
    let found = run(&mut db, all(), "find_meetings", json!({})).unwrap();
    let ids: Vec<_> = found["meetings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].clone())
        .collect();
    assert_eq!(ids, [json!("m2"), json!("m1")]);
    assert_eq!(
        found["meetings"][1]["project"],
        json!({"id":"launch","name":"Product launch"})
    );
    assert_eq!(found["meetings"][1]["has_notes"], true);
    assert_eq!(found["meetings"][0]["has_notes"], false);
    let launch = run(
        &mut db,
        all(),
        "find_meetings",
        json!({"project":"product LAUNCH"}),
    )
    .unwrap();
    assert_eq!(launch["meetings"].as_array().unwrap().len(), 1);
    let paged = run(&mut db, all(), "find_meetings", json!({"limit":1})).unwrap();
    assert_eq!(paged["has_more"], true);
    let titled = run(&mut db, all(), "find_meetings", json!({"title":"prep"})).unwrap();
    assert_eq!(titled["meetings"][0]["id"], "m2");
    let future = run(
        &mut db,
        all(),
        "find_meetings",
        json!({"since":"2999-01-01"}),
    )
    .unwrap();
    assert_eq!(future["meetings"], json!([]));
    assert!(
        run(&mut db, all(), "find_meetings", json!({"project":"Nope"}))
            .unwrap_err()
            .contains("No project is called “Nope”")
    );
}

#[test]
fn a_meeting_shows_its_notes_with_timed_sources_and_people() {
    let mut db = library();
    let notes_only = Grants {
        read_transcripts: false,
        ..all()
    };
    let shown = run(
        &mut db,
        notes_only,
        "show_meeting",
        json!({"meeting_id":"m1"}),
    )
    .unwrap();
    assert_eq!(shown["title"], "Product sync");
    // The partial line at 00:05 is not numbered, so [S1] is the line at 00:10.
    assert_eq!(
        shown["decisions"],
        json!([{"text":"Search ships first","time":"00:10","sources":[
            {"id":"S1","start":10.0,"end":14.0,"time":"00:10","speaker":"Lena"}]}])
    );
    assert_eq!(shown["actions"][0]["owner"], "Priya");
    assert_eq!(shown["actions"][0]["due"], "Friday");
    assert_eq!(shown["actions"][0]["time"], "00:15");
    assert_eq!(
        shown["open_questions"][0]["text"],
        "Who tells the customer?"
    );
    assert_eq!(
        shown["people"],
        json!([
            {"name":"Lena","spoke":true,"invited":true,"email":"lena@example.test"},
            {"name":"Priya","spoke":true,"invited":false},
            {"name":"Marco Rossi","spoke":false,"invited":true,"email":"marco@example.test"}
        ])
    );
    assert!(shown["notes"]["markdown"]
        .as_str()
        .unwrap()
        .starts_with("## Summary"));
    let quoted = run(&mut db, all(), "show_meeting", json!({"meeting_id":"m1"})).unwrap();
    assert_eq!(
        quoted["decisions"][0]["sources"][0]["quote"],
        "We ship search first."
    );
    assert_eq!(
        run(&mut db, all(), "show_meeting", json!({"meeting_id":"gone"})).unwrap_err(),
        "Meeting not found"
    );
}

#[test]
fn transcripts_are_numbered_like_note_sources() {
    let mut db = library();
    let page = run(
        &mut db,
        all(),
        "read_transcript",
        json!({"meeting_id":"m1","limit":2,"offset":1}),
    )
    .unwrap();
    assert_eq!(page["total_count"], 3);
    assert_eq!(page["has_more"], false);
    assert_eq!(
        page["lines"][0],
        json!({"source":"S2","start":15.0,"end":20.0,"time":"00:15","speaker":"Priya",
               "text":"Priya will rework the tables by Friday."})
    );
}

#[test]
fn search_covers_what_is_granted_and_says_what_was_not_searched() {
    let mut db = library();
    let meetings_only = Grants {
        read_meetings: true,
        ..Default::default()
    };
    let titles = run(
        &mut db,
        meetings_only,
        "search_meetings",
        json!({"query":"SYNC"}),
    )
    .unwrap();
    assert_eq!(titles["results"].as_array().unwrap().len(), 1);
    assert_eq!(titles["results"][0]["kind"], "title");
    assert_eq!(titles["not_searched"].as_array().unwrap().len(), 2);
    let everything = run(&mut db, all(), "search_meetings", json!({"query":"search"})).unwrap();
    let hits: Vec<_> = everything["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|hit| {
            (
                hit["kind"].as_str().unwrap(),
                hit["id"].as_str().unwrap(),
                hit["time"].clone(),
            )
        })
        .collect();
    assert_eq!(
        hits,
        [
            ("notes", "m1", json!("00:10")),
            ("transcript", "m2", json!("00:05")),
            ("transcript", "m1", json!("00:10")),
            ("transcript", "m1", json!("00:30")),
        ]
    );
    assert_eq!(everything["results"][0]["text"], "Search ships first");
    assert_eq!(everything["results"][3]["source"], "S3");
    assert_eq!(everything["not_searched"], json!([]));
}

#[test]
fn projects_count_their_meetings_and_open_actions() {
    let mut db = library();
    let listed = run(&mut db, all(), "list_projects", json!({})).unwrap();
    assert_eq!(listed["projects"].as_array().unwrap().len(), 1);
    let launch = &listed["projects"][0];
    assert_eq!(
        (launch["meetings"].clone(), launch["open_actions"].clone()),
        (json!(1), json!(2))
    );
    assert_eq!(launch["next_event"]["title"], "Launch review");
    let every = run(
        &mut db,
        all(),
        "list_projects",
        json!({"include_archived":true}),
    )
    .unwrap();
    assert_eq!(every["projects"][1]["archived"], true);
}

#[test]
fn actions_list_tracked_and_noted_work_with_sources() {
    let mut db = library();
    let actions = run(&mut db, all(), "list_actions", json!({})).unwrap();
    let tracked = actions["tracked"].as_array().unwrap();
    assert_eq!(tracked.len(), 2);
    assert_eq!(tracked[0]["title"], "Rework the tables");
    assert_eq!(tracked[0]["overdue"], true);
    assert_eq!(tracked[1]["overdue"], false);
    assert_eq!(
        tracked[0]["sources"],
        json!([{"meeting_id":"m1","start":15.0,"end":20.0,"time":"00:15",
                "quote":"Priya will rework the tables by Friday."}])
    );
    assert_eq!(actions["from_notes"][0]["text"], "Rework the tables");
    assert_eq!(actions["from_notes"][0]["meeting"]["id"], "m1");
    let overdue = run(
        &mut db,
        all(),
        "list_actions",
        json!({"due":"overdue","include_notes":false}),
    )
    .unwrap();
    assert_eq!(overdue["tracked"].as_array().unwrap().len(), 1);
    assert_eq!(overdue["from_notes"], json!([]));
    let marco = run(&mut db, all(), "list_actions", json!({"owner":"marco"})).unwrap();
    assert_eq!(marco["tracked"][0]["title"], "Send the update");
    assert_eq!(marco["from_notes"], json!([]));
    let unquoted = Grants {
        read_transcripts: false,
        ..all()
    };
    let actions = run(&mut db, unquoted, "list_actions", json!({})).unwrap();
    assert!(actions["tracked"][0]["sources"][0].get("quote").is_none());
    assert!(run(&mut db, all(), "list_actions", json!({"due":"someday"})).is_err());
}

#[test]
fn briefs_cover_upcoming_events_and_name_their_sources() {
    let mut db = library();
    let briefs = run(&mut db, all(), "upcoming_briefs", json!({})).unwrap();
    let events = briefs["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    let brief = &events[0]["brief"];
    assert_eq!(events[0]["project"]["name"], "Product launch");
    assert_eq!(brief["status"], "ready");
    assert_eq!(brief["summary"], "Third review.");
    let item = &brief["sections"][0]["items"][0];
    assert_eq!(item["source"]["meeting_title"], "Product sync");
    assert_eq!(item["source"]["time"], "00:10");
    assert_eq!(
        brief["agenda"][0],
        json!({"text":"Agree who sends the update","source":null})
    );
    let month = run(&mut db, all(), "upcoming_briefs", json!({"days":30})).unwrap();
    assert_eq!(month["events"][1]["title"], "Quarterly planning");
    assert_eq!(month["events"][1]["brief"], Value::Null);
}

#[test]
fn grants_come_only_from_true_values() {
    let grants = Grants::from_settings(
        &json!({"read_meetings":true,"read_transcripts":"true","read_summaries":1}),
    );
    assert_eq!(
        grants,
        Grants {
            read_meetings: true,
            ..Default::default()
        }
    );
}
