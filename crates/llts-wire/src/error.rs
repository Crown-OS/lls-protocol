/// Every way a byte sequence can fail to be a valid LLTS packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WireError {
    #[error("need {required} bytes, buffer holds {available}")]
    TooShort { required: usize, available: usize },

    #[error("missing protocol magic")]
    BadMagic,

    #[error("peer speaks protocol version {peer}, this build speaks {local}")]
    UnsupportedVersion { peer: u8, local: u8 },

    #[error("packet kind {0} is not recognised")]
    UnknownPacketKind(u8),

    #[error("expected a {expected} packet, found {found}")]
    UnexpectedPacketKind {
        expected: &'static str,
        found: &'static str,
    },

    #[error("field `{field}` holds a value this protocol cannot represent")]
    MalformedField { field: &'static str },
}
