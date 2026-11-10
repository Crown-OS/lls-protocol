use llts_node::FeatureRequest;
use llts_signaling::message::{StartAudio, StartCamera, StartCast, StartMirror, StartMonitor};
use serde::{Deserialize, Serialize};

/// The JSON form of a [`FeatureRequest`], the same both ways: `{"Mirror":{…}}`,
/// `{"Camera":{…}}`, `"Mic"`, `{"Monitor":{…}}`, `{"Cast":{…}}` or `{"Audio":{…}}`, with the
/// signalling types' own field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FeatureRequestJson {
    Mirror(StartMirror),
    Camera(StartCamera),
    Mic,
    Monitor(StartMonitor),
    Cast(StartCast),
    Audio(StartAudio),
}

impl From<FeatureRequestJson> for FeatureRequest {
    fn from(request: FeatureRequestJson) -> Self {
        match request {
            FeatureRequestJson::Mirror(mirror) => Self::Mirror(mirror),
            FeatureRequestJson::Camera(camera) => Self::Camera(camera),
            FeatureRequestJson::Mic => Self::Mic,
            FeatureRequestJson::Monitor(monitor) => Self::Monitor(monitor),
            FeatureRequestJson::Cast(cast) => Self::Cast(cast),
            FeatureRequestJson::Audio(audio) => Self::Audio(audio),
        }
    }
}

impl From<FeatureRequest> for FeatureRequestJson {
    fn from(request: FeatureRequest) -> Self {
        match request {
            FeatureRequest::Mirror(mirror) => Self::Mirror(mirror),
            FeatureRequest::Camera(camera) => Self::Camera(camera),
            FeatureRequest::Mic => Self::Mic,
            FeatureRequest::Monitor(monitor) => Self::Monitor(monitor),
            FeatureRequest::Cast(cast) => Self::Cast(cast),
            FeatureRequest::Audio(audio) => Self::Audio(audio),
        }
    }
}
