use serde::{Deserialize, Serialize};

/// Which way a stream flows, seen from the device sending the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    ToPeer,
    FromPeer,
}

/// A side of a screen, matching the `crownconnect` IPC `Edge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// How far along an edge a point lies, from `0` at its left or top end to `u16::MAX` at its
/// right or bottom end, so both screens map it to their own resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgePosition(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CameraLens {
    Back,
    Front,
}

/// Upper bounds for a video stream, matching the `crownconnect` IPC `VideoLimits`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VideoLimits {
    pub max_width: u16,
    pub max_height: u16,
    pub max_fps: u16,
}
