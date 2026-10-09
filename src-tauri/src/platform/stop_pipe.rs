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
        // Atomically set both flags: neither an intermediate fcntl failure nor
        // a concurrent child spawn may leak one of these descriptors.
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } != 0 {
            return Err(io::Error::last_os_error());
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn stop_pipe_is_nonblocking_close_on_exec_and_delivers_stop() {
        let pair = StopPipePair::new().unwrap();
        for fd in [pair.read_fd, pair.write_fd] {
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFL) } & libc::O_NONBLOCK,
                0
            );
            assert_ne!(
                unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC,
                0
            );
        }
        let read_fd = pair.reader_fd();
        let mut byte = 0u8;
        assert_eq!(
            unsafe { libc::read(read_fd, (&mut byte as *mut u8).cast(), 1) },
            -1
        );
        assert_eq!(io::Error::last_os_error().kind(), io::ErrorKind::WouldBlock);
        pair.into_writer().trigger();
        assert_eq!(
            unsafe { libc::read(read_fd, (&mut byte as *mut u8).cast(), 1) },
            1
        );
        assert_eq!(byte, b'x');
        assert_eq!(
            unsafe { libc::read(read_fd, (&mut byte as *mut u8).cast(), 1) },
            0
        );
        // into_writer transfers this end to the monitor, represented here.
        assert_eq!(unsafe { libc::close(read_fd) }, 0);
    }
}
