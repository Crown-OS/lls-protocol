use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use llts_signaling::device::{DeviceId, FeatureSet};

use crate::error::BridgeError;

/// Which features the user allows each paired device, the one thing the node cannot keep
/// across restarts itself; the trust store holds the rest.
#[derive(Debug)]
pub(crate) struct PeerFeatureStore {
    path: PathBuf,
    entries: Vec<(DeviceId, FeatureSet)>,
}

impl PeerFeatureStore {
    pub(crate) fn open(path: impl Into<PathBuf>) -> Result<Self, BridgeError> {
        let path = path.into();
        let entries = match fs::read(&path) {
            Ok(bytes) => postcard::from_bytes(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, entries })
    }

    /// The store at `path`, or the one at `legacy` from before [`FeatureSet::GRANTS`] existed,
    /// with every grant given to each device, as the old build gave them implicitly.
    pub(crate) fn open_or_upgrade(
        path: impl Into<PathBuf>,
        legacy: &Path,
    ) -> Result<Self, BridgeError> {
        let path = path.into();
        if path.exists() || !legacy.exists() {
            return Self::open(path);
        }
        let mut store = Self::open(legacy)?;
        store.path = path;
        for (_, enabled) in &mut store.entries {
            *enabled = enabled.union(FeatureSet::GRANTS);
        }
        store.save()?;
        Ok(store)
    }

    pub(crate) fn enabled(&self, peer: &DeviceId) -> Option<FeatureSet> {
        self.entries
            .iter()
            .find(|(id, _)| id == peer)
            .map(|(_, enabled)| *enabled)
    }

    pub(crate) fn set(&mut self, peer: DeviceId, enabled: FeatureSet) -> Result<(), BridgeError> {
        match self.entries.iter_mut().find(|(id, _)| *id == peer) {
            Some(entry) => entry.1 = enabled,
            None => self.entries.push((peer, enabled)),
        }
        self.save()
    }

    pub(crate) fn remove(&mut self, peer: &DeviceId) -> Result<(), BridgeError> {
        self.entries.retain(|(id, _)| id != peer);
        self.save()
    }

    fn save(&self) -> Result<(), BridgeError> {
        let bytes = postcard::to_allocvec(&self.entries)?;
        let temporary = self.path.with_extension("tmp");
        write_synced(&temporary, &bytes)?;
        fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use llts_signaling::device::Feature;

    use super::*;

    #[test]
    fn survives_a_reopen_and_forgets_on_request() {
        let directory = std::env::temp_dir().join(format!("llts-android-{}", std::process::id()));
        let _ = fs::create_dir_all(&directory);
        let path = directory.join("peer_features.bin");
        let _ = fs::remove_file(&path);
        let peer = DeviceId([3; 32]);
        let enabled = FeatureSet::EMPTY.with(Feature::Battery);

        let stored = PeerFeatureStore::open(&path).and_then(|mut store| store.set(peer, enabled));
        assert!(stored.is_ok());
        let reopened = PeerFeatureStore::open(&path).ok();
        assert_eq!(
            reopened.as_ref().and_then(|store| store.enabled(&peer)),
            Some(enabled)
        );

        let removed = reopened.map(|mut store| store.remove(&peer).is_ok());
        assert_eq!(removed, Some(true));
        let after = PeerFeatureStore::open(&path).ok();
        assert_eq!(after.and_then(|store| store.enabled(&peer)), None);
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_store_from_before_the_grants_gives_every_device_the_grants() {
        let directory =
            std::env::temp_dir().join(format!("llts-android-grants-{}", std::process::id()));
        let _ = fs::create_dir_all(&directory);
        let legacy = directory.join("peer_features.bin");
        let path = directory.join("peer_features-v2.bin");
        let _ = fs::remove_file(&path);
        let peer = DeviceId([5; 32]);
        let enabled = FeatureSet::EMPTY.with(Feature::Battery);
        let stored = PeerFeatureStore::open(&legacy).and_then(|mut store| store.set(peer, enabled));
        assert!(stored.is_ok());

        let upgraded = PeerFeatureStore::open_or_upgrade(&path, &legacy).ok();
        assert_eq!(
            upgraded.and_then(|store| store.enabled(&peer)),
            Some(enabled.union(FeatureSet::GRANTS))
        );
        let reopened = PeerFeatureStore::open(&path).ok();
        assert_eq!(
            reopened.and_then(|store| store.enabled(&peer)),
            Some(enabled.union(FeatureSet::GRANTS))
        );
        let _ = fs::remove_dir_all(&directory);
    }
}
