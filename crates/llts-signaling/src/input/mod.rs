//! Remote input on an llts Input stream, in the wire format `crownconnect-linux` defined.
//!
//! Each input-stream batch is a run of framed [`InputMessage`]s, `[len: u8][postcard message]`
//! each, ending in [`InputEvent::Frame`], so one batch is one moment of input. A key-state
//! snapshot is a batch of `Key { pressed: true }` messages, one per held key. The layout of
//! every event is fixed: new kinds of events are appended, never inserted.

mod batch;
mod event;

pub use batch::{
    FRAME_CAPACITY, FramedMessage, InputBatch, MAX_MESSAGE_BYTES, frame, messages, pressed_keys,
};
pub use event::{InputEvent, InputMessage};
