use llts_wire::WireError;

/// Every way the protocol engine can refuse to make progress.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CoreError {
    #[error("malformed packet")]
    Wire(#[from] WireError),

    #[error("write of {requested} bytes exceeds the {capacity}-byte buffer")]
    BufferOverflow { requested: usize, capacity: usize },

    #[error("frame {frame_id} needs {required} fragments, more than the {limit} supported")]
    FrameTooFragmented {
        frame_id: u16,
        required: usize,
        limit: usize,
    },

    #[error("a {payload_len}-byte payload does not fit a {mtu}-byte datagram")]
    PayloadExceedsMtu { payload_len: usize, mtu: usize },

    #[error("no repair block is open")]
    RepairBlockNotStarted,

    #[error("{data_shards} data and {parity_shards} parity shards is not a codeable geometry")]
    UnsupportedRepairGeometry {
        data_shards: u16,
        parity_shards: u16,
    },

    #[error("repair shard must be {expected} bytes, got {actual}")]
    RepairShardSize { expected: usize, actual: usize },

    #[error("repair block needs {required} shards, only {available} are available")]
    InsufficientRepairShards { required: u16, available: u16 },

    #[error("no open stream matches identifier {stream:#010x}")]
    UnknownStream { stream: u32 },

    #[error("a {class} stream cannot carry a {kind} packet")]
    StreamClassMismatch {
        class: &'static str,
        kind: &'static str,
    },

    #[error("{class} stream {index} is already open")]
    StreamAlreadyOpen { class: &'static str, index: u8 },

    #[error("a session holds at most {limit} streams")]
    TooManyStreams { limit: usize },

    #[error("a {len}-byte message exceeds the {limit}-byte limit")]
    MessageTooLarge { len: usize, limit: usize },

    #[error("the stream already holds {limit} bytes waiting to be acknowledged")]
    SendQueueFull { limit: usize },

    #[error("message type {message_type:#06x} is reserved for the session itself")]
    ReservedMessageType { message_type: u16 },
}
