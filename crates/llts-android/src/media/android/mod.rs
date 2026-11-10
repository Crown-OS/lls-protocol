//! `MediaCodec` video in both directions and AAudio + Opus microphone capture.

mod codec_events;
#[cfg(feature = "opus")]
mod microphone;
#[cfg(feature = "opus")]
mod speaker;
mod surface;
mod video_decoder;
mod video_encoder;
mod video_format;

use llts_core::receive::AssembledFrame;
use llts_core::send::EncodedSlice;
use llts_core::stream::StreamKey;
use llts_core::units::Bitrate;
use llts_node::{FeatureRequest, MediaRole};
use llts_signaling::device::DeviceId;
use llts_signaling::message::{StreamParams, StreamRef, VideoParams};

pub(crate) use surface::Surface;
use video_decoder::{ParkedKeyframe, VideoDecoder};
use video_encoder::VideoEncoder;

use super::MediaError;
use crate::util::stream::stream_key;
use crate::util::wake::Waker;

#[cfg(feature = "opus")]
type Microphone = microphone::Microphone;
#[cfg(feature = "opus")]
type Speaker = speaker::Speaker;

/// One negotiated video stream, waiting for Kotlin's surface or running on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StreamSpec {
    peer: DeviceId,
    stream: StreamKey,
    role: MediaRole,
    params: VideoParams,
}

impl StreamSpec {
    fn is(&self, peer: DeviceId, stream: StreamKey) -> bool {
        self.peer == peer && self.stream == stream
    }
}

/// Every pipeline this device runs, by peer and stream.
#[derive(Debug)]
pub(crate) struct MediaPipelines {
    waker: Waker,
    pending: Vec<StreamSpec>,
    /// The latest keyframe of each decoder stream still waiting for its surface.
    parked: Vec<(StreamSpec, ParkedKeyframe)>,
    encoders: Vec<VideoEncoder>,
    decoders: Vec<VideoDecoder>,
    #[cfg(feature = "opus")]
    microphones: Vec<Microphone>,
    #[cfg(feature = "opus")]
    speakers: Vec<Speaker>,
}

impl MediaPipelines {
    pub(crate) const fn new(waker: Waker) -> Self {
        Self {
            waker,
            pending: Vec::new(),
            parked: Vec::new(),
            encoders: Vec::new(),
            decoders: Vec::new(),
            #[cfg(feature = "opus")]
            microphones: Vec::new(),
            #[cfg(feature = "opus")]
            speakers: Vec::new(),
        }
    }

    /// Video waits for a surface from Kotlin; the microphone and speaker start at once.
    pub(crate) fn start(
        &mut self,
        peer: DeviceId,
        _request: FeatureRequest,
        role: MediaRole,
        stream: StreamRef,
        params: StreamParams,
    ) -> Result<(), MediaError> {
        let Some(stream) = stream_key(stream) else {
            return Ok(());
        };
        self.stop_key(peer, stream);
        match (params, role) {
            (StreamParams::Video(params), _) => {
                self.pending.push(StreamSpec {
                    peer,
                    stream,
                    role,
                    params,
                });
                Ok(())
            }
            (StreamParams::Audio(_), MediaRole::Encoder) => self.start_microphone(peer, stream),
            (StreamParams::Audio(_), MediaRole::Decoder) => self.start_speaker(peer, stream),
        }
    }

    #[cfg(feature = "opus")]
    fn start_microphone(&mut self, peer: DeviceId, stream: StreamKey) -> Result<(), MediaError> {
        self.microphones
            .push(Microphone::start(peer, stream, &self.waker)?);
        Ok(())
    }

    #[cfg(feature = "opus")]
    fn start_speaker(&mut self, peer: DeviceId, stream: StreamKey) -> Result<(), MediaError> {
        self.speakers.push(Speaker::start(peer, stream)?);
        Ok(())
    }

    #[cfg(not(feature = "opus"))]
    const fn start_speaker(
        &mut self,
        _peer: DeviceId,
        _stream: StreamKey,
    ) -> Result<(), MediaError> {
        Err(MediaError::NoOpus)
    }

    #[cfg(not(feature = "opus"))]
    const fn start_microphone(
        &mut self,
        _peer: DeviceId,
        _stream: StreamKey,
    ) -> Result<(), MediaError> {
        Err(MediaError::NoOpus)
    }

    pub(crate) fn attach(
        &mut self,
        peer: DeviceId,
        index: u8,
        role: MediaRole,
        surface: Surface,
    ) -> Result<(), MediaError> {
        let position = self
            .pending
            .iter()
            .position(|spec| spec.peer == peer && spec.stream.index == index && spec.role == role)
            .ok_or(MediaError::NoPendingStream {
                index,
                expected: match role {
                    MediaRole::Encoder => "encoder",
                    MediaRole::Decoder => "decoder",
                },
            })?;
        let spec = self.pending.swap_remove(position);
        match role {
            MediaRole::Encoder => self.encoders.push(VideoEncoder::start(
                spec,
                surface.into_window(),
                &self.waker,
            )?),
            MediaRole::Decoder => {
                let parked = self
                    .parked
                    .iter()
                    .position(|(parked, _)| *parked == spec)
                    .map(|position| self.parked.swap_remove(position).1);
                self.decoders.push(VideoDecoder::start(
                    spec,
                    surface.into_window(),
                    parked,
                    &self.waker,
                )?);
            }
        }
        Ok(())
    }

    /// Stops the pipeline on stream `index` but keeps it negotiated, so a new surface — after a
    /// rotation, say — can take over.
    pub(crate) fn detach(&mut self, peer: DeviceId, index: u8) {
        let on_index = |spec: &StreamSpec| spec.peer == peer && spec.stream.index == index;
        let detached = self
            .encoders
            .iter()
            .map(VideoEncoder::spec)
            .chain(self.decoders.iter().map(VideoDecoder::spec))
            .filter(on_index)
            .collect::<Vec<_>>();
        self.encoders.retain(|encoder| !on_index(&encoder.spec()));
        self.decoders.retain(|decoder| !on_index(&decoder.spec()));
        self.pending.extend(detached);
    }

    pub(crate) fn stop(&mut self, peer: DeviceId, stream: StreamRef) {
        if let Some(stream) = stream_key(stream) {
            self.stop_key(peer, stream);
        }
    }

    fn stop_key(&mut self, peer: DeviceId, stream: StreamKey) {
        self.pending.retain(|spec| !spec.is(peer, stream));
        self.parked.retain(|(spec, _)| !spec.is(peer, stream));
        self.encoders
            .retain(|encoder| !encoder.spec().is(peer, stream));
        self.decoders
            .retain(|decoder| !decoder.spec().is(peer, stream));
        #[cfg(feature = "opus")]
        self.microphones
            .retain(|microphone| !microphone.is(peer, stream));
        #[cfg(feature = "opus")]
        self.speakers.retain(|speaker| !speaker.is(peer, stream));
    }

    /// Feeds a frame to its decoder. Until the stream's surface arrives only its latest keyframe
    /// is kept, to start the picture with.
    pub(crate) fn present(
        &mut self,
        peer: DeviceId,
        stream: StreamKey,
        frame: &AssembledFrame,
    ) -> Result<(), MediaError> {
        #[cfg(feature = "opus")]
        if let Some(speaker) = self
            .speakers
            .iter_mut()
            .find(|speaker| speaker.is(peer, stream))
        {
            speaker.play(frame);
            return Ok(());
        }
        if let Some(decoder) = self
            .decoders
            .iter_mut()
            .find(|decoder| decoder.spec().is(peer, stream))
        {
            return decoder.decode(frame);
        }
        let waiting = self
            .pending
            .iter()
            .find(|spec| spec.role == MediaRole::Decoder && spec.is(peer, stream));
        if let (Some(spec), Some(keyframe)) = (waiting, ParkedKeyframe::of(frame)) {
            let spec = *spec;
            self.parked.retain(|(parked, _)| *parked != spec);
            self.parked.push((spec, keyframe));
        }
        Ok(())
    }

    pub(crate) fn set_bitrate(&mut self, peer: DeviceId, stream: StreamKey, bitrate: Bitrate) {
        if let Some(encoder) = self.encoder(peer, stream) {
            encoder.set_bitrate(bitrate);
        }
    }

    pub(crate) fn request_keyframe(&mut self, peer: DeviceId, stream: StreamKey) {
        if let Some(encoder) = self.encoder(peer, stream) {
            encoder.request_keyframe();
        }
    }

    fn encoder(&mut self, peer: DeviceId, stream: StreamKey) -> Option<&mut VideoEncoder> {
        self.encoders
            .iter_mut()
            .find(|encoder| encoder.spec().is(peer, stream))
    }

    /// Moves every finished access unit and audio packet into `send`, and renders every decoded
    /// frame. A pipeline whose codec failed is dropped and its error returned.
    pub(crate) fn pump(
        &mut self,
        send: &mut dyn FnMut(DeviceId, StreamKey, EncodedSlice<'_>),
    ) -> Result<(), MediaError> {
        let mut failure = None;
        self.encoders
            .retain_mut(|encoder| keep_or_record(encoder.drain(send), &mut failure));
        self.decoders
            .retain_mut(|decoder| keep_or_record(decoder.service(), &mut failure));
        #[cfg(feature = "opus")]
        for microphone in &self.microphones {
            microphone.drain(send);
        }
        failure.map_or(Ok(()), Err)
    }
}

fn keep_or_record(outcome: Result<(), MediaError>, failure: &mut Option<MediaError>) -> bool {
    match outcome {
        Ok(()) => true,
        Err(error) => {
            *failure = Some(error);
            false
        }
    }
}

fn codec_step(step: &'static str) -> impl FnOnce(ndk::media_error::MediaError) -> MediaError {
    move |source| MediaError::Codec { step, source }
}
