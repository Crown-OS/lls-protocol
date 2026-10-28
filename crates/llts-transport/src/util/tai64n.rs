use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Seconds between the TAI64 label origin and the Unix epoch, plus the leap seconds TAI ran ahead
/// by in 1972, as WireGuard labels its handshake timestamps.
const TAI64_UNIX_OFFSET: u64 = (1 << 62) + 10;

/// A TAI64N label: 64-bit seconds and 32-bit nanoseconds, big-endian, so byte order is time order.
///
/// Carried inside the first reconnect message, where the responder requires it to rise for each
/// peer. A captured first message then replays as stale, without the responder keeping any state
/// beyond one timestamp per peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Tai64N([u8; Tai64N::LEN]);

impl Tai64N {
    pub const LEN: usize = 12;

    pub fn from_system_time(time: SystemTime) -> Self {
        let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        let seconds = TAI64_UNIX_OFFSET.saturating_add(since_epoch.as_secs());
        let mut bytes = [0u8; Self::LEN];
        let (seconds_slot, nanos_slot) = bytes.split_at_mut(8);
        seconds_slot.copy_from_slice(&seconds.to_be_bytes());
        nanos_slot.copy_from_slice(&since_epoch.subsec_nanos().to_be_bytes());
        Self(bytes)
    }

    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    pub const fn from_bytes(bytes: [u8; Self::LEN]) -> Self {
        Self(bytes)
    }

    pub const fn to_bytes(self) -> [u8; Self::LEN] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::Tai64N;

    #[test]
    fn later_instants_order_later_bytewise() {
        let earlier =
            Tai64N::from_system_time(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123));
        let later = Tai64N::from_system_time(UNIX_EPOCH + Duration::from_millis(1_700_000_000_124));
        assert!(earlier < later);
        assert!(earlier.to_bytes() < later.to_bytes());
    }
}
