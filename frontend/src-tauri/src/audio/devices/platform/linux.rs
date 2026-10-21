//! Desktop audio discovery and capture through PulseAudio (including PipeWire-Pulse).
//! Enumeration never changes profiles, ports, volume, defaults, or server policy.
use crate::audio::{
    devices::{AudioDevice, DeviceType},
    pipeline::AudioCapture,
};
use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use libpulse_binding as pulse;
use pulse::{callbacks::ListResult, context::Context, mainloop::standard::Mainloop};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Route {
    card: String,
    port: String,
    input: bool,
}
#[derive(Clone, Debug)]
struct Port {
    name: String,
    label: String,
    input: bool,
}
#[derive(Clone, Debug)]
struct Card {
    index: u32,
    name: String,
    label: String,
    ports: Vec<Port>,
}
#[derive(Clone, Debug)]
struct Endpoint {
    card: Option<u32>,
    name: String,
    label: String,
    source: String,
    ports: Vec<String>,
    active_port: Option<String>,
    input: bool,
}
#[derive(Default)]
struct Snapshot {
    cards: Vec<Card>,
    endpoints: Vec<Endpoint>,
    default_input: String,
    default_output: String,
}

struct Connection {
    context: Context,
    mainloop: Mainloop,
}
impl Connection {
    fn open() -> Result<Self> {
        let mut mainloop = Mainloop::new()
            .ok_or_else(|| anyhow!("Could not open the desktop audio event loop"))?;
        let mut context = Context::new(&mainloop, "Overtalk")
            .ok_or_else(|| anyhow!("Could not create the desktop audio connection"))?;
        context
            .connect(None, pulse::context::FlagSet::NOAUTOSPAWN, None)
            .map_err(|e| anyhow!("Could not connect to the desktop audio server: {e}"))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            mainloop.iterate(false);
            match context.get_state() {
                pulse::context::State::Ready => return Ok(Self { context, mainloop }),
                pulse::context::State::Failed | pulse::context::State::Terminated => {
                    bail!("Desktop audio server unavailable")
                }
                _ if Instant::now() >= deadline => {
                    bail!("Desktop audio server did not respond within 3 seconds")
                }
                _ => thread::sleep(Duration::from_millis(3)),
            }
        }
    }
    fn snapshot(&mut self) -> Result<Snapshot> {
        let result = Rc::new(RefCell::new(Snapshot::default()));
        let r = result.clone();
        let cards = self.context.introspect().get_card_info_list(move |item| {
            if let ListResult::Item(c) = item {
                let Some(name) = c.name.as_deref() else {
                    return;
                };
                let ports = c
                    .ports
                    .iter()
                    .filter(|p| p.available != pulse::def::PortAvailable::No)
                    .filter_map(|p| {
                        Some(Port {
                            name: p.name.as_deref()?.into(),
                            label: p.description.as_deref()?.into(),
                            input: p.direction.contains(pulse::direction::FlagSet::INPUT),
                        })
                    })
                    .collect();
                r.borrow_mut().cards.push(Card {
                    index: c.index,
                    name: name.into(),
                    label: c
                        .proplist
                        .get_str("device.description")
                        .unwrap_or_else(|| name.into()),
                    ports,
                });
            }
        });
        let r = result.clone();
        let sinks = self.context.introspect().get_sink_info_list(move |item| {
            if let ListResult::Item(s) = item {
                if let (Some(name), Some(source)) =
                    (s.name.as_deref(), s.monitor_source_name.as_deref())
                {
                    r.borrow_mut().endpoints.push(Endpoint {
                        card: s.card,
                        name: name.into(),
                        label: s.description.as_deref().unwrap_or(name).into(),
                        source: source.into(),
                        ports: s
                            .ports
                            .iter()
                            .filter_map(|p| p.name.as_deref().map(String::from))
                            .collect(),
                        active_port: s
                            .active_port
                            .as_ref()
                            .and_then(|p| p.name.as_deref())
                            .map(String::from),
                        input: false,
                    });
                }
            }
        });
        let r = result.clone();
        let sources = self.context.introspect().get_source_info_list(move |item| {
            if let ListResult::Item(s) = item {
                // Monitor sources belong exclusively in the system-audio menu.
                if s.monitor_of_sink.is_some() {
                    return;
                }
                if let Some(name) = s.name.as_deref() {
                    r.borrow_mut().endpoints.push(Endpoint {
                        card: s.card,
                        name: name.into(),
                        label: s.description.as_deref().unwrap_or(name).into(),
                        source: name.into(),
                        ports: s
                            .ports
                            .iter()
                            .filter_map(|p| p.name.as_deref().map(String::from))
                            .collect(),
                        active_port: s
                            .active_port
                            .as_ref()
                            .and_then(|p| p.name.as_deref())
                            .map(String::from),
                        input: true,
                    });
                }
            }
        });
        let r = result.clone();
        let server = self.context.introspect().get_server_info(move |s| {
            let mut r = r.borrow_mut();
            r.default_input = s.default_source_name.as_deref().unwrap_or("").into();
            r.default_output = s.default_sink_name.as_deref().unwrap_or("").into();
        });
        let deadline = Instant::now() + Duration::from_secs(3);
        while [
            cards.get_state(),
            sinks.get_state(),
            sources.get_state(),
            server.get_state(),
        ]
        .contains(&pulse::operation::State::Running)
        {
            self.mainloop.iterate(false);
            if Instant::now() >= deadline {
                bail!("Desktop audio device discovery timed out");
            }
            if self.context.get_state() != pulse::context::State::Ready {
                bail!("Desktop audio server disconnected");
            }
            thread::sleep(Duration::from_millis(3));
        }
        let snapshot = std::mem::take(&mut *result.borrow_mut());
        Ok(snapshot)
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.context.disconnect();
    }
}

fn route_name(route: &Route) -> String {
    format!(
        "pulse:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(route).expect("route is serializable"))
    )
}
fn route_for(card: &Card, port: &Port) -> Route {
    Route {
        card: card.name.clone(),
        port: port.name.clone(),
        input: port.input,
    }
}
fn endpoint_for<'a>(s: &'a Snapshot, route: &Route) -> Option<&'a Endpoint> {
    if route.port.is_empty() {
        return s
            .endpoints
            .iter()
            .find(|e| e.name == route.card && e.input == route.input);
    }
    let card = s.cards.iter().find(|c| c.name == route.card)?;
    s.endpoints.iter().find(|e| {
        e.card == Some(card.index)
            && e.input == route.input
            && e.ports.contains(&route.port)
            && (!route.input || e.active_port.as_deref() == Some(route.port.as_str()))
    })
}
fn device_for(route: &Route, label: String, active: bool) -> AudioDevice {
    let mut d = AudioDevice::new(
        route_name(route),
        if route.input {
            DeviceType::Input
        } else {
            DeviceType::Output
        },
    );
    d.display_name = Some(label);
    if !active {
        d.unavailable_reason = Some("Select this port in system sound settings first".into());
    }
    d
}
fn catalog(s: &Snapshot) -> Vec<AudioDevice> {
    let mut devices = Vec::new();
    for c in &s.cards {
        for p in &c.ports {
            let route = route_for(c, p);
            devices.push(device_for(
                &route,
                format!("{} – {}", p.label, c.label),
                endpoint_for(s, &route).is_some(),
            ));
        }
    }
    // Bluetooth and virtual audio nodes may have no card/port. Keep their desktop descriptions.
    for e in &s.endpoints {
        if !s.cards.iter().any(|c| {
            Some(c.index) == e.card
                && c.ports
                    .iter()
                    .any(|p| p.input == e.input && e.ports.contains(&p.name))
        }) && e.ports.is_empty()
        {
            devices.push(device_for(
                &Route {
                    card: e.name.clone(),
                    port: String::new(),
                    input: e.input,
                },
                e.label.clone(),
                true,
            ));
        }
    }
    devices
}
pub fn configure_linux_audio(_host: &cpal::Host) -> Result<Vec<AudioDevice>> {
    Ok(catalog(&Connection::open()?.snapshot()?))
}
pub fn default_device(input: bool) -> Result<AudioDevice> {
    let s = Connection::open()?.snapshot()?;
    let name = if input {
        &s.default_input
    } else {
        &s.default_output
    };
    let e = s
        .endpoints
        .iter()
        .find(|e| &e.name == name && e.input == input)
        .ok_or_else(|| anyhow!("No desktop default audio device"))?;
    let devices = catalog(&s);
    // Resolve current desktop defaults at the start of each recording.
    for d in devices {
        let route = decode_route(&d.name)?;
        if endpoint_for(&s, &route)
            .map(|x| {
                x.name == e.name
                    && (x.active_port.is_none()
                        || x.active_port.as_deref() == Some(route.port.as_str()))
            })
            .unwrap_or(false)
        {
            return Ok(d);
        }
    }
    Ok(device_for(
        &Route {
            card: e.name.clone(),
            port: String::new(),
            input,
        },
        e.label.clone(),
        true,
    ))
}
fn decode_route(name: &str) -> Result<Route> {
    if name.len() > 8192 {
        bail!("Invalid desktop audio device");
    }
    let bytes = URL_SAFE_NO_PAD.decode(
        name.strip_prefix("pulse:")
            .ok_or_else(|| anyhow!("Not a desktop audio device"))?,
    )?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub fn device_exists(device: &AudioDevice) -> bool {
    decode_route(&device.name)
        .and_then(|route| {
            if route.input != (device.device_type == DeviceType::Input) {
                bail!("Device direction mismatch");
            }
            Ok(endpoint_for(&Connection::open()?.snapshot()?, &route).is_some())
        })
        .unwrap_or(false)
}

/// Each capture owns its connection and a cancellable nonblocking event loop.
/// Dropping the stream releases the microphone even if the server disconnects.
pub struct PulseCapture {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl PulseCapture {
    pub async fn start(device: Arc<AudioDevice>, capture: AudioCapture) -> Result<Self> {
        tokio::task::spawn_blocking(move || {
            let stop = Arc::new(AtomicBool::new(false));
            let flag = stop.clone();
            let (tx, rx) = std::sync::mpsc::sync_channel(1);
            let thread = thread::Builder::new().name("sonefold-desktop-audio".into()).spawn(move || {
                let setup = (|| -> Result<(Connection, pulse::stream::Stream)> {
                    let mut conn = Connection::open()?;
                    let s = conn.snapshot()?;
                    let route = decode_route(&device.name)?;
                    if route.input != (device.device_type == DeviceType::Input) { bail!("Device direction mismatch"); }
                    let source = endpoint_for(&s, &route).ok_or_else(|| anyhow!("This audio port is inactive. Select it in system sound settings."))?.source.clone();
                    let spec = pulse::sample::Spec { format: pulse::sample::Format::F32le, channels: 1, rate: 48000 };
                    let mut stream = pulse::stream::Stream::new(&mut conn.context, "Meeting recording", &spec, None).ok_or_else(|| anyhow!("Could not create the desktop recording stream"))?;
                    let attr = pulse::def::BufferAttr { maxlength: 48000 * 4, tlength: u32::MAX, prebuf: u32::MAX, minreq: u32::MAX, fragsize: 480 * 4 };
                    stream.connect_record(Some(&source), Some(&attr), pulse::stream::FlagSet::DONT_MOVE | pulse::stream::FlagSet::ADJUST_LATENCY).map_err(|e| anyhow!("Could not record the selected desktop device: {e}"))?;
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while stream.get_state() != pulse::stream::State::Ready {
                        conn.mainloop.iterate(false);
                        if flag.load(Ordering::Acquire) || Instant::now() >= deadline || matches!(stream.get_state(), pulse::stream::State::Failed | pulse::stream::State::Terminated) { bail!("Desktop recording stream did not become ready"); }
                        thread::sleep(Duration::from_millis(3));
                    }
                    Ok((conn, stream))
                })();
                let (mut conn, mut stream) = match setup { Ok(s) => { let _ = tx.send(Ok(())); s }, Err(e) => { let _ = tx.send(Err(e)); return; } };
                while !flag.load(Ordering::Acquire) {
                    conn.mainloop.iterate(false);
                    if stream.get_state() != pulse::stream::State::Ready || conn.context.get_state() != pulse::context::State::Ready {
                        capture.handle_stream_error(cpal::StreamError::DeviceNotAvailable); break;
                    }
                    while stream.readable_size().unwrap_or(0) > 0 && !flag.load(Ordering::Acquire) {
                        let samples = match stream.peek() {
                            Ok(pulse::stream::PeekResult::Data(data)) => data.chunks_exact(4).map(|v| f32::from_le_bytes(v.try_into().unwrap())).collect::<Vec<_>>(),
                            Ok(pulse::stream::PeekResult::Hole(bytes)) => vec![0.0; (bytes / 4).min(48000)],
                            Ok(pulse::stream::PeekResult::Empty) => break,
                            Err(_) => { capture.handle_stream_error(cpal::StreamError::DeviceNotAvailable); flag.store(true, Ordering::Release); break; }
                        };
                        capture.process_audio_data(&samples);
                        if stream.discard().is_err() { flag.store(true, Ordering::Release); break; }
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                let _ = stream.disconnect();
            })?;
            let mut handle = Self { stop, thread: Some(thread) };
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(())) => Ok(handle),
                Ok(Err(e)) => { handle.close(); Err(e) },
                Err(_) => { handle.close(); Err(anyhow!("Desktop audio setup timed out")) },
            }
        }).await.map_err(|e| anyhow!("Desktop audio thread failed: {e}"))?
    }
    fn close(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
impl Drop for PulseCapture {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Snapshot {
        Snapshot {
            cards: vec![Card {
                index: 2,
                name: "card".into(),
                label: "Shure MV6".into(),
                ports: vec![
                    Port {
                        name: "mic".into(),
                        label: "Analog Input".into(),
                        input: true,
                    },
                    Port {
                        name: "analog".into(),
                        label: "Analog Output".into(),
                        input: false,
                    },
                    Port {
                        name: "digital".into(),
                        label: "Digital Output (S/PDIF)".into(),
                        input: false,
                    },
                ],
            }],
            endpoints: vec![
                Endpoint {
                    card: Some(2),
                    name: "input".into(),
                    label: "Shure".into(),
                    source: "input".into(),
                    ports: vec!["mic".into()],
                    active_port: Some("mic".into()),
                    input: true,
                },
                Endpoint {
                    card: Some(2),
                    name: "output".into(),
                    label: "Shure".into(),
                    source: "output.monitor".into(),
                    ports: vec!["digital".into()],
                    active_port: Some("digital".into()),
                    input: false,
                },
            ],
            default_input: "input".into(),
            default_output: "output".into(),
        }
    }
    #[test]
    fn desktop_labels_keep_inactive_ports_without_misrouting() {
        let s = fixture();
        let devices = catalog(&s);
        assert_eq!(devices.len(), 3);
        assert_eq!(
            devices[0].display_name.as_deref(),
            Some("Analog Input – Shure MV6")
        );
        assert_eq!(devices[0].device_type, DeviceType::Input);
        assert!(devices[1].unavailable_reason.is_some());
        assert!(endpoint_for(&s, &decode_route(&devices[1].name).unwrap()).is_none());
        assert_eq!(
            endpoint_for(&s, &decode_route(&devices[2].name).unwrap())
                .unwrap()
                .source,
            "output.monitor"
        );
        assert!(devices[2].unavailable_reason.is_none());
    }
    #[test]
    fn route_ids_survive_renaming_and_reject_invalid_input() {
        let route = Route {
            card: "card:1 @ 日本語".into(),
            port: "[In] Microphone".into(),
            input: true,
        };
        let decoded = decode_route(&route_name(&route)).unwrap();
        assert_eq!(decoded.card, route.card);
        assert_eq!(decoded.port, route.port);
        assert!(decode_route("hw:CARD=Shure,DEV=0").is_err());
        assert!(decode_route("pulse:invalid").is_err());
    }
}
