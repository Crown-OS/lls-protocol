#![allow(
    unsafe_code,
    reason = "SO_BUSY_POLL has no socket2 wrapper and needs a raw setsockopt"
)]

use std::net::SocketAddr;

use socket2::{Domain, Protocol, SockRef, Socket, Type};

use crate::TransportError;

/// DSCP AF41, which consumer access points map to the WMM video access category.
///
/// AC_VI's 3 ms transmit opportunity is long enough for a whole frame's burst to clear on one
/// contention win. Marking bulk video as voice instead is a common mistake: AC_VO's 1.5 ms
/// opportunity splits a frame across two contention cycles and makes latency worse.
pub const DSCP_VIDEO: u32 = 0x88;

/// DSCP EF, the voice access category, for the small control and feedback packets whose whole
/// value is arriving quickly.
pub const DSCP_CONTROL: u32 = 0xB8;

/// Best effort, for bulk data that must never delay a frame.
pub const DSCP_BULK: u32 = 0x00;

/// Large enough that a burst of frames cannot overflow the socket while the engine is busy, small
/// enough that a backlog is a bug rather than hidden latency.
const BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// Microseconds the kernel will spin waiting for a packet before sleeping.
///
/// Busy-polling barely moves the median but cuts the tail hard — measurements on Linux put worst
/// case at 38 µs against 195 µs without it, and the tail is what a frame deadline cares about.
#[cfg(target_os = "linux")]
const BUSY_POLL_MICROS: u32 = 50;

/// An unbound UDP socket for `local`'s family. An IPv6 socket is made dual-stack explicitly, so
/// binding `[::]` reaches IPv4 peers whatever the `bindv6only` sysctl says.
pub fn udp_socket(local: SocketAddr) -> Result<Socket, TransportError> {
    let socket = Socket::new(Domain::for_address(local), Type::DGRAM, Some(Protocol::UDP))?;
    if local.is_ipv6() {
        socket.set_only_v6(false)?;
    }
    Ok(socket)
}

/// Applies the settings that matter for a real-time media socket.
///
/// Every one of these is best-effort: a kernel or a container may refuse any of them, and none is
/// worth failing a session over.
pub fn apply_low_latency_options(socket: &Socket, dscp: u32) -> Result<(), TransportError> {
    let reference = SockRef::from(socket);
    let _ = reference.set_recv_buffer_size(BUFFER_BYTES);
    let _ = reference.set_send_buffer_size(BUFFER_BYTES);
    let _ = reference.set_tos_v4(dscp);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let _ = reference.set_tclass_v6(dscp);
    }

    #[cfg(target_os = "linux")]
    {
        let _ = set_busy_poll(socket, BUSY_POLL_MICROS);
    }
    socket.set_nonblocking(true)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn set_busy_poll(socket: &Socket, micros: u32) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;

    let value = libc::c_int::try_from(micros).unwrap_or(libc::c_int::MAX);
    // SAFETY: the file descriptor is owned by `socket` and outlives the call, and `value` is a
    // correctly sized and aligned `c_int`, which is what SO_BUSY_POLL expects per socket(7).
    let outcome = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_BUSY_POLL,
            (&raw const value).cast(),
            libc::socklen_t::try_from(size_of_val(&value)).unwrap_or(0),
        )
    };
    if outcome == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
