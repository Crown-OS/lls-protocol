use crate::TransportError;

/// `N` bytes from the operating system's CSPRNG.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], TransportError> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(TransportError::Randomness)?;
    Ok(bytes)
}
