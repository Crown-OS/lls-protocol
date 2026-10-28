//! Two peer sessions joined by a simulated link that can drop, delay and reorder.

#![allow(
    dead_code,
    clippy::cast_possible_truncation,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "shared test scaffolding; each test file uses a different part of it"
)]

use core::time::Duration;

use llts_core::buffer::BufferPool;
use llts_core::channel::MAX_MESSAGE_LEN;
use llts_core::clock::Instant;
use llts_core::link::LinkKind;
use llts_core::session::{Action, Event, PathId, PeerConfig, PeerSession, SessionPools};
use llts_core::stream::StreamKey;
use llts_core::units::Bitrate;
use llts_wire::SessionTag;

pub(crate) const MTU: usize = 1200;

/// A deterministic pseudo-random source, so a lossy run is exactly reproducible.
#[derive(Debug, Clone)]
pub(crate) struct Lcg(u64);

impl Lcg {
    pub(crate) const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) const fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    /// True with probability `per_mille / 1000`.
    pub(crate) const fn chance(&mut self, per_mille: u32) -> bool {
        self.next_u32() % 1_000 < per_mille
    }

    pub(crate) fn below(&mut self, bound: u32) -> u32 {
        self.next_u32() % bound.max(1)
    }
}

/// How the simulated network treats every datagram.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LinkModel {
    pub(crate) one_way: Duration,
    /// Extra delay drawn uniformly from zero to this, which reorders datagrams.
    pub(crate) jitter: Duration,
    pub(crate) loss_per_mille: u32,
    /// When non-zero, exactly one datagram in this many is dropped instead of drawing at random.
    pub(crate) drop_every: u64,
}

impl LinkModel {
    pub(crate) const CLEAN: Self = Self {
        one_way: Duration::from_micros(500),
        jitter: Duration::ZERO,
        loss_per_mille: 0,
        drop_every: 0,
    };
}

#[derive(Debug)]
struct InFlight {
    deliver_at: Instant,
    to_right: bool,
    path: PathId,
    bytes: Vec<u8>,
}

/// Everything one side's session asked the application to do, in order.
#[derive(Debug, Default)]
pub(crate) struct Observed {
    pub(crate) presented: Vec<(StreamKey, Vec<u8>)>,
    pub(crate) delivered: Vec<(StreamKey, Vec<u8>)>,
    pub(crate) key_states: Vec<(StreamKey, Vec<u8>)>,
    pub(crate) keyframe_requests: Vec<StreamKey>,
    pub(crate) bitrates: Vec<(StreamKey, Bitrate)>,
    pub(crate) clock_updates: Vec<(i64, i64)>,
    pub(crate) path_changes: Vec<PathId>,
    pub(crate) peer_dead: u32,
    pub(crate) transmitted: u64,
    pub(crate) paths_used: Vec<PathId>,
}

pub(crate) fn pools() -> SessionPools {
    SessionPools {
        datagrams: BufferPool::new(MTU, 4_096),
        frames: BufferPool::new(512 * 1024, 8),
        shards: BufferPool::new(MTU, 512),
        messages: BufferPool::new(MAX_MESSAGE_LEN, 8),
    }
}

pub(crate) fn session(local: u16, peer: u16) -> PeerSession {
    PeerSession::new(
        PeerConfig::lan(
            SessionTag::new(local),
            SessionTag::new(peer),
            LinkKind::Wifi,
        ),
        pools(),
    )
}

/// Two sessions back to back.
pub(crate) struct Pair {
    pub(crate) left: PeerSession,
    pub(crate) right: PeerSession,
    pub(crate) left_seen: Observed,
    pub(crate) right_seen: Observed,
    pub(crate) now: Instant,
    pub(crate) model: LinkModel,
    /// The path the left side's datagrams appear to arrive on, as the right side sees them.
    pub(crate) left_path_at_right: PathId,
    /// Drops everything in both directions while set.
    pub(crate) blackout: bool,
    random: Lcg,
    sent: u64,
    dropped: u64,
    in_flight: Vec<InFlight>,
}

impl Pair {
    pub(crate) fn new(model: LinkModel) -> Self {
        Self {
            left: session(0x1111, 0x2222),
            right: session(0x2222, 0x1111),
            left_seen: Observed::default(),
            right_seen: Observed::default(),
            now: Instant::SESSION_START,
            model,
            left_path_at_right: PathId::default(),
            blackout: false,
            random: Lcg::new(0x5EED),
            sent: 0,
            dropped: 0,
            in_flight: Vec::new(),
        }
    }

    pub(crate) const fn dropped(&self) -> u64 {
        self.dropped
    }

    pub(crate) fn left_event(&mut self, event: &Event<'_>) {
        self.left.handle(self.now, event).unwrap();
    }

    pub(crate) fn right_event(&mut self, event: &Event<'_>) {
        self.right.handle(self.now, event).unwrap();
    }

    /// Runs both sessions and the link until `span` of simulated time has passed.
    pub(crate) fn run_for(&mut self, span: Duration) {
        let until = self.now.saturating_add(span);
        loop {
            self.collect();
            if self.deliver_due() {
                continue;
            }
            let next = self
                .in_flight
                .iter()
                .map(|flight| flight.deliver_at)
                .chain(self.left.next_timeout(self.now))
                .chain(self.right.next_timeout(self.now))
                .filter(|instant| instant.is_after(self.now))
                .min();
            match next {
                Some(next) if !next.is_after(until) => {
                    self.now = next;
                    self.left.poll(self.now).unwrap();
                    self.right.poll(self.now).unwrap();
                }
                _ => {
                    self.now = until;
                    self.left.poll(self.now).unwrap();
                    self.right.poll(self.now).unwrap();
                    self.collect();
                    return;
                }
            }
        }
    }

    fn collect(&mut self) {
        while let Some(action) = self.left.poll_action() {
            self.record(action, false);
        }
        while let Some(action) = self.right.poll_action() {
            self.record(action, true);
        }
    }

    fn record(&mut self, action: Action, from_right: bool) {
        let seen = if from_right {
            &mut self.right_seen
        } else {
            &mut self.left_seen
        };
        match action {
            Action::Transmit { path, datagram } => {
                seen.transmitted += 1;
                self.sent += 1;
                let lost = if self.blackout {
                    true
                } else if self.model.drop_every > 0 {
                    self.sent.is_multiple_of(self.model.drop_every)
                } else {
                    self.random.chance(self.model.loss_per_mille)
                };
                if lost {
                    self.dropped += 1;
                    return;
                }
                let jitter = self.random.below(
                    u32::try_from(self.model.jitter.as_micros())
                        .unwrap_or(0)
                        .max(1),
                );
                let delay = self.model.one_way + Duration::from_micros(u64::from(jitter));
                let arrival_path = if from_right {
                    PathId::default()
                } else {
                    self.left_path_at_right
                };
                seen.paths_used.push(path);
                self.in_flight.push(InFlight {
                    deliver_at: self.now.saturating_add(delay),
                    to_right: !from_right,
                    path: arrival_path,
                    bytes: datagram.bytes().to_vec(),
                });
            }
            Action::PresentFrame { stream, frame } => {
                seen.presented.push((stream, frame.bytes().to_vec()));
            }
            Action::Deliver { stream, message } => {
                seen.delivered.push((stream, message.filled().to_vec()));
            }
            Action::InputKeyState { stream, state } => {
                seen.key_states.push((stream, state.filled().to_vec()));
            }
            Action::RequestKeyframe { stream } => seen.keyframe_requests.push(stream),
            Action::SetTargetBitrate { stream, bitrate } => seen.bitrates.push((stream, bitrate)),
            Action::PeerClockUpdated {
                offset_micros,
                skew_ppb,
            } => seen.clock_updates.push((offset_micros, skew_ppb)),
            Action::PathChanged(path) => seen.path_changes.push(path),
            Action::PeerDead => seen.peer_dead += 1,
            Action::SetFrameByteBudget { .. } | Action::ArmTimer(_) => {}
        }
    }

    fn deliver_due(&mut self) -> bool {
        let now = self.now;
        let mut due: Vec<InFlight> = Vec::new();
        let mut index = 0;
        while index < self.in_flight.len() {
            if self.in_flight[index].deliver_at.is_after(now) {
                index += 1;
            } else {
                due.push(self.in_flight.swap_remove(index));
            }
        }
        due.sort_by_key(|flight| flight.deliver_at);
        let delivered = !due.is_empty();
        for flight in due {
            let session = if flight.to_right {
                &mut self.right
            } else {
                &mut self.left
            };
            let _ = session.handle(
                now,
                &Event::DatagramReceived {
                    path: flight.path,
                    bytes: &flight.bytes,
                    arrived_at: now.wire_micros(),
                },
            );
        }
        delivered
    }
}

pub(crate) fn ramp(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| seed.wrapping_add(index.to_le_bytes()[0]))
        .collect()
}
