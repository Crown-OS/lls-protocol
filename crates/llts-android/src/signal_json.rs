//! Application messages between the JSON Kotlin writes and reads, and the envelopes llts
//! carries.
//!
//! One table names every message Kotlin may send or receive, by its `MessageType` name, and
//! the body type serde shapes it with: tolerant values are plain numbers, frozen enums are
//! externally tagged, `Option` is `null`. Blob messages never cross: their bytes stay in Rust.
//! Errors name the kind and where the JSON broke, never a value from it, because bodies carry
//! messages, contacts and passwords.

use llts_signaling::SignalingError;
use llts_signaling::device::DeviceId;
use llts_signaling::message::{
    BlobCancel, BlobCancelReason, BlobKey, BlobRole, CallCounters, CallLogPage, Capture,
    ContactCounters, ContactPage, ConversationPage, DragCancel, DragDrop, DragEnter, FetchBlob,
    FileListening, FilePush, ListCallLog, ListContacts, ListConversations, ListMessages,
    ListVoicemail, LiveActivityAction, Locate, Location, MarkRead, Message, MessageCounters,
    MessageEvent, MessagePage, MessageType, OpenUri, ReleaseHeadset, RequestCancel, RequestDone,
    RequestError, RequestFailed, RequestId, Ring, SendMessage, Signal, TakeHeadset, Validate,
    VoicemailPage, WifiCredentials, WifiRequest,
};
use serde_json::{Value, json};

use crate::error::BridgeError;
use crate::util::device_hex::device_hex;

/// What a key that does not parse, or names another feature's data, is answered with.
const UNKNOWN_KEY: &str = "the key names nothing this device serves";

macro_rules! signal_kinds {
    ($($kind:ident => $body:ty),+ $(,)?) => {
        /// Every message Kotlin sends and receives, by the name both sides use for it.
        pub(crate) const KINDS: &[(&str, MessageType)] =
            &[$((stringify!($kind), MessageType::$kind)),+];

        fn encode_kind(
            kind: MessageType,
            name: &'static str,
            body: &Value,
        ) -> Result<Vec<u8>, BridgeError> {
            match kind {
                $(MessageType::$kind => encode_body::<$body>(name, body),)+
                _ => Err(unknown_kind(name)),
            }
        }

        fn body_json(signal: &Signal<'_>) -> Option<Result<Value, serde_json::Error>> {
            match signal {
                $(Signal::$kind(body) => Some(serde_json::to_value(body)),)+
                _ => None,
            }
        }
    };
}

signal_kinds! {
    OpenUriV1 => OpenUri<'_>,
    LiveActivityActionV1 => LiveActivityAction<'_>,
    RingV1 => Ring,
    WifiCredentialsV1 => WifiCredentials<'_>,
    TakeHeadsetV1 => TakeHeadset<'_>,
    ReleaseHeadsetV1 => ReleaseHeadset<'_>,
    FileListeningV1 => FileListening<'_>,
    FilePushV1 => FilePush<'_>,
    DragEnterV1 => DragEnter<'_>,
    DragDropV1 => DragDrop,
    DragCancelV1 => DragCancel,
    RequestFailedV1 => RequestFailed<'_>,
    RequestDoneV1 => RequestDone<'_>,
    ListConversationsV1 => ListConversations<'_>,
    ConversationPageV1 => ConversationPage<'_>,
    ListMessagesV1 => ListMessages<'_>,
    MessagePageV1 => MessagePage<'_>,
    SendMessageV1 => SendMessage<'_>,
    MarkReadV1 => MarkRead,
    ListCallLogV1 => ListCallLog<'_>,
    CallLogPageV1 => CallLogPage<'_>,
    ListVoicemailV1 => ListVoicemail<'_>,
    VoicemailPageV1 => VoicemailPage<'_>,
    ListContactsV1 => ListContacts<'_>,
    ContactPageV1 => ContactPage<'_>,
    FetchBlobV1 => FetchBlob<'_>,
    CaptureV1 => Capture,
    LocateV1 => Locate,
    LocationV1 => Location,
    WifiRequestV1 => WifiRequest<'_>,
    RequestCancelV1 => RequestCancel,
    MessageEventV1 => MessageEvent<'_>,
    MessageCountersV1 => MessageCounters,
    CallCountersV1 => CallCounters,
    ContactCountersV1 => ContactCounters,
}

/// The envelope of `kind` (a `MessageType` name from [`KINDS`]) with the body Kotlin wrote,
/// checked against the caps its type keeps to.
pub(crate) fn encode_signal(kind: &str, json: &str) -> Result<Vec<u8>, BridgeError> {
    let (name, kind) = KINDS
        .iter()
        .find(|(name, _)| *name == kind)
        .copied()
        .ok_or_else(|| unknown_kind(kind))?;
    let body: Value = serde_json::from_str(json).map_err(malformed(name))?;
    encode_kind(kind, name, &body)
}

/// What the runtime does with an application message a peer sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Inbound {
    /// A `signal` event for Kotlin.
    Deliver(String),
    /// An answer Rust gives itself, never troubling Kotlin.
    Answer(Vec<u8>),
    /// Nothing to do: blob plumbing this build has no transfer for.
    Ignore,
}

/// Decides what becomes of `message` from `peer`. A blob offered to a build without a blob
/// engine is refused, and a fetch whose key does not parse or names another feature's data is
/// answered as not found, so Kotlin only ever sees keys it can trust.
pub(crate) fn inbound(peer: &DeviceId, message: &[u8]) -> Result<Inbound, BridgeError> {
    let signal = Signal::decode(message).map_err(BridgeError::Signal)?;
    match signal {
        Signal::BlobStartV1(start) => {
            let cancel = BlobCancel {
                blob: start.blob,
                canceller: BlobRole::Receiver,
                reason: BlobCancelReason::UNEXPECTED,
            };
            Ok(Inbound::Answer(cancel.encode(Vec::new())?))
        }
        Signal::BlobChunkV1(_)
        | Signal::BlobEndV1(_)
        | Signal::BlobAckV1(_)
        | Signal::BlobCancelV1(_) => Ok(Inbound::Ignore),
        Signal::FetchBlobV1(fetch) if served_key(&fetch).is_none() => {
            Ok(Inbound::Answer(not_found(fetch.request)?))
        }
        signal => Ok(Inbound::Deliver(event(peer, &signal)?.to_string())),
    }
}

/// The `signal` event for an application message, for a caller that only needs the JSON.
pub(crate) fn signal_event(peer: &DeviceId, message: &[u8]) -> Result<Value, BridgeError> {
    let signal = Signal::decode(message).map_err(BridgeError::Signal)?;
    event(peer, &signal)
}

fn event(peer: &DeviceId, signal: &Signal<'_>) -> Result<Value, BridgeError> {
    let name = signal
        .message_type()
        .and_then(|kind| KINDS.iter().find(|(_, listed)| *listed == kind))
        .map(|(name, _)| *name)
        .ok_or_else(|| unknown_kind("an unlisted message"))?;
    signal
        .validate()
        .map_err(|source| BridgeError::InvalidSignal { kind: name, source })?;
    let body = body_json(signal)
        .ok_or_else(|| unknown_kind(name))?
        .map_err(malformed(name))?;
    let mut event = json!({
        "type": "signal",
        "peer": device_hex(peer),
        "kind": name,
        "body": body,
    });
    if let (Signal::FetchBlobV1(fetch), Value::Object(fields)) = (signal, &mut event) {
        let key = served_key(fetch).ok_or_else(|| unknown_kind(name))?;
        fields.insert(
            "parsed_key".to_owned(),
            serde_json::to_value(key).map_err(malformed(name))?,
        );
    }
    Ok(event)
}

/// The key of `fetch`, if it parses and belongs to the feature the fetch was gated by.
fn served_key(fetch: &FetchBlob<'_>) -> Option<BlobKey> {
    BlobKey::parse(fetch.key)
        .ok()
        .filter(|key| fetch.feature() == Some(key.feature()))
}

fn not_found(request: RequestId) -> Result<Vec<u8>, SignalingError> {
    RequestFailed {
        request,
        error: RequestError::NOT_FOUND,
        detail: UNKNOWN_KEY,
    }
    .encode(Vec::new())
}

fn encode_body<'v, M: Message<'v> + Validate>(
    name: &'static str,
    body: &'v Value,
) -> Result<Vec<u8>, BridgeError> {
    let message = M::deserialize(body).map_err(malformed(name))?;
    message
        .validate()
        .map_err(|source| BridgeError::InvalidSignal { kind: name, source })?;
    Ok(message.encode(Vec::new())?)
}

/// Keeps where the JSON broke and drops serde's message, which can quote the offending value.
fn malformed(kind: &'static str) -> impl Fn(serde_json::Error) -> BridgeError {
    move |error| BridgeError::MalformedSignal {
        kind,
        line: error.line(),
        column: error.column(),
    }
}

fn unknown_kind(name: &str) -> BridgeError {
    BridgeError::UnknownName {
        what: "signal kind",
        name: name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::error::describe;

    const PEER: DeviceId = DeviceId([7; 32]);

    const MESSAGE_ITEM: &str = r#"{"thread_id":7,"message_id":"mms/12","folder":1,"sender":"+15550100","unix_ms":1700000000000,"text":"see you at 8","text_truncated":false,"read":false,"status":0,"attachments":[{"key":"mms-part/3/1700000000000","mime":"image/jpeg","name":"IMG_0001.jpg","len":1048576,"thumbnail_key":"mms-thumb/3/1700000000000/256","width":4032,"height":3024}],"attachments_omitted":0,"sim_slot":1}"#;

    /// One body per kind, in exactly the JSON Kotlin writes and reads; the Kotlin tests decode
    /// these same strings.
    fn fixtures() -> Vec<(&'static str, String)> {
        let fixed = [
            ("OpenUriV1", r#"{"uri":"https://crownos.org/?q=1"}"#),
            (
                "LiveActivityActionV1",
                r#"{"activity_id":"0|com.timer|1|null|10123","action_id":"pause"}"#,
            ),
            ("RingV1", r#"{"action":"Start"}"#),
            (
                "WifiCredentialsV1",
                r#"{"request":4294967295,"ssid":"Crown \"5G\"","security":4,"passphrase":"correct horse","hidden":false}"#,
            ),
            ("TakeHeadsetV1", r#"{"address":"AA:BB:CC:DD:EE:FF"}"#),
            ("ReleaseHeadsetV1", r#"{"address":"00:11:22:33:44:55"}"#),
            (
                "FileListeningV1",
                r#"{"ffsp_transfer_id":9,"address":"192.168.1.4:47471"}"#,
            ),
            (
                "FilePushV1",
                r#"{"ffsp_transfer_id":10,"name":"report.pdf","len":123456}"#,
            ),
            (
                "DragEnterV1",
                r#"{"drag":3,"edge":"Right","position":16384,"items":[{"kind":0,"mime":"text/plain","name":null,"len":12},{"kind":2,"mime":"application/pdf","name":"slides.pdf","len":9000000}]}"#,
            ),
            ("DragDropV1", r#"{"drag":3}"#),
            ("DragCancelV1", r#"{"drag":4}"#),
            (
                "RequestFailedV1",
                r#"{"request":7,"error":1,"detail":"READ_SMS is not granted"}"#,
            ),
            ("RequestDoneV1", r#"{"request":1,"reference":"sms/88"}"#),
            (
                "ListConversationsV1",
                r#"{"request":2,"page":{"cursor":null,"limit":50}}"#,
            ),
            (
                "ConversationPageV1",
                r#"{"request":2,"items":[{"thread_id":7,"participants":[{"address":"+15550100","name":"Ada","contact_key":"0r1-2B3D"}],"participant_count":1,"snippet":"see you at 8","last_unix_ms":1700000000000,"unread":2,"has_attachments":true}],"next":"1700000000000:7"}"#,
            ),
            (
                "ListMessagesV1",
                r#"{"request":3,"thread_id":7,"page":{"cursor":"1700000000000:s:41","limit":200}}"#,
            ),
            (
                "SendMessageV1",
                r#"{"request":4,"thread_id":null,"recipients":["+15550100"],"text":"running late","attachments":[{"blob":1,"len":2048,"mime":"image/png"}],"sim_slot":null}"#,
            ),
            ("MarkReadV1", r#"{"request":5,"thread_id":7}"#),
            (
                "ListCallLogV1",
                r#"{"request":6,"page":{"cursor":null,"limit":100}}"#,
            ),
            (
                "CallLogPageV1",
                r#"{"request":6,"items":[{"entry_id":"441","number":"+15550100","name":"Ada","contact_key":null,"kind":3,"start_unix_ms":1700000000000,"duration_s":0,"is_new":true,"sim_slot":0}],"next":"1700000000000:441"}"#,
            ),
            (
                "ListVoicemailV1",
                r#"{"request":7,"page":{"cursor":null,"limit":20}}"#,
            ),
            (
                "VoicemailPageV1",
                r#"{"request":7,"items":[{"voicemail_id":"12","number":"+15550100","name":null,"received_unix_ms":1700000000000,"duration_s":31,"heard":false,"transcript":"call me back","transcript_truncated":false,"audio_key":"voicemail/12/1700000000000"}],"next":null}"#,
            ),
            (
                "ListContactsV1",
                r#"{"request":8,"page":{"cursor":"lovelace:ada:31","limit":100}}"#,
            ),
            (
                "ContactPageV1",
                r#"{"request":8,"items":[{"contact_key":"0r1-2B3D","name":"Ada Lovelace","starred":true,"phones":[{"number":"+15550100","kind":2,"label":null}],"emails":["ada@example.org"],"organization":null,"photo_key":"contact-photo/31/77"}],"next":null}"#,
            ),
            (
                "FetchBlobV1",
                r#"{"request":9,"feature":11,"key":"mms-thumb/3/1700000000000/256"}"#,
            ),
            ("CaptureV1", r#"{"request":10,"kind":1,"max_edge":0}"#),
            ("LocateV1", r#"{"request":11}"#),
            (
                "LocationV1",
                r#"{"request":11,"latitude_e7":515007000,"longitude_e7":-1246000,"accuracy_m":12,"altitude_m":null,"fix_unix_ms":1700000000000}"#,
            ),
            ("WifiRequestV1", r#"{"request":12,"ssid":"Crown"}"#),
            ("RequestCancelV1", r#"{"request":10}"#),
            (
                "MessageEventV1",
                r#"{"change":{"Deleted":{"thread_id":7,"message_id":"sms/3"}}}"#,
            ),
            (
                "MessageCountersV1",
                r#"{"revision":41,"unread":3,"capabilities":3,"mms_max_bytes":1048576}"#,
            ),
            (
                "CallCountersV1",
                r#"{"call_log_revision":5,"voicemail_revision":2,"missed":1,"unheard_voicemails":1,"capabilities":0}"#,
            ),
            (
                "ContactCountersV1",
                r#"{"revision":9,"count":412,"capabilities":1}"#,
            ),
        ]
        .map(|(kind, body)| (kind, body.to_owned()));
        let with_items = [
            (
                "MessagePageV1",
                format!(r#"{{"request":3,"thread_id":7,"items":[{MESSAGE_ITEM}],"next":null}}"#),
            ),
            (
                "MessageEventV1",
                format!(r#"{{"change":{{"Added":{MESSAGE_ITEM}}}}}"#),
            ),
        ];
        fixed.into_iter().chain(with_items).collect()
    }

    fn delivered(message: &[u8]) -> Option<Value> {
        match inbound(&PEER, message).ok()? {
            Inbound::Deliver(event) => serde_json::from_str(&event).ok(),
            Inbound::Answer(_) | Inbound::Ignore => None,
        }
    }

    fn answered(message: &[u8]) -> Option<Vec<u8>> {
        match inbound(&PEER, message).ok()? {
            Inbound::Answer(answer) => Some(answer),
            Inbound::Deliver(_) | Inbound::Ignore => None,
        }
    }

    #[test]
    fn every_kind_crosses_both_ways_in_the_same_shape() {
        for (kind, body) in fixtures() {
            let message = encode_signal(kind, &body);
            assert!(message.is_ok(), "{kind}: {:?}", message.as_ref().err());
            let event = message.ok().and_then(|message| delivered(&message));
            let expected: Option<Value> = serde_json::from_str(&body).ok();
            assert_eq!(
                event.as_ref().and_then(|event| event.get("body").cloned()),
                expected,
                "{kind}"
            );
            assert_eq!(
                event.as_ref().and_then(|event| event.get("kind").cloned()),
                Some(json!(kind))
            );
            assert_eq!(
                event.and_then(|event| event.get("peer").cloned()),
                Some(json!(device_hex(&PEER)))
            );
        }
    }

    #[test]
    fn every_routed_message_but_blobs_is_in_the_table_and_has_a_fixture() {
        let listed: BTreeSet<&str> = KINDS.iter().map(|(name, _)| *name).collect();
        let fixtures: BTreeSet<&str> = fixtures().into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(listed, fixtures);
        for &kind in MessageType::ALL {
            let blob = matches!(
                kind,
                MessageType::BlobStartV1
                    | MessageType::BlobChunkV1
                    | MessageType::BlobEndV1
                    | MessageType::BlobAckV1
                    | MessageType::BlobCancelV1
            );
            let crosses = kind.app_route().is_some() && !blob;
            assert_eq!(
                KINDS.iter().any(|(_, listed)| *listed == kind),
                crosses,
                "{kind:?}"
            );
            if crosses {
                assert!(listed.contains(format!("{kind:?}").as_str()), "{kind:?}");
            }
        }
    }

    #[test]
    fn a_fetch_reaches_kotlin_with_its_key_parsed() {
        let message = encode_signal(
            "FetchBlobV1",
            r#"{"request":9,"feature":11,"key":"mms-thumb/3/1700000000000/256"}"#,
        );
        let event = message.ok().and_then(|message| delivered(&message));
        assert_eq!(
            event.and_then(|event| event.get("parsed_key").cloned()),
            Some(
                json!({ "MmsThumb": { "part_id": 3, "date_ms": 1_700_000_000_000_u64, "edge": 256 } })
            )
        );
    }

    #[test]
    fn a_fetch_for_a_key_this_device_does_not_serve_is_answered_not_found() {
        for body in [
            r#"{"request":5,"feature":11,"key":"content://sms/1"}"#,
            r#"{"request":5,"feature":13,"key":"mms-part/1/2"}"#,
        ] {
            let answer = encode_signal("FetchBlobV1", body)
                .ok()
                .and_then(|message| answered(&message));
            assert!(
                matches!(
                    answer.as_deref().map(Signal::decode),
                    Some(Ok(Signal::RequestFailedV1(RequestFailed {
                        request: RequestId(5),
                        error: RequestError::NOT_FOUND,
                        ..
                    })))
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn blobs_stay_in_rust_and_are_refused_without_an_engine() {
        let start = llts_signaling::message::BlobStart {
            blob: llts_signaling::message::BlobId(77),
            purpose: llts_signaling::message::BlobPurpose::Response(RequestId(1)),
            part: 0,
            parts: 1,
            len: 10,
            mime: "image/png",
            name: None,
        }
        .encode(Vec::new());
        let answer = start.ok().and_then(|start| answered(&start));
        assert_eq!(
            answer
                .as_deref()
                .and_then(|answer| Signal::decode(answer).ok()),
            Some(Signal::BlobCancelV1(BlobCancel {
                blob: llts_signaling::message::BlobId(77),
                canceller: BlobRole::Receiver,
                reason: BlobCancelReason::UNEXPECTED,
            }))
        );
        let ack = llts_signaling::message::BlobAck {
            blob: llts_signaling::message::BlobId(1),
            received: 3,
        }
        .encode(Vec::new());
        assert_eq!(
            ack.ok().and_then(|ack| inbound(&PEER, &ack).ok()),
            Some(Inbound::Ignore)
        );
        assert!(encode_signal("BlobStartV1", "{}").is_err());
    }

    #[test]
    fn errors_name_the_kind_and_place_but_never_a_value() {
        let secret = "hunter22-secret";
        let wrong_type = format!(
            r#"{{"request":1,"ssid":"Crown","security":3,"passphrase":7,"hidden":"{secret}"}}"#
        );
        let crowd = format!(
            r#"{{"request":1,"thread_id":null,"recipients":[{}],"text":"{secret}","attachments":[],"sim_slot":null}}"#,
            vec![r#""+1""#; 33].join(",")
        );
        let refusals = [
            encode_signal("WifiCredentialsV1", &wrong_type),
            encode_signal("WifiCredentialsV1", &format!("{{\"ssid\":\"{secret}\"")),
            encode_signal("SendMessageV1", &crowd),
            encode_signal("OpenUriV1", &format!(r#"{{"uri":"javascript:{secret}"}}"#)),
        ];
        for refusal in refusals {
            assert!(refusal.is_err());
            if let Err(error) = refusal {
                assert!(error.is_caller_mistake());
                let text = describe(&error);
                assert!(!text.contains(secret), "{text}");
            }
        }
        assert!(matches!(
            encode_signal("Explode", "{}"),
            Err(BridgeError::UnknownName { .. })
        ));
    }
}
