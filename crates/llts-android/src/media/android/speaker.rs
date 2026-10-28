use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use llts_core::receive::AssembledFrame;
use llts_core::stream::StreamKey;
use llts_signaling::device::DeviceId;
use ndk::audio::{
    AudioCallbackResult, AudioDirection, AudioFormat, AudioPerformanceMode, AudioSharingMode,
    AudioStream, AudioStreamBuilder, AudioUsage,
};
use opus::{Channels, Decoder};

use crate::media::MediaError;

const SAMPLE_RATE: u32 = 48_000;
/// 10 ms at 48 kHz, the frame the node negotiates for voice.
const FRAME_SAMPLES: usize = 480;
/// Opus carries at most 120 ms in one packet.
const MAX_PACKET_SAMPLES: usize = 5_760;
/// Decoded frames waiting for the output callback. Four is 40 ms; past that the oldest frame is
/// dropped, so a burst after a stall costs one glitch instead of lasting latency.
const PLAYOUT_FRAMES: usize = 4;

/// One decoded 10 ms frame, carried by value so the audio thread never allocates.
#[derive(Debug)]
struct PcmFrame {
    samples: [i16; FRAME_SAMPLES],
}

/// The peer's Opus voice played through low-latency AAudio. Packets are decoded on the runtime
/// thread as they arrive; the output callback only copies samples.
#[derive(Debug)]
pub(super) struct Speaker {
    peer: DeviceId,
    stream: StreamKey,
    decoder: Decoder,
    decoded: Vec<i16>,
    frames: Arc<ArrayQueue<PcmFrame>>,
    playback: AudioStream,
}

impl Speaker {
    pub(super) fn start(peer: DeviceId, stream: StreamKey) -> Result<Self, MediaError> {
        let frames = Arc::new(ArrayQueue::new(PLAYOUT_FRAMES));
        let mut drain = PcmDrain {
            frames: Arc::clone(&frames),
            current: None,
            offset: 0,
        };
        let playback = AudioStreamBuilder::new()
            .map_err(audio_step("creating the playback builder"))?
            .direction(AudioDirection::Output)
            .sharing_mode(AudioSharingMode::Exclusive)
            .performance_mode(AudioPerformanceMode::LowLatency)
            .usage(AudioUsage::Media)
            .format(AudioFormat::PCM_I16)
            .sample_rate(SAMPLE_RATE.cast_signed())
            .channel_count(1)
            .data_callback(Box::new(move |_stream, data, frames| {
                let len = usize::try_from(frames).unwrap_or(0);
                // SAFETY: AAudio hands a mono PCM_I16 output callback room for `frames` samples
                // at `data`, valid and exclusively ours for the duration of the call.
                let out = unsafe { std::slice::from_raw_parts_mut(data.cast::<i16>(), len) };
                drain.fill(out);
                AudioCallbackResult::Continue
            }))
            .open_stream()
            .map_err(audio_step("opening the speaker"))?;
        playback
            .request_start()
            .map_err(audio_step("starting the speaker"))?;
        Ok(Self {
            peer,
            stream,
            decoder: Decoder::new(SAMPLE_RATE, Channels::Mono)?,
            decoded: vec![0; MAX_PACKET_SAMPLES],
            frames,
            playback,
        })
    }

    pub(super) fn is(&self, peer: DeviceId, stream: StreamKey) -> bool {
        self.peer == peer && self.stream == stream
    }

    /// Decodes one packet into the playout queue; a packet Opus cannot read is skipped rather
    /// than ending the stream.
    pub(super) fn play(&mut self, frame: &AssembledFrame) {
        let Ok(samples) = self.decoder.decode(frame.bytes(), &mut self.decoded, false) else {
            return;
        };
        let Some(decoded) = self.decoded.get(..samples) else {
            return;
        };
        for chunk in decoded.chunks(FRAME_SAMPLES) {
            let mut pcm = PcmFrame {
                samples: [0; FRAME_SAMPLES],
            };
            if let Some(target) = pcm.samples.get_mut(..chunk.len()) {
                target.copy_from_slice(chunk);
            }
            let _ = self.frames.force_push(pcm);
        }
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        if let Err(error) = self.playback.request_stop() {
            log::warn!("the speaker did not stop cleanly: {error}");
        }
    }
}

fn audio_step(step: &'static str) -> impl FnOnce(ndk::audio::AudioError) -> MediaError {
    move |source| MediaError::Audio { step, source }
}

/// Feeds the output callback from the playout queue, whatever burst size it asks for, and plays
/// silence when the network runs dry.
struct PcmDrain {
    frames: Arc<ArrayQueue<PcmFrame>>,
    current: Option<PcmFrame>,
    offset: usize,
}

impl PcmDrain {
    fn fill(&mut self, mut out: &mut [i16]) {
        while !out.is_empty() {
            if self.current.is_none() {
                self.current = self.frames.pop();
                self.offset = 0;
            }
            let Some(frame) = &self.current else {
                out.fill(0);
                return;
            };
            let available = frame.samples.get(self.offset..).unwrap_or_default();
            let take = available.len().min(out.len());
            let (head, rest) = out.split_at_mut(take);
            if let Some(source) = available.get(..take) {
                head.copy_from_slice(source);
            }
            out = rest;
            self.offset += take;
            if self.offset >= FRAME_SAMPLES {
                self.current = None;
            }
        }
    }
}
