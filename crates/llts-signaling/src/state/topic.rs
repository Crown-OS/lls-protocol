use serde::{Deserialize, Serialize};

/// One document a device publishes about itself.
///
/// Topics are only ever appended: the variant index is what postcard puts on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Topic {
    DeviceInfo,
    Battery,
    MediaSession,
    CallState,
    Notifications,
    Clipboard,
    Volume,
    Hotspot,
}

impl Topic {
    pub const COUNT: usize = Self::ALL.len();

    pub const ALL: [Self; 8] = [
        Self::DeviceInfo,
        Self::Battery,
        Self::MediaSession,
        Self::CallState,
        Self::Notifications,
        Self::Clipboard,
        Self::Volume,
        Self::Hotspot,
    ];

    pub(crate) const fn index(self) -> usize {
        self as usize
    }

    const fn bit(self) -> u16 {
        1 << self as u8
    }
}

/// A set of [`Topic`]s packed into one word, as a subscription carries them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TopicSet(u16);

impl TopicSet {
    pub const EMPTY: Self = Self(0);
    pub const ALL: Self = Self((1 << Topic::COUNT) - 1);

    #[must_use]
    pub const fn contains(self, topic: Topic) -> bool {
        self.0 & topic.bit() != 0
    }

    #[must_use]
    pub const fn with(self, topic: Topic) -> Self {
        Self(self.0 | topic.bit())
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    #[must_use]
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Topic> {
        Topic::ALL
            .into_iter()
            .filter(move |topic| self.contains(*topic))
    }
}

impl From<Topic> for TopicSet {
    fn from(topic: Topic) -> Self {
        Self::EMPTY.with(topic)
    }
}

impl FromIterator<Topic> for TopicSet {
    fn from_iter<I: IntoIterator<Item = Topic>>(topics: I) -> Self {
        topics.into_iter().fold(Self::EMPTY, Self::with)
    }
}
