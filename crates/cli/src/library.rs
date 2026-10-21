//! Reads go straight to the library file, read-only, so they work while the
//! app is closed. A protected library is read through the running app instead,
//! which records each read in its audit log. Changes always go through the app.
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use overtalk_library_reads::{call, complete, operation, Arg, Grant, Grants, Store};
use rusqlite::{types::Value as Sql, Connection, OpenFlags, OptionalExtension};
use serde_json::Value;

use crate::config::Access;
use crate::http::CallError;

struct Library(Connection);

impl Store for Library {
    fn rows(
        &mut self,
        sql: &str,
        args: &[Arg],
    ) -> impl Future<Output = Result<Vec<Value>, String>> + Send {
        std::future::ready(rows(&self.0, sql, args))
    }
}

fn rows(connection: &Connection, sql: &str, args: &[Arg]) -> Result<Vec<Value>, String> {
    let unreadable = |error: rusqlite::Error| format!("Cannot read the meeting library: {error}");
    let mut statement = connection.prepare(sql).map_err(unreadable)?;
    let args = args.iter().map(|arg| match arg {
        Arg::Null => Sql::Null,
        Arg::Int(value) => Sql::Integer(*value),
        Arg::Text(value) => Sql::Text(value.clone()),
    });
    let rows = statement
        .query_map(rusqlite::params_from_iter(args), |row| {
            row.get::<_, String>(0)
        })
        .map_err(unreadable)?;
    rows.map(|row| {
        serde_json::from_str(&row.map_err(unreadable)?)
            .map_err(|_| "The meeting library returned an unreadable row".to_owned())
    })
    .collect()
}

/// A read-only connection, or `None` when the library is protected.
fn open(path: &Path) -> Result<Option<Connection>, String> {
    if !path.is_file() {
        return Err(format!(
            "No meeting library at {}. Open Overtalk once to create it.",
            path.display()
        ));
    }
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| format!("Cannot open the meeting library: {error}"))?;
    let failed = |error: rusqlite::Error| format!("Cannot read the meeting library: {error}");
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(failed)?;
    connection
        .pragma_update(None, "query_only", true)
        .map_err(failed)?;
    // Libraries from before privacy controls have no policy. A policy that is
    // present must be valid: a damaged one never permits an unaudited read.
    let policy: Option<String> = connection
        .query_row(
            "SELECT policy FROM privacy_settings WHERE id=1",
            [],
            |row| row.get(0),
        )
        .optional()
        .or_else(|error| match error {
            rusqlite::Error::SqliteFailure(_, Some(message))
                if message.contains("no such table") =>
            {
                Ok(None)
            }
            error => Err(failed(error)),
        })?;
    let protected = match policy.as_deref().map(serde_json::from_str::<Value>) {
        None => false,
        Some(Ok(Value::Object(policy))) => match policy.get("protected_mode") {
            None | Some(Value::Bool(false)) => false,
            Some(Value::Bool(true)) => true,
            Some(_) => return Err(damaged()),
        },
        Some(_) => return Err(damaged()),
    };
    Ok((!protected).then_some(connection))
}

fn damaged() -> String {
    "The library’s privacy policy is unreadable. Open Overtalk to check it.".into()
}

/// Runs a library operation for this client, under the grants saved right now.
pub(crate) fn read(access: &Access, name: &str, arguments: &Value) -> Result<Value, String> {
    let settings = access.settings()?;
    let grants = Grants::from_settings(&settings);
    grants.require(operation(name).ok_or("Unknown library operation")?.grants)?;
    match open(&access.database(&settings))? {
        Some(connection) => complete(call(&mut Library(connection), grants, name, arguments)),
        None => access
            .app(&settings)?
            .call(name, arguments)
            .map_err(|error| match error {
                CallError::NotRunning => "This library is protected, so reads go through Overtalk \
                    and are recorded in its audit log. Start Overtalk, then try again."
                    .into(),
                error => error.message(),
            }),
    }
}

/// Grants an app operation needs; the app checks them again.
pub(crate) fn control_grants(name: &str) -> &'static [Grant] {
    match name {
        "start_recording" | "stop_recording" | "pause_recording" | "resume_recording" => {
            &[Grant::Meetings, Grant::RecordControl]
        }
        _ => &[Grant::Meetings],
    }
}

/// Runs an operation in the running app, which applies the change itself.
pub(crate) fn control(access: &Access, name: &str, arguments: &Value) -> Result<Value, String> {
    let settings = access.settings()?;
    Grants::from_settings(&settings).require(control_grants(name))?;
    access
        .app(&settings)?
        .call(name, arguments)
        .map_err(CallError::message)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::Client;
    use serde_json::json;

    /// A library file built from the app's migrations, with one meeting.
    pub(crate) fn library(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("meeting_minutes.sqlite");
        let connection = Connection::open(&path).unwrap();
        // The app keeps its library in WAL mode; readers must cope with that.
        connection
            .pragma_update_and_check(None, "journal_mode", "wal", |row| row.get::<_, String>(0))
            .unwrap();
        let migrations = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../frontend/src-tauri/migrations"
        );
        let mut files: Vec<_> = std::fs::read_dir(migrations)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
            .collect();
        files.sort();
        for file in files {
            connection
                .execute_batch(&std::fs::read_to_string(file).unwrap())
                .unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO meetings(id,title,created_at,updated_at)
                  VALUES('m1','Synthetic review','2026-10-18T09:00:00Z','2026-10-18T09:00:00Z');
                 INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time)
                  VALUES('t1','m1','We ship on Friday.','00:04',4,7);
                 INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result)
                  VALUES('m1','completed','now','now','{\"markdown\":\"## Decisions\\n- Ship on Friday [S1]\"}');",
            )
            .unwrap();
        path
    }

    pub(crate) fn grant(dir: &Path, client: Client, settings: Value) -> Access {
        let path = dir.join("integrations.json");
        let mut settings = settings;
        settings["database_path"] = json!(dir.join("meeting_minutes.sqlite"));
        std::fs::write(&path, settings.to_string()).unwrap();
        Access { path, client }
    }

    #[test]
    fn reads_open_the_library_read_only_under_current_grants() {
        let dir = tempfile::tempdir().unwrap();
        library(dir.path());
        let access = grant(
            dir.path(),
            Client::Cli,
            json!({"cli_enabled":true,"read_meetings":true}),
        );
        let found = read(&access, "find_meetings", &json!({})).unwrap();
        assert_eq!(found["meetings"][0]["title"], "Synthetic review");
        assert_eq!(
            read(&access, "show_meeting", &json!({"meeting_id":"m1"})).unwrap_err(),
            "Turn on “Allow reading notes” in Overtalk → Settings → Automation → Connections."
        );
        let access = grant(
            dir.path(),
            Client::Cli,
            json!({"cli_enabled":true,"read_meetings":true,"read_summaries":true}),
        );
        let shown = read(&access, "show_meeting", &json!({"meeting_id":"m1"})).unwrap();
        assert_eq!(shown["decisions"][0]["time"], "00:04");
        // The connection refuses writes even if a query tried one.
        let connection = open(&dir.path().join("meeting_minutes.sqlite"))
            .unwrap()
            .unwrap();
        assert!(connection.execute("DELETE FROM meetings", []).is_err());
    }

    #[test]
    fn a_protected_library_is_read_only_through_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let path = library(dir.path());
        Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE privacy_settings SET policy='{\"protected_mode\":true}' WHERE id=1",
                [],
            )
            .unwrap();
        let closed = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let access = grant(
            dir.path(),
            Client::Cli,
            json!({"cli_enabled":true,"cli_token":"c".repeat(64),"api_port":port,"read_meetings":true}),
        );
        let error = read(&access, "find_meetings", &json!({})).unwrap_err();
        assert!(error.starts_with("This library is protected"), "{error}");
        Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE privacy_settings SET policy='not json' WHERE id=1",
                [],
            )
            .unwrap();
        assert_eq!(
            read(&access, "find_meetings", &json!({})).unwrap_err(),
            damaged()
        );
    }

    #[test]
    fn controls_need_their_grant_before_contacting_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let access = grant(
            dir.path(),
            Client::Cli,
            json!({"cli_enabled":true,"cli_token":"c".repeat(64),"read_meetings":true}),
        );
        assert_eq!(
            control(&access, "start_recording", &json!({})).unwrap_err(),
            "Turn on “Allow recording control” in Overtalk → Settings → Automation → Connections."
        );
    }
}
