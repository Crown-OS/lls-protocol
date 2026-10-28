use alloc::collections::VecDeque;
use core::time::Duration;

use crate::buffer::Datagram;
use crate::clock::Instant;
use crate::units::Bitrate;

/// Shapes when datagrams leave, so a frame's burst neither arrives as one instantaneous spike nor
/// dribbles out across the whole frame period.
///
/// Modelled as a departure clock rather than a token bucket: each datagram pushes the clock
/// forward by its own transmission time, and an idle pacer may rewind the clock by at most
/// [`DeparturePacer::burst_tolerance`]. That is the same shape as `SO_TXTIME` earliest-departure
/// pacing, so the Linux driver can later hand these timestamps straight to the kernel.
///
/// On a LAN the point is not to match link capacity — there is two orders of magnitude of
/// headroom — but to keep a frame inside one Wi-Fi aggregate without overflowing the access
/// point's queue, and to leave the delay-gradient estimator a signal it can read.
#[derive(Debug)]
pub struct DeparturePacer {
    rate: Bitrate,
    burst_tolerance: Duration,
    next_departure: Instant,
    queue: VecDeque<Datagram>,
    queued_bytes: usize,
    capacity_bytes: usize,
    dropped_on_overflow: u64,
}

impl DeparturePacer {
    /// How far behind the present the departure clock may sit after an idle spell, and so how
    /// large a burst the pacer will release at once. Two milliseconds at 20 Mbit/s is about
    /// 5 kB — a few datagrams, comfortably inside one aggregate.
    pub const DEFAULT_BURST_TOLERANCE: Duration = Duration::from_millis(2);

    pub fn new(rate: Bitrate, capacity_bytes: usize) -> Self {
        Self {
            rate,
            burst_tolerance: Self::DEFAULT_BURST_TOLERANCE,
            next_departure: Instant::SESSION_START,
            queue: VecDeque::with_capacity(64),
            queued_bytes: 0,
            capacity_bytes,
            dropped_on_overflow: 0,
        }
    }

    #[must_use]
    pub const fn with_burst_tolerance(mut self, tolerance: Duration) -> Self {
        self.burst_tolerance = tolerance;
        self
    }

    /// Adopts a new pacing rate, which the rate controller derives from its capacity estimate.
    pub const fn set_rate(&mut self, rate: Bitrate) {
        self.rate = rate;
    }

    pub const fn rate(&self) -> Bitrate {
        self.rate
    }

    pub const fn burst_tolerance(&self) -> Duration {
        self.burst_tolerance
    }

    pub const fn queued_bytes(&self) -> usize {
        self.queued_bytes
    }

    pub fn queued_datagrams(&self) -> usize {
        self.queue.len()
    }

    pub const fn dropped_on_overflow(&self) -> u64 {
        self.dropped_on_overflow
    }

    /// How long the queue would take to drain at the current rate.
    ///
    /// This is self-inflicted latency, so the congestion controller watches it: a queue that is
    /// not draining means the pacing rate is below what the encoder is producing.
    pub fn drain_time(&self) -> Duration {
        self.rate
            .transmission_time(u32::try_from(self.queued_bytes).unwrap_or(u32::MAX))
    }

    /// Queues a datagram, evicting the oldest if the queue is already full.
    ///
    /// Dropping the head rather than the newcomer is deliberate: under overload the freshest
    /// frame is the one worth delivering, and a stale datagram at the front is one nobody will
    /// wait for.
    pub fn enqueue(&mut self, datagram: Datagram) {
        while self.queued_bytes.saturating_add(datagram.byte_len()) > self.capacity_bytes {
            if self.discard_oldest().is_none() {
                break;
            }
        }
        self.queued_bytes = self.queued_bytes.saturating_add(datagram.byte_len());
        self.queue.push_back(datagram);
    }

    /// Releases the next datagram if its departure time has arrived.
    pub fn poll_departure(&mut self, now: Instant) -> Option<Datagram> {
        let earliest = now.saturating_sub(self.burst_tolerance);
        if self.next_departure < earliest {
            self.next_departure = earliest;
        }
        if self.next_departure.is_after(now) {
            return None;
        }

        let datagram = self.queue.pop_front()?;
        self.queued_bytes = self.queued_bytes.saturating_sub(datagram.byte_len());
        let on_wire = self
            .rate
            .transmission_time(u32::try_from(datagram.byte_len()).unwrap_or(u32::MAX));
        self.next_departure = self.next_departure.saturating_add(on_wire);
        Some(datagram)
    }

    /// When the next datagram becomes eligible, for the timer wheel to wake on.
    pub fn next_departure(&self, now: Instant) -> Option<Instant> {
        if self.queue.is_empty() {
            return None;
        }
        let earliest = now.saturating_sub(self.burst_tolerance);
        Some(self.next_departure.max(earliest))
    }

    fn discard_oldest(&mut self) -> Option<Datagram> {
        let datagram = self.queue.pop_front()?;
        self.queued_bytes = self.queued_bytes.saturating_sub(datagram.byte_len());
        self.dropped_on_overflow = self.dropped_on_overflow.saturating_add(1);
        Some(datagram)
    }
}
