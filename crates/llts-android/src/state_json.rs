//! State documents between the JSON Kotlin writes and reads, and the postcard llts carries.
//!
//! Kotlin never encodes postcard: it hands over the same JSON shape it receives in
//! `remoteState` events, and this checks it against the topic's payload type on the way in.

use llts_signaling::state::{
    AudioRoutes, Battery, CallState, Clipboard, ClipboardContent, DeviceInfo, Focus, Hotspot,
    HybridTimestamp, LiveActivities, MediaSession, Notifications, Topic, TopicPayload, Volume,
    validate_payload,
};
use serde_json::{Value, json};

use crate::error::BridgeError;

/// Where the stamp of a document ordered across devices comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stamp {
    /// The document carries its own, or has none to carry.
    AsWritten,
    /// A local change: the runtime stamps it on the node's hybrid clock before publishing.
    Fresh,
}

/// A document Kotlin wrote, encoded and ready to publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalDocument {
    pub(crate) payload: Vec<u8>,
    pub(crate) stamp: Stamp,
}

/// Postcard for `topic` from its JSON form, refused here when it breaks a cap of its topic so
/// the caller hears of it rather than the runtime thread. A clipboard is always a local copy and is stamped
/// fresh. A focus is stamped fresh when it leaves out its `stamp`; one that carries a stamp is
/// a peer's mode this device adopted, which keeps the peer's stamp so the two do not ping-pong.
pub(crate) fn encode_document(topic: Topic, json: &str) -> Result<LocalDocument, BridgeError> {
    let mut value: Value = serde_json::from_str(json).map_err(malformed(topic))?;
    let stamp = match (topic, &mut value) {
        (Topic::Clipboard, Value::Object(fields)) => {
            fields.entry("stamp").or_insert_with(unstamped);
            Stamp::Fresh
        }
        (Topic::Focus, Value::Object(fields)) if !fields.contains_key("stamp") => {
            fields.insert("stamp".to_owned(), unstamped());
            Stamp::Fresh
        }
        _ => Stamp::AsWritten,
    };
    let payload = match topic {
        Topic::DeviceInfo => encode::<DeviceInfo<'_>>(&value),
        Topic::Battery => encode::<Battery>(&value),
        Topic::MediaSession => encode::<Option<MediaSession<'_>>>(&value),
        Topic::CallState => encode::<CallState<'_>>(&value),
        Topic::Notifications => encode::<Notifications<'_>>(&value),
        Topic::Clipboard => encode::<Clipboard<'_>>(&value),
        Topic::Volume => encode::<Volume>(&value),
        Topic::Hotspot => encode::<Hotspot<'_>>(&value),
        Topic::Focus => encode::<Focus<'_>>(&value),
        Topic::LiveActivities => encode::<LiveActivities<'_>>(&value),
        Topic::AudioRoutes => encode::<AudioRoutes<'_>>(&value),
    }?;
    validate_payload(topic, &payload)?;
    Ok(LocalDocument { payload, stamp })
}

/// The JSON form of a postcard document for `topic`.
pub(crate) fn document_json(topic: Topic, payload: &[u8]) -> Result<Value, BridgeError> {
    match topic {
        Topic::DeviceInfo => to_json::<DeviceInfo<'_>>(payload),
        Topic::Battery => to_json::<Battery>(payload),
        Topic::MediaSession => to_json::<Option<MediaSession<'_>>>(payload),
        Topic::CallState => to_json::<CallState<'_>>(payload),
        Topic::Notifications => to_json::<Notifications<'_>>(payload),
        Topic::Clipboard => Ok(clipboard_json(&postcard::from_bytes(payload)?)),
        Topic::Volume => to_json::<Volume>(payload),
        Topic::Hotspot => to_json::<Hotspot<'_>>(payload),
        Topic::Focus => to_json::<Focus<'_>>(payload),
        Topic::LiveActivities => to_json::<LiveActivities<'_>>(payload),
        Topic::AudioRoutes => to_json::<AudioRoutes<'_>>(payload),
    }
}

/// Keeps where the JSON broke and drops serde's message, which can quote the offending value:
/// documents carry notification text, focus names and headset names.
fn malformed(topic: Topic) -> impl Fn(serde_json::Error) -> BridgeError {
    move |error| BridgeError::MalformedDocument {
        topic,
        line: error.line(),
        column: error.column(),
    }
}

fn unstamped() -> Value {
    json!({ "wall_ms": 0, "counter": 0 })
}

fn encode<'v, P: TopicPayload<'v>>(value: &'v Value) -> Result<Vec<u8>, BridgeError> {
    let payload = P::deserialize(value).map_err(malformed(P::TOPIC))?;
    Ok(postcard::to_allocvec(&payload)?)
}

fn to_json<'p, P: TopicPayload<'p>>(payload: &'p [u8]) -> Result<Value, BridgeError> {
    let document: P = postcard::from_bytes(payload)?;
    serde_json::to_value(&document).map_err(malformed(P::TOPIC))
}

/// Inline clipboard bytes go out as a string, the same form they are accepted in, rather than
/// serde's array of numbers.
fn clipboard_json(clipboard: &Clipboard<'_>) -> Value {
    let content = match clipboard.content {
        ClipboardContent::Inline(bytes) => json!({ "Inline": String::from_utf8_lossy(bytes) }),
        ClipboardContent::Offer {
            ffsp_transfer_id,
            len,
        } => json!({ "Offer": { "ffsp_transfer_id": ffsp_transfer_id.0, "len": len } }),
    };
    json!({
        "mime": clipboard.mime,
        "content": content,
        "stamp": { "wall_ms": clipboard.stamp.wall_ms, "counter": clipboard.stamp.counter },
    })
}

/// Replaces the stamp of a clipboard or focus document, returning it re-encoded; documents of
/// other topics carry no stamp and come back as they were.
pub(crate) fn restamp(
    topic: Topic,
    payload: &[u8],
    stamp: HybridTimestamp,
) -> Result<Vec<u8>, BridgeError> {
    match topic {
        Topic::Clipboard => {
            let mut clipboard: Clipboard<'_> = postcard::from_bytes(payload)?;
            clipboard.stamp = stamp;
            Ok(postcard::to_allocvec(&clipboard)?)
        }
        Topic::Focus => {
            let mut focus: Focus<'_> = postcard::from_bytes(payload)?;
            focus.stamp = stamp;
            Ok(postcard::to_allocvec(&focus)?)
        }
        _ => Ok(payload.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use llts_signaling::SignalingError;
    use llts_signaling::state::{MAX_LIVE_ACTIVITIES, MAX_LIVE_FIELD_BYTES};

    use super::*;
    use crate::error::describe;

    fn round_trip(topic: Topic, json: &str) -> Option<Value> {
        let document = encode_document(topic, json).ok()?;
        document_json(topic, &document.payload).ok()
    }

    #[test]
    fn every_topic_round_trips_through_postcard() {
        let documents = [
            (
                Topic::Battery,
                r#"{"percent":58,"charging":true,"time_to_empty_min":null}"#,
            ),
            (Topic::Volume, r#"{"percent":30,"muted":false}"#),
            (Topic::Hotspot, r#"{"enabled":true,"ssid":"Crown \"5G\""}"#),
            (Topic::MediaSession, "null"),
            (
                Topic::MediaSession,
                r#"{"app":"Music","title":"Line\nbreak","artist":"A","position_ms":1,"duration_ms":2,"playing":true}"#,
            ),
            (
                Topic::CallState,
                r#"{"calls":[{"call_id":"1","number":"+1","contact_name":null,"status":"Ringing","answered_unix_ms":null}]}"#,
            ),
            (
                Topic::Notifications,
                r#"{"active":[{"id":"n","app":"Chat","title":"t","body":"b","actions":[{"id":"r","label":"Reply","accepts_text":true}]}]}"#,
            ),
            (
                Topic::DeviceInfo,
                r#"{"name":"Pixel","model":"9","class":"Phone"}"#,
            ),
            (
                Topic::Focus,
                r#"{"mode":{"Named":"Work"},"until_unix_ms":1700003600000,"stamp":{"wall_ms":5,"counter":1}}"#,
            ),
            (
                Topic::Focus,
                r#"{"mode":"DoNotDisturb","until_unix_ms":null,"stamp":{"wall_ms":5,"counter":0}}"#,
            ),
            (
                Topic::LiveActivities,
                r#"{"activities":[{"id":"0|com.clock|7|null|10101","package":"com.clock","app":"Clock","title":"Timer","text":"Pasta","short_text":"4:59","category":"stopwatch","progress":{"value":1,"max":300,"indeterminate":false},"chronometer":{"base_unix_ms":1700000300000,"counts_down":true},"promoted":true,"color_argb":4280320064,"actions":[{"id":"pause","label":"Pause","accepts_text":false}]}]}"#,
            ),
            (
                Topic::AudioRoutes,
                r#"{"headsets":[{"address":"AA:BB:CC:DD:EE:FF","name":"Buds","connected":true}]}"#,
            ),
        ];
        for (topic, json) in documents {
            let expected: Option<Value> = serde_json::from_str(json).ok();
            assert_eq!(round_trip(topic, json), expected, "{topic:?}");
        }
    }

    #[test]
    fn clipboards_travel_as_text_and_are_always_stamped_on_the_node_clock() {
        let json = r#"{"mime":"text/plain","content":{"Inline":"héllo"}}"#;
        let expected = json!({
            "mime": "text/plain",
            "content": { "Inline": "héllo" },
            "stamp": { "wall_ms": 0, "counter": 0 },
        });
        assert_eq!(round_trip(Topic::Clipboard, json), Some(expected));

        let document = encode_document(Topic::Clipboard, json).ok();
        assert_eq!(
            document.as_ref().map(|document| document.stamp),
            Some(Stamp::Fresh)
        );
        let stamp = HybridTimestamp {
            wall_ms: 7,
            counter: 2,
        };
        let restamped =
            document.and_then(|document| restamp(Topic::Clipboard, &document.payload, stamp).ok());
        let value = restamped.and_then(|bytes| document_json(Topic::Clipboard, &bytes).ok());
        assert_eq!(
            value.and_then(|value| value.pointer("/stamp/wall_ms").cloned()),
            Some(json!(7))
        );
    }

    #[test]
    fn a_focus_keeps_an_adopted_stamp_and_is_stamped_only_when_it_has_none() {
        let adopted = encode_document(
            Topic::Focus,
            r#"{"mode":"Off","until_unix_ms":null,"stamp":{"wall_ms":40,"counter":2}}"#,
        );
        assert_eq!(
            adopted.as_ref().map(|document| document.stamp).ok(),
            Some(Stamp::AsWritten)
        );
        let local = encode_document(
            Topic::Focus,
            r#"{"mode":"DoNotDisturb","until_unix_ms":null}"#,
        );
        assert_eq!(
            local.as_ref().map(|document| document.stamp).ok(),
            Some(Stamp::Fresh)
        );
        let stamp = HybridTimestamp {
            wall_ms: 99,
            counter: 0,
        };
        let restamped = local
            .ok()
            .and_then(|document| restamp(Topic::Focus, &document.payload, stamp).ok())
            .and_then(|bytes| document_json(Topic::Focus, &bytes).ok());
        assert_eq!(
            restamped,
            Some(json!({
                "mode": "DoNotDisturb",
                "until_unix_ms": null,
                "stamp": { "wall_ms": 99, "counter": 0 },
            }))
        );
    }

    #[test]
    fn documents_of_the_wrong_shape_are_refused() {
        assert!(encode_document(Topic::Battery, r#"{"percent":"full"}"#).is_err());
        assert!(encode_document(Topic::Volume, r#"{"percent":300,"muted":false}"#).is_err());
        assert!(encode_document(Topic::Battery, "not json").is_err());
        assert!(
            encode_document(Topic::Focus, r#"{"mode":"Sleeping","until_unix_ms":null}"#).is_err()
        );
        let crowded = format!(
            r#"{{"headsets":[{}]}}"#,
            vec![r#"{"address":"AA:BB:CC:DD:EE:FF","name":"Buds","connected":false}"#; 17]
                .join(",")
        );
        assert!(encode_document(Topic::AudioRoutes, &crowded).is_err());
    }

    #[test]
    fn malformed_documents_are_refused_without_quoting_their_content() {
        let secret = "Dinner with Ada at 8";
        for (topic, json) in [
            (
                Topic::LiveActivities,
                format!(r#"{{"activities":[{{"id":7,"title":"{secret}"}}]}}"#),
            ),
            (
                Topic::Focus,
                format!(r#"{{"mode":"{secret}","until_unix_ms":null}}"#),
            ),
            (
                Topic::AudioRoutes,
                format!(r#"{{"headsets":[{{"address":"{secret}","name":1}}]}}"#),
            ),
            (Topic::Notifications, format!(r#"{{"active":"{secret}"}}"#)),
            (Topic::Clipboard, format!(r#"{{"mime":"{secret}""#)),
        ] {
            let refusal = encode_document(topic, &json);
            assert!(
                matches!(refusal, Err(BridgeError::MalformedDocument { .. })),
                "{topic:?}"
            );
            if let Err(error) = refusal {
                assert!(error.is_caller_mistake());
                assert!(!describe(&error).contains(secret), "{topic:?}");
            }
        }
    }

    #[test]
    fn a_live_activities_document_over_its_byte_cap_is_the_callers_mistake() {
        let field = "f".repeat(MAX_LIVE_FIELD_BYTES);
        let action = format!(r#"{{"id":"{field}","label":"{field}","accepts_text":false}}"#);
        let activity = format!(
            r#"{{"id":"{field}","package":"{field}","app":"{field}","title":"{field}","text":"{field}","short_text":"{field}","category":"{field}","progress":null,"chronometer":null,"promoted":false,"color_argb":null,"actions":[{action},{action},{action}]}}"#
        );
        let document = |count: usize| {
            format!(
                r#"{{"activities":[{}]}}"#,
                vec![activity.as_str(); count].join(",")
            )
        };
        assert!(encode_document(Topic::LiveActivities, &document(1)).is_ok());
        let refusal = encode_document(Topic::LiveActivities, &document(MAX_LIVE_ACTIVITIES));
        assert!(matches!(
            refusal,
            Err(BridgeError::State(SignalingError::DocumentTooLarge { .. }))
        ));
        assert!(refusal.is_err_and(|error| error.is_caller_mistake()));
    }
}
