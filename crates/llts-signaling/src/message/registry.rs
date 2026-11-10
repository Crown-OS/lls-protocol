use crate::SignalingError;
use crate::message::{
    AcceptStream, BlobAck, BlobCancel, BlobChunk, BlobEnd, BlobStart, Call, CallCounters,
    CallLogPage, Capabilities, Capture, CloseStream, ContactCounters, ContactPage,
    ConversationPage, DragCancel, DragDrop, DragEnter, Envelope, FetchBlob, FileListening,
    FileOffer, FilePush, Goodbye, KeyframeRequest, ListCallLog, ListContacts, ListConversations,
    ListMessages, ListVoicemail, LiveActivityAction, Locate, Location, Lock, MarkRead, Media,
    Message, MessageCounters, MessageEvent, MessagePage, OpenStream, OpenUri, Rekey,
    ReleaseHeadset, Reply, RequestCancel, RequestDone, RequestFailed, Ring, SendMessage,
    SetHotspot, SetVolume, Snapshot, StartAudio, StartCamera, StartCast, StartMic, StartMirror,
    StartMonitor, Stop, Subscribe, TakeHeadset, UnicursorEnter, UnicursorLeave, Unsubscribe,
    Update, Validate, VoicemailPage, WifiCredentials, WifiRequest,
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

            /// Checks the body against the caps its type keeps to; see [`Validate`].
            ///
            /// # Errors
            ///
            /// Returns the first cap the body breaks.
            pub fn validate(&self) -> Result<(), SignalingError> {
                match self {
                    $(Self::$variant(message) => Validate::validate(message),)+
                    Self::Unknown { .. } => Ok(()),
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

    OpenUriV1 = 0x0407 => OpenUri<'a>,
    LiveActivityActionV1 = 0x0408 => LiveActivityAction<'a>,
    RingV1 = 0x0409 => Ring,
    WifiCredentialsV1 = 0x040A => WifiCredentials<'a>,
    TakeHeadsetV1 = 0x040B => TakeHeadset<'a>,
    ReleaseHeadsetV1 = 0x040C => ReleaseHeadset<'a>,
    FileListeningV1 = 0x040D => FileListening<'a>,
    FilePushV1 = 0x040E => FilePush<'a>,

    StartCastV1 = 0x0507 => StartCast,
    StartAudioV1 = 0x0508 => StartAudio,
    DragEnterV1 = 0x0509 => DragEnter<'a>,
    DragDropV1 = 0x050A => DragDrop,
    DragCancelV1 = 0x050B => DragCancel,

    RequestFailedV1 = 0x0600 => RequestFailed<'a>,
    RequestDoneV1 = 0x0601 => RequestDone<'a>,
    ListConversationsV1 = 0x0602 => ListConversations<'a>,
    ConversationPageV1 = 0x0603 => ConversationPage<'a>,
    ListMessagesV1 = 0x0604 => ListMessages<'a>,
    MessagePageV1 = 0x0605 => MessagePage<'a>,
    SendMessageV1 = 0x0606 => SendMessage<'a>,
    MarkReadV1 = 0x0607 => MarkRead,
    ListCallLogV1 = 0x0608 => ListCallLog<'a>,
    CallLogPageV1 = 0x0609 => CallLogPage<'a>,
    ListVoicemailV1 = 0x060A => ListVoicemail<'a>,
    VoicemailPageV1 = 0x060B => VoicemailPage<'a>,
    ListContactsV1 = 0x060C => ListContacts<'a>,
    ContactPageV1 = 0x060D => ContactPage<'a>,
    FetchBlobV1 = 0x060E => FetchBlob<'a>,
    CaptureV1 = 0x060F => Capture,
    LocateV1 = 0x0610 => Locate,
    LocationV1 = 0x0611 => Location,
    WifiRequestV1 = 0x0612 => WifiRequest<'a>,
    RequestCancelV1 = 0x0613 => RequestCancel,

    MessageEventV1 = 0x0700 => MessageEvent<'a>,
    BlobStartV1 = 0x0701 => BlobStart<'a>,
    BlobChunkV1 = 0x0702 => BlobChunk<'a>,
    BlobEndV1 = 0x0703 => BlobEnd,
    BlobAckV1 = 0x0704 => BlobAck,
    BlobCancelV1 = 0x0705 => BlobCancel,
    MessageCountersV1 = 0x0706 => MessageCounters,
    CallCountersV1 = 0x0707 => CallCounters,
    ContactCountersV1 = 0x0708 => ContactCounters,
}

impl Validate for Capabilities {}
impl Validate for Goodbye {}
impl Validate for Rekey {}
impl Validate for KeyframeRequest {}
impl Validate for OpenStream {}
impl Validate for AcceptStream {}
impl Validate for CloseStream {}
impl Validate for Subscribe {}
impl Validate for Unsubscribe {}
impl Validate for Update<'_> {}
impl Validate for Snapshot<'_> {}
impl Validate for Call<'_> {}
impl Validate for Media {}
impl Validate for Reply<'_> {}
impl Validate for SetVolume {}
impl Validate for Lock {}
impl Validate for SetHotspot {}
impl Validate for FileOffer {}
impl Validate for StartMirror {}
impl Validate for StartCamera {}
impl Validate for StartMic {}
impl Validate for StartMonitor {}
impl Validate for UnicursorEnter {}
impl Validate for UnicursorLeave {}
impl Validate for Stop {}
impl Validate for StartCast {}
impl Validate for StartAudio {}
