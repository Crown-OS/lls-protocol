use serde::{Deserialize, Serialize};

use crate::device::DeviceClass;
use crate::state::{Topic, TopicPayload};

/// How a device presents itself once a session is up.
///
/// Names never travel in discovery beacons, which anyone in range can read; they are published
/// here, to authenticated peers only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceInfo<'a> {
    pub name: &'a str,
    pub model: &'a str,
    pub class: DeviceClass,
}

impl<'a> TopicPayload<'a> for DeviceInfo<'a> {
    const TOPIC: Topic = Topic::DeviceInfo;
}
