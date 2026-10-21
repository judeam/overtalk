// A release build is a windowed program: Windows opens no console beside it.
#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    overtalk_native::run()
}
