use std::collections::VecDeque;
use std::sync::Arc;

use llts_core::receive::AssembledFrame;
use llts_wire::ClockRate;
use ndk::media::media_codec::{MediaCodec, MediaCodecDirection};
use ndk::native_window::NativeWindow;

use super::codec_events::{self, CodecEvent, CodecEvents};
use super::{StreamSpec, codec_step, video_format};
use crate::media::MediaError;
use crate::util::wake::Waker;

/// A keyframe no decoder could take yet. Held rather than dropped: the receiving side cannot ask
/// for another, and without it the picture stays black until the encoder's next one.
#[derive(Debug)]
pub(super) struct ParkedKeyframe {
    bytes: Vec<u8>,
    presentation_us: u64,
}

impl ParkedKeyframe {
    pub(super) fn of(frame: &AssembledFrame) -> Option<Self> {
        frame.is_keyframe().then(|| Self {
            bytes: frame.bytes().to_vec(),
            presentation_us: presentation_us(frame),
        })
    }
}

/// A hardware decoder rendering into a view's surface as soon as each frame decodes.
#[derive(Debug)]
pub(super) struct VideoDecoder {
    spec: StreamSpec,
    codec: MediaCodec,
    events: Arc<CodecEvents>,
    free_inputs: VecDeque<usize>,
    parked: Option<ParkedKeyframe>,
    _surface: NativeWindow,
}

impl VideoDecoder {
    pub(super) fn start(
        spec: StreamSpec,
        surface: NativeWindow,
        parked: Option<ParkedKeyframe>,
        waker: &Waker,
    ) -> Result<Self, MediaError> {
        let mut codec = MediaCodec::from_decoder_type(video_format::mime(spec.params.codec))
            .ok_or(MediaError::NoCodec(spec.params.codec))?;
        let events = CodecEvents::new();
        codec
            .set_async_notify_callback(Some(codec_events::callback(&events, waker)))
            .map_err(codec_step("registering the decoder callbacks"))?;
        codec
            .configure(
                &video_format::decoder_format(&spec.params),
                Some(&surface),
                MediaCodecDirection::Decoder,
            )
            .map_err(codec_step("configuring the decoder"))?;
        codec.start().map_err(codec_step("starting the decoder"))?;
        Ok(Self {
            spec,
            codec,
            events,
            free_inputs: VecDeque::new(),
            parked,
            _surface: surface,
        })
    }

    pub(super) const fn spec(&self) -> StreamSpec {
        self.spec
    }

    /// Queues one access unit. With no input buffer free a keyframe is parked for the next one
    /// and anything else is dropped: queueing behind a busy decoder would only add latency.
    pub(super) fn decode(&mut self, frame: &AssembledFrame) -> Result<(), MediaError> {
        self.service()?;
        match self.free_inputs.pop_front() {
            Some(index) => self.queue(index, frame.bytes(), presentation_us(frame)),
            None => {
                if let Some(keyframe) = ParkedKeyframe::of(frame) {
                    self.parked = Some(keyframe);
                }
                Ok(())
            }
        }
    }

    fn queue(&self, index: usize, bytes: &[u8], presentation_us: u64) -> Result<(), MediaError> {
        let copied = self
            .codec
            .input_buffer(index)
            .and_then(|buffer| buffer.get_mut(..bytes.len()))
            .map_or(0, |target| {
                for (slot, byte) in target.iter_mut().zip(bytes) {
                    slot.write(*byte);
                }
                bytes.len()
            });
        self.codec
            .queue_input_buffer_by_index(index, 0, copied, presentation_us, 0)
            .map_err(codec_step("queueing a frame to decode"))
    }

    /// Collects free input buffers and renders every decoded frame at once.
    pub(super) fn service(&mut self) -> Result<(), MediaError> {
        while let Some(event) = self.events.pop() {
            match event {
                CodecEvent::InputAvailable(index) => match self.parked.take() {
                    Some(keyframe) => {
                        self.queue(index, &keyframe.bytes, keyframe.presentation_us)?;
                    }
                    None => self.free_inputs.push_back(index),
                },
                CodecEvent::OutputAvailable(output) => self
                    .codec
                    .release_output_buffer_by_index(output.index, output.size > 0)
                    .map_err(codec_step("rendering a decoded frame"))?,
                CodecEvent::Failed(detail) => return Err(MediaError::CodecFailed(detail)),
            }
        }
        Ok(())
    }
}

fn presentation_us(frame: &AssembledFrame) -> u64 {
    let presentation = frame.timestamp().to_duration(ClockRate::VIDEO).as_micros();
    u64::try_from(presentation).unwrap_or(u64::MAX)
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        if let Err(error) = self.codec.stop() {
            log::warn!("the decoder did not stop cleanly: {error}");
        }
    }
}
