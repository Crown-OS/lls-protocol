use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::Validate;
use crate::message::limits::{ALLOWED_URI_SCHEMES, MAX_URI_BYTES};
use crate::state::MAX_LIVE_FIELD_BYTES;
use crate::util::bounds;

/// Asks the receiver to open `uri` after its user agrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpenUri<'a> {
    pub uri: &'a str,
}

impl OpenUri<'_> {
    /// The text before the first `:`, in whatever case the sender wrote it.
    pub fn scheme(&self) -> Option<&str> {
        self.uri.split_once(':').map(|(scheme, _)| scheme)
    }
}

impl Validate for OpenUri<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("uri", self.uri, MAX_URI_BYTES)?;
        let allowed = self.scheme().is_some_and(|scheme| {
            ALLOWED_URI_SCHEMES
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(scheme))
        });
        if allowed {
            Ok(())
        } else {
            Err(SignalingError::UriSchemeRefused)
        }
    }
}

/// Presses a button of one of the receiver's live activities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LiveActivityAction<'a> {
    pub activity_id: &'a str,
    pub action_id: &'a str,
}

impl Validate for LiveActivityAction<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("live activity id", self.activity_id, MAX_LIVE_FIELD_BYTES)?;
        bounds::text("live action id", self.action_id, MAX_LIVE_FIELD_BYTES)
    }
}
