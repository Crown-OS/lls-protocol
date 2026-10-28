//! State documents between the JSON Kotlin writes and reads, and the postcard llts carries.
//!
//! Kotlin never encodes postcard: it hands over the same JSON shape it receives in
//! `remoteState` events, and this checks it against the topic's payload type on the way in.

use llts_signaling::state::{
    Battery, CallState, Clipboard, ClipboardContent, DeviceInfo, Hotspot, MediaSession,
    Notifications, Topic, TopicPayload, Volume,
};
use serde_json::{Value, json};

use crate::error::BridgeError;

const WHAT: &str = "the state document";

/// Postcard for `topic` from its JSON form. A clipboard may leave out its `stamp`; the runtime
/// stamps it on the node's hybrid clock before publishing.
pub(crate) fn encode_document(topic: Topic, json: &str) -> Result<Vec<u8>, BridgeError> {
    let mut value: Value = serde_json::from_str(json).map_err(BridgeError::json(WHAT))?;
    if topic == Topic::Clipboard
        && let Value::Object(fields) = &mut value
    {
        fields
            .entry("stamp")
            .or_insert_with(|| json!({ "wall_ms": 0, "counter": 0 }));
    }
    match topic {
        Topic::DeviceInfo => encode::<DeviceInfo<'_>>(&value),
        Topic::Battery => encode::<Battery>(&value),
        Topic::MediaSession => encode::<Option<MediaSession<'_>>>(&value),
        Topic::CallState => encode::<CallState<'_>>(&value),
        Topic::Notifications => encode::<Notifications<'_>>(&value),
        Topic::Clipboard => encode::<Clipboard<'_>>(&value),
        Topic::Volume => encode::<Volume>(&value),
        Topic::Hotspot => encode::<Hotspot<'_>>(&value),
    }
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
    }
}

fn encode<'v, P: TopicPayload<'v>>(value: &'v Value) -> Result<Vec<u8>, BridgeError> {
    let payload = P::deserialize(value).map_err(BridgeError::json(WHAT))?;
    Ok(postcard::to_allocvec(&payload)?)
}

fn to_json<'p, P: TopicPayload<'p>>(payload: &'p [u8]) -> Result<Value, BridgeError> {
    let document: P = postcard::from_bytes(payload)?;
    serde_json::to_value(&document).map_err(BridgeError::json(WHAT))
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

/// Stamps a clipboard document Kotlin sent, returning it re-encoded.
pub(crate) fn restamp_clipboard(
    payload: &[u8],
    stamp: llts_signaling::state::HybridTimestamp,
) -> Result<Vec<u8>, BridgeError> {
    let mut clipboard: Clipboard<'_> = postcard::from_bytes(payload)?;
    clipboard.stamp = stamp;
    Ok(postcard::to_allocvec(&clipboard)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(topic: Topic, json: &str) -> Option<Value> {
        let payload = encode_document(topic, json).ok()?;
        document_json(topic, &payload).ok()
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
        ];
        for (topic, json) in documents {
            let expected: Option<Value> = serde_json::from_str(json).ok();
            assert_eq!(round_trip(topic, json), expected, "{topic:?}");
        }
    }

    #[test]
    fn clipboards_travel_as_text_and_get_a_stamp_when_missing() {
        let json = r#"{"mime":"text/plain","content":{"Inline":"héllo"}}"#;
        let expected = json!({
            "mime": "text/plain",
            "content": { "Inline": "héllo" },
            "stamp": { "wall_ms": 0, "counter": 0 },
        });
        assert_eq!(round_trip(Topic::Clipboard, json), Some(expected));

        let payload = encode_document(Topic::Clipboard, json).ok();
        let stamp = llts_signaling::state::HybridTimestamp {
            wall_ms: 7,
            counter: 2,
        };
        let restamped = payload.and_then(|payload| restamp_clipboard(&payload, stamp).ok());
        let value = restamped.and_then(|bytes| document_json(Topic::Clipboard, &bytes).ok());
        assert_eq!(
            value.and_then(|value| value.pointer("/stamp/wall_ms").cloned()),
            Some(json!(7))
        );
    }

    #[test]
    fn documents_of_the_wrong_shape_are_refused() {
        assert!(encode_document(Topic::Battery, r#"{"percent":"full"}"#).is_err());
        assert!(encode_document(Topic::Volume, r#"{"percent":300,"muted":false}"#).is_err());
        assert!(encode_document(Topic::Battery, "not json").is_err());
    }
}
