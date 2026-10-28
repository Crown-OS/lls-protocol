use serde::{Deserialize, Serialize};

use crate::TransportError;

/// What kind of device is on the other end, for the UI and for feature defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceClass {
    Laptop,
    Desktop,
    Phone,
    Tablet,
    Other,
}

/// The introduction each side sends inside the encrypted part of the handshake.
///
/// Carries the session tag the sender wants the peer to stamp on everything it sends back, so
/// the routing tag the clear handshake prefix announced is confirmed under the handshake's
/// authentication before any session uses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u8,
    pub capabilities: u64,
    pub session_tag: u16,
    pub name: String,
    pub class: DeviceClass,
}

impl Hello {
    pub const PROTOCOL_VERSION: u8 = 1;
    pub const MAX_NAME_LEN: usize = 64;
    pub const MAX_ENCODED_LEN: usize = 96;

    pub fn new(name: impl Into<String>, class: DeviceClass, capabilities: u64) -> Self {
        Self {
            protocol_version: Self::PROTOCOL_VERSION,
            capabilities,
            session_tag: 0,
            name: name.into(),
            class,
        }
    }

    #[must_use]
    pub const fn with_session_tag(mut self, session_tag: u16) -> Self {
        self.session_tag = session_tag;
        self
    }

    pub fn encode_into(&self, destination: &mut [u8]) -> Result<usize, TransportError> {
        if self.name.len() > Self::MAX_NAME_LEN {
            return Err(TransportError::HandshakeRejected {
                reason: "the device name is too long",
            });
        }
        Ok(postcard::to_slice(self, destination)?.len())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TransportError> {
        let hello: Self = postcard::from_bytes(bytes)?;
        if hello.name.len() > Self::MAX_NAME_LEN {
            return Err(TransportError::HandshakeRejected {
                reason: "the peer's device name is too long",
            });
        }
        if hello.protocol_version != Self::PROTOCOL_VERSION {
            return Err(TransportError::HandshakeRejected {
                reason: "the peer speaks another protocol version",
            });
        }
        if hello.session_tag == 0 {
            return Err(TransportError::HandshakeRejected {
                reason: "the peer offered no session tag",
            });
        }
        Ok(hello)
    }
}
