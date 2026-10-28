use serde::Serializer;

/// Writes a borrowed slice as one postcard byte string instead of a sequence of `u8` elements,
/// which is the same wire shape but a single copy.
pub(crate) fn serialize<S: Serializer>(bytes: &&[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(bytes)
}
