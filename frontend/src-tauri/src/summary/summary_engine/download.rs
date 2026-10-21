//! Validate remote metadata separately from the byte budget enforced on writes.
use anyhow::{anyhow, bail, Result};
use reqwest::{header, Response, StatusCode};

pub(super) fn open_destination(
    path: &std::path::Path,
    resume: bool,
    existing: u64,
) -> Result<std::fs::File> {
    use std::io::{Seek, SeekFrom};
    let mut file = if resume {
        overtalk_worker_sandbox::open_regular(path, true, false)?
    } else {
        // Unlink a partial file without following it, then create exclusively.
        // A planted link appearing between those operations makes creation fail.
        match std::fs::remove_file(path) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        let mut options = std::fs::File::options();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path)?
    };
    let metadata = file.metadata()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 || metadata.uid() != unsafe { libc::geteuid() } {
            bail!("Model download must be a private regular file");
        }
    }
    if resume && metadata.len() != existing {
        bail!("Partial download changed; retry");
    }
    file.seek(SeekFrom::End(0))?;
    Ok(file)
}

pub(super) fn plan(response: &Response, existing: u64, limit: u64) -> Result<(u64, bool)> {
    validate_plan(response.status(), response.headers(), existing, limit)
}

fn validate_plan(
    status: StatusCode,
    headers: &header::HeaderMap,
    existing: u64,
    limit: u64,
) -> Result<(u64, bool)> {
    let length = headers
        .get(header::CONTENT_LENGTH)
        .map(|h| {
            h.to_str()
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| anyhow!("Invalid download length"))
        })
        .transpose()?;
    if status == StatusCode::PARTIAL_CONTENT {
        let range = headers
            .get(header::CONTENT_RANGE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("bytes "))
            .and_then(|s| s.split_once('/'))
            .ok_or_else(|| anyhow!("Missing download resume range"))?;
        let (start, end) = range
            .0
            .split_once('-')
            .ok_or_else(|| anyhow!("Invalid download range"))?;
        let (start, end, total) = (
            start.parse::<u64>()?,
            end.parse::<u64>()?,
            range.1.parse::<u64>()?,
        );
        if start != existing
            || end < start
            || end.checked_add(1) != Some(total)
            || total > limit
            || length.is_some_and(|n| n != total - start)
        {
            bail!("Download resume range does not match the requested file");
        }
        Ok((total, true))
    } else if status == StatusCode::OK {
        if length.is_some_and(|n| n > limit) {
            bail!("Download exceeds the model size limit");
        }
        Ok((length.unwrap_or(0), false))
    } else {
        bail!("Download failed with status: {status}")
    }
}

pub(super) fn next_size(current: u64, count: usize, total: u64, limit: u64) -> Result<u64> {
    let next = current
        .checked_add(count as u64)
        .ok_or_else(|| anyhow!("Download size overflow"))?;
    if next > limit || (total != 0 && next > total) {
        bail!("Download exceeds its declared or model size limit");
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn resume_preserves_prefix_and_rejects_changed_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model");
        open_destination(&path, false, 0)
            .unwrap()
            .write_all(b"GGUF")
            .unwrap();
        assert!(open_destination(&path, true, 3).is_err());
        open_destination(&path, true, 4)
            .unwrap()
            .write_all(b"tail")
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"GGUFtail");
    }

    #[cfg(unix)]
    #[test]
    fn download_never_truncates_or_appends_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("unrelated");
        let path = dir.path().join("download");
        std::fs::write(&target, b"keep").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(open_destination(&path, true, 4).is_err());
        open_destination(&path, false, 0)
            .unwrap()
            .write_all(b"model")
            .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        std::fs::remove_file(&path).unwrap();
        std::fs::hard_link(&target, &path).unwrap();
        assert!(open_destination(&path, true, 4).is_err());
    }

    #[test]
    #[ignore = "manual disk/CPU performance audit"]
    fn benchmark_download_verification() {
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let data = vec![37u8; 64 * 1024];
        let mut expected = Sha256::new();
        for _ in 0..2048 {
            expected.update(&data);
        }
        let expected = expected.finalize();
        for round in 0..10 {
            let inline = round % 2 != 0;
            let path = dir.path().join("synthetic-model");
            let mut hash = Sha256::new();
            let started = std::time::Instant::now();
            let mut output = std::io::BufWriter::with_capacity(
                8 * 1024 * 1024,
                open_destination(&path, false, 0).unwrap(),
            );
            for _ in 0..2048 {
                output.write_all(&data).unwrap();
                if inline {
                    hash.update(&data);
                }
            }
            output.flush().unwrap();
            output.get_ref().sync_all().unwrap();
            drop(output);
            if !inline {
                (hash, _) =
                    crate::security::storage::hash_regular_file(&path, 128 * 1024 * 1024).unwrap();
            }
            assert_eq!(hash.finalize(), expected);
            eprintln!(
                "download-benchmark inline={inline} ms={:.3} reread_bytes={}",
                started.elapsed().as_secs_f64() * 1000.,
                if inline { 0 } else { 128 * 1024 * 1024 }
            );
        }
    }
    #[test]
    fn resumes_require_exact_ranges_and_bounded_totals() {
        for (range, length, ok) in [
            ("bytes 5-9/10", "5", true),
            ("bytes 0-9/10", "10", false),
            ("bytes 5-8/10", "4", false),
            ("bytes 5-9/10", "4", false),
            ("bytes 5-20/21", "16", false),
            ("bytes 5-9/*", "5", false),
            ("bytes 5-18446744073709551615/0", "1", false),
        ] {
            let mut h = header::HeaderMap::new();
            h.insert(header::CONTENT_RANGE, range.parse().unwrap());
            h.insert(header::CONTENT_LENGTH, length.parse().unwrap());
            assert_eq!(
                validate_plan(StatusCode::PARTIAL_CONTENT, &h, 5, 20).is_ok(),
                ok,
                "{range}"
            );
        }
        let empty = header::HeaderMap::new();
        assert!(validate_plan(StatusCode::PARTIAL_CONTENT, &empty, 5, 20).is_err());
        assert!(validate_plan(StatusCode::NO_CONTENT, &empty, 0, 20).is_err());
        assert_eq!(
            validate_plan(StatusCode::OK, &empty, 5, 20).unwrap(),
            (0, false)
        );
    }
    #[test]
    fn chunked_downloads_and_overflow_cannot_bypass_write_budget() {
        assert_eq!(next_size(5, 5, 0, 10).unwrap(), 10);
        assert!(next_size(5, 6, 0, 10).is_err());
        assert!(next_size(5, 5, 9, 20).is_err());
        assert!(next_size(u64::MAX, 1, 0, u64::MAX).is_err());
    }
}
