//! Read-only views of the meeting library for assistants and scripts.
//!
//! The engine's local API and `overtalk-cli` share these operations: the
//! engine runs them over its own pool, the CLI over a read-only SQLite file
//! while the app is closed. The grants they need and the answers they give are
//! therefore the same either way. Every query returns one JSON object per row,
//! built by SQLite, so a store only has to decode text.
use serde_json::Value;
use std::future::Future;

/// A meeting's fields as every operation reports them, from `meetings m` and
/// its project `p`.
macro_rules! meeting_fields {
    () => {
        "'id',m.id,'title',m.title,'created_at',m.created_at,\
         'local_time',strftime('%Y-%m-%d %H:%M',m.created_at,'localtime'),\
         'project',json(CASE WHEN p.id IS NULL THEN NULL \
         ELSE json_object('id',p.id,'name',p.name) END)"
    };
}

/// Meetings that are not in the trash, with their project; `$join` adds rows.
macro_rules! meeting_rows {
    ($join:literal) => {
        concat!(
            " FROM meetings m ",
            $join,
            " LEFT JOIN project_meetings pm ON pm.meeting_id=m.id \
             LEFT JOIN projects p ON p.id=pm.project_id \
             WHERE NOT EXISTS(SELECT 1 FROM meeting_workspace_state w \
             WHERE w.meeting_id=m.id AND w.trashed_at IS NOT NULL)"
        )
    };
}

mod args;
mod meetings;
mod notes;
mod planning;
pub mod proof;
#[cfg(test)]
mod tests;

pub use notes::{clock, notes_markdown};

/// Where a person turns access on. Error messages name it exactly.
pub const SETTINGS_PLACE: &str = "Overtalk → Settings → Automation → Connections";

/// One switch in the app's connection settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    Meetings,
    Transcripts,
    Notes,
    RecordControl,
    NoteEditing,
}

impl Grant {
    /// The switch's label in the app.
    pub fn label(self) -> &'static str {
        match self {
            Grant::Meetings => "Allow reading meetings",
            Grant::Transcripts => "Allow reading transcripts",
            Grant::Notes => "Allow reading notes",
            Grant::RecordControl => "Allow recording control",
            Grant::NoteEditing => "Allow note editing",
        }
    }

    /// What to tell someone whose request needs this switch.
    pub fn missing(self) -> String {
        format!("Turn on “{}” in {SETTINGS_PLACE}.", self.label())
    }
}

/// The grants saved in `integrations.json`. Anything but `true` is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Grants {
    pub read_meetings: bool,
    pub read_transcripts: bool,
    pub read_summaries: bool,
    pub record_control: bool,
    pub write_notes: bool,
}

impl Grants {
    pub fn from_settings(settings: &Value) -> Self {
        let on = |key: &str| settings.get(key) == Some(&Value::Bool(true));
        Grants {
            read_meetings: on("read_meetings"),
            read_transcripts: on("read_transcripts"),
            read_summaries: on("read_summaries"),
            record_control: on("record_control"),
            write_notes: on("write_notes"),
        }
    }

    pub fn allows(&self, grant: Grant) -> bool {
        match grant {
            Grant::Meetings => self.read_meetings,
            Grant::Transcripts => self.read_transcripts,
            Grant::Notes => self.read_summaries,
            Grant::RecordControl => self.record_control,
            Grant::NoteEditing => self.write_notes,
        }
    }

    /// Fails with the first missing grant, naming the switch that turns it on.
    pub fn require(&self, grants: &[Grant]) -> Result<(), String> {
        match grants.iter().find(|grant| !self.allows(**grant)) {
            Some(grant) => Err(grant.missing()),
            None => Ok(()),
        }
    }
}

/// A positional SQL parameter, bound to ?1, ?2, … in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Null,
    Int(i64),
    Text(String),
}

impl From<i64> for Arg {
    fn from(value: i64) -> Self {
        Arg::Int(value)
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Arg::Text(value.to_owned())
    }
}

impl From<Option<String>> for Arg {
    fn from(value: Option<String>) -> Self {
        value.map_or(Arg::Null, Arg::Text)
    }
}

/// Read access to the app database.
pub trait Store {
    /// Runs `sql`, whose only column is a JSON object, and decodes each row.
    fn rows(
        &mut self,
        sql: &str,
        args: &[Arg],
    ) -> impl Future<Output = Result<Vec<Value>, String>> + Send;
}

/// A read operation and the grants it needs before it reads anything.
pub struct Operation {
    pub name: &'static str,
    pub grants: &'static [Grant],
}

pub const OPERATIONS: &[Operation] = &[
    Operation {
        name: "find_meetings",
        grants: &[Grant::Meetings],
    },
    Operation {
        name: "show_meeting",
        grants: &[Grant::Meetings, Grant::Notes],
    },
    Operation {
        name: "read_transcript",
        grants: &[Grant::Meetings, Grant::Transcripts],
    },
    Operation {
        name: "search_meetings",
        grants: &[Grant::Meetings],
    },
    Operation {
        name: "list_projects",
        grants: &[Grant::Meetings],
    },
    Operation {
        name: "list_actions",
        grants: &[Grant::Meetings, Grant::Notes],
    },
    Operation {
        name: "upcoming_briefs",
        grants: &[Grant::Meetings, Grant::Notes],
    },
];

pub fn operation(name: &str) -> Option<&'static Operation> {
    OPERATIONS.iter().find(|operation| operation.name == name)
}

/// Completes an operation on this thread, for stores that answer synchronously
/// such as the CLI's SQLite connection. Such an operation never waits.
pub fn complete(future: impl Future<Output = Result<Value, String>>) -> Result<Value, String> {
    let mut future = std::pin::pin!(future);
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => Err("The library store did not answer synchronously".into()),
    }
}

/// Runs a read operation after checking its grants.
pub async fn call<S: Store>(
    store: &mut S,
    grants: Grants,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let operation = operation(name).ok_or("Unknown library operation")?;
    grants.require(operation.grants)?;
    match operation.name {
        "find_meetings" => meetings::find(store, args).await,
        "show_meeting" => meetings::show(store, grants, args).await,
        "read_transcript" => meetings::transcript(store, args).await,
        "search_meetings" => meetings::search(store, grants, args).await,
        "list_projects" => planning::projects(store, args).await,
        "list_actions" => planning::actions(store, grants, args).await,
        _ => planning::briefs(store, args).await,
    }
}
