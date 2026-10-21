use sha2::{Digest, Sha256};
use std::{io, path::Path};

pub(crate) const INPUTS: &[&str] = &[
    "Cargo.lock",
    "crates/worker-sandbox/Cargo.toml",
    "crates/worker-sandbox/build.rs",
    "crates/worker-sandbox/build_identity.rs",
    "crates/worker-sandbox/src/lib.rs",
    "crates/worker-sandbox/src/main.rs",
    "crates/worker-sandbox/src/windows.rs",
];

pub(crate) fn fingerprint(root: &Path) -> io::Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"overtalk-worker-source-v1\0");
    for name in INPUTS {
        hash.update(name.as_bytes());
        hash.update(b"\0");
        hash.update(std::fs::read(root.join(name))?);
        hash.update(b"\0");
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
