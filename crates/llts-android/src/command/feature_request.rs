use llts_node::FeatureRequest;
use llts_signaling::message::{StartCamera, StartMirror, StartMonitor};
use serde::{Deserialize, Serialize};

/// The JSON form of a [`FeatureRequest`], the same both ways: `{"Mirror":{…}}`,
/// `{"Camera":{…}}`, `"Mic"` or `{"Monitor":{…}}`, with the signalling types' own field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum FeatureRequestJson {
    Mirror(StartMirror),
    Camera(StartCamera),
    Mic,
    Monitor(StartMonitor),
}

impl From<FeatureRequestJson> for FeatureRequest {
    fn from(request: FeatureRequestJson) -> Self {
        match request {
            FeatureRequestJson::Mirror(mirror) => Self::Mirror(mirror),
            FeatureRequestJson::Camera(camera) => Self::Camera(camera),
            FeatureRequestJson::Mic => Self::Mic,
            FeatureRequestJson::Monitor(monitor) => Self::Monitor(monitor),
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
        }
    }
}
