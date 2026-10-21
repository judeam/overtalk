//! Decode recordings incrementally with the bundled FFmpeg, including fragmented MP4.
//! At most four small PCM blocks wait in memory, independently of meeting length.
use rodio::Source;
use std::{path::Path, time::Duration};

const CHANNELS: u16 = 2;
const SAMPLE_RATE: u32 = 48_000;

pub(super) struct RecordingSource {
    reader: crate::audio::decoder::IsolatedPcmReader,
    current: std::vec::IntoIter<f32>,
    ended: bool,
}
impl RecordingSource {
    pub(super) fn open(path: &Path, position: f64) -> Result<Self, String> {
        let mut reader =
            crate::audio::decoder::IsolatedPcmReader::open(path, SAMPLE_RATE, CHANNELS, position)
                .map_err(|e| e.to_string())?;
        let first = reader
            .read_samples()
            .map_err(|e| e.to_string())?
            .ok_or("No playable audio at this position. The recording may be empty or damaged.")?;
        Ok(Self {
            reader,
            current: first.into_iter(),
            ended: false,
        })
    }
}

impl Iterator for RecordingSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if let Some(sample) = self.current.next() {
            return Some(sample);
        }
        if self.ended {
            return None;
        }
        self.current = match self.reader.read_samples() {
            Ok(Some(block)) => block.into_iter(),
            result => {
                self.ended = true;
                if let Err(error) = result {
                    log::warn!("Audio playback failed: {error}");
                }
                return None;
            }
        };
        self.current.next()
    }
}

impl Source for RecordingSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        CHANNELS
    }
    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn fragmented_recordings_stream_and_seek_with_bounded_buffers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("audio.mp4");
        let ffmpeg = crate::audio::ffmpeg::find_ffmpeg_path().expect("FFmpeg test dependency");
        let status = Command::new(ffmpeg)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=mono",
                "-t",
                "2",
                "-c:a",
                "aac",
                "-movflags",
                "+frag_keyframe+empty_moov",
            ])
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        let source = RecordingSource::open(&path, 0.0).unwrap();
        assert_eq!(source.channels(), 2);
        assert_eq!(source.sample_rate(), 48_000);
        let count = source.count();
        assert!(
            (190_000..200_000).contains(&count),
            "decoded {count} samples"
        );
        let count = RecordingSource::open(&path, 1.0).unwrap().count();
        assert!(
            (94_000..104_000).contains(&count),
            "seek decoded {count} samples"
        );
        std::fs::write(&path, b"damaged recording").unwrap();
        assert!(RecordingSource::open(&path, 0.0).is_err());
    }
}
