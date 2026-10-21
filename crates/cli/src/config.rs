//! Finding the app's connection settings, and what they allow this client.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use overtalk_library_reads::{Grants, SETTINGS_PLACE};
use serde_json::Value;

use crate::http::App;

/// The app's identifier, which names its data folder on every system.
pub(crate) const APP_IDENTIFIER: &str = "com.judeam.sonefold";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Linux
        }
    }
}

/// Reads an environment variable; tests pass their own.
pub(crate) type Env<'a> = &'a dyn Fn(&str) -> Option<OsString>;

pub(crate) fn system_env(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// An absolute directory from an environment variable.
pub(crate) fn directory(env: Env, name: &str) -> Option<PathBuf> {
    env(name)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub(crate) fn home(os: Os, env: Env) -> Option<PathBuf> {
    match os {
        Os::Windows => directory(env, "USERPROFILE"),
        _ => directory(env, "HOME"),
    }
}

/// The app's data folder, resolved as the app itself resolves it: XDG data
/// home on Linux, Application Support on macOS and roaming AppData on Windows.
pub(crate) fn app_data_dir(os: Os, env: Env) -> Option<PathBuf> {
    let base = match os {
        Os::Linux => directory(env, "XDG_DATA_HOME")
            .or_else(|| home(os, env).map(|home| home.join(".local").join("share")))?,
        Os::Mac => home(os, env)?.join("Library").join("Application Support"),
        Os::Windows => directory(env, "APPDATA")?,
    };
    Some(base.join(APP_IDENTIFIER))
}

/// --config, else OVERTALK_CONFIG, else the app's own settings file.
pub(crate) fn settings_path(explicit: Option<&Path>, os: Os, env: Env) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    if let Some(path) = env("OVERTALK_CONFIG").filter(|path| !path.is_empty()) {
        return Ok(path.into());
    }
    app_data_dir(os, env)
        .map(|dir| dir.join("integrations.json"))
        .ok_or_else(|| {
            "Cannot find your home folder. Pass --config with the settings file Overtalk uses."
                .into()
        })
}

/// Which switch and token a process uses: commands use the command line's,
/// the MCP server its own, so either can be revoked alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Client {
    Cli,
    Mcp,
}

impl Client {
    fn key(self) -> &'static str {
        match self {
            Client::Cli => "cli",
            Client::Mcp => "mcp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Client::Cli => "Command line access",
            Client::Mcp => "MCP access",
        }
    }
}

pub(crate) struct Access {
    pub path: PathBuf,
    pub client: Client,
}

impl Access {
    /// The saved settings, read again for every request so a revoked grant applies at once.
    pub fn settings(&self) -> Result<Value, String> {
        let turn_on = format!("Turn on “{}” in {SETTINGS_PLACE}", self.client.label());
        let bytes = match std::fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "{turn_on}, then select Save. (No settings at {} yet.)",
                    self.path.display()
                ))
            }
            Err(error) => return Err(format!("Cannot read {}: {error}", self.path.display())),
        };
        if bytes.len() > 1024 * 1024 {
            return Err("The connection settings file is too large".into());
        }
        let settings: Value = serde_json::from_slice(&bytes)
            .map_err(|_| format!("{} is not valid JSON", self.path.display()))?;
        if settings[format!("{}_enabled", self.client.key())] != true {
            return Err(format!("{turn_on}."));
        }
        Ok(settings)
    }

    pub fn grants(&self) -> Result<Grants, String> {
        Ok(Grants::from_settings(&self.settings()?))
    }

    /// The library the app saved with its settings, or the one beside them.
    pub fn database(&self, settings: &Value) -> PathBuf {
        settings["database_path"]
            .as_str()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| self.path.with_file_name("meeting_minutes.sqlite"))
    }

    /// The running app's local API, with this client's own token.
    pub fn app(&self, settings: &Value) -> Result<App, String> {
        let token = settings[format!("{}_token", self.client.key())]
            .as_str()
            .filter(|token| token.len() >= 32)
            .ok_or_else(|| {
                format!("Select Save in {SETTINGS_PLACE} to create this client’s access token.")
            })?;
        let port = match settings["api_port"].as_u64() {
            None | Some(0) => 8421,
            Some(port) if (1024..=65535).contains(&port) => port as u16,
            Some(_) => return Err("The saved local API port is invalid".into()),
        };
        Ok(App {
            port,
            token: token.to_owned(),
            client: self.client.key(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), OsString::from(value)))
            .collect();
        move |name| vars.get(name).cloned()
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
    fn the_settings_file_is_found_where_each_system_keeps_app_data() {
        let linux = env(&[("HOME", &host("/home/ada"))]);
        assert_eq!(
            settings_path(None, Os::Linux, &linux).unwrap(),
            PathBuf::from(host(
                "/home/ada/.local/share/com.judeam.sonefold/integrations.json"
            ))
        );
        let xdg = env(&[
            ("HOME", &host("/home/ada")),
            ("XDG_DATA_HOME", &host("/data")),
        ]);
        assert_eq!(
            settings_path(None, Os::Linux, &xdg).unwrap(),
            PathBuf::from(host("/data/com.judeam.sonefold/integrations.json"))
        );
        // A relative XDG_DATA_HOME is invalid by the specification and ignored.
        let relative = env(&[("HOME", &host("/home/ada")), ("XDG_DATA_HOME", "data")]);
        assert_eq!(
            app_data_dir(Os::Linux, &relative).unwrap(),
            PathBuf::from(host("/home/ada/.local/share/com.judeam.sonefold"))
        );
        let mac = env(&[("HOME", &host("/Users/ada"))]);
        assert_eq!(
            settings_path(None, Os::Mac, &mac).unwrap(),
            PathBuf::from(host(
                "/Users/ada/Library/Application Support/com.judeam.sonefold/integrations.json"
            ))
        );
        let appdata = if cfg!(windows) {
            r"C:\Users\ada\AppData\Roaming"
        } else {
            "/c/Users/ada/AppData/Roaming"
        };
        let windows = env(&[("APPDATA", appdata)]);
        assert_eq!(
            settings_path(None, Os::Windows, &windows).unwrap(),
            PathBuf::from(appdata)
                .join("com.judeam.sonefold")
                .join("integrations.json")
        );
        assert!(settings_path(None, Os::Linux, &env(&[])).is_err());
    }

    #[test]
    fn an_explicit_path_wins_over_the_environment() {
        let vars = env(&[
            ("HOME", "/home/ada"),
            ("OVERTALK_CONFIG", "/elsewhere.json"),
        ]);
        assert_eq!(
            settings_path(None, Os::Linux, &vars).unwrap(),
            PathBuf::from("/elsewhere.json")
        );
        assert_eq!(
            settings_path(Some(Path::new("/given.json")), Os::Linux, &vars).unwrap(),
            PathBuf::from("/given.json")
        );
    }

    #[test]
    fn each_client_needs_its_own_switch_and_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("integrations.json");
        let access = |client| Access {
            path: path.clone(),
            client,
        };
        let missing = access(Client::Cli).settings().unwrap_err();
        assert!(missing.starts_with("Turn on “Command line access” in Overtalk → Settings → Automation → Connections, then select Save."));
        std::fs::write(
            &path,
            json!({"cli_enabled":true,"cli_token":"c".repeat(64),"mcp_token":"short","read_meetings":true,"api_port":9000}).to_string(),
        )
        .unwrap();
        let settings = access(Client::Cli).settings().unwrap();
        assert!(access(Client::Cli).grants().unwrap().read_meetings);
        assert_eq!(access(Client::Cli).app(&settings).unwrap().port, 9000);
        assert_eq!(
            access(Client::Mcp).settings().unwrap_err(),
            "Turn on “MCP access” in Overtalk → Settings → Automation → Connections."
        );
        assert!(access(Client::Mcp).app(&settings).is_err());
        assert_eq!(
            access(Client::Cli).database(&settings),
            dir.path().join("meeting_minutes.sqlite")
        );
        assert_eq!(
            access(Client::Cli).database(&json!({"database_path":"/lib.sqlite"})),
            PathBuf::from("/lib.sqlite")
        );
    }
}
