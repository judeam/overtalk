//! Conservative device-encryption evidence. Unknown never means encrypted.
use serde::Serialize;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

#[derive(Serialize)]
pub struct Evidence {
    pub verified: bool,
    pub detail: String,
}

#[cfg(target_os = "linux")]
fn encrypted_device(device: &Path, depth: usize) -> bool {
    if depth > 10 {
        return false;
    }
    let Ok(device) = device.canonicalize() else {
        return false;
    };
    let Some(name) = device.file_name() else {
        return false;
    };
    let block = PathBuf::from("/sys/class/block").join(name);
    if std::fs::read_to_string(block.join("dm/uuid")).is_ok_and(|v| v.starts_with("CRYPT-")) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(block.join("slaves")) else {
        return false;
    };
    let entries: Vec<_> = entries.filter_map(Result::ok).collect();
    !entries.is_empty()
        && entries
            .iter()
            .all(|e| encrypted_device(&e.path(), depth + 1))
}

#[cfg(target_os = "linux")]
fn mount_device(mounts: &[u8], path: &Path) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let path = path.canonicalize().ok()?;
    let mut best = None;
    let mut best_depth = 0;
    for line in mounts.split(|c| *c == b'\n').filter(|l| !l.is_empty()) {
        let mut fields = line
            .split(|c| c.is_ascii_whitespace())
            .filter(|f| !f.is_empty());
        fields.next()?; // mount ID
        fields.next()?; // parent ID
        let device = fields.next()?;
        fields.next()?; // root within the filesystem
        let mount = fields.next()?;
        let device = std::str::from_utf8(device).ok()?;
        let Some((major, minor)) = device.split_once(':') else {
            continue;
        };
        if major.parse::<u32>().is_err() || minor.parse::<u32>().is_err() {
            continue;
        }
        // mountinfo escapes whitespace and backslashes as octal bytes. Preserve
        // non-UTF-8 paths and use component matching, not string-prefix matching.
        let mut bytes = Vec::with_capacity(mount.len());
        let mut at = 0;
        while at < mount.len() {
            let source = mount;
            if source[at] == b'\\' {
                let digits = source.get(at + 1..at + 4)?;
                if !digits.iter().all(|c| matches!(c, b'0'..=b'7')) {
                    return None;
                }
                let value = (digits[0] - b'0') as u16 * 64
                    + (digits[1] - b'0') as u16 * 8
                    + (digits[2] - b'0') as u16;
                if value == 0 || value > 255 {
                    return None;
                }
                bytes.push(value as u8);
                at += 4;
            } else {
                bytes.push(source[at]);
                at += 1;
            }
        }
        let mount = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        let depth = mount.components().count();
        if path.starts_with(&mount) && depth >= best_depth {
            best = Some(PathBuf::from("/sys/dev/block").join(device));
            best_depth = depth;
        }
    }
    best
}

#[cfg(windows)]
pub(super) fn powershell() -> std::io::Result<std::process::Command> {
    let root = std::env::var_os("SystemRoot")
        .ok_or_else(|| std::io::Error::other("Windows system directory is unavailable"))?;
    let path =
        std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    if !path.is_absolute() {
        return Err(std::io::Error::other("Invalid Windows system directory"));
    }
    let mut command = std::process::Command::new(path);
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    Ok(command)
}

pub fn inspect(path: &Path) -> Evidence {
    #[cfg(target_os = "linux")]
    {
        if let Ok(mounts) =
            super::storage::read_limited(Path::new("/proc/self/mountinfo"), 4 * 1024 * 1024)
        {
            if let Some(device) = mount_device(&mounts, path) {
                if encrypted_device(&device, 0) {
                    return Evidence {
                        verified: true,
                        detail: "Storage is backed by an active dm-crypt encrypted device.".into(),
                    };
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("/usr/sbin/diskutil");
        command.arg("info").arg(path);
        let output = overtalk_worker_sandbox::run_bounded(
            command,
            64 * 1024,
            std::time::Duration::from_secs(5),
        );
        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            if output.status.success()
                && text
                    .lines()
                    .any(|l| l.trim().starts_with("FileVault:") && l.contains("Yes"))
            {
                return Evidence {
                    verified: true,
                    detail: "macOS reports FileVault for this storage volume.".into(),
                };
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let text = path.to_string_lossy();
        if text.as_bytes().get(1) == Some(&b':') && text.as_bytes()[0].is_ascii_alphabetic() {
            let drive = &text[..2];
            let script=format!("$v=Get-BitLockerVolume -MountPoint '{drive}' -ErrorAction Stop; if ($v.ProtectionStatus -eq 'On' -and $v.EncryptionPercentage -eq 100) {{ 'OVERTALK_ENCRYPTED' }}");
            if powershell()
                .and_then(|mut command| {
                    command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
                    overtalk_worker_sandbox::run_bounded(
                        command,
                        64 * 1024,
                        std::time::Duration::from_secs(5),
                    )
                })
                .is_ok_and(|o| {
                    o.status.success()
                        && String::from_utf8_lossy(&o.stdout).trim() == "OVERTALK_ENCRYPTED"
                })
            {
                return Evidence {
                    verified: true,
                    detail: "Windows reports active, fully encrypted BitLocker storage.".into(),
                };
            }
        }
    }
    Evidence{verified:false,detail:"Device encryption could not be verified for this location. Protected mode remains locked; an administrator must configure encrypted storage.".into()}
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn mount_lookup_uses_deepest_component_and_decodes_kernel_escapes() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("with space");
        std::fs::create_dir(&folder).unwrap();
        let child = folder.join("library");
        std::fs::create_dir(&child).unwrap();
        let mount = folder.to_string_lossy().replace(' ', "\\040");
        let mounts = format!("1 0 8:1 / / rw - ext4 /dev/root rw\n2 1 253:2 / {mount} rw - ext4 /dev/dm-2 rw\n3 1 8:2 / {mount}-other rw - ext4 /dev/sdb rw\n");
        assert_eq!(
            mount_device(mounts.as_bytes(), &child),
            Some(PathBuf::from("/sys/dev/block/253:2"))
        );
        assert_eq!(
            mount_device(mounts.as_bytes(), root.path()),
            Some(PathBuf::from("/sys/dev/block/8:1"))
        );
        assert!(mount_device(b"broken", &child).is_none());
        assert!(mount_device(b"1 0 ../../escape / / rw", &child).is_none());
    }

    #[test]
    #[ignore = "Manual before/after device-check microbenchmark"]
    fn benchmark_device_inspection() {
        let root = tempfile::tempdir().unwrap();
        let mut prior = Vec::new();
        let mut current = Vec::new();
        for _ in 0..50 {
            let started = std::time::Instant::now();
            let output = std::process::Command::new("/usr/bin/findmnt")
                .args(["--noheadings", "--raw", "--output", "SOURCE", "--target"])
                .arg(root.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            let source = String::from_utf8_lossy(&output.stdout);
            let old_verified =
                encrypted_device(Path::new(source.trim().split('[').next().unwrap_or("")), 0);
            prior.push(started.elapsed().as_secs_f64() * 1000.);
            let started = std::time::Instant::now();
            let new_verified = inspect(root.path()).verified;
            current.push(started.elapsed().as_secs_f64() * 1000.);
            assert_eq!(old_verified, new_verified);
        }
        prior.sort_by(f64::total_cmp);
        current.sort_by(f64::total_cmp);
        println!(
            "DEVICE_INSPECTION old_median_ms={:.3} new_median_ms={:.3} samples=50",
            prior[25], current[25]
        );
    }
}
