//! Hardware media pipelines, run on the runtime thread.
//!
//! Frames stay in codec memory: an encoder renders from a `Surface` the screen or camera draws
//! into and its access units go from `MediaCodec`'s output buffers straight into the llts
//! stream; a decoder renders into the `Surface` of a view. Off Android the pipelines are absent
//! and every attach fails, which keeps the runtime itself testable on a host.

mod error;

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod unsupported;

#[cfg(target_os = "android")]
pub(crate) use android::{MediaPipelines, Surface};
pub(crate) use error::MediaError;
#[cfg(not(target_os = "android"))]
pub(crate) use unsupported::{MediaPipelines, Surface};

use llts_signaling::device::Feature;

/// Whether this build can run `feature`'s half on this device, which is what it announces.
pub(crate) const fn implements(feature: Feature) -> bool {
    match feature {
        Feature::Mirror | Feature::Camera | Feature::Monitor => cfg!(target_os = "android"),
        Feature::Mic => cfg!(all(target_os = "android", feature = "opus")),
        Feature::Unicursor
        | Feature::Calls
        | Feature::Clipboard
        | Feature::Notifications
        | Feature::Battery
        | Feature::Hotspot
        | Feature::Files => true,
    }
}
