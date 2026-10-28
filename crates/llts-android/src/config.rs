use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use llts_node::LocalDevice;
use llts_signaling::device::{DeviceClass, Feature};
use llts_signaling::message::{AudioCodec, Capabilities, CodecList, VideoCodec};
use serde::Deserialize;

use crate::error::BridgeError;

/// The UDP port llts listens on, the same one the Linux daemon uses.
pub(crate) const DEFAULT_LLTS_PORT: u16 = 47_470;

/// What `nativeStart` is told about this device.
///
/// Kotlin knows which codecs the phone's `MediaCodecList` has in hardware; everything else has
/// a default, so `{"name":"Pixel"}` is a complete configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartConfig {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) model: String,
    #[serde(default = "phone")]
    pub(crate) class: DeviceClass,
    #[serde(default = "default_port")]
    pub(crate) port: u16,
    #[serde(default = "hardware_video_codecs")]
    pub(crate) video_codecs: CodecList<VideoCodec>,
    #[serde(default = "opus")]
    pub(crate) audio_codecs: CodecList<AudioCodec>,
    #[serde(default = "every_feature")]
    pub(crate) features: Vec<Feature>,
}

impl StartConfig {
    pub(crate) fn parse(json: &str) -> Result<Self, BridgeError> {
        serde_json::from_str(json).map_err(BridgeError::json("the start configuration"))
    }

    pub(crate) fn local_device(&self) -> LocalDevice {
        LocalDevice {
            name: self.name.clone(),
            model: self.model.clone(),
            capabilities: Capabilities {
                video_codecs: self.video_codecs.clone(),
                audio_codecs: self.audio_codecs.clone(),
                features: self
                    .features
                    .iter()
                    .copied()
                    .filter(|feature| crate::media::implements(*feature))
                    .collect(),
                device_class: self.class,
            },
        }
    }

    /// The IPv4 wildcard, so the socket survives Wi-Fi and hotspot switches without a rebind.
    pub(crate) const fn bind_address(&self) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), self.port)
    }
}

const fn phone() -> DeviceClass {
    DeviceClass::Phone
}

const fn default_port() -> u16 {
    DEFAULT_LLTS_PORT
}

fn hardware_video_codecs() -> CodecList<VideoCodec> {
    [VideoCodec::Hevc, VideoCodec::H264].into_iter().collect()
}

fn opus() -> CodecList<AudioCodec> {
    [AudioCodec::Opus].into_iter().collect()
}

fn every_feature() -> Vec<Feature> {
    Feature::ALL.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_alone_is_a_complete_configuration() {
        let config = StartConfig::parse(r#"{"name":"Pixel"}"#).ok();
        let config = config.as_ref();
        assert_eq!(config.map(|config| config.port), Some(DEFAULT_LLTS_PORT));
        assert_eq!(config.map(|config| config.class), Some(DeviceClass::Phone));
        assert_eq!(
            config.map(|config| config.video_codecs.as_slice()),
            Some([VideoCodec::Hevc, VideoCodec::H264].as_slice())
        );
    }

    #[test]
    fn every_field_can_be_set_and_typos_are_refused() {
        let json = r#"{"name":"Tab","model":"S9","class":"Tablet","port":5000,
            "videoCodecs":["H264"],"audioCodecs":["Opus"],"features":["Battery","Clipboard"]}"#;
        let config = StartConfig::parse(json).ok();
        assert_eq!(config.as_ref().map(|config| config.port), Some(5000));
        let local = config.map(|config| config.local_device());
        assert_eq!(
            local.map(|local| local.capabilities.features.iter().count()),
            Some(2)
        );
        assert!(StartConfig::parse(r#"{"name":"x","prot":1}"#).is_err());
        assert!(StartConfig::parse(r#"{"model":"x"}"#).is_err());
    }
}
