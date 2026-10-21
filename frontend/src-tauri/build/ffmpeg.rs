//! Bundle only an explicitly trusted local FFmpeg. Build scripts never fetch or
//! unpack an unauthenticated executable from the network.
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

const LIMIT: u64 = 512 * 1024 * 1024;
fn digest(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("FFmpeg must be a regular file".into());
    }
    let mut hash = Sha256::new();
    let mut bytes = [0; 64 * 1024];
    let mut count = 0;
    loop {
        let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        count += n as u64;
        if count > LIMIT {
            return Err("FFmpeg exceeds the bundle size limit".into());
        }
        hash.update(&bytes[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn ensure_ffmpeg_binary() {
    for name in ["OVERTALK_FFMPEG_PATH", "OVERTALK_FFMPEG_SHA256"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    let target = std::env::var("TARGET").expect("Cargo target missing");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let name = format!(
        "ffmpeg-{target}{}",
        if target.contains("windows") {
            ".exe"
        } else {
            ""
        }
    );
    let destination = Path::new(&manifest).join("binaries").join(name);
    let source = std::env::var_os("OVERTALK_FFMPEG_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| destination.clone());
    let expected = std::env::var("OVERTALK_FFMPEG_SHA256")
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(expected.len() == 64 && expected.bytes().all(|c| c.is_ascii_hexdigit()),
        "Set OVERTALK_FFMPEG_PATH and OVERTALK_FFMPEG_SHA256 from a verified package/release. Unverified build-time downloads are disabled; see docs/BUILDING.md.");
    println!("cargo:rerun-if-changed={}", source.display());
    assert_eq!(
        digest(&source).expect("Cannot verify FFmpeg"),
        expected,
        "FFmpeg checksum mismatch; refusing to execute or bundle it"
    );
    if source != destination && digest(&destination).ok().as_ref() != Some(&expected) {
        fs::create_dir_all(destination.parent().unwrap()).expect("Cannot create bundle directory");
        let temporary = destination.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| -> Result<(), String> {
            let mut input = fs::File::open(&source).map_err(|e| e.to_string())?;
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut input.by_ref().take(LIMIT + 1), &mut output)
                .map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
            if digest(&temporary)? != expected {
                return Err("FFmpeg changed while being copied".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))
                    .map_err(|e| e.to_string())?;
            }
            if destination.exists() {
                fs::remove_file(&destination).map_err(|e| e.to_string())?;
            }
            fs::rename(&temporary, &destination).map_err(|e| e.to_string())
        })();
        let _ = fs::remove_file(&temporary);
        result.expect("Cannot stage verified FFmpeg");
    }
}
