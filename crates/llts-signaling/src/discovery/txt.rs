use data_encoding::HEXLOWER_PERMISSIVE;

use crate::discovery::{ADVERTISEMENT_LEN, Beacon, DiscoveryError};

/// DNS-SD service type browsed and registered on the local network.
pub const SERVICE_TYPE: &str = "_llts._udp";

const BEACON_KEY: &str = "b";
const PORT_KEY: &str = "p";

/// The mDNS half of discovery: the same beacon bytes BLE carries, hex-encoded, plus the llts UDP
/// port.
///
/// Sharing one payload means the two planes cannot drift, and the ephemeral identifier appearing
/// in both is what lets one physical device be recognised as one peer across them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxtRecord {
    pub beacon: Beacon,
    pub port: u16,
}

impl TxtRecord {
    pub fn to_pairs(&self) -> [(&'static str, String); 2] {
        [
            (BEACON_KEY, self.beacon.to_hex()),
            (PORT_KEY, self.port.to_string()),
        ]
    }

    /// # Errors
    ///
    /// Returns a [`DiscoveryError`] for a missing key, a malformed value or an unreadable beacon.
    pub fn parse<'a>(lookup: impl Fn(&str) -> Option<&'a str>) -> Result<Self, DiscoveryError> {
        let hex = lookup(BEACON_KEY).ok_or(DiscoveryError::MissingTxtKey(BEACON_KEY))?;
        let mut buffer = [0; ADVERTISEMENT_LEN];
        let beacon = decode_hex(hex, &mut buffer)
            .ok_or(DiscoveryError::MalformedTxtValue { key: BEACON_KEY })?;
        let port = lookup(PORT_KEY)
            .ok_or(DiscoveryError::MissingTxtKey(PORT_KEY))?
            .parse()
            .map_err(|_| DiscoveryError::MalformedTxtValue { key: PORT_KEY })?;
        Ok(Self {
            beacon: Beacon::parse(beacon)?,
            port,
        })
    }
}

fn decode_hex<'b>(hex: &str, buffer: &'b mut [u8]) -> Option<&'b [u8]> {
    let len = HEXLOWER_PERMISSIVE.decode_len(hex.len()).ok()?;
    let decoded = buffer.get_mut(..len)?;
    HEXLOWER_PERMISSIVE
        .decode_mut(hex.as_bytes(), decoded)
        .ok()?;
    Some(decoded)
}
