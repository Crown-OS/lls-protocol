use serde::{Deserialize, Serialize};

/// An ffsp `TransferId`, naming the transfer that carries a file or an oversized clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FfspTransferId(pub u32);

/// Drives the phone line of the receiving device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Call<'a> {
    Pickup { call_id: &'a str },
    Decline { call_id: &'a str },
    Hangup { call_id: &'a str },
    Dial { number: &'a str },
}

/// Drives the receiving device's active media session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Media {
    Play,
    Pause,
    Next,
    Previous,
    Seek { percent: u8 },
}

/// An inline reply to the conversation a notification came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reply<'a> {
    pub conversation_id: &'a str,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SetVolume {
    pub percent: u8,
    pub muted: bool,
}

/// Locks the receiving device's screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Lock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SetHotspot {
    pub enabled: bool,
}

/// Tells the receiver an ffsp transfer is waiting for it, so it can accept on the ffsp side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileOffer {
    pub ffsp_transfer_id: FfspTransferId,
}
