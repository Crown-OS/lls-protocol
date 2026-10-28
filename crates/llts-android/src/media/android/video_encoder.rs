use std::sync::Arc;
use std::time::Duration;

use llts_core::send::EncodedSlice;
use llts_core::stream::StreamKey;
use llts_core::units::Bitrate;
use llts_signaling::device::DeviceId;
use llts_wire::{ClockRate, MediaTimestamp};
use ndk::media::media_codec::{MediaCodec, MediaCodecDirection};
use ndk::media::media_format::MediaFormat;
use ndk::native_window::NativeWindow;

use super::codec_events::{self, CodecEvent, CodecEvents, OutputReady};
use super::{StreamSpec, codec_step, video_format};
use crate::media::MediaError;
use crate::util::wake::Waker;

/// `AMEDIACODEC_BUFFER_FLAG_KEY_FRAME`.
const KEY_FRAME: u32 = 1;
/// `AMEDIACODEC_BUFFER_FLAG_CODEC_CONFIG`: the parameter sets, sent on their own first.
const CODEC_CONFIG: u32 = 2;

/// A hardware encoder reading from a persistent input surface, whose access units go from its
/// output buffers straight into the llts stream.
#[derive(Debug)]
pub(super) struct VideoEncoder {
    spec: StreamSpec,
    codec: MediaCodec,
    events: Arc<CodecEvents>,
    /// Kept to put in front of any keyframe the encoder did not prefix with them itself, so a
    /// decoder can join at every keyframe.
    parameter_sets: Vec<u8>,
    _surface: NativeWindow,
}

impl VideoEncoder {
    pub(super) fn start(
        spec: StreamSpec,
        surface: NativeWindow,
        waker: &Waker,
    ) -> Result<Self, MediaError> {
        let mut codec = MediaCodec::from_encoder_type(video_format::mime(spec.params.codec))
            .ok_or(MediaError::NoCodec(spec.params.codec))?;
        let events = CodecEvents::new();
        codec
            .set_async_notify_callback(Some(codec_events::callback(&events, waker)))
            .map_err(codec_step("registering the encoder callbacks"))?;
        codec
            .configure(
                &video_format::encoder_format(&spec.params),
                None,
                MediaCodecDirection::Encoder,
            )
            .map_err(codec_step("configuring the encoder"))?;
        codec
            .set_input_surface(&surface)
            .map_err(codec_step("attaching the encoder surface"))?;
        codec.start().map_err(codec_step("starting the encoder"))?;
        Ok(Self {
            spec,
            codec,
            events,
            parameter_sets: Vec::new(),
            _surface: surface,
        })
    }

    pub(super) const fn spec(&self) -> StreamSpec {
        self.spec
    }

    pub(super) fn drain(
        &mut self,
        send: &mut dyn FnMut(DeviceId, StreamKey, EncodedSlice<'_>),
    ) -> Result<(), MediaError> {
        while let Some(event) = self.events.pop() {
            match event {
                CodecEvent::OutputAvailable(output) => {
                    self.forward(&output, send);
                    self.codec
                        .release_output_buffer_by_index(output.index, false)
                        .map_err(codec_step("releasing an encoded buffer"))?;
                }
                CodecEvent::InputAvailable(_) => {}
                CodecEvent::Failed(detail) => return Err(MediaError::CodecFailed(detail)),
            }
        }
        Ok(())
    }

    fn forward(
        &mut self,
        output: &OutputReady,
        send: &mut dyn FnMut(DeviceId, StreamKey, EncodedSlice<'_>),
    ) {
        let Some(unit) = self.codec.output_buffer(output.index).and_then(|buffer| {
            buffer.get(output.offset..output.offset.saturating_add(output.size))
        }) else {
            return;
        };
        if output.flags & CODEC_CONFIG != 0 {
            self.parameter_sets.clear();
            self.parameter_sets.extend_from_slice(unit);
            return;
        }
        if unit.is_empty() {
            return;
        }
        let capture = Duration::from_micros(u64::try_from(output.presentation_us).unwrap_or(0));
        let timestamp = MediaTimestamp::from_duration(capture, ClockRate::VIDEO);
        let (peer, stream) = (self.spec.peer, self.spec.stream);
        let slice = EncodedSlice::new(unit, timestamp).ending_frame();
        if output.flags & KEY_FRAME == 0 {
            send(peer, stream, slice);
            return;
        }
        if !self.parameter_sets.is_empty() && !unit.starts_with(&self.parameter_sets) {
            send(
                peer,
                stream,
                EncodedSlice::new(&self.parameter_sets, timestamp).as_keyframe(),
            );
        }
        send(peer, stream, slice.as_keyframe());
    }

    pub(super) fn set_bitrate(&self, bitrate: Bitrate) {
        let mut parameters = MediaFormat::new();
        parameters.set_i32(
            "video-bitrate",
            i32::try_from(bitrate.bits_per_second()).unwrap_or(i32::MAX),
        );
        if let Err(error) = self.codec.set_parameters(parameters) {
            log::warn!("the encoder refused a new bitrate: {error}");
        }
    }

    pub(super) fn request_keyframe(&self) {
        let mut parameters = MediaFormat::new();
        parameters.set_i32("request-sync", 0);
        if let Err(error) = self.codec.set_parameters(parameters) {
            log::warn!("the encoder refused a keyframe request: {error}");
        }
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        if let Err(error) = self.codec.stop() {
            log::warn!("the encoder did not stop cleanly: {error}");
        }
    }
}
