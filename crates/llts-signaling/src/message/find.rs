use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::{RequestId, Validate};

/// Frozen: a new action needs a new message type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RingAction {
    Start,
    Stop,
}

/// Rings the receiver at full volume, even when it is silenced, until stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ring {
    pub action: RingAction,
}

impl Validate for Ring {}

/// Asks the receiver where it is, answered by [`Location`] or a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Locate {
    pub request: RequestId,
}

impl Validate for Locate {}

const MAX_LATITUDE_E7: i32 = 900_000_000;
const MAX_LONGITUDE_E7: i32 = 1_800_000_000;

/// A position fix, in degrees times 10⁷ like the platforms' own integer forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Location {
    pub request: RequestId,
    pub latitude_e7: i32,
    pub longitude_e7: i32,
    pub accuracy_m: u32,
    pub altitude_m: Option<i32>,
    pub fix_unix_ms: u64,
}

impl Validate for Location {
    fn validate(&self) -> Result<(), SignalingError> {
        if self.latitude_e7.unsigned_abs() > MAX_LATITUDE_E7.unsigned_abs() {
            return Err(SignalingError::OutOfRange { field: "latitude" });
        }
        if self.longitude_e7.unsigned_abs() > MAX_LONGITUDE_E7.unsigned_abs() {
            return Err(SignalingError::OutOfRange { field: "longitude" });
        }
        Ok(())
    }
}
