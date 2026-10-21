//! Optional playback-activity monitoring. The worker owns its lifetime; stopping
//! or dropping a monitor releases the thread instead of leaving OS listeners alive.
use std::sync::Arc;
#[cfg(any(target_os = "macos", test))]
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum SystemAudioEvent {
    SystemAudioStarted(Vec<String>),
    SystemAudioStopped,
}

pub type SystemAudioCallback = Arc<dyn Fn(SystemAudioEvent) + Send + Sync + 'static>;

pub fn new_system_audio_callback(
    callback: impl Fn(SystemAudioEvent) + Send + Sync + 'static,
) -> SystemAudioCallback {
    Arc::new(callback)
}

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct ActivityGate {
    active: bool,
    candidate: Option<(bool, Instant)>,
}

#[cfg(any(target_os = "macos", test))]
impl ActivityGate {
    fn observe(&mut self, sample: Option<bool>, now: Instant) -> Option<bool> {
        let Some(sample) = sample else {
            self.candidate = None;
            return None;
        };
        if sample == self.active {
            self.candidate = None;
            return None;
        }
        match self.candidate {
            Some((candidate, since)) if candidate == sample => {
                if now.saturating_duration_since(since) < Duration::from_millis(500) {
                    return None;
                }
                self.active = sample;
                self.candidate = None;
                Some(sample)
            }
            _ => {
                self.candidate = Some((sample, now));
                None
            }
        }
    }
}

#[cfg(target_os = "macos")]
struct Worker {
    cancel: std::sync::mpsc::SyncSender<()>,
    thread: std::thread::JoinHandle<()>,
}

#[derive(Default)]
pub struct SystemAudioDetector {
    #[cfg(target_os = "macos")]
    worker: Option<Worker>,
}

impl SystemAudioDetector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, callback: SystemAudioCallback) {
        #[cfg(target_os = "macos")]
        {
            if self.worker.is_some() {
                return;
            }
            let (cancel, stopped) = std::sync::mpsc::sync_channel(1);
            let thread = std::thread::Builder::new()
                .name("overtalk-playback-activity".into())
                .spawn(move || {
                    let mut gate = ActivityGate::default();
                    loop {
                        if !matches!(
                            stopped.try_recv(),
                            Err(std::sync::mpsc::TryRecvError::Empty)
                        ) {
                            break;
                        }
                        if let Some(active) = gate.observe(macos::output_active(), Instant::now()) {
                            callback(if active {
                                SystemAudioEvent::SystemAudioStarted(macos::playing_applications())
                            } else {
                                SystemAudioEvent::SystemAudioStopped
                            });
                        }
                        if !matches!(
                            stopped.recv_timeout(Duration::from_millis(250)),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                        ) {
                            break;
                        }
                    }
                });
            match thread {
                Ok(thread) => self.worker = Some(Worker { cancel, thread }),
                Err(_) => log::warn!("Could not start playback activity monitor"),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = callback;
            log::warn!("Playback activity monitoring is unavailable on this platform");
        }
    }

    pub fn stop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(worker) = self.worker.take() {
            let _ = worker.cancel.try_send(());
            if worker.thread.thread().id() != std::thread::current().id() {
                let _ = worker.thread.join();
            }
        }
    }
}

impl Drop for SystemAudioDetector {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use cidre::core_audio as ca;

    pub(super) fn output_active() -> Option<bool> {
        let address = ca::PropAddr {
            selector: ca::PropSelector::DEVICE_IS_RUNNING_SOMEWHERE,
            scope: ca::PropScope::GLOBAL,
            element: ca::PropElement::MAIN,
        };
        ca::System::default_output_device()
            .ok()?
            .prop::<u32>(&address)
            .ok()
            .map(|running| running != 0)
    }

    pub(super) fn playing_applications() -> Vec<String> {
        let Ok(processes) = ca::System::processes() else {
            return Vec::new();
        };
        let mut names = Vec::new();
        for process in processes {
            if !process.is_running_output().unwrap_or(false) {
                continue;
            }
            let Ok(pid) = process.pid() else { continue };
            let name = cidre::ns::RunningApp::with_pid(pid)
                .and_then(|app| app.localized_name().map(|name| name.to_string()))
                .unwrap_or_else(|| format!("Process {pid}"));
            names.push(name);
        }
        names.sort_unstable();
        names.dedup();
        names
    }
}

#[cfg(test)]
mod tests {
    use super::ActivityGate;
    use std::time::{Duration, Instant};

    #[test]
    fn activity_requires_two_observations_separated_by_the_settle_interval() {
        let mut gate = ActivityGate::default();
        let start = Instant::now();
        assert_eq!(gate.observe(Some(false), start), None);
        assert_eq!(gate.observe(Some(true), start), None);
        assert_eq!(
            gate.observe(Some(true), start + Duration::from_millis(499)),
            None
        );
        assert_eq!(
            gate.observe(Some(true), start + Duration::from_millis(500)),
            Some(true)
        );
        assert_eq!(
            gate.observe(Some(true), start + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            gate.observe(Some(false), start + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            gate.observe(Some(false), start + Duration::from_millis(2500)),
            Some(false)
        );
    }

    #[test]
    fn a_failed_read_or_a_reverted_signal_restarts_confirmation() {
        let mut gate = ActivityGate::default();
        let start = Instant::now();
        gate.observe(Some(true), start);
        gate.observe(None, start + Duration::from_secs(1));
        assert_eq!(
            gate.observe(Some(true), start + Duration::from_secs(2)),
            None
        );
        gate.observe(Some(false), start + Duration::from_secs(3));
        assert_eq!(
            gate.observe(Some(true), start + Duration::from_secs(4)),
            None
        );
        assert_eq!(
            gate.observe(Some(true), start + Duration::from_millis(4500)),
            Some(true)
        );
    }
}
