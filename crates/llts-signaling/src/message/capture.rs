use serde::{Deserialize, Serialize};

use crate::message::{RequestId, Validate};

tolerant_value! {
    /// What to capture; a responder answers a kind it does not know as unsupported.
    CaptureKind(u8) {
        PHOTO = 0,
        SCAN_PDF = 1,
        SCAN_JPEG = 2,
        SKETCH = 3,
    }
}

const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];
const PDF_MAGIC: &[u8] = b"%PDF-";
const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

impl CaptureKind {
    /// The only MIME type a result of this kind may carry.
    pub const fn mime(self) -> Option<&'static str> {
        match self {
            Self::PHOTO | Self::SCAN_JPEG => Some("image/jpeg"),
            Self::SCAN_PDF => Some("application/pdf"),
            Self::SKETCH => Some("image/png"),
            _ => None,
        }
    }

    /// The bytes a result of this kind starts with.
    pub const fn magic(self) -> Option<&'static [u8]> {
        match self {
            Self::PHOTO | Self::SCAN_JPEG => Some(JPEG_MAGIC),
            Self::SCAN_PDF => Some(PDF_MAGIC),
            Self::SKETCH => Some(PNG_MAGIC),
            _ => None,
        }
    }
}

/// Asks the receiver's user to take a photo, scan a document or draw a sketch, answered by
/// the result as blobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capture {
    pub request: RequestId,
    pub kind: CaptureKind,
    /// The longest edge of an image result in pixels; `0` leaves it to the device.
    pub max_edge: u16,
}

impl Validate for Capture {}
