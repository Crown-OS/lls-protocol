use crate::SignalingError;

/// The [`limits`](crate::message::limits) a body keeps to: producers check before sending, receivers after
/// decoding, so neither side buffers or shows what the other should never have sent.
pub trait Validate {
    /// # Errors
    ///
    /// Returns the first cap the body breaks, naming the field but never its content.
    fn validate(&self) -> Result<(), SignalingError> {
        Ok(())
    }
}
