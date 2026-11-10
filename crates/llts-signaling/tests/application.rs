//! The continuity vocabulary: wire numbers that must never move, the routing table, the caps
//! every body keeps to, and values that must stay readable when a newer peer sends them.

#![allow(
    clippy::unwrap_used,
    reason = "a test that fails a precondition should fail loudly"
)]

mod support;

use llts_signaling::SignalingError;
use llts_signaling::device::{Feature, FeatureSet};
use llts_signaling::message::{
    AppRoute, BlobId, BlobPurpose, BlobStart, CallKind, CaptureKind, DragEnter, DragId, DragItem,
    DragItemKind, Edge, EdgePosition, FfspTransferId, FilePush, Gate, Lane, ListContacts, Location,
    Message, MessageType, MuxClass, OpenUri, PageRequest, RequestError, RequestFailed, RequestId,
    SendMessage, Signal, StreamRef, TakeHeadset, Validate, WifiCredentials, WifiSecurity, limits,
};
use llts_signaling::state::Topic;
use support::{REQUEST, application_messages, message_item};

fn sample(kind: MessageType) -> Option<Signal<'static>> {
    application_messages()
        .into_iter()
        .find(|message| message.message_type() == Some(kind))
}

#[test]
fn feature_bits_and_their_postcard_indices_never_move() {
    let pinned = [
        (Feature::Mirror, 0),
        (Feature::Camera, 1),
        (Feature::Mic, 2),
        (Feature::Monitor, 3),
        (Feature::Unicursor, 4),
        (Feature::Calls, 5),
        (Feature::Clipboard, 6),
        (Feature::Notifications, 7),
        (Feature::Battery, 8),
        (Feature::Hotspot, 9),
        (Feature::Files, 10),
        (Feature::Messages, 11),
        (Feature::CallHistory, 12),
        (Feature::Contacts, 13),
        (Feature::Capture, 14),
        (Feature::DragDrop, 15),
        (Feature::Cast, 16),
        (Feature::AudioHandoff, 17),
        (Feature::Focus, 18),
        (Feature::LiveActivities, 19),
        (Feature::WifiSharing, 20),
        (Feature::FindMy, 21),
        (Feature::Links, 22),
    ];
    assert_eq!(pinned.map(|(feature, _)| feature), Feature::ALL);
    for (feature, index) in pinned {
        assert_eq!(feature.bit_index(), index);
        assert_eq!(Feature::from_bit(index), Some(feature));
        assert_eq!(postcard::to_allocvec(&feature).unwrap(), [index]);
    }
    assert_eq!(Feature::from_bit(23), None);
}

#[test]
fn topic_indices_never_move() {
    let pinned = [
        (Topic::DeviceInfo, 0),
        (Topic::Battery, 1),
        (Topic::MediaSession, 2),
        (Topic::CallState, 3),
        (Topic::Notifications, 4),
        (Topic::Clipboard, 5),
        (Topic::Volume, 6),
        (Topic::Hotspot, 7),
        (Topic::Focus, 8),
        (Topic::LiveActivities, 9),
        (Topic::AudioRoutes, 10),
    ];
    assert_eq!(pinned.map(|(topic, _)| topic), Topic::ALL);
    for (topic, index) in pinned {
        assert_eq!(postcard::to_allocvec(&topic).unwrap(), [index]);
    }
}

#[test]
fn personal_features_are_never_on_by_default() {
    assert_eq!(FeatureSet::EVERY.iter().count(), Feature::ALL.len());
    assert!(
        FeatureSet::DEFAULT
            .shared_with(FeatureSet::PERSONAL)
            .is_empty()
    );
    assert_eq!(
        FeatureSet::PERSONAL.iter().collect::<Vec<_>>(),
        [
            Feature::Messages,
            Feature::CallHistory,
            Feature::Contacts,
            Feature::WifiSharing,
            Feature::FindMy,
            Feature::Links,
        ]
    );
    assert_eq!(
        FeatureSet::DEFAULT.iter().count() + FeatureSet::PERSONAL.iter().count(),
        Feature::ALL.len()
    );
    assert!(FeatureSet::DEFAULT.contains(Feature::Cast));
}

const fn control(feature: Feature) -> Option<AppRoute> {
    Some(AppRoute {
        lane: Lane::Control,
        gate: Gate::Feature(feature),
    })
}

const fn bulk(feature: Feature) -> Option<AppRoute> {
    Some(AppRoute {
        lane: Lane::Bulk,
        gate: Gate::Feature(feature),
    })
}

const fn open(lane: Lane) -> Option<AppRoute> {
    Some(AppRoute {
        lane,
        gate: Gate::Open,
    })
}

/// The type codes, lanes and gates of every message the continuity features added.
fn table() -> Vec<(MessageType, u16, Option<AppRoute>)> {
    use MessageType as T;
    vec![
        (T::OpenUriV1, 0x0407, control(Feature::Links)),
        (
            T::LiveActivityActionV1,
            0x0408,
            control(Feature::LiveActivities),
        ),
        (T::RingV1, 0x0409, control(Feature::FindMy)),
        (T::WifiCredentialsV1, 0x040A, control(Feature::WifiSharing)),
        (T::TakeHeadsetV1, 0x040B, control(Feature::AudioHandoff)),
        (T::ReleaseHeadsetV1, 0x040C, control(Feature::AudioHandoff)),
        (T::FileListeningV1, 0x040D, control(Feature::Files)),
        (T::FilePushV1, 0x040E, control(Feature::Files)),
        (T::StartCastV1, 0x0507, None),
        (T::StartAudioV1, 0x0508, None),
        (T::DragEnterV1, 0x0509, control(Feature::DragDrop)),
        (T::DragDropV1, 0x050A, control(Feature::DragDrop)),
        (T::DragCancelV1, 0x050B, control(Feature::DragDrop)),
        (T::RequestFailedV1, 0x0600, open(Lane::Bulk)),
        (T::RequestDoneV1, 0x0601, open(Lane::Bulk)),
        (T::ListConversationsV1, 0x0602, control(Feature::Messages)),
        (T::ConversationPageV1, 0x0603, bulk(Feature::Messages)),
        (T::ListMessagesV1, 0x0604, control(Feature::Messages)),
        (T::MessagePageV1, 0x0605, bulk(Feature::Messages)),
        (
            T::SendMessageV1,
            0x0606,
            Some(AppRoute {
                lane: Lane::Blob,
                gate: Gate::Feature(Feature::Messages),
            }),
        ),
        (T::MarkReadV1, 0x0607, control(Feature::Messages)),
        (T::ListCallLogV1, 0x0608, control(Feature::CallHistory)),
        (T::CallLogPageV1, 0x0609, bulk(Feature::CallHistory)),
        (T::ListVoicemailV1, 0x060A, control(Feature::CallHistory)),
        (T::VoicemailPageV1, 0x060B, bulk(Feature::CallHistory)),
        (T::ListContactsV1, 0x060C, control(Feature::Contacts)),
        (T::ContactPageV1, 0x060D, bulk(Feature::Contacts)),
        (
            T::FetchBlobV1,
            0x060E,
            Some(AppRoute {
                lane: Lane::Control,
                gate: Gate::FromBody,
            }),
        ),
        (T::CaptureV1, 0x060F, control(Feature::Capture)),
        (T::LocateV1, 0x0610, control(Feature::FindMy)),
        (T::LocationV1, 0x0611, bulk(Feature::FindMy)),
        (T::WifiRequestV1, 0x0612, control(Feature::WifiSharing)),
        (T::RequestCancelV1, 0x0613, open(Lane::Control)),
        (T::MessageEventV1, 0x0700, bulk(Feature::Messages)),
        (T::BlobStartV1, 0x0701, open(Lane::Blob)),
        (T::BlobChunkV1, 0x0702, open(Lane::Blob)),
        (T::BlobEndV1, 0x0703, open(Lane::Blob)),
        (T::BlobAckV1, 0x0704, open(Lane::Blob)),
        (T::BlobCancelV1, 0x0705, open(Lane::Blob)),
        (T::MessageCountersV1, 0x0706, bulk(Feature::Messages)),
        (T::CallCountersV1, 0x0707, bulk(Feature::CallHistory)),
        (T::ContactCountersV1, 0x0708, bulk(Feature::Contacts)),
    ]
}

#[test]
fn continuity_type_codes_and_routes_are_pinned() {
    for (ty, code, route) in table() {
        assert_eq!(ty.as_u16(), code, "{ty:?}");
        assert_eq!(ty.app_route(), route, "{ty:?}");
    }
}

#[test]
fn every_application_message_has_a_route_and_nothing_older_does() {
    let pinned: Vec<MessageType> = table().into_iter().map(|(ty, _, _)| ty).collect();
    for &ty in MessageType::ALL {
        let code = ty.as_u16();
        let application = matches!(code, 0x0407..=0x04FF | 0x0509..=0x05FF | 0x0600..=0x07FF);
        assert_eq!(ty.app_route().is_some(), application, "{ty:?}");
        if code >= 0x0407 && !matches!(code, 0x0500..=0x0506) {
            assert!(
                pinned.contains(&ty),
                "{ty:?} is missing from the pinned table"
            );
        }
    }
}

#[test]
fn lanes_are_the_control_stream_and_two_data_streams() {
    let stream = |class, index| StreamRef { class, index };
    assert_eq!(Lane::Control.stream(), stream(MuxClass::Control, 0));
    assert_eq!(Lane::Bulk.stream(), stream(MuxClass::Data, 1));
    assert_eq!(Lane::Blob.stream(), stream(MuxClass::Data, 2));
}

#[test]
fn every_sample_keeps_to_its_caps() {
    for message in application_messages() {
        assert_eq!(message.validate(), Ok(()), "{:?}", message.message_type());
    }
}

#[test]
fn tolerant_values_a_newer_peer_sends_still_decode() {
    let failed = RequestFailed {
        request: REQUEST,
        error: RequestError(200),
        detail: "",
    }
    .encode(Vec::new())
    .unwrap();
    let Signal::RequestFailedV1(decoded) = Signal::decode(&failed).unwrap() else {
        unreachable!("encoded a failure");
    };
    assert_eq!(decoded.error, RequestError(200));
    assert_eq!(decoded.error.known(), RequestError::FAILED);
    assert_eq!(RequestError::BUSY.known(), RequestError::BUSY);

    let Some(Signal::CallLogPageV1(mut page)) = sample(MessageType::CallLogPageV1) else {
        unreachable!("the samples hold a call log page");
    };
    for item in &mut page.items {
        item.kind = CallKind(250);
    }
    let encoded = page.encode(Vec::new()).unwrap();
    assert_eq!(
        Signal::decode(&encoded).unwrap(),
        Signal::CallLogPageV1(page)
    );
}

#[test]
fn a_frozen_enum_variant_from_the_future_fails_the_message() {
    let mut start = BlobStart {
        blob: BlobId(1),
        purpose: BlobPurpose::Response(REQUEST),
        part: 0,
        parts: 1,
        len: 1,
        mime: "text/plain",
        name: None,
    }
    .encode(Vec::new())
    .unwrap();
    let purpose = start.get_mut(3).unwrap();
    assert_eq!(*purpose, 0);
    *purpose = 9;
    assert!(matches!(
        Signal::decode(&start),
        Err(SignalingError::MalformedBody {
            msg_type: MessageType::BlobStartV1,
            ..
        })
    ));
}

#[test]
fn wifi_credentials_never_print_their_passphrase() {
    let credentials = WifiCredentials {
        request: None,
        ssid: "Crown",
        security: WifiSecurity::WPA_PSK,
        passphrase: "hunter22",
        hidden: false,
    };
    let printed = format!("{credentials:?}");
    assert!(!printed.contains("hunter22"));
    assert!(printed.contains("<redacted>"));
    assert!(printed.contains("Crown"));
    let signal = format!("{:?}", Signal::WifiCredentialsV1(credentials));
    assert!(!signal.contains("hunter22"));
}

#[test]
fn bodies_breaking_their_caps_are_refused_without_quoting_them() {
    let long_text = "x".repeat(limits::MAX_SEND_TEXT_BYTES + 1);
    let send = SendMessage {
        request: REQUEST,
        thread_id: Some(1),
        recipients: Vec::new(),
        text: &long_text,
        attachments: Vec::new(),
        sim_slot: None,
    };
    assert_eq!(
        send.validate(),
        Err(SignalingError::FieldTooLong {
            field: "message text",
            len: long_text.len(),
            limit: limits::MAX_SEND_TEXT_BYTES,
        })
    );
    let nobody = SendMessage {
        thread_id: None,
        text: "hi",
        ..send.clone()
    };
    assert!(nobody.validate().is_err());
    let crowd = SendMessage {
        recipients: vec!["+1"; limits::MAX_PARTICIPANTS + 1],
        ..nobody
    };
    assert!(matches!(
        crowd.validate(),
        Err(SignalingError::TooManyEntries {
            field: "recipients",
            ..
        })
    ));

    for uri in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "no scheme",
        "intent://x",
    ] {
        assert_eq!(
            OpenUri { uri }.validate(),
            Err(SignalingError::UriSchemeRefused),
            "{uri}"
        );
    }
    assert_eq!(
        OpenUri {
            uri: "TEL:+15550100"
        }
        .validate(),
        Ok(())
    );
    let long_uri = format!("https://{}", "a".repeat(limits::MAX_URI_BYTES));
    assert!(OpenUri { uri: &long_uri }.validate().is_err());

    for address in [
        "aa:bb:cc:dd:ee:ff",
        "AA:BB:CC:DD:EE",
        "AA-BB-CC-DD-EE-FF",
        "AA:BB:CC:DD:EE:FG",
    ] {
        assert!(TakeHeadset { address }.validate().is_err(), "{address}");
    }

    for name in ["../secret", "a/b", "", "..", "nul\0"] {
        let push = FilePush {
            ffsp_transfer_id: FfspTransferId(1),
            name,
            len: 1,
        };
        assert!(push.validate().is_err(), "{name:?}");
    }

    let start = BlobStart {
        blob: BlobId(1),
        purpose: BlobPurpose::Response(REQUEST),
        part: 1,
        parts: 1,
        len: 1,
        mime: "image/png",
        name: None,
    };
    assert!(start.validate().is_err());
    assert!(
        BlobStart {
            part: 0,
            len: limits::MAX_BLOB_BYTES + 1,
            ..start
        }
        .validate()
        .is_err()
    );

    let lost = Location {
        request: REQUEST,
        latitude_e7: 900_000_001,
        longitude_e7: 0,
        accuracy_m: 1,
        altitude_m: None,
        fix_unix_ms: 0,
    };
    assert_eq!(
        lost.validate(),
        Err(SignalingError::OutOfRange { field: "latitude" })
    );

    let long_cursor = "c".repeat(limits::MAX_CURSOR_BYTES + 1);
    let list = ListContacts {
        request: REQUEST,
        page: PageRequest {
            cursor: Some(&long_cursor),
            limit: 10,
        },
    };
    assert!(list.validate().is_err());

    let mut item = message_item();
    item.text = Some(&long_text);
    let error = Signal::MessageEventV1(llts_signaling::message::MessageEvent {
        change: llts_signaling::message::MessageChange::Updated(item),
    })
    .validate()
    .unwrap_err();
    assert!(!error.to_string().contains("xxxx"));
}

#[test]
fn a_drag_too_large_to_describe_is_refused() {
    let name = "n".repeat(limits::MAX_DRAG_NAME_BYTES);
    let items = vec![
        DragItem {
            kind: DragItemKind::FILE,
            mime: "application/octet-stream",
            name: Some(&name),
            len: 1,
        };
        limits::MAX_DRAG_ITEMS
    ];
    let enter = DragEnter {
        drag: DragId(1),
        edge: Edge::Left,
        position: EdgePosition(0),
        items,
    };
    assert!(matches!(
        enter.validate(),
        Err(SignalingError::FieldTooLong {
            field: "drag enter",
            ..
        })
    ));
}

#[test]
fn capture_results_have_one_mime_type_and_its_magic_bytes() {
    assert_eq!(CaptureKind::PHOTO.mime(), Some("image/jpeg"));
    assert_eq!(CaptureKind::SCAN_PDF.mime(), Some("application/pdf"));
    assert_eq!(CaptureKind::SCAN_JPEG.mime(), Some("image/jpeg"));
    assert_eq!(CaptureKind::SKETCH.mime(), Some("image/png"));
    assert_eq!(CaptureKind::SCAN_PDF.magic(), Some(&b"%PDF-"[..]));
    assert_eq!(CaptureKind(4).mime(), None);
    assert_eq!(CaptureKind(4).magic(), None);
}

#[test]
fn a_fetch_names_its_feature_by_bit() {
    let Some(Signal::FetchBlobV1(fetch)) = sample(MessageType::FetchBlobV1) else {
        unreachable!("the samples hold a fetch");
    };
    assert_eq!(fetch.feature(), Some(Feature::Messages));
    assert_eq!(fetch.request, RequestId(9));
    let unknown = llts_signaling::message::FetchBlob {
        feature: 31,
        ..fetch
    };
    assert_eq!(unknown.feature(), None);
}
