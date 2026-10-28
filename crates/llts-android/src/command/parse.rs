use std::net::{IpAddr, SocketAddr};

use llts_node::FeatureRequest;
use llts_signaling::device::Feature;
use llts_signaling::discovery::Beacon;
use llts_signaling::state::Topic;

use super::{FeatureRequestJson, PeerCommand};
use crate::error::BridgeError;
use crate::util::names::parse_name;

pub(crate) fn parse_feature(name: &str) -> Result<Feature, BridgeError> {
    parse_name("feature", name)
}

pub(crate) fn parse_topic(name: &str) -> Result<Topic, BridgeError> {
    parse_name("topic", name)
}

pub(crate) fn parse_feature_request(json: &str) -> Result<FeatureRequest, BridgeError> {
    serde_json::from_str::<FeatureRequestJson>(json)
        .map(FeatureRequest::from)
        .map_err(BridgeError::json("the feature request"))
}

pub(crate) fn parse_peer_command(json: &str) -> Result<PeerCommand, BridgeError> {
    serde_json::from_str(json).map_err(BridgeError::json("the peer command"))
}

/// A JSON array of `"host:port"` strings, IPv6 hosts in brackets, as NSD resolves them.
pub(crate) fn parse_socket_addresses(json: &str) -> Result<Vec<SocketAddr>, BridgeError> {
    serde_json::from_str(json).map_err(BridgeError::json("the socket address list"))
}

/// A JSON array of this device's IP addresses, as `LinkProperties` lists them.
pub(crate) fn parse_ip_addresses(json: &str) -> Result<Vec<IpAddr>, BridgeError> {
    serde_json::from_str(json).map_err(BridgeError::json("the IP address list"))
}

pub(crate) fn parse_beacon(bytes: &[u8]) -> Result<Beacon, BridgeError> {
    Ok(Beacon::parse(bytes)?)
}

#[cfg(test)]
mod tests {
    use llts_signaling::message::{
        CameraLens, Direction, Edge, Media, StartCamera, StartMirror, StartMonitor,
    };

    use super::*;
    use crate::command::peer_command::CallRequest;

    #[test]
    fn feature_requests_use_the_signalling_shapes() {
        let mirror = parse_feature_request(
            r#"{"Mirror":{"direction":"ToPeer","limits":{"max_width":1920,"max_height":1080,"max_fps":60},"remote_input":true}}"#,
        );
        assert!(matches!(
            mirror,
            Ok(FeatureRequest::Mirror(StartMirror {
                direction: Direction::ToPeer,
                remote_input: true,
                ..
            }))
        ));
        let camera = parse_feature_request(
            r#"{"Camera":{"lens":"Front","limits":{"max_width":1280,"max_height":720,"max_fps":30}}}"#,
        );
        assert!(matches!(
            camera,
            Ok(FeatureRequest::Camera(StartCamera {
                lens: CameraLens::Front,
                ..
            }))
        ));
        assert!(matches!(
            parse_feature_request(r#""Mic""#),
            Ok(FeatureRequest::Mic)
        ));
        let monitor = parse_feature_request(
            r#"{"Monitor":{"width":2560,"height":1600,"refresh_mhz":60000,"scale_120":180,"placement":"Right"}}"#,
        );
        assert!(matches!(
            monitor,
            Ok(FeatureRequest::Monitor(StartMonitor {
                placement: Edge::Right,
                ..
            }))
        ));
        assert!(parse_feature_request(r#"{"Mirror":{}}"#).is_err());
    }

    #[test]
    fn peer_commands_own_their_text() {
        let pickup = parse_peer_command(r#"{"Call":{"Pickup":{"call_id":"7"}}}"#);
        assert_eq!(
            pickup.ok(),
            Some(PeerCommand::Call(CallRequest::Pickup {
                call_id: "7".to_owned()
            }))
        );
        assert_eq!(
            parse_peer_command(r#"{"Media":"Play"}"#).ok(),
            Some(PeerCommand::Media(Media::Play))
        );
        assert_eq!(
            parse_peer_command(r#""Lock""#).ok(),
            Some(PeerCommand::Lock)
        );
        let reply =
            parse_peer_command(r#"{"Reply":{"conversation_id":"c","text":"on my \"way\""}}"#);
        assert!(matches!(
            reply.as_ref().map(PeerCommand::as_outgoing),
            Ok(llts_node::OutgoingCommand::Reply(reply)) if reply.text == "on my \"way\""
        ));
        assert!(parse_peer_command(r#"{"Explode":{}}"#).is_err());
    }

    #[test]
    fn addresses_parse_from_nsd_and_link_properties_forms() {
        let sockets = parse_socket_addresses(r#"["192.168.1.4:47470","[fe80::1]:47470"]"#);
        assert_eq!(sockets.map(|sockets| sockets.len()).ok(), Some(2));
        let ips = parse_ip_addresses(r#"["10.0.0.2","2001:db8::5"]"#);
        assert_eq!(ips.map(|ips| ips.len()).ok(), Some(2));
        assert!(parse_socket_addresses(r#"["192.168.1.4"]"#).is_err());
    }

    #[test]
    fn names_and_beacons_are_checked() {
        assert_eq!(parse_feature("Camera").ok(), Some(Feature::Camera));
        assert_eq!(parse_topic("Clipboard").ok(), Some(Topic::Clipboard));
        assert!(parse_topic("Weather").is_err());
        assert!(parse_beacon(&[1, 9, 0]).is_err());
        assert!(parse_beacon(&[]).is_err());
    }
}
