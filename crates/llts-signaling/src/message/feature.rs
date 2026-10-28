use serde::{Deserialize, Serialize};

use crate::device::Feature;
use crate::message::{CameraLens, Direction, Edge, EdgePosition, VideoLimits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StartMirror {
    pub direction: Direction,
    pub limits: VideoLimits,
    /// Whether the viewing side may drive the mirrored screen's pointer and keyboard.
    pub remote_input: bool,
}

/// Asks the receiver to stream one of its cameras to the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StartCamera {
    pub lens: CameraLens,
    pub limits: VideoLimits,
}

/// Asks the receiver to stream its microphone to the sender.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StartMic;

/// Asks the receiver to act as an extra monitor of this mode for the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StartMonitor {
    pub width: u16,
    pub height: u16,
    pub refresh_mhz: u32,
    /// Output scale in 1/120 steps, as `wp_fractional_scale_v1` expresses it.
    pub scale_120: u16,
    /// The side of the sender's desktop the monitor attaches to.
    pub placement: Edge,
}

/// The shared cursor crossed the sender's `edge` at `position` and now belongs to the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnicursorEnter {
    pub edge: Edge,
    pub position: EdgePosition,
}

/// The shared cursor left the sender's screen across `edge`, handing control back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnicursorLeave {
    pub edge: Edge,
    pub position: EdgePosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stop {
    pub feature: Feature,
}
