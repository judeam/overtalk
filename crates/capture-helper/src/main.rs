//! Screen capture is isolated from the meeting/UI processes. OS permission and
//! selection remain visible. The encoder receives a bounded latest-frame stream.
use scap::{
    capturer::{Capturer, Options, Resolution},
    frame::{Frame, FrameType},
};
use std::{
    io::{BufRead, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
// The Windows and macOS packages bundle an LGPL FFmpeg (packaging/ffmpeg/),
// which cannot include libx264, so they use FFmpeg's own MPEG-4 encoder, which
// both systems' players open. Linux uses the distribution's FFmpeg.
#[cfg(any(windows, target_os = "macos"))]
const ENCODER: &[&str] = &["-c:v", "mpeg4", "-q:v", "5"];
#[cfg(not(any(windows, target_os = "macos")))]
const ENCODER: &[&str] = &["-c:v", "libx264", "-preset", "ultrafast", "-crf", "25"];
struct Pixels {
    width: i32,
    height: i32,
    format: &'static str,
    data: Vec<u8>,
}
fn pixels(frame: Frame) -> Result<Pixels, String> {
    let (width, height, format, data) = match frame {
        Frame::BGRA(f) => (f.width, f.height, "bgra", f.data),
        Frame::BGRx(f) => (f.width, f.height, "bgr0", f.data),
        Frame::RGBx(f) => (f.width, f.height, "rgb0", f.data),
        Frame::XBGR(f) => (f.width, f.height, "0bgr", f.data),
        Frame::RGB(f) => (f.width, f.height, "rgb24", f.data),
        Frame::BGR0(f) => (f.width, f.height, "bgr0", f.data),
        _ => return Err("Unsupported screen pixel format".into()),
    };
    if width <= 0
        || height <= 0
        || width > 16384
        || height > 16384
        || data.len() > 256 * 1024 * 1024
    {
        return Err("Invalid capture dimensions".into());
    }
    let row = width as usize * if format == "rgb24" { 3 } else { 4 };
    let expected = row * height as usize;
    let data = if data.len() == expected {
        data
    } else if data.len() % height as usize == 0
        && data.len() / height as usize >= row
        && data.len() / height as usize - row < 512
    {
        data.chunks_exact(data.len() / height as usize)
            .flat_map(|line| line[..row].iter().copied())
            .collect()
    } else {
        return Err("Invalid screen frame stride".into());
    };
    Ok(Pixels {
        width,
        height,
        format,
        data,
    })
}
fn run() -> Result<(), String> {
    let mut input = std::io::BufReader::new(std::io::stdin());
    let mut request = String::new();
    input
        .by_ref()
        .take(8192)
        .read_line(&mut request)
        .map_err(|e| e.to_string())?;
    let request: serde_json::Value = serde_json::from_str(&request).map_err(|e| e.to_string())?;
    if request["action"] == "targets" {
        let targets = scap::get_all_targets()
            .into_iter()
            .map(|t| match t {
                scap::Target::Window(w) => {
                    serde_json::json!({"id":format!("window:{}",w.id),"title":w.title})
                }
                scap::Target::Display(d) => {
                    serde_json::json!({"id":format!("display:{}",d.id),"title":d.title})
                }
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::json!({"targets":targets,"supported":scap::is_supported()})
        );
        return Ok(());
    }
    let path = request["path"]
        .as_str()
        .ok_or("Missing video output")?
        .to_owned();
    let ffmpeg = request["ffmpeg"]
        .as_str()
        .ok_or("Missing encoder")?
        .to_owned();
    if std::path::Path::new(&path).exists() {
        return Err("Video output already exists".into());
    }
    if !scap::is_supported() {
        return Err("Screen capture is unavailable on this desktop session".into());
    }
    if !scap::has_permission() && !scap::request_permission() {
        return Err("Screen recording permission was not granted".into());
    }
    let selected = request["target"].as_str().unwrap_or("system-picker");
    let target = if selected == "system-picker" {
        None
    } else {
        scap::get_all_targets().into_iter().find(|t| match t {
            scap::Target::Window(w) => format!("window:{}", w.id) == selected,
            scap::Target::Display(d) => format!("display:{}", d.id) == selected,
        })
    };
    if selected != "system-picker" && target.is_none() {
        return Err("The selected window is no longer available".into());
    }
    let mut capturer = Capturer::build(Options {
        fps: 15,
        show_cursor: true,
        show_highlight: true,
        target,
        output_type: FrameType::BGRAFrame,
        output_resolution: Resolution::_720p,
        ..Default::default()
    })
    .map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let control = stop.clone();
    let pause = paused.clone();
    std::thread::spawn(move || {
        for line in input.lines() {
            match line.as_deref() {
                Ok("pause") => pause.store(true, Ordering::Release),
                Ok("resume") => pause.store(false, Ordering::Release),
                _ => {
                    control.store(true, Ordering::Release);
                    break;
                }
            }
        }
        control.store(true, Ordering::Release);
    });
    capturer.start_capture();
    let first = pixels(capturer.get_next_frame().map_err(|e| e.to_string())?)?;
    let (sender, receiver) = std::sync::mpsc::sync_channel::<Pixels>(1);
    let finish = stop.clone();
    let encoder = std::thread::spawn(move || -> Result<(), String> {
        let mut sandbox =
            overtalk_worker_sandbox::Sandbox::new(false).map_err(|e| e.to_string())?;
        sandbox
            .output(std::path::Path::new(&path))
            .map_err(|e| e.to_string())?;
        let mut child = sandbox
            .command(std::path::Path::new(&ffmpeg))
            .map_err(|e| e.to_string())?
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-protocol_whitelist",
                "file,pipe",
                "-f",
                "rawvideo",
                "-pixel_format",
                first.format,
                "-video_size",
                &format!("{}x{}", first.width, first.height),
                "-framerate",
                "15",
                "-i",
                "pipe:0",
                "-an",
                "-vf",
                "scale='min(1280,iw)':-2:flags=fast_bilinear",
            ])
            .args(ENCODER)
            .args([
                "-threads",
                "2",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+frag_keyframe+empty_moov+default_base_moof",
                "-f",
                "mp4",
                &path,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let mut pipe = child.stdin.take().ok_or("Missing encoder input")?;
        let mut frame = first;
        println!("{}", serde_json::json!({"ready":true}));
        let _ = std::io::stdout().flush();
        let mut deadline = Instant::now();
        while !finish.load(Ordering::Acquire) {
            if let Ok(next) = receiver.try_recv() {
                if next.width != frame.width
                    || next.height != frame.height
                    || next.format != frame.format
                {
                    finish.store(true, Ordering::Release);
                    break;
                }
                frame = next;
            }
            if !paused.load(Ordering::Acquire) {
                pipe.write_all(&frame.data).map_err(|e| e.to_string())?;
            }
            deadline += Duration::from_nanos(1_000_000_000 / 15);
            let now = Instant::now();
            if deadline > now {
                std::thread::sleep(deadline - now);
            } else {
                deadline = now;
            }
        }
        drop(pipe);
        let result = child.wait().map_err(|e| e.to_string())?;
        if !result.success() {
            return Err("Video encoder failed; the partial file was preserved".into());
        }
        Ok(())
    });
    while !stop.load(Ordering::Acquire) {
        let frame = capturer.get_next_frame().map_err(|e| e.to_string())?;
        let frame = pixels(frame)?;
        match sender.try_send(frame) {
            Ok(()) | Err(std::sync::mpsc::TrySendError::Full(_)) => {}
            Err(_) => {
                stop.store(true, Ordering::Release);
                break;
            }
        }
    }
    capturer.stop_capture();
    stop.store(true, Ordering::Release);
    encoder
        .join()
        .map_err(|_| "Video encoder stopped unexpectedly")??;
    println!("{}", serde_json::json!({"completed":true}));
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        println!("{}", serde_json::json!({"error":error}));
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(width: i32, height: i32, data: Vec<u8>) -> Frame {
        Frame::BGRA(scap::frame::BGRAFrame {
            display_time: 0,
            width,
            height,
            data,
        })
    }
    #[test]
    fn padded_rows_are_removed_without_shifting_pixels() {
        let packed = pixels(frame(
            1,
            2,
            vec![1, 2, 3, 4, 99, 99, 99, 99, 5, 6, 7, 8, 99, 99, 99, 99],
        ))
        .unwrap();
        assert_eq!(packed.data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
    #[test]
    fn rejects_short_and_oversized_frames() {
        assert!(pixels(frame(2, 2, vec![0; 15])).is_err());
        assert!(pixels(frame(16385, 1, vec![])).is_err());
        assert!(pixels(frame(0, 2, vec![])).is_err());
    }
}
