use serde::{Deserialize, Serialize};

/// Pointer and touch positions are normalised to the mirrored screen: 0.0 is the left or top
/// edge and 1.0 the right or bottom edge, whatever size the viewer window has.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    PointerMotion {
        x: f32,
        y: f32,
    },
    /// Linux evdev button code, such as `BTN_LEFT` (0x110).
    PointerButton {
        button: u32,
        pressed: bool,
    },
    /// Scroll distance in 1/120ths of a wheel detent.
    Scroll {
        horizontal_v120: i32,
        vertical_v120: i32,
    },
    /// Linux evdev key code.
    Key {
        code: u32,
        pressed: bool,
    },
    TouchDown {
        id: i32,
        x: f32,
        y: f32,
    },
    TouchMotion {
        id: i32,
        x: f32,
        y: f32,
    },
    TouchUp {
        id: i32,
    },
    TouchCancel,
    /// Ends a group of events that belong to one moment, like a pointer or touch frame.
    Frame,
    /// Motion in logical pixels rather than to a position, as a shared cursor moves.
    PointerRelative {
        dx: f32,
        dy: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InputMessage {
    /// Milliseconds on the sender's input clock.
    pub time_ms: u32,
    pub event: InputEvent,
}
