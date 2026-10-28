use core::time::Duration;

use crate::units::{Bitrate, LossRate};

/// Everything an application needs to decide whether the link is healthy, sampled at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsSnapshot {
    pub frames_presented: u64,
    pub frames_superseded: u64,
    pub frames_expired: u64,
    pub fragments_repaired: u64,
    /// Frames abandoned incomplete because a newer one needed the slot.
    pub frames_incomplete: u64,
    pub retransmissions_requested: u64,
    pub retransmissions_abandoned: u64,
    pub loss_rate: LossRate,
    pub round_trip: Duration,
    pub playout_delay: Duration,
    pub pacing_rate: Bitrate,
    pub protocol_latency_p50: Duration,
    pub protocol_latency_p95: Duration,
    pub protocol_latency_p99: Duration,
}
