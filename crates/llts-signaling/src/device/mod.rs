//! Who a device is and what it can share, as every other module refers to it.

mod device_class;
mod device_id;
mod feature;

pub use device_class::DeviceClass;
pub use device_id::DeviceId;
pub use feature::{Feature, FeatureSet};
