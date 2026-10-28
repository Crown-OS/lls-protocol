use serde::{Deserialize, Serialize};

/// One capability a device can share with a peer, matching the `crownconnect` IPC `Feature`.
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
}

impl Feature {
    pub const ALL: [Self; 11] = [
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
    ];

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
