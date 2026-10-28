use crate::SignalingError;
use crate::message::{
    AcceptStream, Call, Capabilities, CloseStream, Envelope, FileOffer, Goodbye, KeyframeRequest,
    Lock, Media, Message, OpenStream, Rekey, Reply, SetHotspot, SetVolume, Snapshot, StartCamera,
    StartMic, StartMirror, StartMonitor, Stop, Subscribe, UnicursorEnter, UnicursorLeave,
    Unsubscribe, Update,
};

/// The lowest code the application may use; llts-core reserves every control message below it.
pub const FIRST_APPLICATION_TYPE: u16 = 0x0100;

macro_rules! message_registry {
    ($lt:lifetime; $($variant:ident = $code:literal => $message:ty),+ $(,)?) => {
        /// Every message this build understands, by the `u16` its envelope carries.
        ///
        /// Codes are grouped by module in blocks of 256 and are never reused. The first block,
        /// everything below [`FIRST_APPLICATION_TYPE`], belongs to the llts transport, which
        /// consumes those messages itself and refuses to carry them for the application.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u16)]
        #[non_exhaustive]
        pub enum MessageType {
            $($variant = $code),+
        }

        impl MessageType {
            pub const ALL: &[Self] = &[$(Self::$variant),+];

            pub const fn from_u16(raw: u16) -> Option<Self> {
                match raw {
                    $($code => Some(Self::$variant),)+
                    _ => None,
                }
            }

            pub const fn as_u16(self) -> u16 {
                self as u16
            }
        }

        $(impl<$lt> Message<$lt> for $message {
            const TYPE: MessageType = MessageType::$variant;
        })+

        /// A decoded channel message, borrowing its strings and bytes from the received buffer.
        #[derive(Debug, Clone, PartialEq, Eq)]
        #[non_exhaustive]
        pub enum Signal<$lt> {
            $($variant($message),)+
            /// A type registered after this build; the caller skips it.
            Unknown { msg_type: u16 },
        }

        impl<$lt> Signal<$lt> {
            /// # Errors
            ///
            /// Returns [`SignalingError::TruncatedEnvelope`] for a message too short to carry a
            /// type, and [`SignalingError::MalformedBody`] for a known type whose body does not
            /// decode.
            pub fn decode(message: &$lt [u8]) -> Result<Self, SignalingError> {
                let envelope = Envelope::parse(message)?;
                let Some(msg_type) = envelope.message_type() else {
                    return Ok(Self::Unknown { msg_type: envelope.msg_type });
                };
                match msg_type {
                    $(MessageType::$variant => envelope.decode_body().map(Self::$variant),)+
                }
            }

            /// Appends this message's envelope to `buffer`.
            ///
            /// # Errors
            ///
            /// Returns [`SignalingError::Encode`] if the body cannot be encoded, and
            /// [`SignalingError::UnknownMessageType`] for [`Signal::Unknown`], whose body was never
            /// kept.
            pub fn encode(&self, buffer: Vec<u8>) -> Result<Vec<u8>, SignalingError> {
                match self {
                    $(Self::$variant(message) => message.encode(buffer),)+
                    Self::Unknown { msg_type } => Err(SignalingError::UnknownMessageType(*msg_type)),
                }
            }

            pub const fn message_type(&self) -> Option<MessageType> {
                match self {
                    $(Self::$variant(_) => Some(MessageType::$variant),)+
                    Self::Unknown { .. } => None,
                }
            }
        }
    };
}

message_registry! { 'a;
    CapabilitiesV1 = 0x0100 => Capabilities,
    GoodbyeV1 = 0x0101 => Goodbye,
    RekeyV1 = 0x0102 => Rekey,
    KeyframeRequestV1 = 0x0103 => KeyframeRequest,

    OpenStreamV1 = 0x0200 => OpenStream,
    AcceptStreamV1 = 0x0201 => AcceptStream,
    CloseStreamV1 = 0x0202 => CloseStream,

    SubscribeV1 = 0x0300 => Subscribe,
    UnsubscribeV1 = 0x0301 => Unsubscribe,
    UpdateV1 = 0x0302 => Update<'a>,
    SnapshotV1 = 0x0303 => Snapshot<'a>,

    CallV1 = 0x0400 => Call<'a>,
    MediaV1 = 0x0401 => Media,
    ReplyV1 = 0x0402 => Reply<'a>,
    SetVolumeV1 = 0x0403 => SetVolume,
    LockV1 = 0x0404 => Lock,
    SetHotspotV1 = 0x0405 => SetHotspot,
    FileOfferV1 = 0x0406 => FileOffer,

    StartMirrorV1 = 0x0500 => StartMirror,
    StartCameraV1 = 0x0501 => StartCamera,
    StartMicV1 = 0x0502 => StartMic,
    StartMonitorV1 = 0x0503 => StartMonitor,
    UnicursorEnterV1 = 0x0504 => UnicursorEnter,
    UnicursorLeaveV1 = 0x0505 => UnicursorLeave,
    StopV1 = 0x0506 => Stop,
}
