use std::fs::{self, File};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use llts_node::FfspLinkExport;
use llts_signaling::device::{DeviceClass, DeviceId};
use llts_transport::security::DeviceIdentity;
use zeroize::Zeroizing;

use crate::error::BridgeError;

/// ffsp's context for link secrets; both ends derive the same value from the llts one.
const LINK_SECRET_CONTEXT: &str = "crownos.ffsp.v1 link secret";
const DEVICE_ID_CONTEXT: &str = "crownos.ffsp.v1.device-id";

const KEY_LEN: usize = 32;
const HEADER_LEN: usize = KEY_LEN * 3 + 2 + 1 + 2;
/// Wi-Fi infrastructure and BLE, the mediums a paired device is assumed to reach.
const DEFAULT_CAPABILITIES: u16 = (1 << 0) | (1 << 2);

const FFSP_LAPTOP: u8 = 1;
const FFSP_PHONE: u8 = 3;
const FFSP_TABLET: u8 = 4;

/// ffsp's link store and identity, written the way `ffsp-android` reads them, so a device paired
/// over llts is one ffsp already trusts and file transfers need no second pairing.
#[derive(Debug)]
pub(crate) struct FfspLinks {
    links: PathBuf,
}

struct StoredLink {
    device: [u8; KEY_LEN],
    public_key: [u8; KEY_LEN],
    secret: Zeroizing<[u8; KEY_LEN]>,
    capabilities: u16,
    class: u8,
    name: String,
}

impl FfspLinks {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            links: directory.join("links"),
        }
    }

    /// Makes ffsp know this device by its llts key, which is what the paired computer names it
    /// by in its own ffsp store.
    pub(crate) fn adopt_identity(&self, identity: &DeviceIdentity) -> Result<(), BridgeError> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(KEY_LEN * 2));
        bytes.extend_from_slice(identity.private());
        bytes.extend_from_slice(identity.public().as_bytes());
        let path = self.links.with_extension("identity");
        if fs::read(&path)
            .is_ok_and(|existing| Zeroizing::new(existing).as_slice() == bytes.as_slice())
        {
            return Ok(());
        }
        Ok(write_atomically(&path, &bytes)?)
    }

    pub(crate) fn record(&self, export: &FfspLinkExport) -> Result<(), BridgeError> {
        let device = device_id(&export.peer.0);
        let mut links = self.load()?;
        links.retain(|stored| stored.device != device);
        links.push(StoredLink {
            device,
            public_key: export.peer.0,
            secret: Zeroizing::new(blake3::derive_key(
                LINK_SECRET_CONTEXT,
                export.link_secret.as_bytes(),
            )),
            capabilities: DEFAULT_CAPABILITIES,
            class: ffsp_class(export.class),
            name: export.name.clone(),
        });
        Ok(write_atomically(&self.links, &encode(&links))?)
    }

    pub(crate) fn forget(&self, peer: &DeviceId) -> Result<(), BridgeError> {
        let device = device_id(&peer.0);
        let mut links = self.load()?;
        let before = links.len();
        links.retain(|stored| stored.device != device);
        if links.len() == before {
            return Ok(());
        }
        Ok(write_atomically(&self.links, &encode(&links))?)
    }

    fn load(&self) -> Result<Vec<StoredLink>, BridgeError> {
        match fs::read(&self.links) {
            Ok(bytes) => Ok(decode(&Zeroizing::new(bytes))?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(error.into()),
        }
    }
}

/// ffsp names devices by a hash of their public key rather than the key itself.
fn device_id(public_key: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    blake3::derive_key(DEVICE_ID_CONTEXT, public_key)
}

const fn ffsp_class(class: DeviceClass) -> u8 {
    match class {
        DeviceClass::Computer => FFSP_LAPTOP,
        DeviceClass::Tablet => FFSP_TABLET,
        DeviceClass::Phone | DeviceClass::Watch => FFSP_PHONE,
    }
}

fn encode(links: &[StoredLink]) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(links.len() * (HEADER_LEN + KEY_LEN)));
    for link in links {
        let name = link.name.as_bytes();
        let name = name.get(..usize::from(u16::MAX)).unwrap_or(name);
        out.extend_from_slice(&link.device);
        out.extend_from_slice(&link.public_key);
        out.extend_from_slice(&*link.secret);
        out.extend_from_slice(&link.capabilities.to_le_bytes());
        out.push(link.class);
        out.extend_from_slice(&u16::try_from(name.len()).unwrap_or(u16::MAX).to_le_bytes());
        out.extend_from_slice(name);
    }
    out
}

fn decode(mut bytes: &[u8]) -> io::Result<Vec<StoredLink>> {
    let corrupt = || io::Error::other("the ffsp link store is corrupt");
    let mut links = Vec::new();
    while !bytes.is_empty() {
        let (header, rest) = bytes.split_at_checked(HEADER_LEN).ok_or_else(corrupt)?;
        let key = |at: usize| -> io::Result<[u8; KEY_LEN]> {
            header
                .get(at..at + KEY_LEN)
                .and_then(|slice| <[u8; KEY_LEN]>::try_from(slice).ok())
                .ok_or_else(corrupt)
        };
        let &[.., cap_low, cap_high, class, len_low, len_high] = header else {
            return Err(corrupt());
        };
        let name_len = usize::from(u16::from_le_bytes([len_low, len_high]));
        let (name, rest) = rest.split_at_checked(name_len).ok_or_else(corrupt)?;
        links.push(StoredLink {
            device: key(0)?,
            public_key: key(KEY_LEN)?,
            secret: Zeroizing::new(key(KEY_LEN * 2)?),
            capabilities: u16::from_le_bytes([cap_low, cap_high]),
            class,
            name: String::from_utf8(name.to_vec()).map_err(|_| corrupt())?,
        });
        bytes = rest;
    }
    Ok(links)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use llts_node::LinkSecret;
    use llts_transport::security::StaticKey;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("llts-android-ffsp-{}-{name}", std::process::id()))
    }

    fn export(key: u8, name: &str) -> Option<FfspLinkExport> {
        let identity = DeviceIdentity::generate().ok()?;
        let peer = DeviceId([key; KEY_LEN]);
        Some(FfspLinkExport {
            peer,
            name: name.to_owned(),
            class: DeviceClass::Computer,
            link_secret: LinkSecret::derive(&identity, &peer),
        })
    }

    #[test]
    fn links_are_upserted_forgotten_and_named_by_key_hash() -> Result<(), BridgeError> {
        let directory = scratch("links");
        let links = FfspLinks::new(&directory);
        let (Some(first), Some(second), Some(renamed)) = (
            export(1, "Laptop"),
            export(2, "Desk"),
            export(1, "CrownBook"),
        ) else {
            return Ok(());
        };
        links.record(&first)?;
        links.record(&second)?;
        links.record(&renamed)?;
        let stored = links.load()?;
        let names: Vec<&str> = stored.iter().map(|link| link.name.as_str()).collect();
        assert_eq!(names, ["Desk", "CrownBook"]);
        assert_eq!(
            stored.last().map(|link| link.device),
            Some(device_id(&[1; KEY_LEN]))
        );
        assert_eq!(stored.last().map(|link| link.class), Some(FFSP_LAPTOP));
        links.forget(&DeviceId([2; KEY_LEN]))?;
        assert_eq!(links.load()?.len(), 1);
        let _ = fs::remove_dir_all(&directory);
        Ok(())
    }

    #[test]
    fn the_identity_file_holds_the_llts_keys() -> Result<(), BridgeError> {
        let directory = scratch("identity");
        let identity = DeviceIdentity::from_parts([4; KEY_LEN], StaticKey([5; KEY_LEN]));
        FfspLinks::new(&directory).adopt_identity(&identity)?;
        let bytes = fs::read(directory.join("links.identity"))?;
        assert_eq!(bytes.get(..KEY_LEN), Some([4; KEY_LEN].as_slice()));
        assert_eq!(bytes.get(KEY_LEN..), Some([5; KEY_LEN].as_slice()));
        let _ = fs::remove_dir_all(&directory);
        Ok(())
    }

    #[test]
    fn a_truncated_store_is_corrupt() {
        assert!(decode(&[0; 10]).is_err());
    }
}
