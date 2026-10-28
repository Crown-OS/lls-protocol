use llts_core::stream::StreamKey;
use llts_signaling::message::StreamRef;
use llts_wire::StreamClass;

/// Signalling and the core number the multiplexing classes identically.
pub(crate) fn stream_key(stream: StreamRef) -> Option<StreamKey> {
    StreamClass::from_byte(stream.class.as_byte()).map(|class| StreamKey::new(class, stream.index))
}
