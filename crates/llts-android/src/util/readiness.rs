use std::io;
use std::os::fd::RawFd;
use std::time::Duration;

const WATCHED: usize = 2;

/// Blocks until either descriptor is readable or `timeout` passes, with the nanosecond
/// resolution of `ppoll(2)`: the pacer schedules packets well inside a millisecond.
pub(crate) fn wait_readable(
    descriptors: [RawFd; WATCHED],
    timeout: Option<Duration>,
) -> io::Result<()> {
    let mut watched = descriptors.map(|fd| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    });
    let timespec = timeout.map(|timeout| libc::timespec {
        tv_sec: libc::time_t::try_from(timeout.as_secs()).unwrap_or(libc::time_t::MAX),
        tv_nsec: libc::c_long::from(timeout.subsec_nanos()),
    });
    let timespec_ptr = timespec
        .as_ref()
        .map_or(std::ptr::null(), std::ptr::from_ref);
    // SAFETY: `watched` is a live array of `WATCHED` pollfds, the timespec (when present)
    // outlives the call, and a null signal mask keeps the thread's own.
    let ready = unsafe {
        libc::ppoll(
            watched.as_mut_ptr(),
            libc::nfds_t::try_from(WATCHED).unwrap_or(0),
            timespec_ptr,
            std::ptr::null(),
        )
    };
    if ready < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(())
}
