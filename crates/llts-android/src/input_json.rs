//! Remote input between Kotlin and the Input stream's wire batches.
//!
//! One moment of input crosses as `{"timeMs": …, "events": [...]}`, each event in serde's own
//! shape of [`InputEvent`], such as `{"PointerRelative":{"dx":3.0,"dy":-1.0}}` or `"Frame"`.

use llts_signaling::device::DeviceId;
use llts_signaling::input::{InputBatch, InputEvent, messages, pressed_keys};
use serde::Deserialize;
use serde_json::json;

use crate::error::BridgeError;
use crate::util::device_hex::device_hex;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputMomentJson {
    time_ms: u32,
    events: Vec<InputEvent>,
}

/// The wire batch for one moment Kotlin sends, closed with the `Frame` every batch ends in.
pub(crate) fn parse_input_moment(json: &str) -> Result<Vec<u8>, BridgeError> {
    let moment: InputMomentJson =
        serde_json::from_str(json).map_err(BridgeError::json("the input moment"))?;
    let closed = moment.events.last() == Some(&InputEvent::Frame);
    let mut batch = InputBatch::default();
    for event in moment
        .events
        .into_iter()
        .chain((!closed).then_some(InputEvent::Frame))
    {
        batch.push(moment.time_ms, event)?;
    }
    Ok(batch.into_bytes())
}

/// The `input` event for a batch the peer sent, or `None` if it held no readable message.
pub(crate) fn input_event(peer: &DeviceId, batch: &[u8]) -> Option<String> {
    let mut decoded = messages(batch).peekable();
    let time_ms = decoded.peek()?.time_ms;
    let events: Vec<InputEvent> = decoded.map(|message| message.event).collect();
    Some(
        json!({
            "type": "input",
            "peer": device_hex(peer),
            "timeMs": time_ms,
            "events": events,
        })
        .to_string(),
    )
}

/// The `inputKeys` event for the peer's snapshot of every key it holds down.
pub(crate) fn key_state_event(peer: &DeviceId, snapshot: &[u8]) -> String {
    let pressed: Vec<u32> = pressed_keys(snapshot).collect();
    json!({ "type": "inputKeys", "peer": device_hex(peer), "pressed": pressed }).to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    const PEER: DeviceId = DeviceId([7; 32]);

    fn parsed(event: &str) -> Value {
        serde_json::from_str(event).unwrap_or(Value::Null)
    }

    #[test]
    fn a_moment_from_kotlin_round_trips_into_the_input_event() -> Result<(), BridgeError> {
        let batch = parse_input_moment(
            r#"{"timeMs":42,"events":[{"PointerRelative":{"dx":3.0,"dy":-1.5}},{"PointerButton":{"button":272,"pressed":true}}]}"#,
        )?;
        let event = input_event(&PEER, &batch).map(|event| parsed(&event));
        let expected = json!({
            "type": "input",
            "peer": device_hex(&PEER),
            "timeMs": 42,
            "events": [
                {"PointerRelative": {"dx": 3.0, "dy": -1.5}},
                {"PointerButton": {"button": 272, "pressed": true}},
                "Frame",
            ],
        });
        assert_eq!(event, Some(expected));
        Ok(())
    }

    #[test]
    fn a_moment_already_closed_gets_no_second_frame() -> Result<(), BridgeError> {
        let batch = parse_input_moment(r#"{"timeMs":1,"events":["TouchCancel","Frame"]}"#)?;
        assert_eq!(messages(&batch).count(), 2);
        assert!(parse_input_moment(r#"{"timeMs":1,"events":["Wiggle"]}"#).is_err());
        assert_eq!(input_event(&PEER, &[]), None);
        Ok(())
    }

    #[test]
    fn a_key_snapshot_lists_the_held_keys() -> Result<(), BridgeError> {
        let mut snapshot = InputBatch::default();
        for code in [29, 42] {
            snapshot.push(
                0,
                InputEvent::Key {
                    code,
                    pressed: true,
                },
            )?;
        }
        assert_eq!(
            parsed(&key_state_event(&PEER, snapshot.as_bytes())).get("pressed"),
            Some(&json!([29, 42]))
        );
        Ok(())
    }
}
