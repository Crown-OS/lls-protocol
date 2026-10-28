use crate::SignalingError;
use crate::input::{InputEvent, InputMessage};

/// Postcard bytes of the largest message, well under the one-byte length prefix.
pub const MAX_MESSAGE_BYTES: usize = 32;

/// Bytes of the largest framed message: the length prefix and the message.
pub const FRAME_CAPACITY: usize = MAX_MESSAGE_BYTES + 1;

/// One message framed as `[len: u8][postcard message]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramedMessage {
    bytes: [u8; FRAME_CAPACITY],
}

impl FramedMessage {
    pub fn as_bytes(&self) -> &[u8] {
        let len = self.bytes.first().copied().map_or(0, usize::from);
        self.bytes.get(..=len).unwrap_or_default()
    }
}

/// Frames `message` for an input-stream batch or a byte pipe.
///
/// # Errors
///
/// Fails only if the message does not encode into [`MAX_MESSAGE_BYTES`], which no event does.
pub fn frame(message: &InputMessage) -> Result<FramedMessage, SignalingError> {
    let mut bytes = [0u8; FRAME_CAPACITY];
    let (prefix, body) = bytes.split_at_mut(1);
    let len = postcard::to_slice(message, body)
        .map_err(SignalingError::Encode)?
        .len();
    prefix.fill(u8::try_from(len).map_err(|_| SignalingError::InputMessageTooLong { len })?);
    Ok(FramedMessage { bytes })
}

/// One input-stream batch being written.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InputBatch {
    bytes: Vec<u8>,
}

impl InputBatch {
    /// Appends one framed message.
    ///
    /// # Errors
    ///
    /// See [`frame`].
    pub fn push(&mut self, time_ms: u32, event: InputEvent) -> Result<(), SignalingError> {
        self.bytes
            .extend_from_slice(frame(&InputMessage { time_ms, event })?.as_bytes());
        Ok(())
    }

    pub const fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Reads the next framed message off the front of `bytes`; `None` once they run out.
fn next_message(bytes: &mut &[u8]) -> Option<Result<InputMessage, SignalingError>> {
    let (&len, rest) = bytes.split_first()?;
    let len = usize::from(len);
    let Some(body) = rest.get(..len).filter(|_| len <= MAX_MESSAGE_BYTES) else {
        *bytes = &[];
        return Some(Err(SignalingError::InputMessageTooLong { len }));
    };
    *bytes = rest.get(len..).unwrap_or_default();
    Some(postcard::from_bytes(body).map_err(SignalingError::MalformedInput))
}

/// Every message of a batch, stopping at the first malformed one.
pub fn messages(mut batch: &[u8]) -> impl Iterator<Item = InputMessage> + '_ {
    core::iter::from_fn(move || next_message(&mut batch)?.ok())
}

/// The keys a key-state snapshot says are held.
pub fn pressed_keys(snapshot: &[u8]) -> impl Iterator<Item = u32> + '_ {
    messages(snapshot).filter_map(|message| match message.event {
        InputEvent::Key {
            code,
            pressed: true,
        } => Some(code),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_EVENT: [InputEvent; 10] = [
        InputEvent::PointerMotion { x: 0.25, y: 0.75 },
        InputEvent::PointerButton {
            button: 0x110,
            pressed: true,
        },
        InputEvent::Scroll {
            horizontal_v120: 0,
            vertical_v120: -120,
        },
        InputEvent::Key {
            code: 30,
            pressed: true,
        },
        InputEvent::TouchDown {
            id: 3,
            x: 1.0,
            y: 0.0,
        },
        InputEvent::TouchMotion {
            id: i32::MIN,
            x: f32::MAX,
            y: f32::MIN,
        },
        InputEvent::TouchUp { id: 3 },
        InputEvent::TouchCancel,
        InputEvent::Frame,
        InputEvent::PointerRelative { dx: 3.0, dy: -1.0 },
    ];

    #[test]
    fn a_batch_round_trips_every_event() -> Result<(), SignalingError> {
        let mut batch = InputBatch::default();
        for event in EVERY_EVENT {
            batch.push(u32::MAX, event)?;
        }
        let decoded: Vec<InputEvent> = messages(batch.as_bytes())
            .map(|message| message.event)
            .collect();
        assert_eq!(decoded, EVERY_EVENT);
        Ok(())
    }

    #[test]
    fn the_wire_layout_matches_crownconnect_linux() -> Result<(), SignalingError> {
        let key = frame(&InputMessage {
            time_ms: 1000,
            event: InputEvent::Key {
                code: 30,
                pressed: true,
            },
        })?;
        assert_eq!(key.as_bytes(), [5, 0xe8, 0x07, 3, 30, 1]);
        let relative = frame(&InputMessage {
            time_ms: 6,
            event: InputEvent::PointerRelative { dx: 3.0, dy: -1.0 },
        })?;
        assert_eq!(
            relative.as_bytes(),
            [10, 6, 9, 0, 0, 0x40, 0x40, 0, 0, 0x80, 0xbf]
        );
        Ok(())
    }

    #[test]
    fn a_malformed_message_ends_the_batch() -> Result<(), SignalingError> {
        let mut batch = InputBatch::default();
        batch.push(1, InputEvent::Frame)?;
        let mut bytes = batch.into_bytes();
        bytes.extend_from_slice(&[2, 0xff, 0xff]);
        let trailing = frame(&InputMessage {
            time_ms: 2,
            event: InputEvent::Frame,
        })?;
        bytes.extend_from_slice(trailing.as_bytes());
        assert_eq!(messages(&bytes).count(), 1);
        assert_eq!(messages(&[40]).count(), 0);
        Ok(())
    }

    #[test]
    fn a_snapshot_lists_the_held_keys() -> Result<(), SignalingError> {
        let mut snapshot = InputBatch::default();
        for (code, pressed) in [(29, true), (30, false), (42, true)] {
            snapshot.push(0, InputEvent::Key { code, pressed })?;
        }
        assert_eq!(
            pressed_keys(snapshot.as_bytes()).collect::<Vec<_>>(),
            [29, 42]
        );
        Ok(())
    }
}
