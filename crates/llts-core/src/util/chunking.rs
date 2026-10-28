/// Splits `payload` into chunks of at most `chunk_len`.
///
/// An empty payload yields one empty chunk rather than nothing, so a zero-byte slice still
/// produces a packet able to carry its flags.
pub(crate) fn chunks_including_empty(
    payload: &[u8],
    chunk_len: usize,
) -> impl Iterator<Item = &[u8]> {
    let mut owes_empty_chunk = payload.is_empty();
    let mut chunks = payload.chunks(chunk_len.max(1));
    core::iter::from_fn(move || {
        if owes_empty_chunk {
            owes_empty_chunk = false;
            return Some(<&[u8]>::default());
        }
        chunks.next()
    })
}
