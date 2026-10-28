use llts_wire::MediaTimestamp;

/// An independently transmittable chunk of encoded media, handed to the protocol by an encoder.
///
/// Hardware encoders configured for sub-frame output emit several of these per frame, which is
/// what lets the first bytes reach the network while the last macroblock row is still encoding.
/// An unsliced frame simply produces one slice with [`EncodedSlice::ends_frame`] set.
#[derive(Debug, Clone, Copy)]
pub struct EncodedSlice<'a> {
    bytes: &'a [u8],
    timestamp: MediaTimestamp,
    is_keyframe: bool,
    ends_frame: bool,
}

impl<'a> EncodedSlice<'a> {
    pub const fn new(bytes: &'a [u8], timestamp: MediaTimestamp) -> Self {
        Self {
            bytes,
            timestamp,
            is_keyframe: false,
            ends_frame: false,
        }
    }

    /// Marks the slice as part of a frame that decodes without any predecessor.
    #[must_use]
    pub const fn as_keyframe(mut self) -> Self {
        self.is_keyframe = true;
        self
    }

    /// Marks the slice as the last of its frame, closing the frame's repair block.
    #[must_use]
    pub const fn ending_frame(mut self) -> Self {
        self.ends_frame = true;
        self
    }

    pub const fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub const fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    pub const fn timestamp(&self) -> MediaTimestamp {
        self.timestamp
    }

    pub const fn is_keyframe(&self) -> bool {
        self.is_keyframe
    }

    pub const fn ends_frame(&self) -> bool {
        self.ends_frame
    }
}
