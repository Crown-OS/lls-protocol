//! Runtime events, in the one format both sides can read.
//!
//! JSON on this boundary and nowhere else: events arrive a few times a second and never carry
//! media, so the cost is nothing, while a binary codec mirrored in Kotlin would drift silently.
//! Every event is an object with a `type`; envelope keys are camelCase, and anything that is a
//! signalling type (stream refs, feature requests, state documents, commands) keeps serde's own
//! shape, the same one Kotlin sends back.

use core::fmt::Debug;
use std::net::SocketAddr;

use llts_node::{AppEvent, DeviceView, MediaRequest, Node};
use llts_signaling::device::{DeviceId, FeatureSet};
use llts_signaling::discovery::{Beacon, PairingFlags};
use llts_signaling::message::StreamParams;
use serde_json::{Value, json};

use crate::command::{FeatureRequestJson, PeerCommand};
use crate::error::describe;
use crate::state_json::document_json;
use crate::util::device_hex::device_hex;

/// The first event of every run: who this device is and the port it listens on, which Kotlin
/// advertises over NSD.
pub(crate) fn ready(id: &DeviceId, port: u16) -> String {
    json!({ "type": "ready", "deviceId": device_hex(id), "port": port }).to_string()
}

pub(crate) fn error(error: &dyn std::error::Error) -> String {
    json!({ "type": "error", "message": describe(error) }).to_string()
}

/// The full set of beacons to advertise changed; the bytes are fetched with
/// `nativeAdvertisements`, and are here too for a UI that wants to show them.
pub(crate) fn advertising_changed(beacons: &[Beacon]) -> String {
    let beacons: Vec<String> = beacons.iter().map(Beacon::to_hex).collect();
    json!({ "type": "advertisingChanged", "beacons": beacons }).to_string()
}

/// A file the peer is about to send over ffsp, with the address its llts session uses now,
/// which is where its ffsp listener waits.
pub(crate) fn file_offer(peer: &DeviceId, transfer: u32, host: Option<SocketAddr>) -> String {
    json!({
        "type": "fileOffer",
        "peer": device_hex(peer),
        "transferId": transfer,
        "host": host.map(|address| address.ip().to_string()),
    })
    .to_string()
}

pub(crate) fn app_event(event: &AppEvent, node: &Node) -> String {
    app_event_value(event, node).to_string()
}

pub(crate) fn media_request(request: &MediaRequest) -> String {
    match request {
        MediaRequest::Start {
            peer,
            request,
            role,
            stream,
            params,
        } => json!({
            "type": "mediaStart",
            "peer": device_hex(peer),
            "feature": request.feature(),
            "role": variant(role),
            "stream": stream,
            "params": params_value(params),
            "request": FeatureRequestJson::from(*request),
        }),
        MediaRequest::Stop {
            peer,
            feature,
            stream,
        } => json!({
            "type": "mediaStop",
            "peer": device_hex(peer),
            "feature": feature,
            "stream": stream,
        }),
    }
    .to_string()
}

fn app_event_value(event: &AppEvent, node: &Node) -> Value {
    match event {
        AppEvent::DevicesChanged => json!({
            "type": "devicesChanged",
            "devices": node.devices().map(|device| device_value(&device)).collect::<Vec<_>>(),
        }),
        AppEvent::DeviceConnected { peer, connected } => json!({
            "type": "deviceConnected",
            "peer": device_hex(peer),
            "name": node.device(peer).map(|device| device.name),
            "connected": connected,
        }),
        AppEvent::PairingOffer {
            uri,
            expires_unix_ms,
        } => json!({ "type": "pairingOffer", "uri": uri, "expiresUnixMs": expires_unix_ms }),
        AppEvent::PairingRequest { peer, name, code } => json!({
            "type": "pairingRequest",
            "peer": device_hex(peer),
            "name": name,
            "code": format!("{code:06}"),
        }),
        AppEvent::PairingResult { peer, accepted } => json!({
            "type": "pairingResult",
            "peer": peer.as_ref().map(device_hex),
            "accepted": accepted,
        }),
        AppEvent::PairingWindowClosed => json!({ "type": "pairingWindowClosed" }),
        AppEvent::PairableDeviceSeen {
            class,
            flags,
            address,
        } => json!({
            "type": "pairableDeviceSeen",
            "class": class,
            "nearbyCode": flags.contains(PairingFlags::NEARBY_CODE),
            "qr": flags.contains(PairingFlags::QR),
            "address": address.map(|address| address.to_string()),
        }),
        AppEvent::RemoteStateChanged { peer, topic } => {
            let state = node
                .remote_update(peer, *topic)
                .map(|update| document_json(*topic, update.payload))
                .transpose();
            match state {
                Ok(state) => json!({
                    "type": "remoteState",
                    "peer": device_hex(peer),
                    "topic": topic,
                    "state": state,
                }),
                Err(failure) => json!({ "type": "error", "message": describe(&failure) }),
            }
        }
        AppEvent::FeatureStateChanged {
            peer,
            feature,
            state,
        } => json!({
            "type": "featureState",
            "peer": device_hex(peer),
            "feature": feature,
            "state": variant(state),
        }),
        AppEvent::FeatureFailed {
            peer,
            feature,
            reason,
        } => json!({
            "type": "featureFailed",
            "peer": device_hex(peer),
            "feature": feature,
            "reason": variant(reason),
        }),
        AppEvent::RemoteCommand { peer, command } => json!({
            "type": "remoteCommand",
            "peer": device_hex(peer),
            "command": PeerCommand::from(command),
        }),
        AppEvent::Error(failure) => json!({ "type": "error", "message": describe(failure) }),
    }
}

fn device_value(device: &DeviceView<'_>) -> Value {
    json!({
        "id": device_hex(&device.id),
        "name": device.name,
        "class": device.class,
        "connected": device.connected,
        "enabled": features(device.enabled),
        "available": features(device.available),
        "active": features(device.active),
    })
}

fn features(set: FeatureSet) -> Value {
    set.iter().map(|feature| json!(feature)).collect()
}

fn params_value(params: &StreamParams) -> Value {
    match params {
        StreamParams::Video(video) => json!({
            "kind": "video",
            "codec": video.codec,
            "width": video.width,
            "height": video.height,
            "fpsMhz": video.fps_mhz,
            "bitrateKbps": video.bitrate_kbps,
        }),
        StreamParams::Audio(audio) => json!({
            "kind": "audio",
            "codec": audio.codec,
            "sampleRate": audio.sample_rate,
            "channels": audio.channels,
            "frameMs": audio.frame_ms,
        }),
    }
}

/// The name of a unit variant from the node, which has no serde of its own.
fn variant(value: &impl Debug) -> String {
    format!("{value:?}")
}

#[cfg(test)]
mod tests {
    use llts_node::{
        FeatureFailure, FeatureRequest, FeatureState, KnownPeer, LocalDevice, MediaRole,
        NodeConfig, RemoteCommand,
    };
    use llts_signaling::device::{DeviceClass, Feature};
    use llts_signaling::message::{
        CameraLens, Capabilities, MuxClass, StartCamera, StreamRef, VideoCodec, VideoLimits,
        VideoParams,
    };
    use llts_signaling::state::Topic;
    use llts_transport::security::DeviceIdentity;

    use super::*;
    use crate::util::clock::NodeClock;

    const PEER: DeviceId = DeviceId([7; 32]);

    fn node() -> Option<Node> {
        let identity = DeviceIdentity::generate().ok()?;
        let local = LocalDevice {
            name: "Pixel".to_owned(),
            model: "9".to_owned(),
            capabilities: Capabilities {
                video_codecs: [VideoCodec::Hevc].into_iter().collect(),
                audio_codecs: Default::default(),
                features: Feature::ALL.into_iter().collect(),
                device_class: DeviceClass::Phone,
            },
        };
        let known = KnownPeer {
            id: PEER,
            name: "Laptop".to_owned(),
            class: DeviceClass::Computer,
            enabled: FeatureSet::EMPTY.with(Feature::Battery),
        };
        Node::new(
            &identity,
            local,
            [known],
            NodeConfig::default(),
            NodeClock::start().now(),
        )
        .ok()
    }

    fn encode(event: &AppEvent) -> Option<Value> {
        let node = node()?;
        serde_json::from_str(&app_event(event, &node)).ok()
    }

    #[test]
    fn the_device_list_carries_every_paired_device() {
        let event = encode(&AppEvent::DevicesChanged);
        let expected = json!({
            "type": "devicesChanged",
            "devices": [{
                "id": device_hex(&PEER),
                "name": "Laptop",
                "class": "Computer",
                "connected": false,
                "enabled": ["Battery"],
                "available": [],
                "active": [],
            }],
        });
        assert_eq!(event, Some(expected));
    }

    #[test]
    fn pairing_events_pad_codes_and_keep_unknown_peers_null() {
        let request = encode(&AppEvent::PairingRequest {
            peer: PEER,
            name: "Laptop".to_owned(),
            code: 4_207,
        });
        assert_eq!(
            request.and_then(|value| value.get("code").cloned()),
            Some(json!("004207"))
        );
        let result = encode(&AppEvent::PairingResult {
            peer: None,
            accepted: false,
        });
        assert_eq!(
            result,
            Some(json!({ "type": "pairingResult", "peer": null, "accepted": false }))
        );
    }

    #[test]
    fn states_features_and_commands_use_their_variant_names() {
        let state = encode(&AppEvent::RemoteStateChanged {
            peer: PEER,
            topic: Topic::Battery,
        });
        assert_eq!(
            state,
            Some(json!({
                "type": "remoteState",
                "peer": device_hex(&PEER),
                "topic": "Battery",
                "state": null,
            }))
        );
        let feature = encode(&AppEvent::FeatureStateChanged {
            peer: PEER,
            feature: Feature::Camera,
            state: FeatureState::Active,
        });
        assert_eq!(
            feature.map(|value| (value.get("feature").cloned(), value.get("state").cloned())),
            Some((Some(json!("Camera")), Some(json!("Active"))))
        );
        let failed = encode(&AppEvent::FeatureFailed {
            peer: PEER,
            feature: Feature::Mirror,
            reason: FeatureFailure::NoCommonCodec,
        });
        assert_eq!(
            failed.and_then(|value| value.get("reason").cloned()),
            Some(json!("NoCommonCodec"))
        );
        let command = encode(&AppEvent::RemoteCommand {
            peer: PEER,
            command: RemoteCommand::Lock,
        });
        assert_eq!(
            command.and_then(|value| value.get("command").cloned()),
            Some(json!("Lock"))
        );
    }

    #[test]
    fn media_starts_describe_the_stream_to_attach() {
        let start = MediaRequest::Start {
            peer: PEER,
            request: FeatureRequest::Camera(StartCamera {
                lens: CameraLens::Back,
                limits: VideoLimits {
                    max_width: 1280,
                    max_height: 720,
                    max_fps: 30,
                },
            }),
            role: MediaRole::Encoder,
            stream: StreamRef {
                class: MuxClass::Video,
                index: 2,
            },
            params: StreamParams::Video(VideoParams {
                codec: VideoCodec::Hevc,
                width: 1280,
                height: 720,
                fps_mhz: 30_000,
                bitrate_kbps: 20_000,
            }),
        };
        let value: Option<Value> = serde_json::from_str(&media_request(&start)).ok();
        let expected = json!({
            "type": "mediaStart",
            "peer": device_hex(&PEER),
            "feature": "Camera",
            "role": "Encoder",
            "stream": { "class": "Video", "index": 2 },
            "params": {
                "kind": "video", "codec": "Hevc", "width": 1280, "height": 720,
                "fpsMhz": 30_000, "bitrateKbps": 20_000,
            },
            "request": { "Camera": {
                "lens": "Back",
                "limits": { "max_width": 1280, "max_height": 720, "max_fps": 30 },
            } },
        });
        assert_eq!(value, Some(expected));
    }

    #[test]
    fn a_file_offer_names_where_to_dial() {
        let address: Option<SocketAddr> = "192.168.1.4:47470".parse().ok();
        let value: Option<Value> = serde_json::from_str(&file_offer(&PEER, 7, address)).ok();
        assert_eq!(
            value,
            Some(
                json!({ "type": "fileOffer", "peer": device_hex(&PEER), "transferId": 7, "host": "192.168.1.4" })
            )
        );
    }

    #[test]
    fn the_ready_event_names_the_device_and_port() {
        let value: Option<Value> = serde_json::from_str(&ready(&PEER, 47_470)).ok();
        assert_eq!(
            value,
            Some(json!({ "type": "ready", "deviceId": device_hex(&PEER), "port": 47_470 }))
        );
    }
}
