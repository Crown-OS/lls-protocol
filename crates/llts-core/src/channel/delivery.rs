use crate::buffer::PooledBuffer;

/// Whether a reliable message must wait for the ones sent before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Delivery {
    #[default]
    Ordered,
    /// Delivered as soon as it is complete, which suits independent state updates.
    Unordered,
}

/// What an input stream hands the application.
#[derive(Debug)]
pub enum InputDelivery {
    /// One batch of events, each batch delivered exactly once.
    Events(PooledBuffer),
    /// The sender's whole pressed-key set, replacing whatever the receiver believed.
    KeyState(PooledBuffer),
}
