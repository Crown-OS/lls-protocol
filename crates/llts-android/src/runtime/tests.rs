//! Two runtimes on loopback, driven only through commands and events, as Kotlin drives them.

#![allow(
    clippy::panic,
    reason = "a test that fails a precondition should fail loudly"
)]

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use llts_signaling::device::{DeviceId, FeatureSet};
use llts_signaling::state::Topic;
use serde_json::Value;

use super::Instance;
use crate::command::{Command, NodeCommand};
use crate::config::StartConfig;
use crate::input_json::parse_input_moment;
use crate::signal_json::encode_signal;
use crate::state_json::encode_document;
use crate::util::device_hex::parse_device_hex;
use crate::util::names::parse_name;

const PATIENCE: Duration = Duration::from_secs(10);

struct Harness {
    instance: Arc<Instance>,
    events: Receiver<Value>,
    directory: PathBuf,
}

impl Harness {
    fn start(name: &str) -> Option<Self> {
        let directory = std::env::temp_dir().join(format!(
            "llts-android-runtime-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let config = StartConfig::parse(&format!(r#"{{"name":"{name}","port":0}}"#)).ok()?;
        let instance = Arc::new(Instance::start(&config, &directory).ok()?);
        let (forward, events) = mpsc::channel();
        let reader = Arc::clone(&instance);
        thread::spawn(move || {
            while let Some(event) = reader.next_event() {
                if let Ok(value) = serde_json::from_str::<Value>(&event)
                    && forward.send(value).is_err()
                {
                    return;
                }
            }
        });
        let harness = Self {
            instance,
            events,
            directory,
        };
        harness.send(Command::Node(NodeCommand::NetworkChanged {
            addresses: vec![IpAddr::V4(Ipv4Addr::LOCALHOST)],
        }));
        Some(harness)
    }

    fn send(&self, command: Command) {
        assert!(self.instance.send(command).is_ok());
    }

    /// The first event of `kind` that `accept` takes, skipping everything else.
    fn wait_for(&self, kind: &str, accept: impl Fn(&Value) -> bool) -> Option<Value> {
        let deadline = Instant::now() + PATIENCE;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let event = self.events.recv_timeout(left).ok()?;
            if event.get("type").and_then(Value::as_str) == Some(kind) && accept(&event) {
                return Some(event);
            }
        }
        None
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.instance.stop();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// Two runtimes paired by QR and connected to each other, with their device ids.
fn paired(laptop: &str, phone: &str) -> (Harness, Harness, Option<String>, Option<String>) {
    let (Some(laptop), Some(phone)) = (Harness::start(laptop), Harness::start(phone)) else {
        panic!("a runtime did not start");
    };
    let laptop_id = laptop
        .wait_for("ready", |_| true)
        .and_then(|ready| text(&ready, "deviceId"));
    let phone_id = phone
        .wait_for("ready", |_| true)
        .and_then(|ready| text(&ready, "deviceId"));

    laptop.send(Command::Node(NodeCommand::BeginPairing { duration: None }));
    let uri = laptop
        .wait_for("pairingOffer", |_| true)
        .and_then(|offer| text(&offer, "uri"));
    let Some(uri) = uri else {
        panic!("the laptop never showed a QR offer");
    };
    phone.send(Command::Node(NodeCommand::PairWithQr { uri }));

    let connected = |peer: Option<String>| {
        move |event: &Value| {
            event.get("connected") == Some(&Value::Bool(true)) && text(event, "peer") == peer
        }
    };
    assert!(
        laptop
            .wait_for("deviceConnected", connected(phone_id.clone()))
            .is_some()
    );
    assert!(
        phone
            .wait_for("deviceConnected", connected(laptop_id.clone()))
            .is_some()
    );
    (laptop, phone, laptop_id, phone_id)
}

fn device(id: Option<&str>) -> DeviceId {
    let Some(device) = id.and_then(|id| parse_device_hex(id).ok()) else {
        panic!("a runtime reported no device id");
    };
    device
}

#[test]
fn two_runtimes_pair_by_qr_and_sync_state_through_commands_and_events() {
    let (laptop, phone, laptop_id, phone_id) = paired("laptop", "phone");

    let battery = r#"{"percent":58,"charging":true,"time_to_empty_min":null}"#;
    let Ok(document) = encode_document(Topic::Battery, battery) else {
        panic!("the battery document did not encode");
    };
    phone.send(Command::PublishState {
        topic: Topic::Battery,
        document,
    });
    let state = laptop.wait_for("remoteState", |event| {
        text(event, "topic").as_deref() == Some("Battery")
            && text(event, "peer") == phone_id
            && event.get("state").is_some_and(|state| !state.is_null())
    });
    let expected: Option<Value> = serde_json::from_str(battery).ok();
    assert_eq!(
        state.and_then(|state| state.get("state").cloned()),
        expected
    );

    let phone_device = device(phone_id.as_deref());
    let Ok(batch) =
        parse_input_moment(r#"{"timeMs":9,"events":[{"PointerRelative":{"dx":4.0,"dy":-2.0}}]}"#)
    else {
        panic!("the input moment did not encode");
    };
    laptop.send(Command::SendInput {
        peer: phone_device,
        batch,
    });
    let input = phone.wait_for("input", |event| text(event, "peer") == laptop_id);
    assert_eq!(
        input.and_then(|input| input.get("events").cloned()),
        serde_json::from_str(r#"[{"PointerRelative":{"dx":4.0,"dy":-2.0}},"Frame"]"#).ok(),
        "input reaches a phone that never sent any"
    );

    let beacons = laptop.instance.advertisements();
    assert!(
        !beacons.is_empty(),
        "a paired laptop advertises its peer's beacon"
    );
}

#[test]
fn a_stopped_runtime_ends_the_event_stream_and_refuses_commands() {
    let Some(harness) = Harness::start("solo") else {
        panic!("the runtime did not start");
    };
    assert!(harness.wait_for("ready", |_| true).is_some());
    harness.instance.stop();
    assert!(harness.instance.next_event().is_none());
    assert!(
        harness
            .instance
            .send(Command::Node(NodeCommand::CancelPairing))
            .is_err()
    );
}

#[test]
fn a_starting_runtime_lists_its_paired_devices_right_after_ready() {
    let Some(harness) = Harness::start("fresh") else {
        panic!("the runtime did not start");
    };
    assert!(harness.wait_for("ready", |_| true).is_some());
    let devices = harness.wait_for("devicesChanged", |_| true);
    assert_eq!(
        devices.and_then(|event| event.get("devices").cloned()),
        Some(Value::Array(Vec::new()))
    );
}

#[test]
fn a_freshly_adopted_peer_has_no_personal_features() {
    let (laptop, _phone, _, phone_id) = paired("adopter", "adoptee");
    let devices = laptop.wait_for("devicesChanged", |event| {
        event
            .get("devices")
            .and_then(Value::as_array)
            .is_some_and(|devices| !devices.is_empty())
    });
    let Some(device) = devices
        .as_ref()
        .and_then(|event| event.get("devices"))
        .and_then(Value::as_array)
        .and_then(|devices| devices.iter().find(|device| text(device, "id") == phone_id))
    else {
        panic!("the laptop never listed the phone");
    };
    let enabled: FeatureSet = device
        .get("enabled")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|name| parse_name("feature", name).ok())
        .collect();
    assert!(!enabled.is_empty());
    assert!(enabled.shared_with(FeatureSet::PERSONAL).is_empty());
}

#[test]
fn signals_cross_on_their_lanes_and_unshared_features_are_refused_at_once() {
    let (laptop, phone, laptop_id, _) = paired("sender", "receiver");
    let laptop_device = device(laptop_id.as_deref());
    let send = |kind: &str, body: &str| {
        encode_signal(kind, body)
            .map_err(|error| error.to_string())
            .and_then(|message| {
                phone
                    .instance
                    .send_application(laptop_device, message)
                    .map_err(|error| error.to_string())
            })
    };

    let answers = [
        (
            "RequestDoneV1",
            r#"{"request":4294967295,"reference":"sms/1"}"#,
        ),
        (
            "FilePushV1",
            r#"{"ffsp_transfer_id":3,"name":"notes.txt","len":12}"#,
        ),
    ];
    for (kind, body) in answers {
        assert_eq!(send(kind, body), Ok(()), "{kind}");
        let event = laptop.wait_for("signal", |event| {
            text(event, "kind").as_deref() == Some(kind)
        });
        assert_eq!(
            event.and_then(|event| event.get("body").cloned()),
            serde_json::from_str::<Value>(body).ok(),
            "{kind}"
        );
    }

    assert!(send("RingV1", r#"{"action":"Start"}"#).is_err());
}
