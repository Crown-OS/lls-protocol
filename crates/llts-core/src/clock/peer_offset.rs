use core::time::Duration;

use llts_wire::MonotonicMicros;

use super::Instant;

/// Estimates the offset between the two devices' monotonic clocks.
///
/// Cristian's algorithm, filtered on minimum round trip: the exchange with the shortest round trip
/// is the one whose path was least asymmetric, so it carries the smallest offset error. That gets
/// within a few hundred microseconds on a LAN, which is all the delay-gradient estimator and the
/// per-stage latency attribution need. PTP would reach a microsecond, but there is no Android
/// daemon for it and nothing here would use the precision.
#[derive(Debug, Default)]
pub struct PeerClockOffset {
    best_round_trip: Option<Duration>,
    offset_micros: i64,
    samples: u32,
}

impl PeerClockOffset {
    pub const fn new() -> Self {
        Self {
            best_round_trip: None,
            offset_micros: 0,
            samples: 0,
        }
    }

    pub const fn samples(&self) -> u32 {
        self.samples
    }

    pub const fn round_trip(&self) -> Option<Duration> {
        self.best_round_trip
    }

    /// Microseconds to add to a peer timestamp to express it on the local clock.
    pub const fn offset_micros(&self) -> i64 {
        self.offset_micros
    }

    /// Folds in one probe exchange.
    ///
    /// `sent` and `received` are local; `peer_received` and `peer_sent` come from the peer's reply.
    pub fn observe(
        &mut self,
        sent: Instant,
        peer_received: MonotonicMicros,
        peer_sent: MonotonicMicros,
        received: Instant,
    ) {
        let elapsed_locally = received.saturating_elapsed_since(sent);
        let peer_turnaround =
            Duration::from_micros(u64::from(peer_sent.elapsed_since(peer_received)));
        let round_trip = elapsed_locally.saturating_sub(peer_turnaround);

        self.samples = self.samples.saturating_add(1);
        if self.best_round_trip.is_some_and(|best| best <= round_trip) {
            return;
        }
        self.best_round_trip = Some(round_trip);

        let one_way = i64::try_from(round_trip.as_micros() / 2).unwrap_or(i64::MAX);
        let arrival_on_peer_clock = i64::from(peer_received.get());
        let departure_locally = i64::try_from(sent.as_micros()).unwrap_or(i64::MAX);
        self.offset_micros = departure_locally + one_way - arrival_on_peer_clock;
    }

    /// Expresses a peer timestamp on the local clock.
    pub fn to_local(&self, peer: MonotonicMicros) -> Instant {
        let local = i64::from(peer.get()).saturating_add(self.offset_micros);
        Instant::from_micros(u64::try_from(local).unwrap_or(0))
    }
}
