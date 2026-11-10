#[cfg(target_os = "android")]
use llts_signaling::message::VideoCodec;

#[derive(Debug, thiserror::Error)]
pub(crate) enum MediaError {
    #[cfg(not(target_os = "android"))]
    #[error("hardware media pipelines exist only on Android")]
    Unsupported,

    #[cfg(target_os = "android")]
    #[error("no stream {index} is waiting for a {expected} surface")]
    NoPendingStream { index: u8, expected: &'static str },

    #[cfg(target_os = "android")]
    #[error("this device has no {0:?} codec")]
    NoCodec(VideoCodec),

    #[cfg(target_os = "android")]
    #[error("the Surface has no native window")]
    NoWindow,

    #[cfg(target_os = "android")]
    #[error("MediaCodec failed while {step}")]
    Codec {
        step: &'static str,
        #[source]
        source: ndk::media_error::MediaError,
    },

    #[cfg(target_os = "android")]
    #[error("MediaCodec reported an unrecoverable error: {0}")]
    CodecFailed(String),

    #[cfg(all(target_os = "android", feature = "opus"))]
    #[error("AAudio failed while {step}")]
    Audio {
        step: &'static str,
        #[source]
        source: ndk::audio::AudioError,
    },

    #[cfg(all(target_os = "android", feature = "opus"))]
    #[error("the Opus codec could not be set up")]
    Opus(#[from] opus::Error),

    #[cfg(all(target_os = "android", not(feature = "opus")))]
    #[error("voice needs the opus feature, which this build leaves out")]
    NoOpus,
}
