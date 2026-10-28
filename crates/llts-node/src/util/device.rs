use llts_signaling::device::{DeviceClass, DeviceId};
use llts_transport::security::{DeviceClass as TransportClass, StaticKey};

/// A device is named by its static key everywhere; only the type differs between crates.
pub(crate) const fn device_id(key: StaticKey) -> DeviceId {
    DeviceId(key.0)
}

pub(crate) const fn static_key(id: DeviceId) -> StaticKey {
    StaticKey(id.0)
}

pub(crate) const fn class_from_transport(class: TransportClass) -> DeviceClass {
    match class {
        TransportClass::Phone => DeviceClass::Phone,
        TransportClass::Tablet => DeviceClass::Tablet,
        TransportClass::Laptop | TransportClass::Desktop | TransportClass::Other => {
            DeviceClass::Computer
        }
    }
}
