use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::TransportError;
use crate::security::{DeviceClass, DeviceIdentity, StaticKey};
use crate::util::tai64n::Tai64N;

/// A device this one has paired with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub static_key: StaticKey,
    pub name: String,
    pub class: DeviceClass,
    /// The newest reconnect timestamp accepted from this peer; anything not newer is a replay.
    pub last_handshake: Option<Tai64N>,
    pub paired_at_unix: u64,
}

/// Where this device keeps its identity and the peers it trusts.
pub trait TrustStore {
    fn identity(&self) -> Option<DeviceIdentity>;

    fn set_identity(&mut self, identity: &DeviceIdentity) -> Result<(), TransportError>;

    fn peer(&self, key: &StaticKey) -> Option<&TrustedPeer>;

    fn peers(&self) -> &[TrustedPeer];

    fn upsert(&mut self, peer: TrustedPeer) -> Result<(), TransportError>;

    fn remove(&mut self, key: &StaticKey) -> Result<bool, TransportError>;

    /// Records a reconnect timestamp once its session has proven itself. Never moves backwards.
    fn record_handshake(
        &mut self,
        key: &StaticKey,
        timestamp: Tai64N,
    ) -> Result<(), TransportError>;

    /// Whether a reconnect from `key` stamped `timestamp` is from a trusted peer and newer than
    /// anything accepted from it before.
    fn admits(&self, key: &StaticKey, timestamp: Tai64N) -> bool {
        self.peer(key)
            .is_some_and(|peer| peer.last_handshake.is_none_or(|last| timestamp > last))
    }
}

#[derive(Serialize, Deserialize)]
struct StoreContents {
    version: u8,
    identity: Option<([u8; StaticKey::LEN], StaticKey)>,
    peers: Vec<TrustedPeer>,
}

impl Drop for StoreContents {
    fn drop(&mut self) {
        if let Some((private, _)) = self.identity.as_mut() {
            private.zeroize();
        }
    }
}

impl StoreContents {
    const VERSION: u8 = 1;

    const fn empty() -> Self {
        Self {
            version: Self::VERSION,
            identity: None,
            peers: Vec::new(),
        }
    }

    fn identity(&self) -> Option<DeviceIdentity> {
        self.identity
            .as_ref()
            .map(|(private, public)| DeviceIdentity::from_parts(*private, *public))
    }

    fn upsert(&mut self, peer: TrustedPeer) {
        match self
            .peers
            .iter_mut()
            .find(|held| held.static_key == peer.static_key)
        {
            Some(held) => *held = peer,
            None => self.peers.push(peer),
        }
    }

    fn remove(&mut self, key: &StaticKey) -> bool {
        let before = self.peers.len();
        self.peers.retain(|peer| peer.static_key != *key);
        before != self.peers.len()
    }

    fn record_handshake(
        &mut self,
        key: &StaticKey,
        timestamp: Tai64N,
    ) -> Result<(), TransportError> {
        let peer = self
            .peers
            .iter_mut()
            .find(|peer| peer.static_key == *key)
            .ok_or(TransportError::UnknownPeer)?;
        if peer.last_handshake.is_none_or(|last| timestamp > last) {
            peer.last_handshake = Some(timestamp);
        }
        Ok(())
    }
}

/// An in-memory store, for tests and for devices that must not persist anything.
pub struct MemoryTrustStore {
    contents: StoreContents,
}

impl Default for MemoryTrustStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryTrustStore {
    pub const fn new() -> Self {
        Self {
            contents: StoreContents::empty(),
        }
    }
}

impl core::fmt::Debug for MemoryTrustStore {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("MemoryTrustStore")
            .field("peers", &self.contents.peers.len())
            .finish_non_exhaustive()
    }
}

impl TrustStore for MemoryTrustStore {
    fn identity(&self) -> Option<DeviceIdentity> {
        self.contents.identity()
    }

    fn set_identity(&mut self, identity: &DeviceIdentity) -> Result<(), TransportError> {
        self.contents.identity = Some((*identity.private(), *identity.public()));
        Ok(())
    }

    fn peer(&self, key: &StaticKey) -> Option<&TrustedPeer> {
        self.contents
            .peers
            .iter()
            .find(|peer| peer.static_key == *key)
    }

    fn peers(&self) -> &[TrustedPeer] {
        &self.contents.peers
    }

    fn upsert(&mut self, peer: TrustedPeer) -> Result<(), TransportError> {
        self.contents.upsert(peer);
        Ok(())
    }

    fn remove(&mut self, key: &StaticKey) -> Result<bool, TransportError> {
        Ok(self.contents.remove(key))
    }

    fn record_handshake(
        &mut self,
        key: &StaticKey,
        timestamp: Tai64N,
    ) -> Result<(), TransportError> {
        self.contents.record_handshake(key, timestamp)
    }
}

/// A store persisted as one postcard file, readable only by its owner.
///
/// Every change is written to a sibling temporary file, synced, and renamed over the original,
/// so a crash mid-write leaves either the old store or the new one — never half of each. The
/// serialised bytes, which hold the private key, are zeroed as soon as they are written.
pub struct FileTrustStore {
    path: PathBuf,
    contents: StoreContents,
}

impl FileTrustStore {
    /// Opens the store at `path`, starting empty if the file does not exist yet.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, TransportError> {
        let path = path.into();
        let contents = match fs::read(&path) {
            Ok(bytes) => {
                let bytes = Zeroizing::new(bytes);
                postcard::from_bytes(&bytes)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoreContents::empty(),
            Err(source) => {
                return Err(TransportError::TrustStore {
                    path,
                    action: "read",
                    source,
                });
            }
        };
        Ok(Self { path, contents })
    }

    /// The identity on file, generating and saving one on first use.
    pub fn identity_or_generate(&mut self) -> Result<DeviceIdentity, TransportError> {
        if let Some(identity) = self.contents.identity() {
            return Ok(identity);
        }
        let identity = DeviceIdentity::generate()?;
        self.set_identity(&identity)?;
        Ok(identity)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn save(&self) -> Result<(), TransportError> {
        let bytes = Zeroizing::new(postcard::to_allocvec(&self.contents)?);
        let temporary = self.path.with_extension("tmp");
        let fail = |action, source| TransportError::TrustStore {
            path: self.path.clone(),
            action,
            source,
        };
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| fail("created", source))?;
        }
        let mut file = owner_only(&temporary).map_err(|source| fail("written", source))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| fail("written", source))?;
        fs::rename(&temporary, &self.path).map_err(|source| fail("replaced", source))
    }
}

fn owner_only(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)
}

impl core::fmt::Debug for FileTrustStore {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("FileTrustStore")
            .field("path", &self.path)
            .field("peers", &self.contents.peers.len())
            .finish_non_exhaustive()
    }
}

impl TrustStore for FileTrustStore {
    fn identity(&self) -> Option<DeviceIdentity> {
        self.contents.identity()
    }

    fn set_identity(&mut self, identity: &DeviceIdentity) -> Result<(), TransportError> {
        self.contents.identity = Some((*identity.private(), *identity.public()));
        self.save()
    }

    fn peer(&self, key: &StaticKey) -> Option<&TrustedPeer> {
        self.contents
            .peers
            .iter()
            .find(|peer| peer.static_key == *key)
    }

    fn peers(&self) -> &[TrustedPeer] {
        &self.contents.peers
    }

    fn upsert(&mut self, peer: TrustedPeer) -> Result<(), TransportError> {
        self.contents.upsert(peer);
        self.save()
    }

    fn remove(&mut self, key: &StaticKey) -> Result<bool, TransportError> {
        let removed = self.contents.remove(key);
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    fn record_handshake(
        &mut self,
        key: &StaticKey,
        timestamp: Tai64N,
    ) -> Result<(), TransportError> {
        self.contents.record_handshake(key, timestamp)?;
        self.save()
    }
}
