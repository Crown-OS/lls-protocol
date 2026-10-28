use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::{Duration, Instant as WallClock};

use anyhow::Context as _;
use llts_core::channel::Delivery;
use llts_core::clock::Instant;
use llts_core::link::LinkKind;
use llts_core::repair::{RepairMode, RepairRate};
use llts_core::send::EncodedSlice;
use llts_core::session::{Action, Event, PathId, PeerConfig, PeerSession, Role, SessionPools};
use llts_core::stats::LatencyHistogram;
use llts_core::stream::{MediaStreamConfig, StreamKey};
use llts_core::units::FrameRate;
use llts_transport::PacketLink;
use llts_transport::udp::UdpLink;
use llts_wire::{MediaTimestamp, SessionTag, StreamClass};

use crate::options::BenchOptions;
use crate::report::{BenchReport, SideTraffic};

const MTU: usize = 1200;
const SCREEN: StreamKey = StreamKey::new(StreamClass::Video, 0);
const MICROPHONE: StreamKey = StreamKey::new(StreamClass::Audio, 0);
const SENDER_TAG: SessionTag = SessionTag::new(0x5E4D);
const RECEIVER_TAG: SessionTag = SessionTag::new(0x4ECF);

/// An Opus frame at 64 kbit/s and 10 ms.
const OPUS_FRAME_BYTES: usize = 80;
/// A control message the size of a typical state update.
const CONTROL_MESSAGE_BYTES: usize = 96;
const CONTROL_MESSAGE_TYPE: u16 = 0x4000;

/// Two peer sessions talking over real loopback sockets, driven by a synthetic encoder.
///
/// Real sockets rather than an in-memory channel, so the measurement includes the syscalls,
/// socket options and kernel queueing that a deployment will actually pay for. Unless told
/// otherwise, a control message rides along with every frame and a 10 ms audio frame goes out
/// between them, so the gate measures video with the rest of the mux competing for the pacer.
pub(crate) struct Loopback {
    options: BenchOptions,
    sender: PeerSession,
    receiver: PeerSession,
    /// The sender's socket: it writes here and reads the receiver's feedback here.
    sender_link: UdpLink,
    /// The receiver's socket, the other end of the same pair.
    receiver_link: UdpLink,
    started: WallClock,
    submitted: HashMap<(StreamKey, u32), WallClock>,
    control_submitted: HashMap<u32, WallClock>,
    latency: LatencyHistogram,
    worst: Duration,
    audio: SideTraffic,
    control: SideTraffic,
    datagrams_sent: u64,
    datagrams_dropped: u64,
    bytes_sent: u64,
    payload: Vec<u8>,
    opus: Vec<u8>,
    scratch: Vec<u8>,
}

impl Loopback {
    pub(crate) fn new(options: BenchOptions) -> anyhow::Result<Self> {
        let (sender_link, receiver_link) = paired_links().context("binding loopback sockets")?;

        // The socket is loopback, but the link being *modelled* is Wi-Fi: the repair policy
        // refuses to spend parity on a path that cannot drop a packet, and telling it otherwise
        // while injecting loss would measure a configuration nobody runs.
        let session = |local, peer| {
            let mut config = PeerConfig::lan(local, peer, LinkKind::Wifi);
            config.initial_bitrate = options.bitrate;
            config.max_bitrate = options.bitrate;
            PeerSession::new(config, SessionPools::for_screen_share(MTU))
        };
        let mut sender = session(SENDER_TAG, RECEIVER_TAG);
        let mut receiver = session(RECEIVER_TAG, SENDER_TAG);

        let screen = |role| {
            let mut config = MediaStreamConfig::screen_share(role);
            config.frame_rate = options.frame_rate;
            if let Some(percent) = options.forced_parity_percent {
                config.repair = RepairMode::Fixed(RepairRate::from_percent(percent));
            }
            config
        };
        sender.open_media(SCREEN, screen(Role::Sender))?;
        receiver.open_media(SCREEN, screen(Role::Receiver))?;
        if !options.video_only {
            sender.open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Sender))?;
            receiver.open_media(MICROPHONE, MediaStreamConfig::opus_voice(Role::Receiver))?;
        }

        Ok(Self {
            sender,
            receiver,
            sender_link,
            receiver_link,
            started: WallClock::now(),
            submitted: HashMap::with_capacity(options.frames as usize * 3),
            control_submitted: HashMap::with_capacity(options.frames as usize),
            latency: LatencyHistogram::new(),
            worst: Duration::ZERO,
            audio: SideTraffic::default(),
            control: SideTraffic::default(),
            datagrams_sent: 0,
            datagrams_dropped: 0,
            bytes_sent: 0,
            payload: vec![0xA5; options.frame_bytes()],
            opus: vec![0x3C; OPUS_FRAME_BYTES],
            scratch: vec![0; MTU],
            options,
        })
    }

    pub(crate) fn run(mut self) -> anyhow::Result<BenchReport> {
        let period = self.options.frame_rate.period();
        let audio_period = FrameRate::OPUS_10_MS.period();
        self.started = WallClock::now();
        let mut next_audio = self.started;
        let mut audio_index = 0u32;

        for index in 0..self.options.frames {
            let due = self.started + period * index;
            while WallClock::now() < due {
                if !self.options.video_only && WallClock::now() >= next_audio {
                    self.submit_audio(audio_index)?;
                    audio_index += 1;
                    next_audio += audio_period;
                }
                self.pump()?;
            }
            self.submit_frame(index)?;
            if !self.options.video_only {
                self.submit_control(index)?;
            }
            self.pump()?;
        }

        // Let anything still in flight land before reading the meters.
        let drain_until = WallClock::now() + period * 8;
        while WallClock::now() < drain_until {
            self.pump()?;
        }

        Ok(BenchReport {
            options: self.options,
            frames_sent: self.options.frames,
            datagrams_sent: self.datagrams_sent,
            datagrams_dropped: self.datagrams_dropped,
            bytes_sent: self.bytes_sent,
            wall_clock: self.started.elapsed(),
            latency_p50: self.latency.quantile(50),
            latency_p95: self.latency.quantile(95),
            latency_p99: self.latency.quantile(99),
            latency_worst: self.worst,
            audio: self.audio,
            control: self.control,
            receiver: self
                .receiver
                .stats(SCREEN)
                .context("the receiver lost its screen stream")?,
            sender: self
                .sender
                .stats(SCREEN)
                .context("the sender lost its screen stream")?,
        })
    }

    fn now(&self) -> Instant {
        Instant::from_micros(u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX))
    }

    fn submit_frame(&mut self, index: u32) -> anyhow::Result<()> {
        let ticks = index.saturating_mul(90_000) / u32::from(self.options.frame_rate.hz()).max(1);
        self.submitted.insert((SCREEN, ticks), WallClock::now());

        let now = self.now();
        let slice = EncodedSlice::new(&self.payload, MediaTimestamp::new(ticks))
            .as_keyframe()
            .ending_frame();
        self.sender
            .handle(
                now,
                &Event::SliceEncoded {
                    stream: SCREEN,
                    slice,
                },
            )
            .context("submitting a slice")?;
        Ok(())
    }

    fn submit_audio(&mut self, index: u32) -> anyhow::Result<()> {
        let ticks = index.saturating_mul(480);
        self.submitted.insert((MICROPHONE, ticks), WallClock::now());
        self.audio.sent += 1;
        let now = self.now();
        self.sender
            .handle(
                now,
                &Event::SliceEncoded {
                    stream: MICROPHONE,
                    slice: EncodedSlice::new(&self.opus, MediaTimestamp::new(ticks)).ending_frame(),
                },
            )
            .context("submitting an audio frame")?;
        Ok(())
    }

    fn submit_control(&mut self, index: u32) -> anyhow::Result<()> {
        let mut message = [0u8; CONTROL_MESSAGE_BYTES];
        message[..2].copy_from_slice(&CONTROL_MESSAGE_TYPE.to_le_bytes());
        message[2..6].copy_from_slice(&index.to_le_bytes());
        self.control_submitted.insert(index, WallClock::now());
        self.control.sent += 1;
        let now = self.now();
        self.sender
            .handle(
                now,
                &Event::ControlQueued {
                    stream: StreamKey::CONTROL,
                    message: &message,
                    delivery: Delivery::Ordered,
                },
            )
            .context("queueing a control message")?;
        Ok(())
    }

    fn pump(&mut self) -> anyhow::Result<()> {
        let now = self.now();
        self.sender.poll(now)?;
        self.receiver.poll(now)?;

        while let Some(action) = self.sender.poll_action() {
            if let Action::Transmit { datagram, .. } = action {
                self.datagrams_sent += 1;
                if self.options.drop_every > 0
                    && usize::try_from(self.datagrams_sent)
                        .unwrap_or(0)
                        .is_multiple_of(self.options.drop_every)
                {
                    self.datagrams_dropped += 1;
                    continue;
                }
                self.bytes_sent += u64::try_from(datagram.byte_len()).unwrap_or(0);
                self.sender_link.send(datagram.bytes())?;
            }
        }

        while let Some(len) = self.receiver_link.recv(&mut self.scratch)? {
            let bytes = self.scratch.get(..len).unwrap_or_default().to_vec();
            self.receiver.handle(
                now,
                &Event::DatagramReceived {
                    path: PathId::default(),
                    bytes: &bytes,
                    arrived_at: now.wire_micros(),
                },
            )?;
        }

        while let Some(action) = self.receiver.poll_action() {
            match action {
                Action::PresentFrame { stream, frame } => {
                    self.record_presentation(stream, frame.timestamp().ticks());
                }
                Action::Deliver { stream, message } if stream == StreamKey::CONTROL => {
                    self.record_control(message.filled());
                }
                Action::Transmit { datagram, .. } => {
                    self.receiver_link.send(datagram.bytes())?;
                }
                _ => {}
            }
        }

        while let Some(len) = self.sender_link.recv(&mut self.scratch)? {
            let bytes = self.scratch.get(..len).unwrap_or_default().to_vec();
            self.sender.handle(
                now,
                &Event::DatagramReceived {
                    path: PathId::default(),
                    bytes: &bytes,
                    arrived_at: now.wire_micros(),
                },
            )?;
        }
        Ok(())
    }

    fn record_presentation(&mut self, stream: StreamKey, ticks: u32) {
        let Some(submitted) = self.submitted.remove(&(stream, ticks)) else {
            return;
        };
        let latency = submitted.elapsed();
        if stream == SCREEN {
            self.latency.record(latency);
            self.worst = self.worst.max(latency);
        } else {
            self.audio.record(latency);
        }
    }

    fn record_control(&mut self, message: &[u8]) {
        let Some(index) = message
            .get(2..6)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes)
        else {
            return;
        };
        if let Some(submitted) = self.control_submitted.remove(&index) {
            self.control.record(submitted.elapsed());
        }
    }
}

/// Binds two connected loopback sockets. The first send has to wait until both ports are known,
/// which is why each side is bound before either is connected.
fn paired_links() -> anyhow::Result<(UdpLink, UdpLink)> {
    let probe_left = std::net::UdpSocket::bind(any_port())?;
    let probe_right = std::net::UdpSocket::bind(any_port())?;
    let left_addr = probe_left.local_addr()?;
    let right_addr = probe_right.local_addr()?;
    drop(probe_left);
    drop(probe_right);

    let left = UdpLink::connect(left_addr, right_addr, LinkKind::Loopback, MTU)?;
    let right = UdpLink::connect(right_addr, left_addr, LinkKind::Loopback, MTU)?;
    Ok((left, right))
}

const fn any_port() -> SocketAddr {
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
}
