use serde::{Deserialize, Serialize};

/// Most codecs one media kind can list; three video codecs exist today.
pub const MAX_CODECS: usize = 4;

/// Codecs in preference order, most preferred first, without a heap allocation.
pub type CodecList<C> = heapless::Vec<C, MAX_CODECS>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoCodec {
    Hevc,
    H264,
    Av1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioCodec {
    Opus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreamCodec {
    Video(VideoCodec),
    Audio(AudioCodec),
}

/// The first codec in `ours` that `theirs` also lists, so the side choosing keeps its own order.
pub fn negotiate_codec<C: Copy + PartialEq>(ours: &[C], theirs: &[C]) -> Option<C> {
    ours.iter().copied().find(|codec| theirs.contains(codec))
}
