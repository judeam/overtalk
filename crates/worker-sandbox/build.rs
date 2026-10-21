mod build_identity;

fn main() {
    let directory = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = directory.parent().unwrap().parent().unwrap();
    for name in build_identity::INPUTS {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
    }
    println!(
        "cargo:rustc-env=OVERTALK_WORKER_SOURCE_ID={}",
        build_identity::fingerprint(root).expect("Could not identify worker sources")
    );
}
