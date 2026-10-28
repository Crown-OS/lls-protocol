use std::collections::HashMap;

use crate::device::DeviceId;
use crate::discovery::{Advertisement, DiscoverySecret, EphemeralId, Epoch};

/// Turns an advertised identifier back into the linked peer that sent it.
///
/// Every candidate identifier for every link is precomputed once per epoch, so a scan result
/// resolves with one hash lookup however many devices are linked. A beacon from a device that is
/// not linked resolves to nothing and is indistinguishable from noise.
#[derive(Debug)]
pub struct PeerResolver {
    epoch: Epoch,
    tolerance: u8,
    table: HashMap<EphemeralId, DeviceId>,
}

impl PeerResolver {
    /// Tries the previous, current and next epoch, covering fifteen minutes of clock skew either
    /// way.
    pub const DEFAULT_TOLERANCE: u8 = 1;

    /// For a device whose clock is badly wrong, such as a laptop with a dead real-time clock
    /// immediately after boot.
    pub const SKEWED_TOLERANCE: u8 = 4;

    pub fn build<'s>(
        links: impl IntoIterator<Item = (DeviceId, &'s DiscoverySecret)>,
        epoch: Epoch,
        tolerance: u8,
    ) -> Self {
        let links = links.into_iter();
        let candidates = usize::from(tolerance) * 2 + 1;
        let mut table = HashMap::with_capacity(links.size_hint().0 * candidates);
        for (device, secret) in links {
            table.extend(
                epoch
                    .window(tolerance)
                    .map(|candidate| (EphemeralId::derive(secret, candidate), device)),
            );
        }
        Self {
            epoch,
            tolerance,
            table,
        }
    }

    pub fn resolve(&self, advertisement: &Advertisement) -> Option<DeviceId> {
        self.table.get(&advertisement.ephemeral_id).copied()
    }

    pub const fn epoch(&self) -> Epoch {
        self.epoch
    }

    pub const fn tolerance(&self) -> u8 {
        self.tolerance
    }

    /// Whether the resolver was built for `now`, or needs rebuilding for a new epoch.
    pub const fn is_current_for(&self, now: Epoch) -> bool {
        self.epoch.as_u64() == now.as_u64()
    }

    /// Whether a beacon's epoch byte is one this resolver already tried.
    ///
    /// A beacon that fails to resolve *and* reports an epoch byte outside the window is evidence
    /// of clock skew rather than an unknown device, and the cue to rebuild with
    /// [`SKEWED_TOLERANCE`](Self::SKEWED_TOLERANCE).
    pub fn covers_epoch_byte(&self, epoch_low: u8) -> bool {
        self.epoch
            .window(self.tolerance)
            .any(|epoch| epoch.low_byte() == epoch_low)
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}
