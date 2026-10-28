use crate::{SessionTag, StreamClass};

const SESSION_TAG_SHIFT: u32 = 16;
const CLASS_SHIFT: u32 = 8;

/// Identifies one stream inside a session: `session_tag:u16 ‖ class:u8 ‖ index:u8`.
///
/// The session tag routes the datagram to its peer, the class says how the stream is delivered,
/// and the index tells apart two streams of one class — a screen and a camera. Each direction
/// of each stream is sealed under its own key, so the whole identifier must stay unique per
/// direction for the lifetime of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId(u32);

impl StreamId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn compose(session_tag: SessionTag, class: StreamClass, index: u8) -> Self {
        Self(
            ((session_tag.get() as u32) << SESSION_TAG_SHIFT)
                | ((class.as_byte() as u32) << CLASS_SHIFT)
                | index as u32,
        )
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "the shift leaves exactly the sixteen tag bits"
    )]
    pub const fn session_tag(self) -> SessionTag {
        SessionTag::new((self.0 >> SESSION_TAG_SHIFT) as u16)
    }

    /// `None` for a class this build does not know, which a receiver treats as an unknown stream.
    pub const fn class(self) -> Option<StreamClass> {
        StreamClass::from_byte(self.class_byte())
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "the shift and mask leave exactly the eight class bits"
    )]
    pub const fn class_byte(self) -> u8 {
        (self.0 >> CLASS_SHIFT) as u8
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "the low eight bits are the index by definition"
    )]
    pub const fn index(self) -> u8 {
        self.0 as u8
    }

    /// The same stream as seen by a peer that assigned a different tag.
    #[must_use]
    pub const fn with_session_tag(self, session_tag: SessionTag) -> Self {
        Self(
            (self.0 & ((1 << SESSION_TAG_SHIFT) - 1))
                | ((session_tag.get() as u32) << SESSION_TAG_SHIFT),
        )
    }
}

impl From<u32> for StreamId {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::StreamId;
    use crate::{SessionTag, StreamClass};

    #[test]
    fn the_three_fields_survive_composition() {
        let stream = StreamId::compose(SessionTag::new(0xBEEF), StreamClass::Audio, 3);
        assert_eq!(stream.get(), 0xBEEF_1103);
        assert_eq!(stream.session_tag(), SessionTag::new(0xBEEF));
        assert_eq!(stream.class(), Some(StreamClass::Audio));
        assert_eq!(stream.index(), 3);
    }

    #[test]
    fn retagging_keeps_class_and_index() {
        let stream = StreamId::compose(SessionTag::new(1), StreamClass::Video, 2);
        let retagged = stream.with_session_tag(SessionTag::new(9));
        assert_eq!(
            retagged,
            StreamId::compose(SessionTag::new(9), StreamClass::Video, 2)
        );
    }

    #[test]
    fn an_unknown_class_is_reported_as_such() {
        assert_eq!(StreamId::new(0x0000_7F00).class(), None);
    }
}
