use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::os::fd::{AsRawFd, RawFd};
use std::time::Duration;

use super::batch::{BatchIo, Outgoing};
use super::socket_options::{self, DSCP_CONTROL};
use crate::TransportError;
use crate::util::SocketFamily;

/// One unconnected UDP socket carrying every session this device has.
///
/// A socket per peer would cost a port, a file descriptor and a wakeup source each; one shared
/// socket needs none of that, and the receiver-chosen session tag in every header routes each
/// datagram to its session with one table lookup. Raw UDP rather than QUIC for the LAN case:
/// both ends are ours, so QUIC's interoperability buys nothing, while its congestion controller
/// would gate our datagrams against a second control loop fighting the one in the engine.
#[derive(Debug)]
pub struct SessionSocket {
    socket: UdpSocket,
    io: BatchIo,
}

impl SessionSocket {
    pub fn bind(local: SocketAddr) -> Result<Self, TransportError> {
        let socket = socket_options::udp_socket(local)?;
        socket.bind(&local.into())?;
        socket_options::apply_low_latency_options(&socket, DSCP_CONTROL)?;
        Ok(Self {
            socket: socket.into(),
            io: BatchIo::new(SocketFamily::of(local)),
        })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        Ok(self.socket.local_addr()?)
    }

    pub const fn gso_enabled(&self) -> bool {
        self.io.gso_enabled()
    }

    /// Sends as many datagrams as the socket accepts now; a full queue is not an error, since
    /// the pacer and the reliable channels resend on their own clocks.
    ///
    /// # Errors
    ///
    /// [`TransportError::SendFailed`] when the kernel refused the first datagram, which the
    /// caller should count and skip: every datagram before it was sent, none after it was tried.
    pub fn send_batch(&mut self, datagrams: &[Outgoing<'_>]) -> Result<usize, TransportError> {
        self.io
            .send(&self.socket, datagrams)
            .map_err(|source| match datagrams.first() {
                Some(refused) => TransportError::SendFailed {
                    to: refused.to,
                    source,
                },
                None => TransportError::Io(source),
            })
    }

    /// Reads whatever is waiting without blocking.
    pub fn receive_batch(&mut self) -> Result<usize, TransportError> {
        Ok(self.io.receive(&self.socket)?)
    }

    /// The datagrams the last receive read, each with its sender's canonical address: an IPv4
    /// peer is plain IPv4 even on a dual-stack socket.
    pub fn received_mut(&mut self) -> impl Iterator<Item = (&mut [u8], SocketAddr)> {
        self.io.received_mut()
    }

    /// Blocks until a datagram is waiting or `timeout` passes, returning whether one is.
    pub fn wait_readable(&self, timeout: Option<Duration>) -> Result<bool, TransportError> {
        Ok(poll_readable(self.socket.as_raw_fd(), timeout)?)
    }
}

impl AsRawFd for SessionSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

#[allow(
    unsafe_code,
    reason = "poll(2) has no std wrapper, and the driver must wait on a socket without a runtime"
)]
fn poll_readable(fd: RawFd, timeout: Option<Duration>) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let millis = timeout.map_or(-1, |timeout| {
        libc::c_int::try_from(timeout.as_millis().max(u128::from(!timeout.is_zero())))
            .unwrap_or(libc::c_int::MAX)
    });
    // SAFETY: `descriptor` is a valid `pollfd` for the duration of the call and the count is one.
    let outcome = unsafe { libc::poll(&raw mut descriptor, 1, millis) };
    match outcome {
        -1 => {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                Ok(false)
            } else {
                Err(error)
            }
        }
        0 => Ok(false),
        _ => Ok(descriptor.revents & libc::POLLIN != 0),
    }
}
