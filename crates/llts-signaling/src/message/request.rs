use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::Validate;
use crate::message::limits::{MAX_CURSOR_BYTES, MAX_DETAIL_BYTES};
use crate::util::bounds;

/// Names one request, allocated by the requester per peer and wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RequestId(pub u32);

/// Where a page starts and how long it may be.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageRequest<'a> {
    /// The previous page's `next`, opaque to the requester; `None` asks for the first page.
    #[serde(borrow)]
    pub cursor: Option<&'a str>,
    /// Responders clamp it to [`MAX_PAGE_ITEMS`](crate::message::limits::MAX_PAGE_ITEMS).
    pub limit: u16,
}

impl PageRequest<'_> {
    pub(crate) const fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("page cursor", self.cursor, MAX_CURSOR_BYTES)
    }
}

tolerant_value! {
    /// Why a request failed; a value this build does not know reads as [`Self::FAILED`].
    RequestError(u8) {
        UNSUPPORTED = 0,
        PERMISSION_DENIED = 1,
        NOT_FOUND = 2,
        BUSY = 3,
        CANCELLED = 4,
        TOO_LARGE = 5,
        TIMEOUT = 6,
        FAILED = 7,
    }
}

impl RequestError {
    /// This value, or [`Self::FAILED`] when this build gives it no meaning.
    #[must_use]
    pub const fn known(self) -> Self {
        if self.0 <= Self::FAILED.0 {
            self
        } else {
            Self::FAILED
        }
    }
}

/// The answer to a request that could not be served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestFailed<'a> {
    pub request: RequestId,
    pub error: RequestError,
    /// A short explanation for a log; never user data.
    pub detail: &'a str,
}

impl Validate for RequestFailed<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("failure detail", self.detail, MAX_DETAIL_BYTES)
    }
}

/// The answer to a request that changed something rather than returning data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestDone<'a> {
    pub request: RequestId,
    /// What the change created, such as the id of a sent message.
    #[serde(borrow)]
    pub reference: Option<&'a str>,
}

impl Validate for RequestDone<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::optional_text("request reference", self.reference, MAX_CURSOR_BYTES)
    }
}

/// The requester gave up on `request`; the responder stops its work and sends nothing back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RequestCancel {
    pub request: RequestId,
}

impl Validate for RequestCancel {}
