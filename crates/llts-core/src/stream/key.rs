use llts_wire::{SessionTag, StreamClass, StreamId};

/// A stream as both peers name it: class and index, without either side's session tag.
///
/// The wire identifier differs by direction — each side stamps the tag its peer chose — so the
/// application and the session table speak in keys and convert at the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StreamKey {
    pub class: StreamClass,
    pub index: u8,
}

impl StreamKey {
    /// The session's own reliable channel, always open.
    pub const CONTROL: Self = Self::new(StreamClass::Control, 0);

    pub const fn new(class: StreamClass, index: u8) -> Self {
        Self { class, index }
    }

    pub const fn with_tag(self, session_tag: SessionTag) -> StreamId {
        StreamId::compose(session_tag, self.class, self.index)
    }

    /// `None` when the identifier names a class this build does not know.
    pub const fn from_stream_id(stream: StreamId) -> Option<Self> {
        match stream.class() {
            Some(class) => Some(Self::new(class, stream.index())),
            None => None,
        }
    }
}
