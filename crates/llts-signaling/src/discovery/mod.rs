//! Private discovery: finding linked devices nearby without telling anyone else who is here.
//!
//! A linked device advertises an [`Advertisement`] whose [`EphemeralId`] is a keyed BLAKE3 of the
//! current [`Epoch`] under a per-link [`DiscoverySecret`]. Only a device holding that secret can
//! resolve it, and it changes every fifteen minutes. A device waiting to be paired advertises a
//! [`PairingBeacon`] instead, which carries no identity at all. BLE service data and the mDNS
//! [`TxtRecord`] carry byte-identical beacons, so one device seen over both is one peer.
//! These are codecs only; the radios and the mDNS responder live in the daemon.

mod advertisement;
mod beacon;
mod discovery_secret;
mod ephemeral_id;
mod epoch;
mod error;
mod pairing_beacon;
mod resolver;
mod txt;

pub use advertisement::{ADVERTISEMENT_LEN, Advertisement};
pub use beacon::{BEACON_VERSION, Beacon, SERVICE_UUID_16};
pub use discovery_secret::DiscoverySecret;
pub use ephemeral_id::EphemeralId;
pub use epoch::{EPOCH_SECONDS, Epoch};
pub use error::DiscoveryError;
pub use pairing_beacon::{PAIRING_BEACON_LEN, PairingBeacon, PairingFlags, PairingNonce};
pub use resolver::PeerResolver;
pub use txt::{SERVICE_TYPE, TxtRecord};
