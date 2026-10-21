#[path = "build/ffmpeg.rs"]
mod ffmpeg;
#[path = "build/onnxruntime.rs"]
mod onnxruntime;

fn main() {
    // Tauri copies externalBin seeds into Cargo's output directory. Our worker
    // is a workspace target: copying a bundled seed can silently overwrite the
    // binary Cargo just compiled. The CLI still bundles the fresh seed staged
    // by beforeBuildCommand; this override applies only inside this build script.
    let mut overrides: serde_json::Value = std::env::var("TAURI_CONFIG")
        .ok()
        .map(|value| serde_json::from_str(&value).expect("Invalid TAURI_CONFIG"))
        .unwrap_or_else(|| serde_json::json!({}));
    let config: serde_json::Value =
        serde_json::from_str(include_str!("tauri.conf.json")).expect("Invalid Tauri configuration");
    let mut binaries = overrides["bundle"]["externalBin"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| config["bundle"]["externalBin"].as_array().unwrap().clone());
    binaries.retain(|entry| entry.as_str() != Some("binaries/overtalk-worker"));
    overrides["bundle"]["externalBin"] = serde_json::Value::Array(binaries);
    std::env::set_var("TAURI_CONFIG", overrides.to_string());

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=Cocoa");
        println!("cargo:rustc-link-lib=framework=Foundation");

        // Let the enhanced_macos crate handle its own Swift compilation
        // The swift-rs crate build will be handled in the enhanced_macos crate's build.rs
    }

    // Bundle an authenticated local FFmpeg; never fetch an unverified executable.
    ffmpeg::ensure_ffmpeg_binary();
    onnxruntime::ensure_onnxruntime_runtime();

    tauri_build::build()
}
