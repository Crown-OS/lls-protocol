use core::net::SocketAddr;

use serde::{Deserialize, Serialize};

use crate::SignalingError;
use crate::message::limits::MAX_DRAG_NAME_BYTES;
use crate::message::{FfspTransferId, Validate};
use crate::util::bounds;

/// The computer's ffsp receiver for `ffsp_transfer_id` is listening at `address`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileListening<'a> {
    pub ffsp_transfer_id: FfspTransferId,
    /// `"<ip>:<port>"` exactly as the receiver reported binding it.
    pub address: &'a str,
}

impl FileListening<'_> {
    /// # Errors
    ///
    /// Returns [`SignalingError::OutOfRange`] when `address` is not an `ip:port` pair.
    pub fn socket_address(&self) -> Result<SocketAddr, SignalingError> {
        self.address
            .parse()
            .map_err(|_| SignalingError::OutOfRange {
                field: "listening address",
            })
    }
}

impl Validate for FileListening<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        self.socket_address().map(drop)
    }
}

/// The phone wants to send the file `name` of `len` bytes over ffsp transfer
/// `ffsp_transfer_id`, and waits for [`FileListening`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FilePush<'a> {
    pub ffsp_transfer_id: FfspTransferId,
    pub name: &'a str,
    pub len: u64,
}

impl Validate for FilePush<'_> {
    fn validate(&self) -> Result<(), SignalingError> {
        bounds::file_name("file name", self.name, MAX_DRAG_NAME_BYTES)
    }
}
