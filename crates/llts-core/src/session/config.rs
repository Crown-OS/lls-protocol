use llts_wire::SessionTag;

use crate::buffer::BufferPool;
use crate::link::LinkKind;
use crate::send::DatagramSizing;
use crate::units::Bitrate;

/// Everything a peer session needs to know before it sees a packet.
#[derive(Debug, Clone, Copy)]
pub struct PeerConfig {
    /// The tag this side chose, which the peer stamps on everything it sends here.
    pub local_tag: SessionTag,
    /// The tag the peer chose, stamped on everything sent there.
    pub peer_tag: SessionTag,
    pub link: LinkKind,
    pub sizing: DatagramSizing,
    pub initial_bitrate: Bitrate,
    pub min_bitrate: Bitrate,
    pub max_bitrate: Bitrate,
}

impl PeerConfig {
    /// A LAN session tuned for 1080p60 screen sharing, the shape the whole design targets.
    pub const fn lan(local_tag: SessionTag, peer_tag: SessionTag, link: LinkKind) -> Self {
        Self {
            local_tag,
            peer_tag,
            link,
            sizing: DatagramSizing::conservative(),
            initial_bitrate: Bitrate::STANDARD,
            min_bitrate: Bitrate::LOW.scaled_by(1, 6),
            max_bitrate: Bitrate::HIGH,
        }
    }
}

/// The buffer pools a session draws from, shared by all its streams.
#[derive(Debug, Clone)]
pub struct SessionPools {
    /// One datagram each.
    pub datagrams: BufferPool,
    /// Whole reassembled video frames.
    pub frames: BufferPool,
    /// Parity shards waiting to see whether they are needed.
    pub shards: BufferPool,
    /// Reliable messages being reassembled, up to [`crate::channel::MAX_MESSAGE_LEN`] each.
    pub messages: BufferPool,
}

impl SessionPools {
    /// Pools sized for one 1080p screen share plus audio, control and input.
    pub fn for_screen_share(mtu: usize) -> Self {
        Self {
            datagrams: BufferPool::new(mtu, 4_096),
            frames: BufferPool::new(1 << 20, 8),
            shards: BufferPool::new(mtu, 1_024),
            messages: BufferPool::new(crate::channel::MAX_MESSAGE_LEN, 4),
        }
    }
}
