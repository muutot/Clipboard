//! Self-pipe used to wake platform event-loop monitors on stop.
//!
//! The X11 (XFixes) and Wayland (data-control) monitors block in `poll(2)`
//! waiting for socket readability. The monitor owner cannot wake a blocked
//! `poll` through the mpsc stop channel, so both monitors also watch the read
//! end of a pipe; writing a single byte to the write end breaks the loop.
//!
//! Ownership contract:
//! - `StopPipePair` owns both ends. If the event-driven monitor fails to
//!   start, dropping the pair closes both fds.
//! - On success the monitor thread takes over the read end (it closes it on
//!   exit) and the caller keeps the `StopPipeWriter`, triggering it from
//!   `stop()`.

#[cfg(target_os = "linux")]
use std::io;

/// Both ends of a freshly created non-blocking pipe.
#[cfg(target_os = "linux")]
pub(crate) struct StopPipePair {
    read_fd: i32,
    write_fd: i32,
}

#[cfg(target_os = "linux")]
impl StopPipePair {
    pub(crate) fn new() -> io::Result<Self> {
        let mut fds = [0 as libc::c_int; 2];
        if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // Non-blocking so the stop write can never block even if the monitor
        // thread already exited.
        for fd in fds {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL, 0) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                let error = io::Error::last_os_error();
                unsafe {
                    libc::close(fd);
                }
                return Err(error);
            }
        }
        Ok(Self {
            read_fd: fds[0],
            write_fd: fds[1],
        })
    }

    /// The fd the monitor thread watches for the stop byte.
    pub(crate) fn reader_fd(&self) -> i32 {
        self.read_fd
    }

    /// Hands the write end to the monitor owner; the read end becomes the
    /// monitor thread's responsibility.
    pub(crate) fn into_writer(mut self) -> StopPipeWriter {
        let fd = self.write_fd;
        self.write_fd = -1;
        self.read_fd = -1;
        StopPipeWriter { fd }
    }
}

#[cfg(target_os = "linux")]
impl Drop for StopPipePair {
    fn drop(&mut self) {
        // A negative fd means ownership was already transferred.
        for fd in [self.read_fd, self.write_fd] {
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
        }
    }
}

/// Write end of the stop pipe, kept by the monitor owner.
#[cfg(target_os = "linux")]
pub(crate) struct StopPipeWriter {
    fd: i32,
}

#[cfg(target_os = "linux")]
impl StopPipeWriter {
    /// Wakes the monitor thread and closes the pipe.
    pub(crate) fn trigger(mut self) {
        if self.fd >= 0 {
            let byte = b"x";
            unsafe {
                libc::write(self.fd, byte.as_ptr().cast(), 1);
                libc::close(self.fd);
            }
            self.fd = -1;
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for StopPipeWriter {
    fn drop(&mut self) {
        if self.fd >= 0 {
            unsafe { libc::close(self.fd) };
        }
    }
}
