use std::{
    io::{Read, Write},
    path::Path,
};

/// Publish only a complete file. The temporary file is opened exclusively in
/// the same directory, so a planted link is never opened for writing. Dropping
/// a failed write removes the temporary file and preserves the old destination.
pub fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut std::io::BufWriter<&mut std::fs::File>) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("File has no parent directory"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    {
        let mut output = std::io::BufWriter::with_capacity(64 * 1024, temporary.as_file_mut());
        write(&mut output)?;
        output.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> std::io::Result<()> {
    write_atomic(path, |output| {
        serde_json::to_writer_pretty(output, value).map_err(std::io::Error::other)
    })
}

/// Bound allocation before parsing. Reject special files before any read can
/// block on them; inspect and read the same open handle to avoid path races.
pub fn read_limited(path: &Path, limit: usize) -> std::io::Result<Vec<u8>> {
    let file = overtalk_worker_sandbox::open_regular(path, false, false)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(std::io::Error::other("File exceeds its size limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(std::io::Error::other("File exceeds its size limit"));
    }
    Ok(bytes)
}

/// Lowercase hexadecimal, the form that saved digests, pins and derived IDs use.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut text, byte| {
            let _ = write!(text, "{byte:02x}");
            text
        })
}

/// Hash a regular file with fixed memory, rejecting growth, truncation and
/// special files. Shared by verification of saved models and resumed downloads.
pub fn hash_regular_file(path: &Path, limit: u64) -> std::io::Result<(sha2::Sha256, u64)> {
    use sha2::Digest;
    let file = overtalk_worker_sandbox::open_regular(path, false, false)?;
    let size = file.metadata()?.len();
    if size > limit {
        return Err(std::io::Error::other("File exceeds its size limit"));
    }
    let mut input = file.take(size.saturating_add(1));
    let mut digest = sha2::Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut count = 0;
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > size {
            break;
        }
        digest.update(&buffer[..n]);
    }
    if count != size {
        return Err(std::io::Error::other(
            "File size changed during verification",
        ));
    }
    Ok((digest, size))
}

pub fn private_directory(path: &Path) -> std::io::Result<()> {
    if path.parent().is_none() {
        return Err(std::io::Error::other(
            "Choose a private application subfolder",
        ));
    }
    std::fs::create_dir_all(path)?;
    if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(std::io::Error::other("Private storage cannot be a symlink"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        let metadata = directory.metadata()?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o1000 != 0 {
            return Err(std::io::Error::other(
                "Choose a folder owned exclusively by your user",
            ));
        }
        directory.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    overtalk_worker_sandbox::restrict_to_current_user(path, true)?;
    Ok(())
}

pub fn private_file(path: &Path) -> std::io::Result<()> {
    let file = match overtalk_worker_sandbox::open_regular(path, false, false) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = file.metadata()?;
        // SAFETY: geteuid has no preconditions and cannot fail.
        if metadata.uid() != unsafe { libc::geteuid() } || metadata.nlink() != 1 {
            return Err(std::io::Error::other(
                "Private files must belong exclusively to your user",
            ));
        }
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(windows)]
    {
        drop(file);
        overtalk_worker_sandbox::restrict_to_current_user(path, false)?;
    }
    Ok(())
}

/// Applied before any worker/file creation, inherited by helper processes.
pub fn restrict_new_files() {
    #[cfg(unix)]
    // SAFETY: umask takes no pointers and cannot fail; it only sets this
    // process's file-creation mask, which every later file and child inherits.
    unsafe {
        libc::umask(0o077);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::{fs::symlink, fs::PermissionsExt};
    #[test]
    fn atomic_writes_preserve_failed_destinations_and_never_follow_links() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let destination = directory.path().join("output");
        std::fs::write(&target, b"unchanged").unwrap();
        symlink(&target, &destination).unwrap();
        assert!(write_atomic(&destination, |out| {
            out.write_all(b"partial")?;
            Err(std::io::Error::other("synthetic failure"))
        })
        .is_err());
        assert!(std::fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink());
        write_json_atomic(&destination, &serde_json::json!({"complete":true})).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"unchanged");
        assert!(!std::fs::symlink_metadata(&destination)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }
    #[test]
    fn private_storage_restricts_permissions_and_rejects_links() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("library");
        private_directory(&path).unwrap();
        let file = path.join("data");
        std::fs::write(&file, b"synthetic").unwrap();
        private_file(&file).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let link = temp.path().join("linked");
        symlink(&file, &link).unwrap();
        assert!(private_file(&link).is_err());
        assert!(private_directory(Path::new("/")).is_err());
    }

    #[test]
    fn bounded_reads_reject_oversized_linked_and_special_files() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("settings");
        std::fs::write(&file, b"12345").unwrap();
        assert_eq!(read_limited(&file, 5).unwrap(), b"12345");
        assert!(read_limited(&file, 4).is_err());
        let link = temp.path().join("link");
        symlink(&file, &link).unwrap();
        assert!(read_limited(&link, 5).is_err());
        let missing_link = temp.path().join("missing-link");
        symlink(temp.path().join("missing"), &missing_link).unwrap();
        assert!(private_file(&missing_link).is_err());
        let hard_link = temp.path().join("hard-link");
        std::fs::hard_link(&file, &hard_link).unwrap();
        assert!(private_file(&hard_link).is_err());
        let fifo = temp.path().join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a NUL-terminated C string that outlives the call; mkfifo only reads it.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(read_limited(&fifo, 5).is_err());
        assert!(private_file(&fifo).is_err());
        assert!(private_file(temp.path()).is_err());
        assert!(hash_regular_file(&file, 4).is_err());
        assert!(hash_regular_file(&link, 5).is_err());
        assert!(hash_regular_file(&fifo, 5).is_err());
        let (digest, size) = hash_regular_file(&file, 5).unwrap();
        assert_eq!(size, 5);
        // Saved pins compare against this exact text; "01" checks zero padding.
        assert_eq!(
            hex(&sha2::Digest::finalize(digest)),
            "5994471abb01112afcc18159f6cc74b4f511b99806da59b3caf5a9c173cacfc5"
        );
    }
}
