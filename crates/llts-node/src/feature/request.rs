use llts_signaling::device::Feature;
use llts_signaling::message::{
    AudioCodec, AudioParams, Direction, Signal, StartAudio, StartCamera, StartCast, StartMic,
    StartMirror, StartMonitor, StreamParams, VideoCodec, VideoLimits, VideoParams,
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
    /// Casts one app's video; `direction` is seen from this device, like a mirror's.
    Cast(StartCast),
    /// Plays one device's audio on the other; `direction` is seen from this device.
    Audio(StartAudio),
}

/// Which half of a media pipeline this device runs for a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaRole {
    Encoder,
    Decoder,
}

/// Where one feature of one device stands, matching the `crownlink` IPC `FeatureState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureState {
    Disabled,
    /// Allowed but idle.
    Enabled,
    /// Allowed and running right now.
    Active,
}

const OPUS_SAMPLE_RATE: u32 = 48_000;
const VOICE_FRAME_MS: u8 = 10;
const MUSIC_FRAME_MS: u8 = 20;

impl FeatureRequest {
    pub const fn feature(&self) -> Feature {
        match self {
            Self::Mirror(_) => Feature::Mirror,
            Self::Camera(_) => Feature::Camera,
            Self::Mic => Feature::Mic,
            Self::Monitor(_) => Feature::Monitor,
            Self::Cast(_) => Feature::Cast,
            Self::Audio(_) => Feature::AudioHandoff,
        }
    }

    /// Which half this device runs when it is the one that asked.
    pub const fn requester_role(&self) -> MediaRole {
        match self {
            Self::Monitor(_) => MediaRole::Encoder,
            Self::Camera(_) | Self::Mic => MediaRole::Decoder,
            Self::Mirror(_) | Self::Cast(_) | Self::Audio(_) => self.directed_role(),
        }
    }

    /// The role of a request with a direction: whoever's media flows to the peer encodes.
    const fn directed_role(&self) -> MediaRole {
        match self.direction() {
            Some(Direction::ToPeer) => MediaRole::Encoder,
            Some(Direction::FromPeer) | None => MediaRole::Decoder,
        }
    }

    const fn direction(&self) -> Option<Direction> {
        match self {
            Self::Mirror(StartMirror { direction, .. })
            | Self::Cast(StartCast { direction, .. })
            | Self::Audio(StartAudio { direction }) => Some(*direction),
            Self::Camera(_) | Self::Mic | Self::Monitor(_) => None,
        }
    }

    pub(crate) const fn as_signal(&self) -> Signal<'static> {
        match *self {
            Self::Mirror(mirror) => Signal::StartMirrorV1(mirror),
            Self::Camera(camera) => Signal::StartCameraV1(camera),
            Self::Mic => Signal::StartMicV1(StartMic),
            Self::Monitor(monitor) => Signal::StartMonitorV1(monitor),
            Self::Cast(cast) => Signal::StartCastV1(cast),
            Self::Audio(audio) => Signal::StartAudioV1(audio),
        }
    }

    /// The request a peer sent, with its direction restated from this device's side.
    pub(crate) const fn from_peer(signal: &Signal<'_>) -> Option<Self> {
        match *signal {
            Signal::StartMirrorV1(mirror) => Some(Self::Mirror(StartMirror {
                direction: flip(mirror.direction),
                ..mirror
            })),
            Signal::StartCameraV1(camera) => Some(Self::Camera(camera)),
            Signal::StartMicV1(_) => Some(Self::Mic),
            Signal::StartMonitorV1(monitor) => Some(Self::Monitor(monitor)),
            Signal::StartCastV1(cast) => Some(Self::Cast(StartCast {
                direction: flip(cast.direction),
                ..cast
            })),
            Signal::StartAudioV1(audio) => Some(Self::Audio(StartAudio {
                direction: flip(audio.direction),
            })),
            _ => None,
        }
    }

    /// The role this device runs when the request came from the peer.
    pub(crate) const fn responder_role(&self) -> MediaRole {
        match self {
            Self::Monitor(_) => MediaRole::Decoder,
            Self::Camera(_) | Self::Mic => MediaRole::Encoder,
            Self::Mirror(_) | Self::Cast(_) | Self::Audio(_) => self.directed_role(),
        }
    }

    /// Whether running `role` lets the peer's input reach this device's seat.
    pub(crate) const fn hands_seat_to_peer(&self, role: MediaRole) -> bool {
        matches!(
            (self, role),
            (
                Self::Mirror(StartMirror {
                    remote_input: true,
                    ..
                }),
                MediaRole::Encoder
            )
        )
    }

    pub(crate) fn params(
        &self,
        video: Option<VideoCodec>,
        audio: Option<AudioCodec>,
        bitrate_kbps: u32,
    ) -> Option<StreamParams> {
        let limits = match self {
            Self::Mic => return audio.map(|codec| opus(codec, 1, VOICE_FRAME_MS)),
            Self::Audio(_) => return audio.map(|codec| opus(codec, 2, MUSIC_FRAME_MS)),
            Self::Monitor(monitor) => (monitor.width, monitor.height, monitor.refresh_mhz),
            Self::Mirror(StartMirror { limits, .. })
            | Self::Camera(StartCamera { limits, .. })
            | Self::Cast(StartCast { limits, .. }) => video_mode(*limits),
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

const fn opus(codec: AudioCodec, channels: u8, frame_ms: u8) -> StreamParams {
    StreamParams::Audio(AudioParams {
        codec,
        sample_rate: OPUS_SAMPLE_RATE,
        channels,
        frame_ms,
    })
}

fn video_mode(limits: VideoLimits) -> (u16, u16, u32) {
    (
        limits.max_width,
        limits.max_height,
        u32::from(limits.max_fps) * 1_000,
    )
}

const fn flip(direction: Direction) -> Direction {
    match direction {
        Direction::ToPeer => Direction::FromPeer,
        Direction::FromPeer => Direction::ToPeer,
    }
}
