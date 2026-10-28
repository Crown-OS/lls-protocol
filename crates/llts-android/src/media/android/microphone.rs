use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use llts_core::send::EncodedSlice;
use llts_core::stream::StreamKey;
use llts_signaling::device::DeviceId;
use llts_wire::MediaTimestamp;
use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioFormat, AudioInputPreset, AudioPerformanceMode,
    AudioSharingMode, AudioStream, AudioStreamBuilder,
};
use opus::{Application, Bitrate, Channels, Encoder};

use crate::media::MediaError;
use crate::util::wake::Waker;

const SAMPLE_RATE: u32 = 48_000;
/// 10 ms at 48 kHz, the frame the node negotiates for voice.
const FRAME_SAMPLES: usize = 480;
/// Opus at 32 kb/s packs 10 ms into about 40 bytes; this leaves room for any bitrate it uses.
const MAX_PACKET: usize = 400;
const PACKET_QUEUE: usize = 32;
const VOICE_BITRATE: i32 = 32_000;
/// Tells Opus to spend bits on in-band FEC for this much loss.
const EXPECTED_LOSS_PERCENT: i32 = 10;

/// One encoded 10 ms frame, carried by value so the audio thread never allocates.
#[derive(Debug)]
struct AudioPacket {
    bytes: [u8; MAX_PACKET],
    len: usize,
    first_sample: u32,
}

/// Low-latency AAudio capture, encoded to Opus on the audio thread; only packets of a few
/// dozen bytes cross to the runtime.
#[derive(Debug)]
pub(super) struct Microphone {
    peer: DeviceId,
    stream: StreamKey,
    packets: Arc<ArrayQueue<AudioPacket>>,
    capture: AudioStream,
}

impl Microphone {
    pub(super) fn start(
        peer: DeviceId,
        stream: StreamKey,
        waker: &Waker,
    ) -> Result<Self, MediaError> {
        let packets = Arc::new(ArrayQueue::new(PACKET_QUEUE));
        let mut framer = PcmFramer {
            pcm: [0; FRAME_SAMPLES],
            filled: 0,
            next_sample: 0,
            encoder: voice_encoder()?,
            packets: Arc::clone(&packets),
            waker: waker.clone(),
        };
        let capture = AudioStreamBuilder::new()
            .map_err(audio_step("creating the capture builder"))?
            .direction(AudioDirection::Input)
            .sharing_mode(AudioSharingMode::Exclusive)
            .performance_mode(AudioPerformanceMode::LowLatency)
            .input_preset(AudioInputPreset::VoiceCommunication)
            .format(AudioFormat::PCM_I16)
            .sample_rate(SAMPLE_RATE.cast_signed())
            .channel_count(1)
            .data_callback(Box::new(move |_stream, data, frames| {
                let len = usize::try_from(frames).unwrap_or(0);
                // SAFETY: AAudio hands a mono PCM_I16 input callback `frames` samples at
                // `data`, valid for the duration of the call.
                let samples = unsafe { std::slice::from_raw_parts(data.cast::<i16>(), len) };
                framer.push(samples);
                AudioCallbackResult::Continue
            }))
            .open_stream()
            .map_err(audio_step("opening the microphone"))?;
        capture
            .request_start()
            .map_err(audio_step("starting the microphone"))?;
        Ok(Self {
            peer,
            stream,
            packets,
            capture,
        })
    }

    pub(super) fn is(&self, peer: DeviceId, stream: StreamKey) -> bool {
        self.peer == peer && self.stream == stream
    }

    pub(super) fn drain(&self, send: &mut dyn FnMut(DeviceId, StreamKey, EncodedSlice<'_>)) {
        while let Some(packet) = self.packets.pop() {
            if let Some(bytes) = packet.bytes.get(..packet.len) {
                let timestamp = MediaTimestamp::new(packet.first_sample);
                send(
                    self.peer,
                    self.stream,
                    EncodedSlice::new(bytes, timestamp).ending_frame(),
                );
            }
        }
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        if let Err(error) = self.capture.request_stop() {
            log::warn!("the microphone did not stop cleanly: {error}");
        }
    }
}

fn voice_encoder() -> Result<Encoder, MediaError> {
    let mut encoder = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip)?;
    encoder.set_bitrate(Bitrate::Bits(VOICE_BITRATE))?;
    encoder.set_inband_fec(true)?;
    encoder.set_packet_loss_perc(EXPECTED_LOSS_PERCENT)?;
    Ok(encoder)
}

fn audio_step(step: &'static str) -> impl FnOnce(ndk::audio::AudioError) -> MediaError {
    move |source| MediaError::Audio { step, source }
}

/// Cuts the capture callback's bursts, whatever their size, into 10 ms Opus frames.
struct PcmFramer {
    pcm: [i16; FRAME_SAMPLES],
    filled: usize,
    next_sample: u32,
    encoder: Encoder,
    packets: Arc<ArrayQueue<AudioPacket>>,
    waker: Waker,
}

impl PcmFramer {
    fn push(&mut self, mut samples: &[i16]) {
        while !samples.is_empty() {
            let take = (FRAME_SAMPLES - self.filled).min(samples.len());
            let Some((head, rest)) = samples.split_at_checked(take) else {
                return;
            };
            if let Some(target) = self.pcm.get_mut(self.filled..self.filled + take) {
                target.copy_from_slice(head);
            }
            self.filled += take;
            samples = rest;
            if self.filled == FRAME_SAMPLES {
                self.encode_frame();
                self.filled = 0;
            }
        }
    }

    fn encode_frame(&mut self) {
        let mut packet = AudioPacket {
            bytes: [0; MAX_PACKET],
            len: 0,
            first_sample: self.next_sample,
        };
        self.next_sample = self
            .next_sample
            .wrapping_add(u32::try_from(FRAME_SAMPLES).unwrap_or(0));
        let Ok(len) = self.encoder.encode(&self.pcm, &mut packet.bytes) else {
            return;
        };
        packet.len = len;
        if self.packets.push(packet).is_ok() {
            self.waker.wake();
        }
    }
}
