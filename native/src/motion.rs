//! Read the desktop accessibility preference once, off the event-loop thread.
//! No polling, shell evaluation, or animation timer is needed while idle.
#[cfg(target_os = "linux")]
use std::path::Path;

pub(crate) fn reduced(mode: i32, system: bool) -> bool {
    match mode {
        1 => true,
        2 => false,
        _ => system,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn setting(command: &str, args: &[&str]) -> Option<String> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                return child.wait_with_output().ok().and_then(|output| {
                    String::from_utf8(output.stdout)
                        .ok()
                        .map(|s| s.trim().into())
                });
            }
            Ok(Some(_)) => return None,
            Err(_) => break,
            _ if Instant::now() >= deadline => break,
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[cfg(target_os = "linux")]
fn disabled_in_file(path: &Path, section: &str, key: &str, disabled: &str) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    let mut current = "";
    contents.lines().any(|line| {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current = &line[1..line.len() - 1];
        }
        current == section
            && line
                .split_once('=')
                .is_some_and(|(k, v)| k.trim() == key && v.trim().eq_ignore_ascii_case(disabled))
    })
}

pub(crate) fn system_reduced() -> bool {
    if std::env::var("OVERTALK_REDUCED_MOTION").as_deref() == Ok("1") {
        return true;
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(config) = dirs::config_dir() {
            for gtk in ["gtk-3.0", "gtk-4.0"] {
                if disabled_in_file(
                    &config.join(gtk).join("settings.ini"),
                    "Settings",
                    "gtk-enable-animations",
                    "false",
                ) {
                    return true;
                }
            }
            if disabled_in_file(
                &config.join("kdeglobals"),
                "KDE",
                "AnimationDurationFactor",
                "0",
            ) {
                return true;
            }
        }
        return setting(
            "gsettings",
            &["get", "org.gnome.desktop.interface", "enable-animations"],
        )
        .as_deref()
            == Some("false");
    }
    #[cfg(target_os = "macos")]
    {
        return setting(
            "/usr/bin/defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .as_deref()
            == Some("1");
    }
    #[cfg(target_os = "windows")]
    {
        #[link(name = "user32")]
        extern "system" {
            fn SystemParametersInfoW(action: u32, param: u32, value: *mut i32, flags: u32) -> i32;
        }
        let mut enabled = 1;
        // SPI_GETCLIENTAREAANIMATION writes a BOOL to the supplied pointer.
        // https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-systemparametersinfow
        // SAFETY: `enabled` is a live, writable i32 (BOOL) for the whole call, which is
        // the only memory SPI_GETCLIENTAREAANIMATION writes; uiParam and flags are 0.
        return unsafe { SystemParametersInfoW(0x1042, 0, &mut enabled, 0) != 0 && enabled == 0 };
    }
    #[allow(unreachable_code)]
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_choice_overrides_desktop_preference() {
        assert!(reduced(0, true));
        assert!(!reduced(0, false));
        assert!(reduced(1, false));
        assert!(!reduced(2, true));
        assert!(reduced(123, true));
    }
}
