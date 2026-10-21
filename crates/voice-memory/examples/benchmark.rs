// SPDX-License-Identifier: Apache-2.0
use overtalk_voice_memory::Encoder;
use std::{path::Path, time::Instant};
fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let start = Instant::now();
    let mut encoder = Encoder::open(Path::new(args.first().ok_or("model path required")?))?;
    eprintln!(
        "CPU model load: {:.3} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    for path in &args[1..] {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let audio: Vec<_> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let start = Instant::now();
        let vector = encoder.encode(&audio)?;
        println!(
            "{{\"path\":{:?},\"milliseconds\":{:.3},\"embedding\":{:?}}}",
            path,
            start.elapsed().as_secs_f64() * 1000.,
            vector
        );
    }
    Ok(())
}
