use llts_node::{CallCommand, OutgoingCommand, RemoteCommand};
use llts_signaling::message::{
    Call, FileOffer, Media, Reply, SetHotspot, SetVolume, UnicursorEnter, UnicursorLeave,
};
use serde::{Deserialize, Serialize};

/// A command between devices that owns its strings, so it can wait in the command queue.
///
/// One JSON shape serves both directions — what Kotlin sends and what a `remoteCommand` event
/// carries — using serde's default for the signalling types, e.g. `{"Media":"Play"}` or
/// `{"Call":{"Pickup":{"call_id":"7"}}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum PeerCommand {
    Call(CallRequest),
    Media(Media),
    Reply {
        conversation_id: String,
        text: String,
    },
    SetVolume(SetVolume),
    Lock,
    SetHotspot(SetHotspot),
    FileOffer(FileOffer),
    UnicursorEnter(UnicursorEnter),
    UnicursorLeave(UnicursorLeave),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum CallRequest {
    Pickup { call_id: String },
    Decline { call_id: String },
    Hangup { call_id: String },
    Dial { number: String },
}

impl PeerCommand {
    pub(crate) fn as_outgoing(&self) -> OutgoingCommand<'_> {
        match self {
            Self::Call(call) => OutgoingCommand::Call(call.as_call()),
            Self::Media(media) => OutgoingCommand::Media(*media),
            Self::Reply {
                conversation_id,
                text,
            } => OutgoingCommand::Reply(Reply {
                conversation_id,
                text,
            }),
            Self::SetVolume(volume) => OutgoingCommand::SetVolume(*volume),
            Self::Lock => OutgoingCommand::Lock,
            Self::SetHotspot(hotspot) => OutgoingCommand::SetHotspot(*hotspot),
            Self::FileOffer(offer) => OutgoingCommand::FileOffer(*offer),
            Self::UnicursorEnter(enter) => OutgoingCommand::UnicursorEnter(*enter),
            Self::UnicursorLeave(leave) => OutgoingCommand::UnicursorLeave(*leave),
        }
    }
}

impl CallRequest {
    fn as_call(&self) -> Call<'_> {
        match self {
            Self::Pickup { call_id } => Call::Pickup { call_id },
            Self::Decline { call_id } => Call::Decline { call_id },
            Self::Hangup { call_id } => Call::Hangup { call_id },
            Self::Dial { number } => Call::Dial { number },
        }
    }
}

impl From<&RemoteCommand> for PeerCommand {
    fn from(command: &RemoteCommand) -> Self {
        match command {
            RemoteCommand::Call(call) => Self::Call(CallRequest::from(call)),
            RemoteCommand::Media(media) => Self::Media(*media),
            RemoteCommand::Reply {
                conversation_id,
                text,
            } => Self::Reply {
                conversation_id: conversation_id.clone(),
                text: text.clone(),
            },
            RemoteCommand::SetVolume(volume) => Self::SetVolume(*volume),
            RemoteCommand::Lock => Self::Lock,
            RemoteCommand::SetHotspot(hotspot) => Self::SetHotspot(*hotspot),
            RemoteCommand::FileOffer(offer) => Self::FileOffer(*offer),
            RemoteCommand::UnicursorEnter(enter) => Self::UnicursorEnter(*enter),
            RemoteCommand::UnicursorLeave(leave) => Self::UnicursorLeave(*leave),
        }
    }
}

impl From<&CallCommand> for CallRequest {
    fn from(call: &CallCommand) -> Self {
        match call {
            CallCommand::Pickup { call_id } => Self::Pickup {
                call_id: call_id.clone(),
            },
            CallCommand::Decline { call_id } => Self::Decline {
                call_id: call_id.clone(),
            },
            CallCommand::Hangup { call_id } => Self::Hangup {
                call_id: call_id.clone(),
            },
            CallCommand::Dial { number } => Self::Dial {
                number: number.clone(),
            },
        }
    }
}
