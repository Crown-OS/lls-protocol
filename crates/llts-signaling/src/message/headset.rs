use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::Validate;

/// "I released this headset: connect it." Sent after the sender disconnected it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TakeHeadset<'a> {
    pub address: &'a str,
}

/// "Give this headset up, then send [`TakeHeadset`]."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReleaseHeadset<'a> {
    pub address: &'a str,
}

impl Validate for TakeHeadset<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        check_address(self.address)
    }
}

impl Validate for ReleaseHeadset<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        check_address(self.address)
    }
}

/// Accepts exactly `AA:BB:CC:DD:EE:FF`, uppercase.
fn check_address(address: &str) -> Result<(), SignalingError> {
    const LEN: usize = 17;
    let well_formed = address.len() == LEN
        && address.bytes().enumerate().all(|(position, byte)| {
            if position % 3 == 2 {
                byte == b':'
            } else {
                byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte)
            }
        });
    if well_formed {
        Ok(())
    } else {
        Err(SignalingError::OutOfRange {
            field: "headset address",
        })
    }
}
