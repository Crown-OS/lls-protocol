use std::net::SocketAddr;

use data_encoding::BASE32_NOPAD;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::TransportError;
use crate::security::{PresharedKey, StaticKey};

const SCHEME: &str = "llts1:";
const OFFER_VERSION: u8 = 1;
const MAX_ADDRESSES: usize = 4;
const MAX_NAME_LEN: usize = 64;

/// Everything a scanning device needs to pair without comparing codes.
///
/// Encoded as `llts1:` followed by unpadded base32 of the postcard body. Base32's uppercase
/// alphabet fits the QR alphanumeric mode, which packs about 20 % denser than bytes, and keeps
/// the code small enough to scan quickly off a laptop screen.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct QrOffer {
    version: u8,
    #[zeroize(skip)]
    pub static_key: StaticKey,
    psk: [u8; 32],
    #[zeroize(skip)]
    pub addresses: Vec<SocketAddr>,
    /// Unix seconds after which the offer must be refused.
    pub expires_at_unix: u64,
    #[zeroize(skip)]
    pub name: String,
}

impl QrOffer {
    pub fn new(
        static_key: StaticKey,
        psk: &PresharedKey,
        addresses: Vec<SocketAddr>,
        expires_at_unix: u64,
        name: impl Into<String>,
    ) -> Self {
        Self {
            version: OFFER_VERSION,
            static_key,
            psk: psk.0,
            addresses,
            expires_at_unix,
            name: name.into(),
        }
    }

    pub const fn psk(&self) -> PresharedKey {
        PresharedKey(self.psk)
    }

    pub const fn is_expired(&self, now_unix: u64) -> bool {
        now_unix >= self.expires_at_unix
    }

    pub fn to_uri(&self) -> Result<String, TransportError> {
        let body = zeroize::Zeroizing::new(postcard::to_allocvec(self)?);
        let mut uri = String::with_capacity(SCHEME.len() + BASE32_NOPAD.encode_len(body.len()));
        uri.push_str(SCHEME);
        BASE32_NOPAD.encode_append(&body, &mut uri);
        Ok(uri)
    }

    /// Parses a scanned code, refusing anything this build did not produce or that has expired.
    pub fn parse(uri: &str, now_unix: u64) -> Result<Self, TransportError> {
        let encoded = uri
            .strip_prefix(SCHEME)
            .ok_or(TransportError::InvalidPairingOffer("missing llts1: prefix"))?;
        let body = zeroize::Zeroizing::new(
            BASE32_NOPAD
                .decode(encoded.as_bytes())
                .map_err(|_| TransportError::InvalidPairingOffer("not base32"))?,
        );
        let offer: Self = postcard::from_bytes(&body)?;
        if offer.version != OFFER_VERSION {
            return Err(TransportError::InvalidPairingOffer("unknown version"));
        }
        if offer.addresses.is_empty() || offer.addresses.len() > MAX_ADDRESSES {
            return Err(TransportError::InvalidPairingOffer("bad address list"));
        }
        if offer.name.len() > MAX_NAME_LEN {
            return Err(TransportError::InvalidPairingOffer("name too long"));
        }
        if offer.is_expired(now_unix) {
            return Err(TransportError::PairingOfferExpired);
        }
        Ok(offer)
    }
}

impl core::fmt::Debug for QrOffer {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("QrOffer")
            .field("static_key", &self.static_key)
            .field("addresses", &self.addresses)
            .field("expires_at_unix", &self.expires_at_unix)
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}
