use std::ffi::CStr;
use std::sync::Arc;

use crossbeam_queue::ArrayQueue;
use ndk::media::media_codec::{ActionCode, AsyncNotifyCallback, BufferInfo};

use crate::util::wake::Waker;

/// Callbacks a codec can have outstanding at once; each held output buffer is one, so this is
/// far above what a low-latency codec ever queues.
const QUEUE_DEPTH: usize = 256;

/// A filled output buffer, as the codec's callback thread described it.
#[derive(Debug, Clone, Copy)]
pub(super) struct OutputReady {
    pub(super) index: usize,
    pub(super) offset: usize,
    pub(super) size: usize,
    pub(super) flags: u32,
    pub(super) presentation_us: i64,
}

#[derive(Debug)]
pub(super) enum CodecEvent {
    InputAvailable(usize),
    OutputAvailable(OutputReady),
    Failed(String),
}

/// What a codec's callback thread hands the runtime thread: buffer indices only, never data,
/// so the runtime can read an output buffer in place and release it.
#[derive(Debug)]
pub(super) struct CodecEvents(ArrayQueue<CodecEvent>);

impl CodecEvents {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self(ArrayQueue::new(QUEUE_DEPTH)))
    }

    pub(super) fn pop(&self) -> Option<CodecEvent> {
        self.0.pop()
    }

    fn push(&self, event: CodecEvent, waker: &Waker) {
        if self.0.push(event).is_err() {
            log::error!("a codec callback queue overflowed; the codec will stall");
        }
        waker.wake();
    }
}

/// Callbacks that queue each event and wake the runtime; they run on the codec's own thread.
pub(super) fn callback(events: &Arc<CodecEvents>, waker: &Waker) -> AsyncNotifyCallback {
    let (input_events, input_waker) = (Arc::clone(events), waker.clone());
    let (output_events, output_waker) = (Arc::clone(events), waker.clone());
    let (error_events, error_waker) = (Arc::clone(events), waker.clone());
    AsyncNotifyCallback {
        on_input_available: Some(Box::new(move |index| {
            input_events.push(CodecEvent::InputAvailable(index), &input_waker);
        })),
        on_output_available: Some(Box::new(move |index, info: &BufferInfo| {
            let ready = OutputReady {
                index,
                offset: usize::try_from(info.offset()).unwrap_or(0),
                size: usize::try_from(info.size()).unwrap_or(0),
                flags: info.flags(),
                presentation_us: info.presentation_time_us(),
            };
            output_events.push(CodecEvent::OutputAvailable(ready), &output_waker);
        })),
        on_format_changed: None,
        on_error: Some(Box::new(move |error, action: ActionCode, detail: &CStr| {
            if !action.is_recoverable() && !action.is_transient() {
                let detail = format!("{error:?}: {}", detail.to_string_lossy());
                error_events.push(CodecEvent::Failed(detail), &error_waker);
            }
        })),
    }
}
