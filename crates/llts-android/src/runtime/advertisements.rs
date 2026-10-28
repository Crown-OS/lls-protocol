use std::sync::{Arc, Mutex, PoisonError};

use llts_signaling::discovery::Beacon;

/// The beacons Kotlin should be advertising, kept encoded for `nativeAdvertisements`.
///
/// The encoding is each beacon's bytes behind a one-byte length, in the node's order, so the
/// BLE advertiser can split them without knowing the beacon formats.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedAdvertisements(Arc<Mutex<Vec<u8>>>);

impl SharedAdvertisements {
    pub(crate) fn replace(&self, beacons: &[Beacon]) {
        let mut encoded = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        encoded.clear();
        for beacon in beacons {
            match beacon {
                Beacon::Linked(advertisement) => {
                    push_framed(&mut encoded, &advertisement.to_bytes())
                }
                Beacon::Pairing(pairing) => push_framed(&mut encoded, &pairing.to_bytes()),
            }
        }
    }

    pub(crate) fn encoded(&self) -> Vec<u8> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn push_framed(encoded: &mut Vec<u8>, beacon: &[u8]) {
    if let Ok(len) = u8::try_from(beacon.len()) {
        encoded.push(len);
        encoded.extend_from_slice(beacon);
    }
}

#[cfg(test)]
mod tests {
    use llts_signaling::device::DeviceClass;
    use llts_signaling::discovery::{PairingBeacon, PairingFlags, PairingNonce};

    use super::*;

    #[test]
    fn beacons_are_length_framed_in_order() {
        let shared = SharedAdvertisements::default();
        let pairing = PairingBeacon {
            device_class: DeviceClass::Phone,
            flags: PairingFlags::QR,
            nonce: PairingNonce([5; 8]),
        };
        shared.replace(&[Beacon::Pairing(pairing), Beacon::Pairing(pairing)]);
        let encoded = shared.encoded();
        let bytes = pairing.to_bytes();
        assert_eq!(encoded.len(), 2 * (1 + bytes.len()));
        assert_eq!(encoded.first().copied(), u8::try_from(bytes.len()).ok());
        assert_eq!(encoded.get(1..=bytes.len()), Some(bytes.as_slice()));
        shared.replace(&[]);
        assert!(shared.encoded().is_empty());
    }
}
