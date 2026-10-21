//! Bound decoder output independently of its progress or exit status.
//! Backpressure stops decoding while paused, without a second forwarding thread.
use std::{
    io::{self, Read},
    process::ChildStdout,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::Duration,
};

pub(super) struct PcmPipe(Receiver<io::Result<Vec<f32>>>);

impl PcmPipe {
    pub(super) fn new(stdout: ChildStdout) -> Self {
        let (sender, receiver) = mpsc::sync_channel(4);
        std::thread::spawn(move || {
            let mut input = io::BufReader::new(stdout);
            loop {
                match block(&mut input) {
                    Ok(Some(samples)) => {
                        if sender.send(Ok(samples)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        Self(receiver)
    }

    pub(super) fn read(&self, timeout: Duration) -> io::Result<Option<Vec<f32>>> {
        match self.0.recv_timeout(timeout) {
            Ok(result) => result.map(Some),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Audio decoder stopped producing samples",
            )),
        }
    }
}

fn block(input: &mut impl Read) -> io::Result<Option<Vec<f32>>> {
    let mut bytes = [0_u8; 16_384];
    let mut filled = 0;
    while filled < bytes.len() {
        match input.read(&mut bytes[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    if filled == 0 {
        return Ok(None);
    }
    if filled % 4 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Audio decoder returned an incomplete sample",
        ));
    }
    let mut samples = Vec::with_capacity(filled / 4);
    for word in bytes[..filled].chunks_exact(4) {
        let sample = f32::from_le_bytes(word.try_into().unwrap());
        if !sample.is_finite() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Audio contains invalid samples",
            ));
        }
        samples.push(sample.clamp(-1., 1.));
    }
    Ok(Some(samples))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pcm_blocks_keep_sample_boundaries_and_reject_invalid_data() {
        assert_eq!(block(&mut &[][..]).unwrap(), None);
        assert!(block(&mut &[0, 0, 0][..]).is_err());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(block(&mut &value.to_le_bytes()[..]).is_err());
        }
        let bytes: Vec<u8> = [-2_f32, -0.5, 0., 0.5, 2.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        assert_eq!(
            block(&mut &bytes[..]).unwrap().unwrap(),
            [-1., -0.5, 0., 0.5, 1.]
        );
    }
}
