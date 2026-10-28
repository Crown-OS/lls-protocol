//! Validated newtypes for the quantities the engine reasons about.

mod bitrate;
mod frame_rate;
mod loss_rate;
mod per_mille;

pub use bitrate::Bitrate;
pub use frame_rate::FrameRate;
pub use loss_rate::LossRate;
pub use per_mille::PerMille;
