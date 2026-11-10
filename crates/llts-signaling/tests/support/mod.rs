//! One sample of every application message, shared by the round-trip and routing tests.

use llts_signaling::device::Feature;
use llts_signaling::message::{
    Attachment, AttachmentSpec, BlobAck, BlobCancel, BlobCancelReason, BlobChunk, BlobEnd, BlobId,
    BlobPurpose, BlobRole, BlobStart, CallCapabilities, CallCounters, CallKind, CallLogEntry,
    CallLogPage, Capture, CaptureKind, Contact, ContactCapabilities, ContactCounters, ContactPage,
    ContactPhone, Conversation, ConversationPage, DeliveryStatus, Direction, DragCancel, DragDrop,
    DragEnter, DragId, DragItem, DragItemKind, Edge, EdgePosition, FetchBlob, FfspTransferId,
    FileListening, FilePush, ListCallLog, ListContacts, ListConversations, ListMessages,
    ListVoicemail, LiveActivityAction, Locate, Location, MarkRead, MessageCapabilities,
    MessageChange, MessageCounters, MessageEvent, MessageFolder, MessageItem, MessagePage, OpenUri,
    PageRequest, Participant, PhoneKind, ReleaseHeadset, RequestCancel, RequestDone, RequestError,
    RequestFailed, RequestId, Ring, RingAction, SendMessage, Signal, StartAudio, StartCast,
    TakeHeadset, VideoLimits, Voicemail, VoicemailPage, WifiCredentials, WifiRequest, WifiSecurity,
};

pub(crate) const REQUEST: RequestId = RequestId(u32::MAX);

const FIRST_PAGE: PageRequest<'static> = PageRequest {
    cursor: None,
    limit: 50,
};

const NEXT_PAGE: PageRequest<'static> = PageRequest {
    cursor: Some("1700000000000:s:41"),
    limit: 200,
};

pub(crate) fn message_item() -> MessageItem<'static> {
    MessageItem {
        thread_id: 7,
        message_id: "mms/12",
        folder: MessageFolder::INBOX,
        sender: Some("+44 20 7946 0000"),
        unix_ms: 1_700_000_000_000,
        text: Some("see you at 8 ✓"),
        text_truncated: false,
        read: false,
        status: DeliveryStatus::NONE,
        attachments: vec![Attachment {
            key: "mms-part/3/1700000000000",
            mime: "image/jpeg",
            name: Some("IMG_0001.jpg"),
            len: 1_048_576,
            thumbnail_key: Some("mms-thumb/3/1700000000000/256"),
            width: 4032,
            height: 3024,
        }],
        attachments_omitted: 0,
        sim_slot: Some(1),
    }
}

pub(crate) fn application_messages() -> Vec<Signal<'static>> {
    vec![
        Signal::OpenUriV1(OpenUri {
            uri: "https://crownos.org/?q=a b",
        }),
        Signal::LiveActivityActionV1(LiveActivityAction {
            activity_id: "0|com.timer|1|null|10123",
            action_id: "pause",
        }),
        Signal::RingV1(Ring {
            action: RingAction::Start,
        }),
        Signal::RingV1(Ring {
            action: RingAction::Stop,
        }),
        Signal::WifiCredentialsV1(WifiCredentials {
            request: Some(REQUEST),
            ssid: "Crown \"5G\"",
            security: WifiSecurity::WPA3_SAE,
            passphrase: "correct horse battery staple",
            hidden: true,
        }),
        Signal::TakeHeadsetV1(TakeHeadset {
            address: "AA:BB:CC:DD:EE:0F",
        }),
        Signal::ReleaseHeadsetV1(ReleaseHeadset {
            address: "00:11:22:33:44:55",
        }),
        Signal::FileListeningV1(FileListening {
            ffsp_transfer_id: FfspTransferId(9),
            address: "192.168.1.4:47471",
        }),
        Signal::FilePushV1(FilePush {
            ffsp_transfer_id: FfspTransferId(10),
            name: "report.pdf",
            len: 123_456,
        }),
        Signal::StartCastV1(StartCast {
            direction: Direction::FromPeer,
            limits: VideoLimits {
                max_width: 1920,
                max_height: 1080,
                max_fps: 60,
            },
        }),
        Signal::StartAudioV1(StartAudio {
            direction: Direction::ToPeer,
        }),
        Signal::DragEnterV1(DragEnter {
            drag: DragId(3),
            edge: Edge::Right,
            position: EdgePosition(0x4000),
            items: vec![
                DragItem {
                    kind: DragItemKind::TEXT,
                    mime: "text/plain;charset=utf-8",
                    name: None,
                    len: 12,
                },
                DragItem {
                    kind: DragItemKind::FILE,
                    mime: "application/pdf",
                    name: Some("slides.pdf"),
                    len: 9_000_000,
                },
            ],
        }),
        Signal::DragDropV1(DragDrop { drag: DragId(3) }),
        Signal::DragCancelV1(DragCancel { drag: DragId(4) }),
        Signal::RequestFailedV1(RequestFailed {
            request: REQUEST,
            error: RequestError::PERMISSION_DENIED,
            detail: "READ_SMS is not granted",
        }),
        Signal::RequestDoneV1(RequestDone {
            request: RequestId(1),
            reference: Some("sms/88"),
        }),
        Signal::ListConversationsV1(ListConversations {
            request: RequestId(2),
            page: FIRST_PAGE,
        }),
        Signal::ConversationPageV1(ConversationPage {
            request: RequestId(2),
            items: vec![Conversation {
                thread_id: 7,
                participants: vec![Participant {
                    address: "+44 20 7946 0000",
                    name: Some("Ada"),
                    contact_key: Some("0r1-2B3D"),
                }],
                participant_count: 1,
                snippet: "see you at 8",
                last_unix_ms: 1_700_000_000_000,
                unread: 2,
                has_attachments: true,
            }],
            next: Some("1700000000000:7"),
        }),
        Signal::ListMessagesV1(ListMessages {
            request: RequestId(3),
            thread_id: 7,
            page: NEXT_PAGE,
        }),
        Signal::MessagePageV1(MessagePage {
            request: RequestId(3),
            thread_id: 7,
            items: vec![message_item()],
            next: None,
        }),
        Signal::SendMessageV1(SendMessage {
            request: RequestId(4),
            thread_id: None,
            recipients: vec!["+1 555 0100", "+1 555 0101"],
            text: "running late",
            attachments: vec![AttachmentSpec {
                blob: BlobId(1),
                len: 2_048,
                mime: "image/png",
            }],
            sim_slot: None,
        }),
        Signal::MarkReadV1(MarkRead {
            request: RequestId(5),
            thread_id: 7,
        }),
        Signal::ListCallLogV1(ListCallLog {
            request: RequestId(6),
            page: FIRST_PAGE,
        }),
        Signal::CallLogPageV1(CallLogPage {
            request: RequestId(6),
            items: vec![CallLogEntry {
                entry_id: "441",
                number: "+1 555 0100",
                name: Some("Ada"),
                contact_key: None,
                kind: CallKind::MISSED,
                start_unix_ms: 1_700_000_000_000,
                duration_s: 0,
                is_new: true,
                sim_slot: Some(0),
            }],
            next: Some("1700000000000:441"),
        }),
        Signal::ListVoicemailV1(ListVoicemail {
            request: RequestId(7),
            page: FIRST_PAGE,
        }),
        Signal::VoicemailPageV1(VoicemailPage {
            request: RequestId(7),
            items: vec![Voicemail {
                voicemail_id: "12",
                number: "+1 555 0100",
                name: None,
                received_unix_ms: 1_700_000_000_000,
                duration_s: 31,
                heard: false,
                transcript: Some("call me back"),
                transcript_truncated: false,
                audio_key: Some("voicemail/12/1700000000000"),
            }],
            next: None,
        }),
        Signal::ListContactsV1(ListContacts {
            request: RequestId(8),
            page: PageRequest {
                cursor: Some("lovelace:ada:31"),
                limit: 100,
            },
        }),
        Signal::ContactPageV1(ContactPage {
            request: RequestId(8),
            items: vec![Contact {
                contact_key: "0r1-2B3D",
                name: "Ada Lovelace",
                starred: true,
                phones: vec![ContactPhone {
                    number: "+44 20 7946 0000",
                    kind: PhoneKind::MOBILE,
                    label: None,
                }],
                emails: vec!["ada@example.org"],
                organization: Some("Analytical Engines"),
                photo_key: Some("contact-photo/31/77"),
            }],
            next: None,
        }),
        Signal::FetchBlobV1(FetchBlob {
            request: RequestId(9),
            feature: Feature::Messages.bit_index(),
            key: "mms-thumb/3/1700000000000/256",
        }),
        Signal::CaptureV1(Capture {
            request: RequestId(10),
            kind: CaptureKind::SCAN_PDF,
            max_edge: 0,
        }),
        Signal::LocateV1(Locate {
            request: RequestId(11),
        }),
        Signal::LocationV1(Location {
            request: RequestId(11),
            latitude_e7: 515_007_000,
            longitude_e7: -1_246_000,
            accuracy_m: 12,
            altitude_m: Some(-3),
            fix_unix_ms: 1_700_000_000_000,
        }),
        Signal::WifiRequestV1(WifiRequest {
            request: RequestId(12),
            ssid: "Crown",
        }),
        Signal::RequestCancelV1(RequestCancel {
            request: RequestId(10),
        }),
        Signal::MessageEventV1(MessageEvent {
            change: MessageChange::Added(message_item()),
        }),
        Signal::MessageEventV1(MessageEvent {
            change: MessageChange::Deleted {
                thread_id: 7,
                message_id: "sms/3",
            },
        }),
        Signal::BlobStartV1(BlobStart {
            blob: BlobId(1),
            purpose: BlobPurpose::MessageAttachment(RequestId(4)),
            part: 0,
            parts: 1,
            len: 2_048,
            mime: "image/png",
            name: Some("sketch.png"),
        }),
        Signal::BlobChunkV1(BlobChunk {
            blob: BlobId(1),
            offset: 1_024,
            bytes: &[0x89, 0x50, 0x4E, 0x47],
        }),
        Signal::BlobEndV1(BlobEnd {
            blob: BlobId(1),
            digest: [0xAB; 32],
        }),
        Signal::BlobAckV1(BlobAck {
            blob: BlobId(1),
            received: 2_048,
        }),
        Signal::BlobCancelV1(BlobCancel {
            blob: BlobId(2),
            canceller: BlobRole::Receiver,
            reason: BlobCancelReason::DIGEST_MISMATCH,
        }),
        Signal::MessageCountersV1(MessageCounters {
            revision: 41,
            unread: 3,
            capabilities: MessageCapabilities::SEND_SMS.with(MessageCapabilities::SEND_MMS),
            mms_max_bytes: 1_048_576,
        }),
        Signal::CallCountersV1(CallCounters {
            call_log_revision: 5,
            voicemail_revision: 2,
            missed: 1,
            unheard_voicemails: 1,
            capabilities: CallCapabilities::EMPTY,
        }),
        Signal::ContactCountersV1(ContactCounters {
            revision: 9,
            count: 412,
            capabilities: ContactCapabilities::PHOTOS,
        }),
    ]
}
