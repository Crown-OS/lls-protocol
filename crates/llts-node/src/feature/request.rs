use llts_signaling::device::Feature;
use llts_signaling::message::{
    AudioCodec, AudioParams, Direction, Signal, StartCamera, StartMic, StartMirror, StartMonitor,
    StreamParams, VideoCodec, VideoParams,
};

/// A feature that needs a media stream, as the user asked for it on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureRequest {
    /// `direction` is seen from this device: `ToPeer` shares this screen.
    Mirror(StartMirror),
    /// Streams the peer's camera to this device.
    Camera(StartCamera),
    /// Streams the peer's microphone to this device.
    Mic,
    /// Uses the peer as an extra monitor for this device.
    Monitor(StartMonitor),
}

/// Which half of a media pipeline this device runs for a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaRole {
    Encoder,
    Decoder,
}

/// Where one feature of one device stands, matching the `crownconnect` IPC `FeatureState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureState {
    Disabled,
    /// Allowed but idle.
    Enabled,
    /// Allowed and running right now.
    Active,
}

const OPUS_SAMPLE_RATE: u32 = 48_000;
const OPUS_FRAME_MS: u8 = 10;

impl FeatureRequest {
    pub const fn feature(&self) -> Feature {
        match self {
            Self::Mirror(_) => Feature::Mirror,
            Self::Camera(_) => Feature::Camera,
            Self::Mic => Feature::Mic,
            Self::Monitor(_) => Feature::Monitor,
        }
    }

    /// Which half this device runs when it is the one that asked.
    pub const fn requester_role(&self) -> MediaRole {
        match self {
            Self::Mirror(StartMirror {
                direction: Direction::ToPeer,
                ..
            })
            | Self::Monitor(_) => MediaRole::Encoder,
            Self::Mirror(_) | Self::Camera(_) | Self::Mic => MediaRole::Decoder,
        }
    }

    pub(crate) const fn as_signal(&self) -> Signal<'static> {
        match *self {
            Self::Mirror(mirror) => Signal::StartMirrorV1(mirror),
            Self::Camera(camera) => Signal::StartCameraV1(camera),
            Self::Mic => Signal::StartMicV1(StartMic),
            Self::Monitor(monitor) => Signal::StartMonitorV1(monitor),
        }
    }

    /// The request a peer sent, with a mirror's direction restated from this device's side.
    pub(crate) const fn from_peer(signal: &Signal<'_>) -> Option<Self> {
        match *signal {
            Signal::StartMirrorV1(mirror) => Some(Self::Mirror(StartMirror {
                direction: flip(mirror.direction),
                ..mirror
            })),
            Signal::StartCameraV1(camera) => Some(Self::Camera(camera)),
            Signal::StartMicV1(_) => Some(Self::Mic),
            Signal::StartMonitorV1(monitor) => Some(Self::Monitor(monitor)),
            _ => None,
        }
    }

    /// The role this device runs when the request came from the peer.
    pub(crate) const fn responder_role(&self) -> MediaRole {
        match self {
            Self::Mirror(StartMirror {
                direction: Direction::ToPeer,
                ..
            }) => MediaRole::Encoder,
            Self::Mirror(_) | Self::Monitor(_) => MediaRole::Decoder,
            Self::Camera(_) | Self::Mic => MediaRole::Encoder,
        }
    }

    pub(crate) fn params(
        &self,
        video: Option<VideoCodec>,
        audio: Option<AudioCodec>,
        bitrate_kbps: u32,
    ) -> Option<StreamParams> {
        let limits = match self {
            Self::Mic => {
                return audio.map(|codec| {
                    StreamParams::Audio(AudioParams {
                        codec,
                        sample_rate: OPUS_SAMPLE_RATE,
                        channels: 1,
                        frame_ms: OPUS_FRAME_MS,
                    })
                });
            }
            Self::Monitor(monitor) => (monitor.width, monitor.height, monitor.refresh_mhz),
            Self::Mirror(StartMirror { limits, .. }) | Self::Camera(StartCamera { limits, .. }) => {
                (
                    limits.max_width,
                    limits.max_height,
                    u32::from(limits.max_fps) * 1_000,
                )
            }
        };
        let (width, height, fps_mhz) = limits;
        video.map(|codec| {
            StreamParams::Video(VideoParams {
                codec,
                width,
                height,
                fps_mhz,
                bitrate_kbps,
            })
        })
    }
}

const fn flip(direction: Direction) -> Direction {
    match direction {
        Direction::ToPeer => Direction::FromPeer,
        Direction::FromPeer => Direction::ToPeer,
    }
}
