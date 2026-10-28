use serde::{Deserialize, Serialize};

use crate::device::{DeviceClass, FeatureSet};
use crate::message::{AudioCodec, CodecList, StreamRef, VideoCodec, negotiate_codec};

/// What a device can encode, decode and share, sent once each way when a session opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Capabilities {
    pub video_codecs: CodecList<VideoCodec>,
    pub audio_codecs: CodecList<AudioCodec>,
    pub features: FeatureSet,
    pub device_class: DeviceClass,
}

impl Capabilities {
    pub fn video_codec_with(&self, peer: &Self) -> Option<VideoCodec> {
        negotiate_codec(&self.video_codecs, &peer.video_codecs)
    }

    pub fn audio_codec_with(&self, peer: &Self) -> Option<AudioCodec> {
        negotiate_codec(&self.audio_codecs, &peer.audio_codecs)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GoodbyeReason {
    Shutdown,
    /// The user removed the pairing; the peer should forget this device too.
    Unpaired,
    Idle,
    /// A newer session with the same peer took over.
    Replaced,
    ProtocolError,
}

/// Ends a session deliberately, so the peer need not wait for liveness to time out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Goodbye {
    pub reason: GoodbyeReason,
}

/// Announces that the sender switches to the key epoch `epoch` on its next packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rekey {
    pub epoch: u32,
}

/// Asks the sender of a video stream for a keyframe, after a frame could not be assembled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyframeRequest {
    pub stream: StreamRef,
}
