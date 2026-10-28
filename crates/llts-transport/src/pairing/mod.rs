//! Everything around the pairing handshakes: offers, the window they are accepted in, the rate
//! they are accepted at, and where the result is kept.

mod qr;
mod rate_limit;
mod trust_store;
mod window;

pub use qr::QrOffer;
pub use rate_limit::HandshakeRateLimiter;
pub use trust_store::{FileTrustStore, MemoryTrustStore, TrustStore, TrustedPeer};
pub use window::{PairingMode, PairingWindow};
