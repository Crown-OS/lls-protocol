/// Every way a beacon or TXT record can fail to decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoveryError {
    #[error("beacon must be {expected} bytes, got {actual}")]
    Length { expected: usize, actual: usize },

    #[error("beacon declares version {0}, which this build cannot read")]
    UnsupportedVersion(u8),

    #[error("beacon kind {0} is not known to this build")]
    UnknownKind(u8),

    #[error("device class {0} is not known to this build")]
    UnknownDeviceClass(u8),

    #[error("mDNS TXT record is missing the {0} key")]
    MissingTxtKey(&'static str),

    #[error("mDNS TXT value for {key} is malformed")]
    MalformedTxtValue { key: &'static str },

    #[error("the operating system has no entropy to draw from")]
    Entropy(#[from] getrandom::Error),
}
