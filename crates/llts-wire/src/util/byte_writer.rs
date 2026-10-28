use crate::WireError;

/// Bounds-checked forward cursor over a mutable buffer.
#[derive(Debug)]
pub(crate) struct ByteWriter<'a> {
    buffer: &'a mut [u8],
    position: usize,
}

impl<'a> ByteWriter<'a> {
    pub(crate) const fn new(buffer: &'a mut [u8]) -> Self {
        Self {
            buffer,
            position: 0,
        }
    }

    pub(crate) const fn written_len(&self) -> usize {
        self.position
    }

    pub(crate) const fn capacity(&self) -> usize {
        self.buffer.len()
    }

    pub(crate) fn put(&mut self, source: &[u8]) -> Result<(), WireError> {
        let capacity = self.buffer.len();
        let end = self
            .position
            .checked_add(source.len())
            .ok_or(WireError::TooShort {
                required: usize::MAX,
                available: capacity,
            })?;
        let destination = self
            .buffer
            .get_mut(self.position..end)
            .ok_or(WireError::TooShort {
                required: end,
                available: capacity,
            })?;
        destination.copy_from_slice(source);
        self.position = end;
        Ok(())
    }

    pub(crate) fn u8(&mut self, value: u8) -> Result<(), WireError> {
        self.put(&[value])
    }

    pub(crate) fn u16_le(&mut self, value: u16) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }

    pub(crate) fn u32_le(&mut self, value: u32) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }

    pub(crate) fn u64_le(&mut self, value: u64) -> Result<(), WireError> {
        self.put(&value.to_le_bytes())
    }
}
