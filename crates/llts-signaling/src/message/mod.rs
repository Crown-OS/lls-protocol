//! Control messages carried on the llts reliable Control and Data channels.
//!
//! Each channel message is one envelope: a little-endian `u16` [`MessageType`] followed by the
//! postcard body. A message's layout never changes once it ships. Changing one means registering
//! a new type (`StartMirrorV2` beside `StartMirrorV1`) and keeping the old one until every peer
//! has moved on. A type this build does not know decodes to [`Signal::Unknown`] instead of an
//! error, so a newer peer's messages are skipped rather than tearing the session down.

mod codec;
mod command;
mod envelope;
mod feature;
mod options;
mod registry;
mod session;
mod state;
mod stream;
mod stream_ref;

pub use codec::{AudioCodec, CodecList, MAX_CODECS, StreamCodec, VideoCodec, negotiate_codec};
pub use command::{Call, FfspTransferId, FileOffer, Lock, Media, Reply, SetHotspot, SetVolume};
pub use envelope::{ENVELOPE_HEADER_LEN, Envelope, Message};
pub use feature::{
    StartCamera, StartMic, StartMirror, StartMonitor, Stop, UnicursorEnter, UnicursorLeave,
};
pub use options::{CameraLens, Direction, Edge, EdgePosition, VideoLimits};
pub use registry::{FIRST_APPLICATION_TYPE, MessageType, Signal};
pub use session::{Capabilities, Goodbye, GoodbyeReason, KeyframeRequest, Rekey};
pub use state::{Snapshot, Subscribe, Unsubscribe, Update};
pub use stream::{AcceptStream, AudioParams, CloseStream, OpenStream, StreamParams, VideoParams};
pub use stream_ref::{MuxClass, StreamRef};
