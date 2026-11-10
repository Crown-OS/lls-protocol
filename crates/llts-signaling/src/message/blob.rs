use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::device::Feature;
use crate::message::limits::{
    BLOB_CHUNK_BYTES, MAX_BLOB_BYTES, MAX_CURSOR_BYTES, MAX_DRAG_NAME_BYTES, MAX_MIME_BYTES,
};
use crate::message::{RequestId, Validate};
use crate::util::bounds;

/// Names one blob, allocated by its sender per peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BlobId(pub u32);

/// What a blob is for. Frozen: a new purpose needs a new message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlobPurpose {
    /// The answer to the receiver's request.
    Response(RequestId),
    /// An attachment of the sender's `SendMessage` request.
    MessageAttachment(RequestId),
}

/// Which end of a blob cancelled it, so the id is looked up among the right side's blobs.
/// Frozen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BlobRole {
    /// "The blob I was sending you."
    Sender,
    /// "The blob you were sending me."
    Receiver,
}

tolerant_value! {
    /// Why a blob was cancelled; a value this build does not know reads as [`Self::FAILED`].
    BlobCancelReason(u8) {
        CANCELLED = 0,
        TOO_LARGE = 1,
        FAILED = 2,
        DIGEST_MISMATCH = 3,
        UNEXPECTED = 4,
        PERMISSION_DENIED = 5,
    }
}

/// Asks the receiver for the blob `key` names, under the feature whose bit is `feature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FetchBlob<'a> {
    pub request: RequestId,
    /// A [`Feature`] bit index, so a feature this build does not know is refused at the gate
    /// rather than failing to decode.
    pub feature: u8,
    pub key: &'a str,
}

impl FetchBlob<'_> {
    pub fn feature(&self) -> Option<Feature> {
        Feature::from_bit(self.feature)
    }
}

impl Validate for FetchBlob<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("blob key", self.key, MAX_CURSOR_BYTES)
    }
}

/// Opens blob `blob`: part `part` of the `parts` that answer its purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobStart<'a> {
    pub blob: BlobId,
    pub purpose: BlobPurpose,
    /// Zero-based.
    pub part: u16,
    pub parts: u16,
    pub len: u64,
    pub mime: &'a str,
    #[serde(borrow)]
    pub name: Option<&'a str>,
}

impl Validate for BlobStart<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        if self.part >= self.parts {
            return Err(SignalingError::OutOfRange { field: "blob part" });
        }
        if self.len > MAX_BLOB_BYTES {
            return Err(SignalingError::OutOfRange {
                field: "blob length",
            });
        }
        bounds::text("blob mime", self.mime, MAX_MIME_BYTES)?;
        self.name.map_or(Ok(()), |name| {
            bounds::file_name("blob name", name, MAX_DRAG_NAME_BYTES)
        })
    }
}

/// The bytes of `blob` from `offset` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobChunk<'a> {
    pub blob: BlobId,
    pub offset: u64,
    #[serde(serialize_with = "crate::util::serde_bytes::serialize")]
    pub bytes: &'a [u8],
}

impl Validate for BlobChunk<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        if self.bytes.len() > BLOB_CHUNK_BYTES {
            return Err(SignalingError::FieldTooLong {
                field: "blob chunk",
                len: self.bytes.len(),
                limit: BLOB_CHUNK_BYTES,
            });
        }
        Ok(())
    }
}

/// Closes `blob`, whose bytes hash to `digest` under BLAKE3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobEnd {
    pub blob: BlobId,
    pub digest: [u8; 32],
}

impl Validate for BlobEnd {}

/// The receiver has the first `received` bytes of `blob`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobAck {
    pub blob: BlobId,
    pub received: u64,
}

impl Validate for BlobAck {}

/// Abandons `blob`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobCancel {
    pub blob: BlobId,
    pub canceller: BlobRole,
    pub reason: BlobCancelReason,
}

impl Validate for BlobCancel {}
