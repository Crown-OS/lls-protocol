use std::io;
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixDatagram;
use std::sync::Arc;

/// Wakes the runtime thread out of its poll, from any thread, including codec and audio
/// callbacks: one non-blocking datagram, coalesced when the runtime is already awake.
#[derive(Debug, Clone)]
pub(crate) struct Waker(Arc<UnixDatagram>);

/// The runtime's end, polled alongside the llts socket.
#[derive(Debug)]
pub(crate) struct WakeReceiver(UnixDatagram);

pub(crate) fn wake_pair() -> io::Result<(Waker, WakeReceiver)> {
    let (sender, receiver) = UnixDatagram::pair()?;
    sender.set_nonblocking(true)?;
    receiver.set_nonblocking(true)?;
    Ok((Waker(Arc::new(sender)), WakeReceiver(receiver)))
}

impl Waker {
    pub(crate) fn wake(&self) {
        // A full queue means a wake-up is already pending, which is all this is for.
        let _ = self.0.send(&[1]);
    }
}

impl WakeReceiver {
    pub(crate) fn drain(&self) {
        let mut sink = [0u8; 64];
        while self.0.recv(&mut sink).is_ok() {}
    }
}

impl AsRawFd for WakeReceiver {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
