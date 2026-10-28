use llts_wire::StreamClass;

use crate::repair::RepairMode;
use crate::session::Role;
use crate::units::{Bitrate, FrameRate};

/// How a media stream is protected and played out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// Parity plus NACK, one-slot playout that always shows the freshest frame.
    Video,
    /// One Opus frame per packet, no NACK and no parity — the codec's in-band redundancy covers
    /// a lost packet sooner than a retransmission could — and every frame delivered as it lands.
    Audio,
}

impl MediaKind {
    pub const fn for_class(class: StreamClass) -> Option<Self> {
        match class {
            StreamClass::Video => Some(Self::Video),
            StreamClass::Audio => Some(Self::Audio),
            StreamClass::Control | StreamClass::Data | StreamClass::Input => None,
        }
    }
}

/// Everything a media stream needs to know before it sees a packet.
#[derive(Debug, Clone, Copy)]
pub struct MediaStreamConfig {
    pub role: Role,
    pub frame_rate: FrameRate,
    pub repair: RepairMode,
    /// Bandwidth the stream needs regardless of congestion, subtracted from the video budget.
    /// Set for audio, whose rate the encoder fixes; `None` for video, which takes what is left.
    pub reserved_bitrate: Option<Bitrate>,
}

impl MediaStreamConfig {
    /// A 1080p60 screen share over Wi-Fi, which is the shape the whole design is tuned for.
    pub fn screen_share(role: Role) -> Self {
        Self {
            role,
            frame_rate: FrameRate::STANDARD,
            repair: RepairMode::default(),
            reserved_bitrate: None,
        }
    }

    /// Opus at 10 ms frames with in-band FEC, the call and microphone configuration.
    pub const fn opus_voice(role: Role) -> Self {
        Self {
            role,
            frame_rate: FrameRate::OPUS_10_MS,
            repair: RepairMode::Off,
            reserved_bitrate: Bitrate::from_kilobits_per_second(64),
        }
    }
}
