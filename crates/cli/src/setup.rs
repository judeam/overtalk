//! MCP configuration for assistant apps, and the Overtalk skill.
//!
//! Writing edits only the Overtalk entry. The rest of the file keeps its bytes,
//! order and comments-free formatting; the original is backed up first, and a
//! file that already says the same thing is left alone.
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::{directory, home, Env, Os};

/// The skill installed for Claude Code, and printed for other assistants.
pub(crate) const SKILL: &str = include_str!("../../../integrations/skills/overtalk/SKILL.md");
pub(crate) const SERVER: &str = "overtalk";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    Cursor,
    VsCode,
}

impl Client {
    pub const ALL: [Client; 5] = [
        Client::ClaudeCode,
        Client::ClaudeDesktop,
        Client::Codex,
        Client::Cursor,
        Client::VsCode,
    ];

    pub fn parse(name: &str) -> Option<Client> {
        Client::ALL.into_iter().find(|client| client.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::ClaudeDesktop => "claude-desktop",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
            Client::VsCode => "vscode",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code",
        }
    }

    /// The user-wide file each app reads its MCP servers from.
    pub fn config_file(self, os: Os, env: Env) -> Option<PathBuf> {
        let config_home = || match os {
            Os::Linux => directory(env, "XDG_CONFIG_HOME")
                .or_else(|| home(os, env).map(|home| home.join(".config"))),
            Os::Mac => home(os, env).map(|home| home.join("Library").join("Application Support")),
            Os::Windows => directory(env, "APPDATA"),
        };
        Some(match self {
            Client::ClaudeCode => directory(env, "CLAUDE_CONFIG_DIR")
                .or_else(|| home(os, env))?
                .join(".claude.json"),
            Client::ClaudeDesktop => config_home()?
                .join("Claude")
                .join("claude_desktop_config.json"),
            Client::Codex => directory(env, "CODEX_HOME")
                .or_else(|| home(os, env).map(|home| home.join(".codex")))?
                .join("config.toml"),
            Client::Cursor => home(os, env)?.join(".cursor").join("mcp.json"),
            Client::VsCode => config_home()?.join("Code").join("User").join("mcp.json"),
        })
    }

    /// The JSON key that holds servers, or `None` for Codex's TOML.
    fn container(self) -> Option<&'static str> {
        match self {
            Client::Codex => None,
            Client::VsCode => Some("servers"),
            _ => Some("mcpServers"),
        }
    }

    fn entry(self, launch: &Launch) -> Value {
        match self {
            Client::ClaudeCode | Client::VsCode => {
                json!({"type": "stdio", "command": launch.command, "args": launch.args})
            }
            _ => json!({"command": launch.command, "args": launch.args}),
        }
    }

    /// The app's own command for adding a server, where it has one.
    pub fn command(self, launch: &Launch, os: Os) -> Option<String> {
        let words = || {
            std::iter::once(&launch.command)
                .chain(&launch.args)
                .map(|word| shell_word(word, os))
                .collect::<Vec<_>>()
                .join(" ")
        };
        match self {
            Client::ClaudeCode => Some(format!(
                "claude mcp add --scope user {SERVER} -- {}",
                words()
            )),
            Client::Codex => Some(format!("codex mcp add {SERVER} -- {}", words())),
            Client::VsCode => {
                let mut entry = self.entry(launch);
                entry["name"] = json!(SERVER);
                Some(format!(
                    "code --add-mcp {}",
                    shell_word(&entry.to_string(), os)
                ))
            }
            Client::ClaudeDesktop | Client::Cursor => None,
        }
    }

    /// What to add to the config file by hand.
    pub fn snippet(self, launch: &Launch) -> String {
        match self.container() {
            Some(container) => {
                serde_json::to_string_pretty(&json!({container: {SERVER: self.entry(launch)}}))
                    .unwrap_or_default()
            }
            None => toml_table(launch),
        }
    }
}

/// How an assistant starts this program's MCP server.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Launch {
    pub command: String,
    pub args: Vec<String>,
}

/// This program, or the AppImage it runs from: files inside a mounted AppImage
/// disappear when it exits, so an assistant must start the AppImage itself.
pub(crate) fn launch(config: Option<&Path>, executable: &Path, env: Env) -> Launch {
    let appimage = env("APPIMAGE").filter(|_| {
        env("APPDIR").is_some_and(|appdir| executable.starts_with(PathBuf::from(appdir)))
    });
    let (command, mut args) = match appimage {
        Some(appimage) => (PathBuf::from(appimage), vec!["cli".to_owned()]),
        None => (executable.to_owned(), Vec::new()),
    };
    if let Some(config) = config {
        args.extend(["--config".to_owned(), config.to_string_lossy().into_owned()]);
    }
    args.push("mcp".to_owned());
    Launch {
        command: command.to_string_lossy().into_owned(),
        args,
    }
}

pub(crate) fn shell_word(word: &str, os: Os) -> String {
    let plain = !word.is_empty()
        && word.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || "/._-:=@+%,".contains(c)
                || (os == Os::Windows && c == '\\')
        });
    if plain {
        word.to_owned()
    } else if os == Os::Windows {
        format!("\"{}\"", word.replace('"', "\\\""))
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

fn toml_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn toml_lines(launch: &Launch) -> [(&'static str, String); 2] {
    [
        ("command", toml_string(&launch.command)),
        (
            "args",
            format!(
                "[{}]",
                launch
                    .args
                    .iter()
                    .map(|arg| toml_string(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ),
    ]
}

fn toml_table(launch: &Launch) -> String {
    let mut table = format!("[mcp_servers.{SERVER}]\n");
    for (key, value) in toml_lines(launch) {
        table += &format!("{key} = {value}\n");
    }
    table
}

#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    Unchanged,
    Created,
    Added { backup: PathBuf },
    Updated { backup: PathBuf },
}

/// Adds or updates the Overtalk server in `path`.
pub(crate) fn write(client: Client, path: &Path, launch: &Launch) -> Result<Outcome, String> {
    let original = match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
    };
    let edited = match (client.container(), original.as_deref()) {
        (Some(_), None) => None,
        (Some(_), Some(text)) if text.trim().is_empty() => None,
        (Some(container), Some(text)) => Some(
            set_json_entry(text, container, &client.entry(launch))
                .map_err(|problem| format!("{} {problem}; it was not changed.", path.display()))?,
        ),
        (None, None) => None,
        (None, Some(text)) => Some(set_toml_entry(text, launch)),
    };
    let (text, replaced) = match edited {
        Some(None) => return Ok(Outcome::Unchanged),
        Some(Some(edit)) => edit,
        None => (
            match client.container() {
                Some(_) => client.snippet(launch) + "\n",
                None => toml_table(launch),
            },
            false,
        ),
    };
    let backup = match &original {
        Some(_) => Some(back_up(path)?),
        None => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| format!("Cannot create {}: {error}", parent.display()))?;
            }
            None
        }
    };
    replace_file(path, &text)?;
    Ok(match backup {
        None => Outcome::Created,
        Some(backup) if replaced => Outcome::Updated { backup },
        Some(backup) => Outcome::Added { backup },
    })
}

fn seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// A copy beside the file, never overwriting an earlier backup.
fn back_up(path: &Path) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stamp = seconds();
    let backup = (0..)
        .map(|n| {
            path.with_file_name(match n {
                0 => format!("{name}.overtalk-backup-{stamp}"),
                n => format!("{name}.overtalk-backup-{stamp}-{n}"),
            })
        })
        .find(|candidate| !candidate.exists())
        .expect("an unused name");
    std::fs::copy(path, &backup)
        .map_err(|error| format!("Cannot back up {}: {error}", path.display()))?;
    Ok(backup)
}

/// Writes beside the file and renames over it, keeping its permissions.
fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.overtalk-{}", std::process::id()));
    let failed = |error: std::io::Error| format!("Cannot write {}: {error}", path.display());
    std::fs::write(&temporary, text).map_err(failed)?;
    if let Ok(metadata) = std::fs::metadata(path) {
        std::fs::set_permissions(&temporary, metadata.permissions()).map_err(failed)?;
    }
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        failed(error)
    })
}

/// A member of a JSON object, as byte offsets into the text.
struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

fn skip_space(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(u8::is_ascii_whitespace) {
        at += 1;
    }
    at
}

/// The end of the string that starts at `at`.
fn string_end(bytes: &[u8], at: usize) -> usize {
    let mut index = at + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return index + 1,
            _ => index += 1,
        }
    }
    bytes.len()
}

/// The end of the value that starts at `at`, in text already known to be valid JSON.
fn value_end(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at) {
        Some(b'"') => string_end(bytes, at),
        Some(b'{' | b'[') => {
            let mut depth = 0;
            let mut index = at;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index = string_end(bytes, index);
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return index + 1;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            bytes.len()
        }
        _ => {
            let mut index = at;
            while index < bytes.len() && !b",}] \t\r\n".contains(&bytes[index]) {
                index += 1;
            }
            index
        }
    }
}

/// The members of the object whose `{` is at `open`, and the index of its `}`.
fn members(text: &str, open: usize) -> (Vec<Member>, usize) {
    let bytes = text.as_bytes();
    let mut members = Vec::new();
    let mut at = open + 1;
    loop {
        at = skip_space(bytes, at);
        match bytes.get(at) {
            Some(b',') => at += 1,
            Some(b'"') => {
                let key_end = string_end(bytes, at);
                let key = serde_json::from_str(&text[at..key_end]).unwrap_or_default();
                let colon = skip_space(bytes, key_end);
                let value_start = skip_space(bytes, colon + 1);
                let end = value_end(bytes, value_start);
                members.push(Member {
                    key,
                    key_start: at,
                    value_start,
                    value_end: end,
                });
                at = end;
            }
            _ => return (members, at),
        }
    }
}

/// The whitespace that starts the line containing `at`.
fn indent_at(text: &str, at: usize) -> &str {
    let line = text[..at].rfind('\n').map_or(0, |newline| newline + 1);
    let width = text[line..]
        .bytes()
        .take_while(|byte| *byte == b' ' || *byte == b'\t')
        .count();
    &text[line..line + width]
}

/// `value` as pretty JSON whose later lines continue at `indent`.
fn render(value: &Value, indent: &str) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_default()
        .replace('\n', &format!("\n{indent}"))
}

/// Adds `key: value` as the last member of the object at `open`.
fn insert(text: &str, open: usize, key: &str, value: &Value) -> String {
    let (existing, close) = members(text, open);
    let outer = indent_at(text, open);
    let multiline = existing
        .first()
        .is_none_or(|first| text[open..first.key_start].contains('\n'));
    let indent = match existing.first() {
        Some(first) if multiline => indent_at(text, first.key_start).to_owned(),
        _ => format!("{outer}  "),
    };
    let member = format!("{}: {}", json!(key), render(value, &indent));
    match existing.last() {
        None => format!(
            "{}{{\n{indent}{member}\n{outer}}}{}",
            &text[..open],
            &text[close + 1..]
        ),
        Some(last) if multiline => format!(
            "{},\n{indent}{member}{}",
            &text[..last.value_end],
            &text[last.value_end..]
        ),
        Some(last) => format!(
            "{}, {}: {}{}",
            &text[..last.value_end],
            json!(key),
            value,
            &text[last.value_end..]
        ),
    }
}

/// Sets `root[container]["overtalk"]` to hold `entry`'s fields, keeping any
/// other fields a person added to it. `Ok(None)` means it already matches;
/// the flag says whether an Overtalk entry was there before.
fn set_json_entry(
    text: &str,
    container: &str,
    entry: &Value,
) -> Result<Option<(String, bool)>, String> {
    let parsed: Value = serde_json::from_str(text)
        .map_err(|_| "is not plain JSON (comments and trailing commas are not supported)")?;
    let root = parsed.as_object().ok_or("does not hold a JSON object")?;
    let servers = match root.get(container) {
        None => None,
        Some(Value::Object(servers)) => Some(servers),
        Some(_) => return Err(format!("has a “{container}” that is not an object")),
    };
    let current = servers.and_then(|servers| servers.get(SERVER));
    let wanted = entry.as_object().expect("entries are objects");
    match current {
        Some(Value::Object(current))
            if wanted
                .iter()
                .all(|(key, value)| current.get(key) == Some(value)) =>
        {
            return Ok(None)
        }
        Some(Value::Object(_)) | None => {}
        Some(_) => return Err(format!("has an “{SERVER}” server that is not an object")),
    }
    let open = text.find('{').expect("a JSON object");
    let (top, _) = members(text, open);
    // Duplicate keys resolve to the last one, as JSON parsers do.
    let Some(servers) = top.iter().rev().find(|member| member.key == container) else {
        return Ok(Some((
            insert(text, open, container, &json!({SERVER: entry})),
            false,
        )));
    };
    let (listed, _) = members(text, servers.value_start);
    let Some(ours) = listed.iter().rev().find(|member| member.key == SERVER) else {
        return Ok(Some((
            insert(text, servers.value_start, SERVER, entry),
            false,
        )));
    };
    // Every edit lies inside our entry, after its opening brace, so that offset holds.
    let mut text = text.to_owned();
    for (key, value) in wanted {
        let (fields, _) = members(&text, ours.value_start);
        match fields.iter().rev().find(|field| field.key == *key) {
            Some(field) => {
                let current = &text[field.value_start..field.value_end];
                if serde_json::from_str::<Value>(current).ok().as_ref() != Some(value) {
                    text = format!(
                        "{}{}{}",
                        &text[..field.value_start],
                        render(value, indent_at(&text, field.key_start)),
                        &text[field.value_end..]
                    );
                }
            }
            None => text = insert(&text, ours.value_start, key, value),
        }
    }
    Ok(Some((text, true)))
}

/// Sets `[mcp_servers.overtalk]`'s command and args, keeping its other keys.
fn set_toml_entry(text: &str, launch: &Launch) -> Option<(String, bool)> {
    let header = format!("[mcp_servers.{SERVER}]");
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(start) = lines.iter().position(|line| line.trim() == header) else {
        let separator = if text.is_empty() || text.ends_with("\n\n") {
            ""
        } else if text.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        return Some((format!("{text}{separator}{}", toml_table(launch)), false));
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let mut section: Vec<String> = lines[start + 1..end]
        .iter()
        .map(|line| line.to_string())
        .collect();
    let mut changed = false;
    for (key, value) in toml_lines(launch) {
        let wanted = format!("{key} = {value}\n");
        let existing = section.iter().position(|line| {
            line.split_once('=')
                .is_some_and(|(name, _)| name.trim() == key)
        });
        match existing {
            Some(index) if section[index].trim_end() == wanted.trim_end() => {}
            Some(index) => {
                section[index] = wanted;
                changed = true;
            }
            None => {
                let at = section
                    .iter()
                    .rposition(|line| !line.trim().is_empty())
                    .map_or(0, |last| last + 1);
                section.insert(at, wanted);
                changed = true;
            }
        }
    }
    if !changed {
        return None;
    }
    if let Some(last) = section
        .iter_mut()
        .rev()
        .find(|line| !line.trim().is_empty())
    {
        if !last.ends_with('\n') {
            last.push('\n');
        }
    }
    Some((
        format!(
            "{}{header}\n{}{}",
            lines[..start].concat(),
            section.concat(),
            lines[end..].concat()
        ),
        true,
    ))
}

/// Where Claude Code looks for personal skills.
pub(crate) fn skill_dir(os: Os, env: Env) -> Option<PathBuf> {
    Some(
        directory(env, "CLAUDE_CONFIG_DIR")
            .or_else(|| home(os, env).map(|home| home.join(".claude")))?
            .join("skills")
            .join(SERVER),
    )
}

/// Installs SKILL.md into `dir`, backing up a different one first.
pub(crate) fn install_skill(dir: &Path) -> Result<(PathBuf, Outcome), String> {
    let path = dir.join("SKILL.md");
    match std::fs::read_to_string(&path) {
        Ok(text) if text == SKILL => return Ok((path, Outcome::Unchanged)),
        Ok(_) => {
            let backup = back_up(&path)?;
            replace_file(&path, SKILL)?;
            return Ok((path, Outcome::Updated { backup }));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot read {}: {error}", path.display())),
    }
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Cannot create {}: {error}", dir.display()))?;
    replace_file(&path, SKILL)?;
    Ok((path, Outcome::Created))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    fn launch() -> Launch {
        Launch {
            command: "/opt/Overtalk/overtalk-cli".into(),
            args: vec!["mcp".into()],
        }
    }

    fn backups(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".overtalk-backup-"))
            .collect();
        names.sort();
        names
    }

    /// A path that is absolute on the host running the tests: simulated Unix paths
    /// need a drive on Windows, where `/home/ada` is not absolute.
    fn host(path: &str) -> String {
        if cfg!(windows) {
            format!("C:{path}")
        } else {
            path.to_owned()
        }
    }

    #[test]
    fn config_files_are_where_each_app_reads_them() {
        let linux = env(&[("HOME", &host("/home/ada"))]);
        let expected = [
            (Client::ClaudeCode, "/home/ada/.claude.json"),
            (
                Client::ClaudeDesktop,
                "/home/ada/.config/Claude/claude_desktop_config.json",
            ),
            (Client::Codex, "/home/ada/.codex/config.toml"),
            (Client::Cursor, "/home/ada/.cursor/mcp.json"),
            (Client::VsCode, "/home/ada/.config/Code/User/mcp.json"),
        ];
        for (client, path) in expected {
            assert_eq!(
                client.config_file(Os::Linux, &linux).unwrap(),
                PathBuf::from(host(path))
            );
        }
        let mac = env(&[
            ("HOME", &host("/Users/ada")),
            ("CODEX_HOME", &host("/codex")),
        ]);
        assert_eq!(
            Client::ClaudeDesktop.config_file(Os::Mac, &mac).unwrap(),
            PathBuf::from(host(
                "/Users/ada/Library/Application Support/Claude/claude_desktop_config.json"
            ))
        );
        assert_eq!(
            Client::Codex.config_file(Os::Mac, &mac).unwrap(),
            PathBuf::from(host("/codex/config.toml"))
        );
        let roaming = if cfg!(windows) {
            r"C:\Users\ada\AppData\Roaming"
        } else {
            "/Roaming"
        };
        let windows = env(&[("USERPROFILE", roaming), ("APPDATA", roaming)]);
        assert_eq!(
            Client::VsCode.config_file(Os::Windows, &windows).unwrap(),
            PathBuf::from(roaming)
                .join("Code")
                .join("User")
                .join("mcp.json")
        );
    }

    #[test]
    fn an_appimage_is_started_through_itself() {
        let vars = env(&[
            ("APPIMAGE", "/home/ada/Overtalk.AppImage"),
            ("APPDIR", "/tmp/.mount_x"),
        ]);
        assert_eq!(
            launch_for("/tmp/.mount_x/usr/bin/overtalk-cli", None, &vars),
            Launch {
                command: "/home/ada/Overtalk.AppImage".into(),
                args: vec!["cli".into(), "mcp".into()]
            }
        );
        assert_eq!(
            launch_for(
                "/usr/lib/overtalk/bin/overtalk-cli",
                Some("/data/i.json"),
                &vars
            ),
            Launch {
                command: "/usr/lib/overtalk/bin/overtalk-cli".into(),
                args: vec!["--config".into(), "/data/i.json".into(), "mcp".into()]
            }
        );
    }

    fn launch_for(executable: &str, config: Option<&str>, vars: Env) -> Launch {
        super::launch(config.map(Path::new), Path::new(executable), vars)
    }

    #[test]
    fn commands_quote_paths_for_the_shell() {
        let spaced = Launch {
            command: "/Applications/Overtalk.app/Contents/MacOS/overtalk-cli".into(),
            args: vec![
                "--config".into(),
                "/Users/ada/My Settings.json".into(),
                "mcp".into(),
            ],
        };
        assert_eq!(
            Client::ClaudeCode.command(&spaced, Os::Mac).unwrap(),
            "claude mcp add --scope user overtalk -- /Applications/Overtalk.app/Contents/MacOS/overtalk-cli \
             --config '/Users/ada/My Settings.json' mcp"
        );
        assert_eq!(
            Client::VsCode.command(&launch(), Os::Linux).unwrap(),
            r#"code --add-mcp '{"args":["mcp"],"command":"/opt/Overtalk/overtalk-cli","name":"overtalk","type":"stdio"}'"#
        );
        assert_eq!(
            shell_word(r"C:\Program Files\x.exe", Os::Windows),
            r#""C:\Program Files\x.exe""#
        );
        assert_eq!(Client::Cursor.command(&launch(), Os::Linux), None);
    }

    #[test]
    fn json_configs_gain_one_entry_and_keep_everything_else() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude_desktop_config.json");
        let original = "{\n    \"mcpServers\": {\n        \"notes\": {\"command\": \"notes\"}\n    },\n    \"zeta\": [1, 2],\n    \"alpha\": true\n}\n";
        std::fs::write(&path, original).unwrap();
        let outcome = write(Client::ClaudeDesktop, &path, &launch()).unwrap();
        let Outcome::Added { backup } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            written,
            r#"{
    "mcpServers": {
        "notes": {"command": "notes"},
        "overtalk": {
          "args": [
            "mcp"
          ],
          "command": "/opt/Overtalk/overtalk-cli"
        }
    },
    "zeta": [1, 2],
    "alpha": true
}
"#
        );
        let parsed: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(parsed["mcpServers"]["notes"], json!({"command":"notes"}));
        assert_eq!(
            parsed["mcpServers"]["overtalk"],
            json!({"command":"/opt/Overtalk/overtalk-cli","args":["mcp"]})
        );
        // Running it again changes nothing and makes no second backup.
        assert_eq!(
            write(Client::ClaudeDesktop, &path, &launch()).unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(backups(dir.path()).len(), 1);
    }

    #[test]
    fn an_existing_entry_is_updated_in_place_and_keeps_added_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mcp.json");
        std::fs::write(
            &path,
            r#"{"servers": {"overtalk": {"type": "stdio", "command": "/old/overtalk-cli", "args": ["mcp"], "env": {"A": "1"}}}, "inputs": []}"#,
        )
        .unwrap();
        assert!(matches!(
            write(Client::VsCode, &path, &launch()).unwrap(),
            Outcome::Updated { .. }
        ));
        let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            parsed,
            json!({"servers":{"overtalk":{"type":"stdio","command":"/opt/Overtalk/overtalk-cli","args":["mcp"],"env":{"A":"1"}}},"inputs":[]})
        );
        assert_eq!(
            write(Client::VsCode, &path, &launch()).unwrap(),
            Outcome::Unchanged
        );
    }

    #[test]
    fn missing_or_empty_configs_are_created_and_broken_ones_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(".claude.json");
        assert_eq!(
            write(Client::ClaudeCode, &path, &launch()).unwrap(),
            Outcome::Created
        );
        let parsed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed["mcpServers"]["overtalk"]["type"], "stdio");
        let empty = dir.path().join("empty.json");
        std::fs::write(&empty, "{}").unwrap();
        assert!(matches!(
            write(Client::Cursor, &empty, &launch()).unwrap(),
            Outcome::Added { .. }
        ));
        assert_eq!(
            serde_json::from_str::<Value>(&std::fs::read_to_string(&empty).unwrap()).unwrap(),
            json!({"mcpServers":{"overtalk":{"command":"/opt/Overtalk/overtalk-cli","args":["mcp"]}}})
        );
        let broken = dir.path().join("broken.json");
        let text = "{\n  // a comment\n  \"mcpServers\": {}\n}";
        std::fs::write(&broken, text).unwrap();
        let error = write(Client::Cursor, &broken, &launch()).unwrap_err();
        assert!(
            error.contains("is not plain JSON") && error.ends_with("it was not changed."),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&broken).unwrap(), text);
        let wrong = dir.path().join("wrong.json");
        std::fs::write(&wrong, r#"{"mcpServers": []}"#).unwrap();
        assert!(write(Client::Cursor, &wrong, &launch()).is_err());
    }

    #[test]
    fn codex_toml_gains_a_table_and_keeps_its_own_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "model = \"o4\"\n\n[mcp_servers.other]\ncommand = \"other\"\n";
        std::fs::write(&path, original).unwrap();
        assert!(matches!(
            write(Client::Codex, &path, &launch()).unwrap(),
            Outcome::Added { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            format!("{original}\n[mcp_servers.overtalk]\ncommand = \"/opt/Overtalk/overtalk-cli\"\nargs = [\"mcp\"]\n")
        );
        assert_eq!(
            write(Client::Codex, &path, &launch()).unwrap(),
            Outcome::Unchanged
        );
        std::fs::write(
            &path,
            "[mcp_servers.overtalk]\ncommand = \"/old\"\nenv = { A = \"1\" }\n\n[profiles.x]\nmodel = \"y\"\n",
        )
        .unwrap();
        assert!(matches!(
            write(Client::Codex, &path, &launch()).unwrap(),
            Outcome::Updated { .. }
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[mcp_servers.overtalk]\ncommand = \"/opt/Overtalk/overtalk-cli\"\nenv = { A = \"1\" }\nargs = [\"mcp\"]\n\n[profiles.x]\nmodel = \"y\"\n"
        );
        assert_eq!(backups(dir.path()).len(), 2);
    }

    #[test]
    fn the_skill_installs_once_and_backs_up_a_changed_copy() {
        let dir = tempfile::tempdir().unwrap();
        let skill = dir.path().join("skills").join("overtalk");
        let (path, outcome) = install_skill(&skill).unwrap();
        assert_eq!(outcome, Outcome::Created);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SKILL);
        assert_eq!(install_skill(&skill).unwrap().1, Outcome::Unchanged);
        std::fs::write(&path, "my edits").unwrap();
        let (_, outcome) = install_skill(&skill).unwrap();
        let Outcome::Updated { backup } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(std::fs::read_to_string(backup).unwrap(), "my edits");
        assert!(SKILL.starts_with("---\nname: overtalk\n"));
    }
}
