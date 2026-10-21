//! overtalk-cli: a person's Overtalk meetings for scripts and AI assistants.
//!
//! Reads work while the app is closed; changes go through the running app.
//! Access follows the grants saved in the app, and nothing here sends
//! anything off this computer.
mod args;
mod config;
mod export;
mod http;
mod library;
mod mcp;
mod output;
mod setup;

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use args::{Command, Invocation, USAGE};
use config::{settings_path, system_env, Access, Client, Os};
use serde_json::{json, Value};

fn main() -> ExitCode {
    let words = std::env::args_os()
        .skip(1)
        .map(|word| word.to_string_lossy().into_owned());
    let invocation = match args::parse(words) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("overtalk-cli: {message}");
            return ExitCode::from(2);
        }
    };
    match run(invocation) {
        Ok(text) => {
            // A closed pipe, as with `| head`, is not an error worth reporting.
            let _ = std::io::stdout().lock().write_all(text.as_bytes());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("overtalk-cli: {message}");
            ExitCode::FAILURE
        }
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

fn run(invocation: Invocation) -> Result<String, String> {
    let os = Os::current();
    let explicit = invocation.config.as_deref();
    let access = |client| -> Result<Access, String> {
        Ok(Access {
            path: settings_path(explicit, os, &system_env)?,
            client,
        })
    };
    let json = invocation.json;
    match invocation.command {
        Command::Help => Ok(USAGE.into()),
        Command::Version => Ok(format!("overtalk-cli {}\n", env!("CARGO_PKG_VERSION"))),
        Command::Read {
            operation,
            arguments,
            view,
        } => {
            let value = library::read(&access(Client::Cli)?, operation, &arguments)?;
            Ok(if json {
                pretty(&value)
            } else {
                output::render(view, &value)
            })
        }
        Command::Transcript { meeting_id } => {
            let (mut meeting, lines) = export::transcript(&access(Client::Cli)?, &meeting_id)?;
            Ok(if json {
                meeting["lines"] = json!(lines);
                pretty(&meeting)
            } else {
                output::transcript(&meeting, &lines)
            })
        }
        Command::Export {
            meeting_id,
            format,
            output,
            overwrite,
        } => {
            let text = export::export(&access(Client::Cli)?, &meeting_id, format)?;
            let Some(path) = output else {
                return Ok(text);
            };
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .create_new(!overwrite)
                .open(&path)
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::AlreadyExists => format!(
                        "{} already exists. Add --overwrite to replace it.",
                        path.display()
                    ),
                    _ => format!("Cannot write {}: {error}", path.display()),
                })?;
            file.write_all(text.as_bytes())
                .map_err(|error| format!("Cannot write {}: {error}", path.display()))?;
            Ok(format!("Saved {}\n", path.display()))
        }
        Command::Record {
            operation,
            arguments,
        } => {
            let value = library::control(&access(Client::Cli)?, operation, &arguments)?;
            Ok(if json {
                pretty(&value)
            } else {
                match operation {
                    "start_recording" => "Recording started.\n".into(),
                    "stop_recording" => "Recording stopped.\n".into(),
                    _ if value["is_paused"] == true => "Recording, paused.\n".into(),
                    _ if value["is_recording"] == true => "Recording.\n".into(),
                    _ => "Not recording.\n".into(),
                }
            })
        }
        Command::Call {
            operation,
            arguments,
        } => Ok(pretty(&library::control(
            &access(Client::Cli)?,
            &operation,
            &arguments,
        )?)),
        Command::Mcp => {
            let access = access(Client::Mcp)?;
            mcp::serve(&access, std::io::stdin().lock(), std::io::stdout().lock())
                .map_err(|error| format!("MCP connection closed: {error}"))?;
            Ok(String::new())
        }
        Command::Setup {
            client,
            write,
            file,
        } => {
            let executable = std::env::current_exe()
                .and_then(|path| path.canonicalize())
                .map_err(|error| format!("Cannot find this program's own path: {error}"))?;
            // A settings file named on the command line must reach the server too.
            let config = explicit
                .map(Path::to_owned)
                .or_else(|| system_env("OVERTALK_CONFIG").map(Into::into));
            let launch = setup::launch(config.as_deref(), &executable, &system_env);
            match client {
                None => Ok(clients(&launch, os)),
                Some(client) => setup_client(client, &launch, write, file.as_deref(), os),
            }
        }
        Command::SkillHelp => Ok(format!(
            "The Overtalk skill teaches an assistant to use these commands and tools.\n\n\
             Install it for Claude Code ({}):\n  overtalk-cli skill install\n\n\
             For another assistant, save the output of `overtalk-cli skill print` as SKILL.md \
             where it reads skills.\n",
            setup::skill_dir(os, &system_env)
                .map(|dir| dir.display().to_string())
                .unwrap_or_else(|| "~/.claude/skills/overtalk".into())
        )),
        Command::SkillPrint => Ok(setup::SKILL.into()),
        Command::SkillInstall { dir } => {
            let dir = dir
                .or_else(|| setup::skill_dir(os, &system_env))
                .ok_or("Cannot find your home folder; pass --dir")?;
            let (path, outcome) = setup::install_skill(&dir)?;
            Ok(match outcome {
                setup::Outcome::Unchanged => {
                    format!("The skill is already installed at {}\n", path.display())
                }
                setup::Outcome::Updated { backup } => format!(
                    "Updated the skill at {}\nYour previous copy is at {}\n",
                    path.display(),
                    backup.display()
                ),
                _ => format!("Installed the skill at {}\n", path.display()),
            })
        }
    }
}

fn clients(launch: &setup::Launch, os: Os) -> String {
    let mut out = String::from("Set up Overtalk's MCP server for an assistant:\n\n");
    for client in setup::Client::ALL {
        out += &format!(
            "  {:<15} overtalk-cli setup {} --write\n",
            client.title(),
            client.name()
        );
    }
    out += &format!(
        "\nWithout --write, setup prints the configuration to add by hand.\nThe server runs: {}\n\n{}\n",
        std::iter::once(&launch.command)
            .chain(&launch.args)
            .map(|word| setup::shell_word(word, os))
            .collect::<Vec<_>>()
            .join(" "),
        mcp::enable_hint()
    );
    out
}

fn setup_client(
    client: setup::Client,
    launch: &setup::Launch,
    write: bool,
    file: Option<&Path>,
    os: Os,
) -> Result<String, String> {
    let path = match file {
        Some(file) => file.to_owned(),
        None => client
            .config_file(os, &system_env)
            .ok_or("Cannot find your home folder; pass --file")?,
    };
    if !write {
        let mut out = format!(
            "{}: add this to {}\n\n{}\n",
            client.title(),
            path.display(),
            client.snippet(launch)
        );
        if let Some(command) = client.command(launch, os) {
            out += &format!("\nOr run:\n  {command}\n");
        }
        out += &format!(
            "\nTo add it for you: overtalk-cli setup {} --write\n{}\n",
            client.name(),
            mcp::enable_hint()
        );
        return Ok(out);
    }
    let restart = format!("Restart {} to use it.", client.title());
    let done = match setup::write(client, &path, launch)? {
        setup::Outcome::Unchanged => {
            "Overtalk is already set up there; nothing changed.".to_owned()
        }
        setup::Outcome::Created => format!("Created it with the Overtalk server. {restart}"),
        setup::Outcome::Added { backup } => format!(
            "Added the Overtalk server; the previous file is saved as {}. {restart}",
            backup.display()
        ),
        setup::Outcome::Updated { backup } => format!(
            "Updated the Overtalk server; the previous file is saved as {}. {restart}",
            backup.display()
        ),
    };
    Ok(format!(
        "{} configuration: {}\n{done}\n{}\n",
        client.title(),
        path.display(),
        mcp::enable_hint()
    ))
}
