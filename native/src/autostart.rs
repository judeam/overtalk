//! Open Overtalk when the user signs in.
//!
//! - Linux: an XDG autostart entry, `~/.config/autostart/overtalk.desktop`.
//! - macOS: a per-user LaunchAgent, `~/Library/LaunchAgents/io.github.judeam.Overtalk.plist`,
//!   which opens the app bundle through Launch Services at login. macOS lists it
//!   under Login Items as a background item.
//! - Windows: the `Overtalk` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//!
//! Each records the program it starts, so `repair` can follow a moved or
//! updated installation.
use std::{
    io,
    path::{Path, PathBuf},
};

pub(crate) fn supported() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos", windows))
}

pub(crate) fn enabled() -> bool {
    supported() && system::target().is_some()
}

pub(crate) fn set(enabled: bool) -> io::Result<()> {
    if enabled {
        system::write(&executable()?)
    } else {
        system::remove()
    }
}

/// Point an existing entry at this executable when its program has gone, for
/// example after an update removed the version it named.
pub(crate) fn repair() {
    if system::target().is_some_and(|program| !program.exists()) {
        if let Ok(exe) = executable() {
            let _ = system::write(&exe);
        }
    }
}

/// The program to start: an AppImage runs from a temporary mount, so start the
/// image itself; a macOS app starts as its bundle.
fn executable() -> io::Result<PathBuf> {
    match std::env::var_os("APPIMAGE") {
        Some(image) if cfg!(target_os = "linux") && !image.is_empty() => Ok(image.into()),
        _ => {
            let exe = std::env::current_exe()?;
            Ok(if cfg!(target_os = "macos") {
                app_bundle(&exe).unwrap_or(exe)
            } else {
                exe
            })
        }
    }
}

/// `/Applications/Overtalk.app` for `/Applications/Overtalk.app/Contents/MacOS/overtalk`.
fn app_bundle(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app")
        .then(|| bundle.to_owned())
}

fn check_path(program: &str) -> io::Result<()> {
    if program.contains(['\n', '\r', '\0']) {
        return Err(io::Error::other(
            "The Overtalk program path cannot be started at sign-in",
        ));
    }
    Ok(())
}

#[cfg_attr(windows, allow(dead_code))]
fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

#[cfg_attr(windows, allow(dead_code))]
fn remove_file(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

#[cfg(target_os = "linux")]
mod system {
    use super::*;

    fn entry() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("autostart").join("overtalk.desktop"))
    }
    pub(super) fn target() -> Option<PathBuf> {
        desktop_target(&std::fs::read_to_string(entry()?).ok()?)
    }
    pub(super) fn write(exe: &Path) -> io::Result<()> {
        let path = entry().ok_or_else(|| io::Error::other("No configuration folder"))?;
        let program = exe.to_string_lossy();
        check_path(&program)?;
        write_file(&path, &desktop_entry(&program))
    }
    pub(super) fn remove() -> io::Result<()> {
        entry().map_or(Ok(()), |path| remove_file(&path))
    }
}

#[cfg(target_os = "macos")]
mod system {
    use super::*;

    fn entry() -> Option<PathBuf> {
        dirs::home_dir().map(|home| {
            home.join("Library/LaunchAgents")
                .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
        })
    }
    pub(super) fn target() -> Option<PathBuf> {
        launch_agent_target(&std::fs::read_to_string(entry()?).ok()?)
    }
    pub(super) fn write(program: &Path) -> io::Result<()> {
        let path = entry().ok_or_else(|| io::Error::other("No home folder"))?;
        let program = program.to_str().ok_or_else(|| {
            io::Error::other("The Overtalk program path cannot be started at sign-in")
        })?;
        check_path(program)?;
        write_file(&path, &launch_agent(program))
    }
    pub(super) fn remove() -> io::Result<()> {
        entry().map_or(Ok(()), |path| remove_file(&path))
    }
}

#[cfg(windows)]
mod system {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteKeyValueW, RegGetValueW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY,
            RRF_RT_REG_SZ,
        },
    };

    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    /// Settings > Apps > Startup records a user's choice to turn an entry off here.
    const APPROVED: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
    const NAME: &str = "Overtalk";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    fn read(key: &str, kind: u32) -> Option<Vec<u8>> {
        let (key, name) = (wide(key), wide(NAME));
        let mut size = 0u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                kind,
                null_mut(),
                null_mut(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS || size == 0 || size > 64 * 1024 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                kind,
                null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        (status == ERROR_SUCCESS).then(|| {
            data.truncate(size as usize);
            data
        })
    }
    fn delete(key: &str) -> io::Result<()> {
        let (key, name) = (wide(key), wide(NAME));
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            status => Err(io::Error::from_raw_os_error(status as i32)),
        }
    }
    pub(super) fn target() -> Option<PathBuf> {
        // The first byte of the approval record is odd once the user has
        // switched the entry off in Settings or Task Manager.
        if read(APPROVED, RRF_RT_REG_BINARY).is_some_and(|data| data.first() == Some(&3)) {
            return None;
        }
        let data = read(RUN, RRF_RT_REG_SZ)?;
        let units: Vec<u16> = data
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        run_value_target(&String::from_utf16(&units).ok()?)
    }
    pub(super) fn write(exe: &Path) -> io::Result<()> {
        let program = exe.to_string_lossy();
        check_path(&program)?;
        let (key, name, value) = (wide(RUN), wide(NAME), wide(&run_value(&program)));
        let mut handle: HKEY = null_mut();
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                null(),
                &mut handle,
                null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let status = unsafe {
            RegSetValueExW(
                handle,
                name.as_ptr(),
                0,
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        unsafe { RegCloseKey(handle) };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        // Turning the setting on here also clears an earlier "off" in Settings.
        delete(APPROVED)
    }
    pub(super) fn remove() -> io::Result<()> {
        delete(RUN)?;
        delete(APPROVED)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod system {
    use super::*;
    pub(super) fn target() -> Option<PathBuf> {
        None
    }
    pub(super) fn write(_: &Path) -> io::Result<()> {
        Err(io::Error::other("Opening at sign-in is not available here"))
    }
    pub(super) fn remove() -> io::Result<()> {
        Ok(())
    }
}

// The formats below are plain text, so they are tested on every platform.

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn desktop_target(entry: &str) -> Option<PathBuf> {
    entry
        .lines()
        .find_map(|line| line.strip_prefix("X-Overtalk-Path="))
        .map(PathBuf::from)
}

#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn desktop_entry(program: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Overtalk\nComment=Local meeting notebook\nExec={}\nIcon=overtalk\nTerminal=false\nX-GNOME-Autostart-enabled=true\nX-Overtalk-Path={program}\n",
        exec_value(program)
    )
}

/// Quote an argument as the Desktop Entry specification requires, then escape
/// the result as a string value.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn exec_value(program: &str) -> String {
    let plain = program
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+,".contains(c));
    let argument = if plain {
        program.to_owned()
    } else {
        let mut quoted = String::from("\"");
        for c in program.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(c);
        }
        quoted.push('"');
        quoted
    };
    argument.replace('\\', "\\\\").replace('%', "%%")
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
const LAUNCH_AGENT_LABEL: &str = "io.github.judeam.Overtalk";

/// A LaunchAgent that runs once at login. A bundle is opened through Launch
/// Services, so macOS applies its privacy permissions to Overtalk.app.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn launch_agent(program: &str) -> String {
    let arguments: Vec<&str> = if program.ends_with(".app") {
        vec!["/usr/bin/open", "-a", program]
    } else {
        vec![program]
    };
    let arguments: String = arguments
        .iter()
        .map(|argument| format!("    <string>{}</string>\n", xml_escape(argument)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
  <key>Label</key>\n  <string>{LAUNCH_AGENT_LABEL}</string>\n\
  <key>ProgramArguments</key>\n  <array>\n{arguments}  </array>\n\
  <key>RunAtLoad</key>\n  <true/>\n\
  <key>LimitLoadToSessionType</key>\n  <string>Aqua</string>\n\
  <key>ProcessType</key>\n  <string>Interactive</string>\n\
</dict>\n</plist>\n"
    )
}

/// The program a LaunchAgent written by `launch_agent` starts: the last of its arguments.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn launch_agent_target(plist: &str) -> Option<PathBuf> {
    let (_, rest) = plist.split_once("<key>ProgramArguments</key>")?;
    let (array, _) = rest.split_once("</array>")?;
    let last = array.rsplit_once("<string>")?.1.split_once("</string>")?.0;
    Some(PathBuf::from(xml_unescape(last)))
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
fn xml_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// A Run value is a command line; the quotes keep a path with spaces whole.
/// Windows file names cannot contain quotes.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn run_value(program: &str) -> String {
    format!("\"{program}\"")
}

#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn run_value_target(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let program = match value.strip_prefix('"') {
        Some(rest) => rest.split_once('"')?.0,
        None => value.split_whitespace().next()?,
    };
    (!program.is_empty()).then(|| PathBuf::from(program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_start_the_program_and_quote_unusual_paths() {
        let entry = desktop_entry("/opt/overtalk/overtalk");
        assert!(entry.contains("\nExec=/opt/overtalk/overtalk\n"));
        assert!(entry.contains("\nX-GNOME-Autostart-enabled=true\n"));
        assert_eq!(
            desktop_target(&entry),
            Some(PathBuf::from("/opt/overtalk/overtalk"))
        );
        assert_eq!(
            exec_value("/home/a b/Over$talk 100%"),
            "\"/home/a b/Over\\\\$talk 100%%\""
        );
        assert_eq!(exec_value("/tmp/x\"y"), "\"/tmp/x\\\\\"y\"");
    }

    #[test]
    fn writing_and_removing_an_entry_file() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("autostart/overtalk.desktop");
        write_file(&path, &desktop_entry("/no/such/overtalk")).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            desktop_target(&text),
            Some(PathBuf::from("/no/such/overtalk"))
        );
        remove_file(&path).unwrap();
        assert!(!path.exists());
        remove_file(&path).unwrap();
        assert!(check_path("/tmp/bad\nname").is_err());
        assert!(check_path("/opt/Overtalk/overtalk").is_ok());
    }

    #[test]
    fn launch_agents_open_the_bundle_and_round_trip_their_target() {
        let bundle = "/Applications/Over & <talk>.app";
        let plist = launch_agent(bundle);
        assert!(plist.contains("<string>/usr/bin/open</string>\n    <string>-a</string>"));
        assert!(plist.contains("Over &amp; &lt;talk&gt;.app"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains(&format!("<string>{LAUNCH_AGENT_LABEL}</string>")));
        assert_eq!(launch_agent_target(&plist), Some(PathBuf::from(bundle)));
        let plain = launch_agent("/Users/me/overtalk/target/release/overtalk");
        assert!(!plain.contains("/usr/bin/open"));
        assert_eq!(
            launch_agent_target(&plain),
            Some(PathBuf::from("/Users/me/overtalk/target/release/overtalk"))
        );
        assert_eq!(launch_agent_target("<plist></plist>"), None);
    }

    #[test]
    fn bundles_are_found_from_their_executable() {
        assert_eq!(
            app_bundle(Path::new(
                "/Applications/Overtalk.app/Contents/MacOS/overtalk"
            )),
            Some(PathBuf::from("/Applications/Overtalk.app"))
        );
        assert_eq!(
            app_bundle(Path::new("/Users/me/src/target/release/overtalk")),
            None
        );
    }

    #[test]
    fn run_values_quote_the_program_and_read_back() {
        let program = r"C:\Users\Ana Lee\AppData\Local\Programs\Overtalk\overtalk.exe";
        let value = run_value(program);
        assert_eq!(value, format!("\"{program}\""));
        assert_eq!(run_value_target(&value), Some(PathBuf::from(program)));
        assert_eq!(
            run_value_target(r"C:\Tools\overtalk.exe --minimized"),
            Some(PathBuf::from(r"C:\Tools\overtalk.exe"))
        );
        assert_eq!(run_value_target("\"unterminated"), None);
        assert_eq!(run_value_target("  "), None);
    }
}
