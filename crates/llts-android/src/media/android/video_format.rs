use llts_signaling::message::{VideoCodec, VideoParams};
use ndk::media::media_format::MediaFormat;

/// `MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface`.
const COLOR_FORMAT_SURFACE: i32 = 0x7F00_0789;
/// `MediaCodecInfo.EncoderCapabilities.BITRATE_MODE_CBR`: a steady rate paces best.
const BITRATE_MODE_CBR: i32 = 2;
/// Keyframes come on request after the first; this only bounds how long a missed request can
/// leave a picture broken.
const KEYFRAME_INTERVAL_SECONDS: i32 = 60;
/// A still screen is re-sent this often, so a viewer that joins late still gets a picture.
const REPEAT_STILL_FRAME_AFTER_US: i64 = 100_000;
const REALTIME_PRIORITY: i32 = 0;

pub(super) const fn mime(codec: VideoCodec) -> &'static str {
    match codec {
        VideoCodec::Hevc => "video/hevc",
        VideoCodec::H264 => "video/avc",
        VideoCodec::Av1 => "video/av01",
    }
}

pub(super) fn encoder_format(params: &VideoParams) -> MediaFormat {
    let mut format = common_format(params);
    format.set_i32("color-format", COLOR_FORMAT_SURFACE);
    format.set_i32("bitrate", bits_per_second(params.bitrate_kbps));
    format.set_i32("bitrate-mode", BITRATE_MODE_CBR);
    format.set_f32("frame-rate", frames_per_second(params.fps_mhz));
    format.set_i32("i-frame-interval", KEYFRAME_INTERVAL_SECONDS);
    format.set_i32("max-bframes", 0);
    format.set_i32("latency", 1);
    format.set_i32("prepend-sps-pps-to-idr-frames", 1);
    format.set_i64("repeat-previous-frame-after", REPEAT_STILL_FRAME_AFTER_US);
    format
}

pub(super) fn decoder_format(params: &VideoParams) -> MediaFormat {
    let mut format = common_format(params);
    format.set_i32("low-latency", 1);
    format
}

fn common_format(params: &VideoParams) -> MediaFormat {
    let mut format = MediaFormat::new();
    format.set_str("mime", mime(params.codec));
    format.set_i32("width", i32::from(params.width));
    format.set_i32("height", i32::from(params.height));
    format.set_i32("priority", REALTIME_PRIORITY);
    format.set_f32("operating-rate", frames_per_second(params.fps_mhz));
    format
}

pub(super) fn bits_per_second(kbps: u32) -> i32 {
    i32::try_from(u64::from(kbps) * 1_000).unwrap_or(i32::MAX)
}

fn frames_per_second(fps_mhz: u32) -> f32 {
    fps_mhz as f32 / 1_000.0
}
