use data_encoding::{HEXLOWER, HEXLOWER_PERMISSIVE};
use llts_signaling::device::DeviceId;

use crate::error::BridgeError;

/// Devices cross the JNI boundary as the full hex of their static key, the one name that is
/// both unique and stable.
pub(crate) fn device_hex(id: &DeviceId) -> String {
    HEXLOWER.encode(&id.0)
}

pub(crate) fn parse_device_hex(text: &str) -> Result<DeviceId, BridgeError> {
    let mut key = [0u8; 32];
    let decoded = HEXLOWER_PERMISSIVE
        .decode_len(text.len())
        .ok()
        .filter(|len| *len == key.len())
        .and_then(|_| {
            HEXLOWER_PERMISSIVE
                .decode_mut(text.as_bytes(), &mut key)
                .ok()
        });
    decoded.map(|_| DeviceId(key)).ok_or(BridgeError::DeviceId)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rejects_short_or_foreign_text() {
        let id = DeviceId([0xab; 32]);
        let hex = device_hex(&id);
        assert_eq!(hex.len(), 64);
        assert_eq!(parse_device_hex(&hex).ok(), Some(id));
        assert_eq!(parse_device_hex(&hex.to_uppercase()).ok(), Some(id));
        assert!(parse_device_hex("abcd").is_err());
        assert!(parse_device_hex(&"zz".repeat(32)).is_err());
    }
}
