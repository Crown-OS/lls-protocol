use core::fmt;

use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::limits::{MAX_PASSPHRASE_BYTES, MAX_SSID_BYTES};
use crate::message::{RequestId, Validate};
use crate::util::bounds;

tolerant_value! {
    /// A network's security; a receiver refuses a value it does not know as unsupported.
    WifiSecurity(u8) {
        OPEN = 0,
        OWE = 1,
        WEP = 2,
        WPA_PSK = 3,
        WPA3_SAE = 4,
    }
}

/// A saved network, either answering the receiver's [`WifiRequest`] or shared unasked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WifiCredentials<'a> {
    /// `Some` answers the receiver's own request; `None` is a share its user must accept.
    pub request: Option<RequestId>,
    pub ssid: &'a str,
    pub security: WifiSecurity,
    pub passphrase: &'a str,
    pub hidden: bool,
}

/// Never prints the passphrase, so credentials can be logged like any other message.
impl fmt::Debug for WifiCredentials<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WifiCredentials")
            .field("request", &self.request)
            .field("ssid", &self.ssid)
            .field("security", &self.security)
            .field("passphrase", &"<redacted>")
            .field("hidden", &self.hidden)
            .finish()
    }
}

impl Validate for WifiCredentials<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("ssid", self.ssid, MAX_SSID_BYTES)?;
        bounds::text("passphrase", self.passphrase, MAX_PASSPHRASE_BYTES)
    }
}

/// Asks the receiver for the credentials of the network it calls `ssid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WifiRequest<'a> {
    pub request: RequestId,
    pub ssid: &'a str,
}

impl Validate for WifiRequest<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::text("ssid", self.ssid, MAX_SSID_BYTES)
    }
}
