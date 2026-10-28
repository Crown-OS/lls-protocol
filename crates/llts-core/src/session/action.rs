use super::PathId;
use crate::buffer::{Datagram, PooledBuffer};
use crate::clock::Instant;
use crate::receive::AssembledFrame;
use crate::stream::StreamKey;
use crate::units::Bitrate;

/// Something the driver must do on the session's behalf.
///
/// Every variant owns its payload, so the session never lends out state it is about to mutate.
#[derive(Debug)]
pub enum Action {
    /// Seal these bytes and put them on `path`.
    Transmit { path: PathId, datagram: Datagram },
    /// Hand this frame to the stream's decoder.
    PresentFrame {
        stream: StreamKey,
        frame: AssembledFrame,
    },
    /// A complete reliable message, or one batch of input events.
    Deliver {
        stream: StreamKey,
        message: PooledBuffer,
    },
    /// The peer's full pressed-key set, which replaces whatever the receiver believed.
    InputKeyState {
        stream: StreamKey,
        state: PooledBuffer,
    },
    /// Reconfigure this stream's encoder rate.
    SetTargetBitrate { stream: StreamKey, bitrate: Bitrate },
    /// Cap the stream's next frame, which is how the rate loop closes inside one frame.
    SetFrameByteBudget { stream: StreamKey, bytes: u32 },
    /// Ask the stream's encoder for a frame that decodes without a predecessor.
    RequestKeyframe { stream: StreamKey },
    /// The peer clock estimate moved: add `offset_micros` to a peer timestamp to read it
    /// locally, and expect it to drift by `skew_ppb` parts per billion.
    PeerClockUpdated { offset_micros: i64, skew_ppb: i64 },
    /// Nothing authenticated has arrived for [`super::Liveness::DEAD_AFTER`].
    PeerDead,
    /// Traffic moved to a newly validated path; the congestion estimate restarted with it.
    PathChanged(PathId),
    /// Wake the session no later than this.
    ArmTimer(Instant),
}
