use alloc::collections::VecDeque;
use core::time::Duration;

use llts_wire::StreamClass;

use crate::buffer::Datagram;
use crate::clock::Instant;
use crate::units::Bitrate;

/// Which queue a paced datagram waits in, highest priority first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PacerLane {
    Control,
    Input,
    Audio,
    /// Bulk application data. Served ahead of video but held to its own share of the rate.
    Data,
    Video,
}

impl PacerLane {
    pub const COUNT: usize = 5;

    pub const ALL: [Self; Self::COUNT] = [
        Self::Control,
        Self::Input,
        Self::Audio,
        Self::Data,
        Self::Video,
    ];

    pub const fn for_class(class: StreamClass) -> Self {
        match class {
            StreamClass::Control => Self::Control,
            StreamClass::Input => Self::Input,
            StreamClass::Audio => Self::Audio,
            StreamClass::Data => Self::Data,
            StreamClass::Video => Self::Video,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Control => 0,
            Self::Input => 1,
            Self::Audio => 2,
            Self::Data => 3,
            Self::Video => 4,
        }
    }

    /// Whether a full lane drops its oldest datagram. Reliable lanes never do: their channel
    /// retransmits on its own timer, and a silently dropped fragment would only stall it.
    const fn drops_when_full(self) -> bool {
        matches!(self, Self::Input | Self::Audio | Self::Video)
    }
}

#[derive(Debug)]
struct LaneQueue {
    queue: VecDeque<Datagram>,
    queued_bytes: usize,
}

impl LaneQueue {
    fn new() -> Self {
        Self {
            queue: VecDeque::with_capacity(64),
            queued_bytes: 0,
        }
    }

    fn pop(&mut self) -> Option<Datagram> {
        let datagram = self.queue.pop_front()?;
        self.queued_bytes = self.queued_bytes.saturating_sub(datagram.byte_len());
        Some(datagram)
    }
}

/// One departure clock shared by every stream to a peer, with a strict-priority queue per class.
///
/// A keypress or an audio frame never waits behind a keyframe's burst: each release takes the
/// highest-priority datagram waiting. Data is the exception to strict priority — it runs on a
/// second clock at [`LanePacer::DATA_SHARE_PERCENT`] of the rate, so bulk transfers get their
/// reserved share even under video load, and never more.
///
/// The clock itself works like [`super::DeparturePacer`]: each datagram pushes it forward by its
/// own transmission time, and an idle pacer may rewind it by at most the burst tolerance.
#[derive(Debug)]
pub struct LanePacer {
    rate: Bitrate,
    burst_tolerance: Duration,
    next_departure: Instant,
    next_data_departure: Instant,
    lanes: [LaneQueue; PacerLane::COUNT],
    capacity_bytes: usize,
    dropped_on_overflow: u64,
}

impl LanePacer {
    pub const DATA_SHARE_PERCENT: u32 = 15;

    pub fn new(rate: Bitrate, capacity_bytes: usize) -> Self {
        Self {
            rate,
            burst_tolerance: super::DeparturePacer::DEFAULT_BURST_TOLERANCE,
            next_departure: Instant::SESSION_START,
            next_data_departure: Instant::SESSION_START,
            lanes: [(); PacerLane::COUNT].map(|()| LaneQueue::new()),
            capacity_bytes,
            dropped_on_overflow: 0,
        }
    }

    pub const fn set_rate(&mut self, rate: Bitrate) {
        self.rate = rate;
    }

    pub const fn rate(&self) -> Bitrate {
        self.rate
    }

    pub const fn dropped_on_overflow(&self) -> u64 {
        self.dropped_on_overflow
    }

    pub fn queued_datagrams(&self, lane: PacerLane) -> usize {
        self.lane(lane).map_or(0, |queue| queue.queue.len())
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.iter().all(|lane| lane.queue.is_empty())
    }

    /// Queues a datagram. A lane over its byte budget sheds its oldest datagrams first, unless
    /// it carries a reliable channel.
    pub fn enqueue(&mut self, lane: PacerLane, datagram: Datagram) {
        let capacity = self.capacity_bytes;
        let Some(queue) = self.lanes.get_mut(lane.index()) else {
            return;
        };
        if lane.drops_when_full() {
            while queue.queued_bytes.saturating_add(datagram.byte_len()) > capacity {
                if queue.pop().is_none() {
                    break;
                }
                self.dropped_on_overflow = self.dropped_on_overflow.saturating_add(1);
            }
        }
        queue.queued_bytes = queue.queued_bytes.saturating_add(datagram.byte_len());
        queue.queue.push_back(datagram);
    }

    /// Releases the most urgent datagram whose departure time has arrived.
    pub fn poll_departure(&mut self, now: Instant) -> Option<(PacerLane, Datagram)> {
        let earliest = now.saturating_sub(self.burst_tolerance);
        self.next_departure = self.next_departure.max(earliest);
        if self.next_departure.is_after(now) {
            return None;
        }
        let lane = self.eligible_lane(now)?;
        let datagram = self.lanes.get_mut(lane.index())?.pop()?;

        let on_wire = self
            .rate
            .transmission_time(u32::try_from(datagram.byte_len()).unwrap_or(u32::MAX));
        self.next_departure = self.next_departure.saturating_add(on_wire);
        if lane == PacerLane::Data {
            let share = self
                .rate
                .scaled_by(Self::DATA_SHARE_PERCENT, 100)
                .transmission_time(u32::try_from(datagram.byte_len()).unwrap_or(u32::MAX));
            self.next_data_departure = self.next_data_departure.max(earliest).saturating_add(share);
        }
        Some((lane, datagram))
    }

    /// When the next datagram becomes eligible, for the timer wheel to wake on.
    pub fn next_departure(&self, now: Instant) -> Option<Instant> {
        let earliest = now.saturating_sub(self.burst_tolerance);
        let clock = self.next_departure.max(earliest);
        let others_waiting = PacerLane::ALL
            .into_iter()
            .filter(|lane| *lane != PacerLane::Data)
            .any(|lane| self.queued_datagrams(lane) > 0);
        if others_waiting {
            return Some(clock);
        }
        (self.queued_datagrams(PacerLane::Data) > 0)
            .then(|| clock.max(self.next_data_departure.max(earliest)))
    }

    fn eligible_lane(&self, now: Instant) -> Option<PacerLane> {
        let data_allowed = !self.next_data_departure.is_after(now);
        PacerLane::ALL.into_iter().find(|lane| {
            self.queued_datagrams(*lane) > 0 && (*lane != PacerLane::Data || data_allowed)
        })
    }

    fn lane(&self, lane: PacerLane) -> Option<&LaneQueue> {
        self.lanes.get(lane.index())
    }
}
