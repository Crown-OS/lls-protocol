#![allow(
    unsafe_code,
    reason = "sendmmsg, recvmmsg and their control messages have no safe wrapper"
)]

use std::io;
use std::net::{SocketAddr, UdpSocket};

use crate::util::{SocketFamily, canonical};

/// Datagrams moved per system call.
pub const MAX_BATCH: usize = 32;

/// Receive buffer per datagram. Anything longer than a path MTU is not ours.
pub const RECEIVE_BUFFER_LEN: usize = 2_048;

/// One datagram to send, with the DSCP it should be marked with.
#[derive(Debug, Clone, Copy)]
pub struct Outgoing<'a> {
    pub bytes: &'a [u8],
    pub to: SocketAddr,
    /// The whole TOS / traffic-class byte, DSCP in the top six bits.
    pub tos: u8,
}

/// Batched datagram I/O over one UDP socket.
///
/// On Linux and Android this is `sendmmsg`/`recvmmsg`, with the TOS byte set per datagram in a
/// control message so one socket can mark voice, video and bulk traffic differently. A run of
/// same-sized datagrams to one peer is coalesced into a single UDP GSO send, which the kernel
/// segments far more cheaply than it handles separate sends; a kernel without GSO turns it off
/// on the first refusal and the batch is resent plainly. Elsewhere it degrades to one system
/// call per datagram.
///
/// Addresses cross this boundary canonical: an IPv6 socket is handed v4-mapped destinations
/// for IPv4 peers, and IPv4 senders come back as plain IPv4.
#[derive(Debug)]
pub struct BatchIo {
    family: SocketFamily,
    receive_buffers: Box<[[u8; RECEIVE_BUFFER_LEN]]>,
    received_lengths: [usize; MAX_BATCH],
    received_from: [Option<SocketAddr>; MAX_BATCH],
    received_count: usize,
    gso_staging: Vec<u8>,
    gso_enabled: bool,
}

impl BatchIo {
    /// Bytes the kernel will segment in one GSO send.
    const GSO_MAX_BYTES: usize = 64_000;
    const GSO_MAX_SEGMENTS: usize = 64;
    /// Coalesced sends per batch, each needing its own staging region.
    const GSO_MESSAGES: usize = 4;

    pub(crate) fn new(family: SocketFamily) -> Self {
        Self {
            family,
            receive_buffers: vec![[0u8; RECEIVE_BUFFER_LEN]; MAX_BATCH].into_boxed_slice(),
            received_lengths: [0; MAX_BATCH],
            received_from: [None; MAX_BATCH],
            received_count: 0,
            gso_staging: vec![0u8; Self::GSO_MAX_BYTES * Self::GSO_MESSAGES],
            gso_enabled: cfg!(any(target_os = "linux", target_os = "android")),
        }
    }

    pub const fn gso_enabled(&self) -> bool {
        self.gso_enabled
    }

    /// Sends as many of `datagrams` as the socket accepts, returning how many left.
    ///
    /// # Errors
    ///
    /// The kernel's refusal of the first datagram. A refusal further in ends the call with the
    /// count sent before it, so the caller never resends a datagram that already left.
    pub fn send(&mut self, socket: &UdpSocket, datagrams: &[Outgoing<'_>]) -> io::Result<usize> {
        let mut sent = 0;
        while let Some(rest) = datagrams.get(sent..).filter(|rest| !rest.is_empty()) {
            match platform::send(self, socket, rest) {
                Ok(0) => break,
                Ok(count) => sent += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if platform::is_gso_refusal(&error) && self.gso_run(rest).is_some() => {
                    self.gso_enabled = false;
                }
                Err(_) if sent > 0 => break,
                Err(error) => return Err(error),
            }
        }
        Ok(sent)
    }

    /// Reads whatever is waiting, up to [`MAX_BATCH`] datagrams, without blocking.
    pub fn receive(&mut self, socket: &UdpSocket) -> io::Result<usize> {
        self.received_count = 0;
        match platform::receive(self, socket) {
            Ok(count) => {
                self.received_count = count;
                Ok(count)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionRefused
                ) =>
            {
                Ok(0)
            }
            Err(error) => Err(error),
        }
    }

    /// The datagrams the last [`BatchIo::receive`] read, mutable so they can be opened in place.
    pub fn received_mut(&mut self) -> impl Iterator<Item = (&mut [u8], SocketAddr)> {
        let count = self.received_count;
        self.receive_buffers
            .iter_mut()
            .zip(self.received_lengths)
            .zip(self.received_from)
            .take(count)
            .filter_map(|((buffer, len), from)| Some((buffer.get_mut(..len)?, canonical(from?))))
    }

    /// Where a run of same-destination, same-marking datagrams starting at the front ends, and
    /// the segment size to coalesce it with, if coalescing is worth it.
    fn gso_run(&self, datagrams: &[Outgoing<'_>]) -> Option<(usize, usize)> {
        if !self.gso_enabled {
            return None;
        }
        let first = datagrams.first()?;
        let segment = first.bytes.len();
        let mut total = 0;
        let mut count = 0;
        for datagram in datagrams.iter().take(Self::GSO_MAX_SEGMENTS) {
            let fits = datagram.to == first.to
                && datagram.tos == first.tos
                && datagram.bytes.len() <= segment
                && total + datagram.bytes.len() <= Self::GSO_MAX_BYTES;
            if !fits {
                break;
            }
            total += datagram.bytes.len();
            count += 1;
            if datagram.bytes.len() < segment {
                break;
            }
        }
        (count > 1).then_some((count, segment))
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[allow(
    clippy::cast_possible_truncation,
    reason = "the FFI lengths here are struct sizes and batch counts, all far below u32::MAX"
)]
mod platform {
    use std::io;
    use std::mem::{size_of, zeroed};
    use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
    use std::os::fd::AsRawFd as _;

    use socket2::SockAddr;

    use super::{BatchIo, MAX_BATCH, Outgoing};
    use crate::util::travels_over_ipv4;

    /// Room for a TOS and a GSO control message, aligned for `cmsghdr`.
    type ControlBuffer = [u64; 8];

    struct Planned {
        first: usize,
        count: usize,
        segment: Option<usize>,
    }

    /// Whether a coalesced send failed because this kernel or device cannot segment UDP.
    pub(super) fn is_gso_refusal(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error(),
            Some(libc::EIO | libc::EINVAL | libc::ENOPROTOOPT)
        )
    }

    pub(super) fn send(
        io: &mut BatchIo,
        socket: &UdpSocket,
        datagrams: &[Outgoing<'_>],
    ) -> io::Result<usize> {
        let mut plans: Vec<Planned> = Vec::with_capacity(MAX_BATCH);
        let mut next = 0;
        let mut gso_messages = 0;
        while plans.len() < MAX_BATCH && next < datagrams.len() {
            let rest = datagrams.get(next..).unwrap_or_default();
            let run = (gso_messages < BatchIo::GSO_MESSAGES)
                .then(|| io.gso_run(rest))
                .flatten();
            let (count, segment) = match run {
                Some((count, segment)) => {
                    gso_messages += 1;
                    (count, Some(segment))
                }
                None => (1, None),
            };
            plans.push(Planned {
                first: next,
                count,
                segment,
            });
            next += count;
        }

        let addresses: Vec<SockAddr> = plans
            .iter()
            .filter_map(|plan| {
                datagrams
                    .get(plan.first)
                    .map(|datagram| SockAddr::from(io.family.destination(datagram.to)))
            })
            .collect();
        let mut iovecs: [libc::iovec; MAX_BATCH] = unsafe { zeroed() };
        let mut controls: [ControlBuffer; MAX_BATCH] = [[0; 8]; MAX_BATCH];
        let mut headers: [libc::mmsghdr; MAX_BATCH] = unsafe { zeroed() };

        let mut staged = 0;
        for (index, plan) in plans.iter().enumerate() {
            let Some(first) = datagrams.get(plan.first) else {
                break;
            };
            let bytes: &[u8] = match plan.segment {
                Some(_) => {
                    let start = staged;
                    for datagram in datagrams.iter().skip(plan.first).take(plan.count) {
                        let end = staged + datagram.bytes.len();
                        if let Some(slot) = io.gso_staging.get_mut(staged..end) {
                            slot.copy_from_slice(datagram.bytes);
                        }
                        staged = end;
                    }
                    io.gso_staging.get(start..staged).unwrap_or_default()
                }
                None => first.bytes,
            };
            let (Some(iovec), Some(control), Some(header), Some(address)) = (
                iovecs.get_mut(index),
                controls.get_mut(index),
                headers.get_mut(index),
                addresses.get(index),
            ) else {
                break;
            };
            iovec.iov_base = bytes.as_ptr().cast_mut().cast();
            iovec.iov_len = bytes.len();
            header.msg_hdr.msg_name = address.as_ptr().cast_mut().cast();
            header.msg_hdr.msg_namelen = address.len();
            header.msg_hdr.msg_iov = iovec;
            header.msg_hdr.msg_iovlen = 1;
            // SAFETY: `control` outlives the call below and is large and aligned enough for both
            // control messages `write_controls` may place in it.
            unsafe {
                write_controls(&mut header.msg_hdr, control, first, plan.segment);
            }
        }

        let messages = libc::c_uint::try_from(plans.len()).unwrap_or(0);
        // SAFETY: every header points at an iovec, an address and a control buffer that live
        // until this call returns, and `messages` never exceeds the headers initialised above.
        let outcome =
            unsafe { libc::sendmmsg(socket.as_raw_fd(), headers.as_mut_ptr(), messages, 0) };
        let sent_messages = usize::try_from(outcome).map_err(|_| io::Error::last_os_error())?;
        Ok(plans
            .iter()
            .take(sent_messages)
            .map(|plan| plan.count)
            .sum())
    }

    /// Writes the TOS control message, and the GSO segment size if there is one.
    ///
    /// # Safety
    ///
    /// `control` must stay alive and unmoved until the message header is used.
    unsafe fn write_controls(
        header: &mut libc::msghdr,
        control: &mut ControlBuffer,
        datagram: &Outgoing<'_>,
        segment: Option<usize>,
    ) {
        let tos_space = unsafe { libc::CMSG_SPACE(size_of::<libc::c_int>() as u32) } as usize;
        let gso_space = unsafe { libc::CMSG_SPACE(size_of::<u16>() as u32) } as usize;
        header.msg_control = control.as_mut_ptr().cast();
        header.msg_controllen = tos_space + segment.map_or(0, |_| gso_space);

        // SAFETY: the control buffer is sized for both messages, so the first and next headers
        // are in bounds, and each data pointer has room for the value written through it.
        unsafe {
            let tos = libc::CMSG_FIRSTHDR(header);
            if tos.is_null() {
                return;
            }
            let (level, kind) = if travels_over_ipv4(datagram.to) {
                (libc::IPPROTO_IP, libc::IP_TOS)
            } else {
                (libc::IPPROTO_IPV6, libc::IPV6_TCLASS)
            };
            (*tos).cmsg_level = level;
            (*tos).cmsg_type = kind;
            (*tos).cmsg_len = libc::CMSG_LEN(size_of::<libc::c_int>() as u32) as _;
            libc::CMSG_DATA(tos)
                .cast::<libc::c_int>()
                .write_unaligned(libc::c_int::from(datagram.tos));

            if let Some(segment) = segment {
                let gso = libc::CMSG_NXTHDR(header, tos);
                if gso.is_null() {
                    return;
                }
                (*gso).cmsg_level = libc::SOL_UDP;
                (*gso).cmsg_type = libc::UDP_SEGMENT;
                (*gso).cmsg_len = libc::CMSG_LEN(size_of::<u16>() as u32) as _;
                libc::CMSG_DATA(gso)
                    .cast::<u16>()
                    .write_unaligned(u16::try_from(segment).unwrap_or(u16::MAX));
            }
        }
    }

    pub(super) fn receive(io: &mut BatchIo, socket: &UdpSocket) -> io::Result<usize> {
        let mut storages: [libc::sockaddr_storage; MAX_BATCH] = unsafe { zeroed() };
        let mut iovecs: [libc::iovec; MAX_BATCH] = unsafe { zeroed() };
        let mut headers: [libc::mmsghdr; MAX_BATCH] = unsafe { zeroed() };
        for ((buffer, iovec), (storage, header)) in io
            .receive_buffers
            .iter_mut()
            .zip(iovecs.iter_mut())
            .zip(storages.iter_mut().zip(headers.iter_mut()))
        {
            iovec.iov_base = buffer.as_mut_ptr().cast();
            iovec.iov_len = buffer.len();
            header.msg_hdr.msg_name = (storage as *mut libc::sockaddr_storage).cast();
            header.msg_hdr.msg_namelen = size_of::<libc::sockaddr_storage>() as libc::socklen_t;
            header.msg_hdr.msg_iov = iovec;
            header.msg_hdr.msg_iovlen = 1;
        }

        // SAFETY: each header points at a live buffer, iovec and address slot, and the count
        // passed is the number of headers initialised above.
        let outcome = unsafe {
            libc::recvmmsg(
                socket.as_raw_fd(),
                headers.as_mut_ptr(),
                MAX_BATCH as libc::c_uint,
                libc::MSG_DONTWAIT,
                std::ptr::null_mut(),
            )
        };
        let count = usize::try_from(outcome).map_err(|_| io::Error::last_os_error())?;
        for (index, (header, storage)) in
            headers.iter().zip(storages.iter()).take(count).enumerate()
        {
            let truncated = header.msg_hdr.msg_flags & libc::MSG_TRUNC != 0;
            if let (Some(len), Some(from)) = (
                io.received_lengths.get_mut(index),
                io.received_from.get_mut(index),
            ) {
                *len = if truncated {
                    0
                } else {
                    header.msg_len as usize
                };
                *from = to_socket_addr(storage);
            }
        }
        Ok(count)
    }

    fn to_socket_addr(storage: &libc::sockaddr_storage) -> Option<SocketAddr> {
        match libc::c_int::from(storage.ss_family) {
            libc::AF_INET => {
                // SAFETY: the family says the storage holds a `sockaddr_in`, which it is large
                // and aligned enough for.
                let address = unsafe {
                    *(storage as *const libc::sockaddr_storage).cast::<libc::sockaddr_in>()
                };
                Some(SocketAddr::V4(SocketAddrV4::new(
                    u32::from_be(address.sin_addr.s_addr).into(),
                    u16::from_be(address.sin_port),
                )))
            }
            libc::AF_INET6 => {
                // SAFETY: as above, for `sockaddr_in6`.
                let address = unsafe {
                    *(storage as *const libc::sockaddr_storage).cast::<libc::sockaddr_in6>()
                };
                Some(SocketAddr::V6(SocketAddrV6::new(
                    address.sin6_addr.s6_addr.into(),
                    u16::from_be(address.sin6_port),
                    address.sin6_flowinfo,
                    address.sin6_scope_id,
                )))
            }
            _ => None,
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
mod platform {
    use std::io;
    use std::net::UdpSocket;

    use super::{BatchIo, MAX_BATCH, Outgoing};

    pub(super) const fn is_gso_refusal(_error: &io::Error) -> bool {
        false
    }

    pub(super) fn send(
        io: &mut BatchIo,
        socket: &UdpSocket,
        datagrams: &[Outgoing<'_>],
    ) -> io::Result<usize> {
        let datagram = datagrams.first().ok_or(io::ErrorKind::InvalidInput)?;
        socket.send_to(datagram.bytes, io.family.destination(datagram.to))?;
        Ok(1)
    }

    pub(super) fn receive(io: &mut BatchIo, socket: &UdpSocket) -> io::Result<usize> {
        let mut count = 0;
        for index in 0..MAX_BATCH {
            let Some(buffer) = io.receive_buffers.get_mut(index) else {
                break;
            };
            match socket.recv_from(buffer) {
                Ok((len, from)) => {
                    if let (Some(slot), Some(source)) = (
                        io.received_lengths.get_mut(index),
                        io.received_from.get_mut(index),
                    ) {
                        *slot = len;
                        *source = Some(from);
                    }
                    count += 1;
                }
                Err(error) if count > 0 && error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        Ok(count)
    }
}
