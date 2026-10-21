//! Command line parsing. Every read becomes a library operation and its
//! arguments, the same ones the MCP tools and the local API take.
use std::collections::VecDeque;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::setup::Client;

pub(crate) const USAGE: &str = "\
overtalk-cli: your Overtalk meetings for scripts and AI assistants, on this computer.

Usage: overtalk-cli [--json] [--config FILE] <command>

Reading (needs the grants you turned on in Overtalk → Settings → Automation → Connections):
  meetings [--project NAME] [--since DATE] [--until DATE] [--title TEXT] [--limit N] [--offset N]
  show <meeting-id>                 Notes, decisions, actions and open questions with source times
  transcript <meeting-id>           The whole transcript, numbered S1, S2… as notes cite it
  search <text> [--limit N]         Titles, notes and transcripts you allow
  projects [--all]                  Projects with meeting and open-action counts
  actions [--project NAME] [--owner NAME] [--due overdue|today|week] [--days N] [--no-notes]
  briefs [--days N] [--project NAME]  Upcoming meetings with their prepared brief and sources
  export <meeting-id> [--format md|json] [--output FILE] [--overwrite]

Through the running app:
  record start [--title TEXT] | record stop | record status
  call <operation> [--arguments JSON]

Assistants:
  mcp                               Serve these tools over MCP on stdin and stdout
  setup [claude-code|claude-desktop|codex|cursor|vscode] [--write] [--file FILE]
  skill [install [--dir DIR] | print]

Dates are YYYY-MM-DD. --json prints machine-readable output. Nothing is sent off this computer.
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
    Meetings,
    Show,
    Search,
    Projects,
    Actions,
    Briefs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Markdown,
    Json,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Command {
    Read {
        operation: &'static str,
        arguments: Value,
        view: View,
    },
    Transcript {
        meeting_id: String,
    },
    Export {
        meeting_id: String,
        format: Format,
        output: Option<PathBuf>,
        overwrite: bool,
    },
    Record {
        operation: &'static str,
        arguments: Value,
    },
    Call {
        operation: String,
        arguments: Value,
    },
    Mcp,
    Setup {
        client: Option<Client>,
        write: bool,
        file: Option<PathBuf>,
    },
    SkillHelp,
    SkillInstall {
        dir: Option<PathBuf>,
    },
    SkillPrint,
    Help,
    Version,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Invocation {
    pub json: bool,
    pub config: Option<PathBuf>,
    pub command: Command,
}

/// The words after the command name, with --name value and --name=value options.
struct Words {
    command: String,
    items: VecDeque<String>,
}

impl Words {
    fn positional(&mut self, what: &str) -> Result<String, String> {
        match self.items.front() {
            Some(word) if !word.starts_with("--") => Ok(self.items.pop_front().unwrap()),
            _ => Err(format!("{} needs {what}", self.command)),
        }
    }

    /// Takes out the words that are not options or option values, wherever they are.
    fn free_words(&mut self, takes_value: &[&str]) -> Vec<String> {
        let mut words = Vec::new();
        let mut options = VecDeque::new();
        while let Some(word) = self.items.pop_front() {
            let valued = word
                .strip_prefix("--")
                .is_some_and(|name| takes_value.contains(&name));
            if word.starts_with("--") {
                options.push_back(word);
                if valued {
                    options.extend(self.items.pop_front());
                }
            } else {
                words.push(word);
            }
        }
        self.items = options;
        words
    }

    /// The next option and its value, if it takes one.
    fn option(&mut self, takes_value: &[&str]) -> Result<Option<(String, Option<String>)>, String> {
        let Some(word) = self.items.pop_front() else {
            return Ok(None);
        };
        let Some(option) = word.strip_prefix("--") else {
            return Err(format!("Unexpected “{word}” after {}", self.command));
        };
        let (name, inline) = match option.split_once('=') {
            Some((name, value)) => (name.to_owned(), Some(value.to_owned())),
            None => (option.to_owned(), None),
        };
        if !takes_value.contains(&name.as_str()) {
            return match inline {
                None => Ok(Some((name, None))),
                Some(_) => Err(format!("--{name} takes no value")),
            };
        }
        let value = match inline {
            Some(value) => value,
            None => self
                .items
                .pop_front()
                .ok_or_else(|| format!("--{name} needs a value"))?,
        };
        Ok(Some((name, Some(value))))
    }

    /// Options into a library argument object, renaming --flags to argument names.
    fn arguments(
        &mut self,
        text: &[(&str, &str)],
        numbers: &[(&str, &str)],
        switches: &[(&str, &str, bool)],
    ) -> Result<Value, String> {
        let valued: Vec<&str> = text.iter().chain(numbers).map(|(flag, _)| *flag).collect();
        let mut arguments = Map::new();
        while let Some((name, value)) = self.option(&valued)? {
            if let Some((_, key)) = text.iter().find(|(flag, _)| *flag == name) {
                arguments.insert((*key).into(), json!(value));
            } else if let Some((_, key)) = numbers.iter().find(|(flag, _)| *flag == name) {
                let value = value.unwrap_or_default();
                let number: i64 = value
                    .parse()
                    .map_err(|_| format!("--{name} needs a whole number, not “{value}”"))?;
                arguments.insert((*key).into(), json!(number));
            } else if let Some((_, key, on)) = switches.iter().find(|(flag, _, _)| *flag == name) {
                arguments.insert((*key).into(), json!(on));
            } else {
                return Err(self.unknown(&name));
            }
        }
        Ok(Value::Object(arguments))
    }

    fn unknown(&self, name: &str) -> String {
        format!(
            "Unknown option --{name} for {}. Run overtalk-cli help.",
            self.command
        )
    }

    fn done(&mut self) -> Result<(), String> {
        match self.option(&[])? {
            Some((name, _)) => Err(self.unknown(&name)),
            None => Ok(()),
        }
    }
}

pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut json = false;
    let mut config = None;
    let mut rest = VecDeque::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--config" => {
                config = Some(PathBuf::from(
                    args.next().ok_or("--config needs a file path")?,
                ))
            }
            "-h" | "--help" => rest.push_front("help".to_owned()),
            "-V" | "--version" => rest.push_front("version".to_owned()),
            _ => match arg.strip_prefix("--config=") {
                Some(path) => config = Some(PathBuf::from(path)),
                None => rest.push_back(arg),
            },
        }
    }
    let command = rest.pop_front().unwrap_or_else(|| "help".into());
    let mut words = Words {
        command: command.clone(),
        items: rest,
    };
    let read = |operation, arguments, view| Command::Read {
        operation,
        arguments,
        view,
    };
    let command = match command.as_str() {
        "help" => Command::Help,
        "version" => Command::Version,
        "meetings" => read(
            "find_meetings",
            words.arguments(
                &[
                    ("project", "project"),
                    ("since", "since"),
                    ("until", "until"),
                    ("title", "title"),
                ],
                &[("limit", "limit"), ("offset", "offset")],
                &[],
            )?,
            View::Meetings,
        ),
        "show" => {
            let id = words.positional("a meeting id")?;
            words.done()?;
            read("show_meeting", json!({"meeting_id": id}), View::Show)
        }
        "transcript" => {
            let meeting_id = words.positional("a meeting id")?;
            words.done()?;
            Command::Transcript { meeting_id }
        }
        "search" => {
            let query = words.free_words(&["limit"]).join(" ");
            if query.trim().is_empty() {
                return Err("search needs the text to look for".into());
            }
            let mut arguments = words.arguments(&[], &[("limit", "limit")], &[])?;
            arguments["query"] = json!(query);
            read("search_meetings", arguments, View::Search)
        }
        "projects" => read(
            "list_projects",
            words.arguments(&[], &[], &[("all", "include_archived", true)])?,
            View::Projects,
        ),
        "actions" => read(
            "list_actions",
            words.arguments(
                &[("project", "project"), ("owner", "owner"), ("due", "due")],
                &[("days", "days"), ("limit", "limit")],
                &[("no-notes", "include_notes", false)],
            )?,
            View::Actions,
        ),
        "briefs" => read(
            "upcoming_briefs",
            words.arguments(&[("project", "project")], &[("days", "days")], &[])?,
            View::Briefs,
        ),
        "export" => {
            let meeting_id = words.positional("a meeting id")?;
            let (mut format, mut output, mut overwrite) = (Format::Markdown, None, false);
            while let Some((name, value)) = words.option(&["format", "output"])? {
                match (name.as_str(), value) {
                    ("format", Some(value)) => {
                        format = match value.as_str() {
                            "md" | "markdown" => Format::Markdown,
                            "json" => Format::Json,
                            _ => return Err("--format is md or json".into()),
                        }
                    }
                    ("output", Some(value)) => output = Some(PathBuf::from(value)),
                    ("overwrite", None) => overwrite = true,
                    _ => return Err(words.unknown(&name)),
                }
            }
            Command::Export {
                meeting_id,
                format,
                output,
                overwrite,
            }
        }
        "record" => {
            let action = words.positional("start, stop or status")?;
            let (operation, arguments) = match action.as_str() {
                "start" => (
                    "start_recording",
                    words.arguments(&[("title", "title")], &[], &[])?,
                ),
                "stop" => ("stop_recording", json!({})),
                "status" => ("get_recording_state", json!({})),
                _ => return Err("record takes start, stop or status".into()),
            };
            words.done()?;
            Command::Record {
                operation,
                arguments,
            }
        }
        "call" => {
            let operation = words.positional("an operation name")?;
            let mut arguments = json!({});
            while let Some((name, value)) = words.option(&["arguments"])? {
                if name != "arguments" {
                    return Err(words.unknown(&name));
                }
                arguments = serde_json::from_str(&value.unwrap_or_default())
                    .ok()
                    .filter(Value::is_object)
                    .ok_or("--arguments must be a JSON object")?;
            }
            Command::Call {
                operation,
                arguments,
            }
        }
        "mcp" => {
            words.done()?;
            Command::Mcp
        }
        "setup" => {
            let client = match words.items.front() {
                Some(word) if !word.starts_with("--") => {
                    let name = words.positional("a client")?;
                    Some(Client::parse(&name).ok_or_else(|| {
                        format!(
                            "Unknown client “{name}”. Use one of: {}",
                            Client::ALL.map(Client::name).join(", ")
                        )
                    })?)
                }
                _ => None,
            };
            let (mut write, mut file) = (false, None);
            while let Some((name, value)) = words.option(&["file"])? {
                match (name.as_str(), value) {
                    ("write", None) => write = true,
                    ("file", Some(value)) => file = Some(PathBuf::from(value)),
                    _ => return Err(words.unknown(&name)),
                }
            }
            if client.is_none() && (write || file.is_some()) {
                return Err(
                    "Name the client to set up, for example: setup claude-code --write".into(),
                );
            }
            Command::Setup {
                client,
                write,
                file,
            }
        }
        "skill" => match words.items.pop_front().as_deref() {
            None => Command::SkillHelp,
            Some("print") => {
                words.done()?;
                Command::SkillPrint
            }
            Some("install") => {
                let mut dir = None;
                while let Some((name, value)) = words.option(&["dir"])? {
                    match (name.as_str(), value) {
                        ("dir", Some(value)) => dir = Some(PathBuf::from(value)),
                        _ => return Err(words.unknown(&name)),
                    }
                }
                Command::SkillInstall { dir }
            }
            Some(other) => return Err(format!("skill takes install or print, not “{other}”")),
        },
        other => return Err(format!("Unknown command “{other}”. Run overtalk-cli help.")),
    };
    Ok(Invocation {
        json,
        config,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_words(line: &str) -> Result<Invocation, String> {
        parse(line.split_whitespace().map(str::to_owned))
    }

    fn command(line: &str) -> Command {
        parse_words(line).unwrap().command
    }

    #[test]
    fn reads_become_library_operations_with_typed_arguments() {
        assert_eq!(
            command("meetings --project Launch --since 2026-10-01 --limit=5"),
            Command::Read {
                operation: "find_meetings",
                arguments: json!({"project":"Launch","since":"2026-10-01","limit":5}),
                view: View::Meetings
            }
        );
        for line in [
            "search launch date --limit 3",
            "search --limit 3 launch date",
        ] {
            assert_eq!(
                command(line),
                Command::Read {
                    operation: "search_meetings",
                    arguments: json!({"query":"launch date","limit":3}),
                    view: View::Search
                }
            );
        }
        assert_eq!(
            command("actions --due overdue --no-notes --owner Priya"),
            Command::Read {
                operation: "list_actions",
                arguments: json!({"due":"overdue","include_notes":false,"owner":"Priya"}),
                view: View::Actions
            }
        );
        assert_eq!(
            command("projects --all"),
            Command::Read {
                operation: "list_projects",
                arguments: json!({"include_archived":true}),
                view: View::Projects
            }
        );
        assert_eq!(
            command("show m1"),
            Command::Read {
                operation: "show_meeting",
                arguments: json!({"meeting_id":"m1"}),
                view: View::Show
            }
        );
    }

    #[test]
    fn global_options_go_anywhere_and_help_is_the_default() {
        let invocation = parse_words("briefs --json --days 3 --config /tmp/x.json").unwrap();
        assert!(invocation.json);
        assert_eq!(invocation.config, Some(PathBuf::from("/tmp/x.json")));
        assert_eq!(
            invocation.command,
            Command::Read {
                operation: "upcoming_briefs",
                arguments: json!({"days":3}),
                view: View::Briefs
            }
        );
        assert_eq!(command(""), Command::Help);
        assert_eq!(command("--help"), Command::Help);
        assert_eq!(command("show --version"), Command::Version);
    }

    #[test]
    fn controls_exports_and_setup_parse() {
        assert_eq!(
            command("record start --title Standup"),
            Command::Record {
                operation: "start_recording",
                arguments: json!({"title":"Standup"})
            }
        );
        assert_eq!(
            command("record status"),
            Command::Record {
                operation: "get_recording_state",
                arguments: json!({})
            }
        );
        assert_eq!(
            command("export m1 --format json --output notes.json --overwrite"),
            Command::Export {
                meeting_id: "m1".into(),
                format: Format::Json,
                output: Some("notes.json".into()),
                overwrite: true
            }
        );
        assert_eq!(
            command("setup vscode --write --file /tmp/mcp.json"),
            Command::Setup {
                client: Some(Client::VsCode),
                write: true,
                file: Some("/tmp/mcp.json".into())
            }
        );
        assert_eq!(
            command("setup"),
            Command::Setup {
                client: None,
                write: false,
                file: None
            }
        );
        assert_eq!(
            command("skill install --dir /tmp/skills"),
            Command::SkillInstall {
                dir: Some("/tmp/skills".into())
            }
        );
        assert_eq!(
            command(r#"call rename_meeting --arguments {"meeting_id":"m1","title":"New"}"#),
            Command::Call {
                operation: "rename_meeting".into(),
                arguments: json!({"meeting_id":"m1","title":"New"})
            }
        );
    }

    #[test]
    fn mistakes_are_reported_not_ignored() {
        for (line, error) in [
            ("show", "show needs a meeting id"),
            ("meetings --limit many", "--limit needs a whole number, not “many”"),
            ("meetings --colour red", "Unknown option --colour for meetings. Run overtalk-cli help."),
            ("meetings --project", "--project needs a value"),
            ("projects --all=yes", "--all takes no value"),
            ("record rewind", "record takes start, stop or status"),
            ("export m1 --format pdf", "--format is md or json"),
            ("setup teams", "Unknown client “teams”. Use one of: claude-code, claude-desktop, codex, cursor, vscode"),
            ("setup --write", "Name the client to set up, for example: setup claude-code --write"),
            ("call x --arguments [1]", "--arguments must be a JSON object"),
            ("search", "search needs the text to look for"),
            ("dance", "Unknown command “dance”. Run overtalk-cli help."),
            ("show m1 extra", "Unexpected “extra” after show"),
        ] {
            assert_eq!(parse_words(line).unwrap_err(), error, "{line}");
        }
    }
}
