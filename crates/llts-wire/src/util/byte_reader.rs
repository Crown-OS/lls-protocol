use crate::WireError;

/// Bounds-checked forward cursor over a borrowed buffer.
///
/// Exists so that no parser in this crate ever indexes a slice directly.
#[derive(Debug, Clone)]
pub(crate) struct ByteReader<'a> {
    buffer: &'a [u8],
    position: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) const fn new(buffer: &'a [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    pub(crate) const fn remaining_len(&self) -> usize {
        self.buffer.len() - self.position
    }

    pub(crate) fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(WireError::TooShort {
                required: usize::MAX,
                available: self.buffer.len(),
            })?;
        let slice = self
            .buffer
            .get(self.position..end)
            .ok_or(WireError::TooShort {
                required: end,
                available: self.buffer.len(),
            })?;
        self.position = end;
        Ok(slice)
    }

    pub(crate) fn take_array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let slice = self.take(N)?;
        <[u8; N]>::try_from(slice).map_err(|_| WireError::TooShort {
            required: N,
            available: slice.len(),
        })
    }

    pub(crate) fn u8(&mut self) -> Result<u8, WireError> {
        self.take_array::<1>().map(|bytes| bytes[0])
    }

    pub(crate) fn u16_le(&mut self) -> Result<u16, WireError> {
        self.take_array::<2>().map(u16::from_le_bytes)
    }

    pub(crate) fn u32_le(&mut self) -> Result<u32, WireError> {
        self.take_array::<4>().map(u32::from_le_bytes)
    }

    pub(crate) fn u64_le(&mut self) -> Result<u64, WireError> {
        self.take_array::<8>().map(u64::from_le_bytes)
    }

    pub(crate) fn into_remaining(self) -> &'a [u8] {
        self.buffer.get(self.position..).unwrap_or_default()
    }
}
