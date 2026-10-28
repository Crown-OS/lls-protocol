use serde::{Deserialize, Serialize};

use crate::device::Feature;
use crate::message::{AudioCodec, MuxClass, StreamCodec, StreamRef, VideoCodec};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VideoParams {
    pub codec: VideoCodec,
    pub width: u16,
    pub height: u16,
    /// Frame rate in millihertz, so 59.94 Hz is exact.
    pub fps_mhz: u32,
    pub bitrate_kbps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AudioParams {
    pub codec: AudioCodec,
    pub sample_rate: u32,
    pub channels: u8,
    pub frame_ms: u8,
}

/// A stream's media parameters, which also fix its multiplexing class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamParams {
    Video(VideoParams),
    Audio(AudioParams),
}

impl StreamParams {
    pub const fn class(&self) -> MuxClass {
        match self {
            Self::Video(_) => MuxClass::Video,
            Self::Audio(_) => MuxClass::Audio,
        }
    }

    pub const fn codec(&self) -> StreamCodec {
        match self {
            Self::Video(video) => StreamCodec::Video(video.codec),
            Self::Audio(audio) => StreamCodec::Audio(audio.codec),
        }
    }
}

/// Proposes a media stream. The class comes from the parameters, so a video stream cannot be
/// opened on the audio class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OpenStream {
    pub index: u8,
    /// Which feature the stream serves, since mirroring and camera both open video streams.
    pub purpose: Feature,
    pub params: StreamParams,
}

impl OpenStream {
    pub const fn stream(&self) -> StreamRef {
        StreamRef {
            class: self.params.class(),
            index: self.index,
        }
    }
}

/// Accepts an [`OpenStream`], confirming the codec the receiver will decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AcceptStream {
    pub stream: StreamRef,
    pub codec: StreamCodec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CloseStream {
    pub stream: StreamRef,
}
