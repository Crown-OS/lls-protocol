use llts_core::stream::StreamKey;
use llts_signaling::message::{Lane, MuxClass, StreamRef};
use llts_wire::StreamClass;

/// Signalling and the core number the multiplexing classes identically.
pub(crate) fn stream_key(stream: StreamRef) -> Option<StreamKey> {
    StreamClass::from_byte(stream.class.as_byte()).map(|class| StreamKey::new(class, stream.index))
}

pub(crate) fn stream_ref(key: StreamKey) -> Option<StreamRef> {
    MuxClass::from_byte(key.class.as_byte()).map(|class| StreamRef {
        class,
        index: key.index,
    })
}

pub(crate) const CONTROL: StreamRef = StreamRef {
    class: MuxClass::Control,
    index: 0,
};

/// The reliable stream state documents travel on, so a large clipboard never queues ahead of
/// session control.
pub(crate) const STATE: StreamRef = StreamRef {
    class: MuxClass::Data,
    index: 0,
};

/// Pages, pushes and answers of application messages.
pub(crate) const BULK: StreamRef = Lane::Bulk.stream();

/// Blobs, and the messages that must stay ordered with them.
pub(crate) const BLOB: StreamRef = Lane::Blob.stream();
