use core::time::Duration;

use llts_wire::MonotonicMicros;

use crate::clock::{Instant, PeerClockOffset};
use crate::util::linear_fit::LinearFit;

#[derive(Debug, Clone, Copy)]
struct OffsetSample {
    local_micros: u64,
    offset_micros: i64,
    round_trip: Duration,
}

/// The peer's clock as a line against ours: an offset and a skew.
///
/// Each probe exchange yields one Cristian sample, whose error is bounded by how asymmetric its
/// round trip was. Samples are grouped into blocks and only each block's shortest round trip
/// survives, since that is the exchange least distorted by queueing; a least-squares line
/// through the survivors then tracks the drift between the two crystals — 100 ppm is 6 ms a
/// minute, far past what a fixed offset could absorb.
#[derive(Debug)]
pub struct ClockSync {
    baseline: PeerClockOffset,
    points: [Option<OffsetSample>; Self::POINTS],
    next_point: usize,
    block_best: Option<OffsetSample>,
    block_samples: u8,
    origin_micros: Option<u64>,
    fit: Option<(f64, f64)>,
}

impl Default for ClockSync {
    fn default() -> Self {
        Self::new()
    }
}

impl ClockSync {
    /// Exchanges per block. At four probes a second a block is two seconds.
    const BLOCK: u8 = 8;

    /// Block minima the line is fitted through: half a minute of history at four probes a second.
    const POINTS: usize = 16;

    pub const fn new() -> Self {
        Self {
            baseline: PeerClockOffset::new(),
            points: [None; Self::POINTS],
            next_point: 0,
            block_best: None,
            block_samples: 0,
            origin_micros: None,
            fit: None,
        }
    }

    pub const fn samples(&self) -> u32 {
        self.baseline.samples()
    }

    /// Microseconds to add to a peer timestamp to express it on the local clock, at `now`.
    pub fn offset_micros(&self, now: Instant) -> i64 {
        let Some((intercept, slope)) = self.fit else {
            return self.baseline.offset_micros();
        };
        let elapsed = now
            .as_micros()
            .saturating_sub(self.origin_micros.unwrap_or(0));
        round_to_i64(intercept + slope * micros_as_f64(elapsed))
    }

    /// How fast the offset drifts, in parts per billion.
    pub fn skew_ppb(&self) -> i64 {
        self.fit
            .map_or(0, |(_, slope)| round_to_i64(slope * 1_000_000_000.0))
    }

    /// Expresses a peer timestamp on the local clock, as of `now`.
    pub fn to_local(&self, peer: MonotonicMicros, now: Instant) -> Instant {
        let local = self
            .unwrap_peer(peer, now)
            .saturating_add(self.offset_micros(now));
        Instant::from_micros(u64::try_from(local).unwrap_or(0))
    }

    /// Folds in one probe exchange, returning whether the estimate moved.
    ///
    /// `sent` and `received` are local; `peer_received` and `peer_sent` come from the peer's reply.
    pub fn observe(
        &mut self,
        sent: Instant,
        peer_received: MonotonicMicros,
        peer_sent: MonotonicMicros,
        received: Instant,
    ) -> bool {
        let turnaround = Duration::from_micros(u64::from(peer_sent.elapsed_since(peer_received)));
        let round_trip = received
            .saturating_elapsed_since(sent)
            .saturating_sub(turnaround);
        let arrival_locally = sent.saturating_add(round_trip / 2);
        let peer_arrival = self.unwrap_peer(peer_received, arrival_locally);
        let local_micros = arrival_locally.as_micros();

        self.baseline
            .observe(sent, peer_received, peer_sent, received);
        let origin = *self.origin_micros.get_or_insert(local_micros);
        let sample = OffsetSample {
            local_micros: local_micros.saturating_sub(origin),
            offset_micros: i64::try_from(local_micros)
                .unwrap_or(i64::MAX)
                .saturating_sub(peer_arrival),
            round_trip,
        };
        if self
            .block_best
            .is_none_or(|best| sample.round_trip < best.round_trip)
        {
            self.block_best = Some(sample);
        }
        self.block_samples += 1;
        if self.block_samples >= Self::BLOCK {
            if let Some(slot) = self.points.get_mut(self.next_point) {
                *slot = self.block_best.take();
            }
            self.next_point = (self.next_point + 1) % Self::POINTS;
            self.block_samples = 0;
        }

        let previous = self.fit;
        self.fit = self.refit().or(previous);
        previous != self.fit
    }

    fn refit(&self) -> Option<(f64, f64)> {
        let mut fit = LinearFit::new();
        for point in self.points.iter().flatten().chain(self.block_best.iter()) {
            fit.add(
                micros_as_f64(point.local_micros),
                i64_as_f64(point.offset_micros),
            );
        }
        fit.solve()
    }

    /// The peer's 32-bit timestamp placed on a 64-bit line, as the reading nearest to where the
    /// current estimate predicts the peer's clock stands at local time `near`.
    fn unwrap_peer(&self, peer: MonotonicMicros, near: Instant) -> i64 {
        if self.baseline.samples() == 0 {
            return i64::from(peer.get());
        }
        let local = i64::try_from(near.as_micros()).unwrap_or(i64::MAX);
        let predicted = local.saturating_sub(self.offset_micros(near));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "only the low 32 bits are compared, which is the point"
        )]
        let predicted_low = predicted as u32;
        let delta = peer.get().wrapping_sub(predicted_low).cast_signed();
        predicted.saturating_add(i64::from(delta))
    }
}

#[allow(
    clippy::cast_precision_loss,
    reason = "session clocks stay far below 2^53 microseconds"
)]
const fn micros_as_f64(micros: u64) -> f64 {
    micros as f64
}

#[allow(
    clippy::cast_precision_loss,
    reason = "clock offsets stay far below 2^53 microseconds"
)]
const fn i64_as_f64(value: i64) -> f64 {
    value as f64
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "a clock offset in microseconds fits an i64 with room to spare, and `as` saturates"
)]
fn round_to_i64(value: f64) -> i64 {
    if value < 0.0 {
        (value - 0.5) as i64
    } else {
        (value + 0.5) as i64
    }
}
