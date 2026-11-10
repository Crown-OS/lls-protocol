use serde::{Deserialize, Serialize};

/// One capability a device can share with a peer, matching the `crownlink` IPC `Feature`.
///
/// The discriminant is the bit a [`FeatureSet`] uses, so variants are only ever appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum Feature {
    Mirror,
    Camera,
    Mic,
    Monitor,
    Unicursor,
    Calls,
    Clipboard,
    Notifications,
    Battery,
    Hotspot,
    Files,
    Messages,
    CallHistory,
    Contacts,
    Capture,
    DragDrop,
    Cast,
    AudioHandoff,
    Focus,
    LiveActivities,
    WifiSharing,
    FindMy,
    Links,
}

impl Feature {
    pub const ALL: [Self; 23] = [
        Self::Mirror,
        Self::Camera,
        Self::Mic,
        Self::Monitor,
        Self::Unicursor,
        Self::Calls,
        Self::Clipboard,
        Self::Notifications,
        Self::Battery,
        Self::Hotspot,
        Self::Files,
        Self::Messages,
        Self::CallHistory,
        Self::Contacts,
        Self::Capture,
        Self::DragDrop,
        Self::Cast,
        Self::AudioHandoff,
        Self::Focus,
        Self::LiveActivities,
        Self::WifiSharing,
        Self::FindMy,
        Self::Links,
    ];

    /// The feature whose [`FeatureSet`] bit is `index`, the number messages name features by.
    pub fn from_bit(index: u8) -> Option<Self> {
        Self::ALL.get(usize::from(index)).copied()
    }

    pub const fn bit_index(self) -> u8 {
        self as u8
    }

    const fn bit(self) -> u32 {
        1 << self as u8
    }
}

/// A set of [`Feature`]s packed into one word.
///
/// Bits this build does not know survive a round trip, so a newer peer's features are not lost
/// when an older device relays or echoes them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureSet(u32);

impl FeatureSet {
    pub const EMPTY: Self = Self(0);

    /// Every feature of [`Feature::ALL`].
    pub const EVERY: Self = Self((1 << Feature::ALL.len()) - 1);

    /// Features that expose personal data, credentials or location; never on by default.
    pub const PERSONAL: Self = Self::EMPTY
        .with(Feature::Messages)
        .with(Feature::CallHistory)
        .with(Feature::Contacts)
        .with(Feature::WifiSharing)
        .with(Feature::FindMy)
        .with(Feature::Links);

    /// What a newly paired device starts with.
    pub const DEFAULT: Self = Self::EVERY.difference(Self::PERSONAL);

    #[must_use]
    pub const fn contains(self, feature: Feature) -> bool {
        self.0 & feature.bit() != 0
    }

    #[must_use]
    pub const fn with(self, feature: Feature) -> Self {
        Self(self.0 | feature.bit())
    }

    #[must_use]
    pub const fn without(self, feature: Feature) -> Self {
        Self(self.0 & !feature.bit())
    }

    /// What both ends can run.
    #[must_use]
    pub const fn shared_with(self, peer: Self) -> Self {
        Self(self.0 & peer.0)
    }

    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Feature> {
        Feature::ALL
            .into_iter()
            .filter(move |feature| self.contains(*feature))
    }
}

impl FromIterator<Feature> for FeatureSet {
    fn from_iter<I: IntoIterator<Item = Feature>>(features: I) -> Self {
        features.into_iter().fold(Self::EMPTY, Self::with)
    }
}
