//! Streaming native playback. The audio device and decoder live off the UI thread.
use once_cell::sync::Lazy;
use rodio::{OutputStream, OutputStreamHandle, Sink};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::mpsc};
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;
mod stream;

struct Player {
    output: Option<(OutputStream, OutputStreamHandle)>,
    sink: Option<Sink>,
    path: Option<PathBuf>,
    offset: f64,
    speed: f32,
}

impl Player {
    fn restart(&mut self, path: PathBuf, position: f64, playing: bool) -> Result<(), String> {
        let source = stream::RecordingSource::open(&path, position)?;
        if self.output.is_none() {
            self.output = Some(OutputStream::try_default().map_err(|e| e.to_string())?);
        }
        let next = Sink::try_new(&self.output.as_ref().ok_or("No audio device")?.1)
            .map_err(|e| e.to_string())?;
        next.pause();
        next.set_speed(self.speed);
        next.append(source);
        if let Some(previous) = self.sink.take() {
            previous.stop();
        }
        if playing {
            next.play();
        }
        self.sink = Some(next);
        self.path = Some(path);
        self.offset = position;
        Ok(())
    }

    fn command(
        &mut self,
        action: &str,
        path: Option<PathBuf>,
        value: f64,
    ) -> Result<Value, String> {
        match action {
            "load" => self.restart(path.ok_or("Missing recording")?, value, true)?,
            "play" => {
                // Pausing already retains the decoder and its queued samples.
                // Allow one UI polling interval of drift; explicit seeks still
                // go through the seek command and always honor their position.
                let resume = self.sink.as_ref().is_some_and(|sink| {
                    !sink.empty()
                        && (self.offset + sink.get_pos().as_secs_f64() - value).abs() <= 0.1
                });
                if resume {
                    self.sink.as_ref().unwrap().play();
                } else {
                    self.restart(self.path.clone().ok_or("No recording loaded")?, value, true)?;
                }
            }
            "pause" => {
                if let Some(sink) = &self.sink {
                    sink.pause();
                }
            }
            "stop" => {
                if let Some(sink) = self.sink.take() {
                    sink.stop();
                }
                self.output = None;
                self.path = None;
                self.offset = 0.0;
            }
            "forget" => {
                if path
                    .as_ref()
                    .is_some_and(|folder| self.path.as_ref().is_some_and(|p| p.starts_with(folder)))
                {
                    if let Some(sink) = self.sink.take() {
                        sink.stop();
                    }
                    self.output = None;
                    self.path = None;
                    self.offset = 0.0;
                }
            }
            "seek" => {
                if let Some(path) = self.path.clone() {
                    let playing = self.sink.as_ref().is_some_and(|sink| !sink.is_paused());
                    self.restart(path, value, playing)?;
                }
            }
            "speed" => {
                if !(0.5..=3.0).contains(&value) {
                    return Err("Invalid playback speed".into());
                }
                self.speed = value as f32;
                if let Some(sink) = &self.sink {
                    sink.set_speed(self.speed);
                }
            }
            "status" => {}
            _ => return Err("Unsupported playback operation".into()),
        }
        Ok(json!({
            "position": self.offset + self.sink.as_ref().map(|sink| sink.get_pos().as_secs_f64()).unwrap_or(0.0),
            "playing": self.sink.as_ref().is_some_and(|sink| !sink.is_paused() && !sink.empty()),
        }))
    }
}

struct Command {
    action: String,
    path: Option<PathBuf>,
    value: f64,
    response: oneshot::Sender<Result<Value, String>>,
}
static PLAYER: Lazy<mpsc::SyncSender<Command>> = Lazy::new(|| {
    let (sender, receiver) = mpsc::sync_channel::<Command>(32);
    std::thread::spawn(move || {
        let mut player = Player {
            output: None,
            sink: None,
            path: None,
            offset: 0.0,
            speed: 1.0,
        };
        for command in receiver {
            let result = player.command(&command.action, command.path, command.value);
            let _ = command.response.send(result);
        }
    });
    sender
});

pub(crate) async fn forget(folder: PathBuf) -> Result<(), String> {
    if Lazy::get(&PLAYER).is_none() {
        return Ok(());
    }
    let (response, result) = oneshot::channel();
    PLAYER
        .try_send(Command {
            action: "forget".into(),
            path: Some(folder),
            value: 0.0,
            response,
        })
        .map_err(|_| "Playback is busy; stop playback before deleting this meeting")?;
    result.await.map_err(|_| "Playback worker stopped")??;
    Ok(())
}

pub(crate) async fn control(app: &AppHandle, args: Value) -> Result<Value, String> {
    let action = args["action"]
        .as_str()
        .ok_or("Missing playback operation")?
        .to_owned();
    if !matches!(
        action.as_str(),
        "load" | "play" | "pause" | "stop" | "seek" | "speed" | "status"
    ) {
        return Err("Unsupported playback operation".into());
    }
    let value = args["value"].as_f64().unwrap_or(0.0);
    if !value.is_finite() || value < 0.0 {
        return Err("Invalid playback position".into());
    }
    let path = if action == "load" {
        let id = args["meetingId"].as_str().ok_or("Missing meeting")?;
        let state = app.state::<crate::state::AppState>();
        let folder: Option<Option<String>> =
            sqlx::query_scalar("SELECT folder_path FROM meetings WHERE id=?")
                .bind(id)
                .fetch_optional(state.db_manager.pool())
                .await
                .map_err(|e| e.to_string())?;
        let folder = PathBuf::from(
            folder
                .flatten()
                .ok_or("This meeting has no saved recording")?,
        )
        .canonicalize()
        .map_err(|e| e.to_string())?;
        let recording = crate::audio::retranscription::find_audio_file(&folder)
            .map_err(|e| e.to_string())?
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !recording.starts_with(&folder) || !recording.is_file() {
            return Err("Recording must belong to this meeting".into());
        }
        Some(recording)
    } else {
        None
    };
    let (response, result) = oneshot::channel();
    PLAYER
        .try_send(Command {
            action,
            path,
            value,
            response,
        })
        .map_err(|_| "Playback is busy")?;
    result
        .await
        .map_err(|_| "Audio device disconnected".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_reuses_paused_audio_but_new_positions_still_seek() {
        let (sink, _output) = Sink::new_idle();
        sink.pause();
        sink.append(rodio::source::SineWave::new(440.));
        let mut player = Player {
            output: None,
            sink: Some(sink),
            // A resume must not open the file or an audio device again.
            path: Some(PathBuf::from("/missing-synthetic-recording.wav")),
            offset: 12.0,
            speed: 1.0,
        };
        let resumed = player.command("play", None, 12.0).unwrap();
        assert_eq!(resumed["playing"], true);
        assert_eq!(resumed["position"], 12.0);
        assert!(player.output.is_none());
        assert!(player.command("play", None, 15.0).is_err());
        assert!(player.command("seek", None, 12.01).is_err());
        player.command("stop", None, 0.0).unwrap();
        assert!(player.command("play", None, 0.0).is_err());
    }
}
