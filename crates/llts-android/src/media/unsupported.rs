use jni::Env;
use jni::objects::JObject;
use llts_core::receive::AssembledFrame;
use llts_core::send::EncodedSlice;
use llts_core::stream::StreamKey;
use llts_core::units::Bitrate;
use llts_node::{FeatureRequest, MediaRole};
use llts_signaling::device::DeviceId;
use llts_signaling::message::{StreamParams, StreamRef};

use super::MediaError;
use crate::util::wake::Waker;

/// No `Surface` exists off Android, so no attach command can be built.
#[derive(Debug)]
pub(crate) enum Surface {}

impl Surface {
    pub(crate) const fn encoder_input<'local>(
        _env: &mut Env<'local>,
    ) -> Result<(Self, JObject<'local>), MediaError> {
        Err(MediaError::Unsupported)
    }

    pub(crate) const fn from_java(
        _env: &mut Env<'_>,
        _surface: &JObject<'_>,
    ) -> Result<Self, MediaError> {
        Err(MediaError::Unsupported)
    }
}

/// Stands in for the Android pipelines on a host: starts are accepted and remembered by
/// nothing, and there is never anything to send.
#[derive(Debug)]
pub(crate) struct MediaPipelines;

impl MediaPipelines {
    pub(crate) fn new(_waker: Waker) -> Self {
        Self
    }

    pub(crate) const fn start(
        &mut self,
        _peer: DeviceId,
        _request: FeatureRequest,
        _role: MediaRole,
        _stream: StreamRef,
        _params: StreamParams,
    ) -> Result<(), MediaError> {
        Ok(())
    }

    #[expect(
        clippy::needless_pass_by_value,
        reason = "the Android pipelines consume the surface they are given"
    )]
    pub(crate) const fn attach(
        &mut self,
        _peer: DeviceId,
        _index: u8,
        _role: MediaRole,
        surface: Surface,
    ) -> Result<(), MediaError> {
        match surface {}
    }

    pub(crate) const fn detach(&mut self, _peer: DeviceId, _index: u8) {}

    pub(crate) const fn stop(&mut self, _peer: DeviceId, _stream: StreamRef) {}

    pub(crate) const fn present(
        &mut self,
        _peer: DeviceId,
        _stream: StreamKey,
        _frame: &AssembledFrame,
    ) -> Result<(), MediaError> {
        Err(MediaError::Unsupported)
    }

    pub(crate) const fn set_bitrate(
        &mut self,
        _peer: DeviceId,
        _stream: StreamKey,
        _bitrate: Bitrate,
    ) {
    }

    pub(crate) const fn request_keyframe(&mut self, _peer: DeviceId, _stream: StreamKey) {}

    pub(crate) fn pump(
        &mut self,
        _send: &mut dyn FnMut(DeviceId, StreamKey, EncodedSlice<'_>),
    ) -> Result<(), MediaError> {
        Ok(())
    }
}
