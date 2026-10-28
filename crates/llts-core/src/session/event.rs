use llts_wire::MonotonicMicros;

use super::PathId;
use crate::channel::Delivery;
use crate::link::LinkKind;
use crate::send::EncodedSlice;
use crate::stream::StreamKey;

/// Something that happened outside the session.
///
/// Borrows only on the way in; every [`super::Action`] the session returns owns its data, so a
/// driver can queue actions freely without threading a lifetime through its own state.
#[derive(Debug)]
pub enum Event<'a> {
    /// A stream's encoder produced a chunk of a frame.
    SliceEncoded {
        stream: StreamKey,
        slice: EncodedSlice<'a>,
    },
    /// An authenticated, decrypted datagram arrived on `path`.
    DatagramReceived {
        path: PathId,
        bytes: &'a [u8],
        arrived_at: MonotonicMicros,
    },
    /// The application queued a message on a reliable stream.
    ControlQueued {
        stream: StreamKey,
        message: &'a [u8],
        delivery: Delivery,
    },
    /// The application queued a batch of input events.
    InputQueued { stream: StreamKey, events: &'a [u8] },
    /// The application dropped frames of a stream it receives and needs a keyframe to resume.
    KeyframeNeeded { stream: StreamKey },
    /// The application's pressed-key set changed.
    KeyStateUpdated { stream: StreamKey, state: &'a [u8] },
    /// A local network change made `path` worth validating before any traffic arrives on it.
    PathCandidate(PathId),
    /// This side's own address changed, so the peer should hear from it at once: a probe goes
    /// out immediately, and the peer validates and follows the new address instead of waiting
    /// for the next keepalive.
    LocalAddressChanged,
    /// The session moved to a different physical path.
    LinkChanged(LinkKind),
    /// A timer the session asked for has fired.
    TimerExpired,
}
