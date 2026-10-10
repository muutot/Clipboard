//! Stop-aware Wayland dispatch. Never wait for a compositor roundtrip inside
//! the monitor loop: readiness is only permission to read the available bytes.
use std::{io, os::fd::AsRawFd, time::Instant};
use wayland_client::EventQueue;

pub(super) fn dispatch_once<State>(
    queue: &mut EventQueue<State>,
    state: &mut State,
    stop_fd: i32,
    deadline: Option<Instant>,
) -> io::Result<()> {
    let dispatched = queue.dispatch_pending(state).map_err(io::Error::other)?;
    queue.flush().map_err(io::Error::other)?;
    if dispatched > 0 {
        return Ok(());
    }
    let Some(guard) = queue.prepare_read() else {
        return Ok(());
    };
    let mut fds = [
        libc::pollfd {
            fd: queue.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: stop_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    loop {
        let timeout = match deadline {
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                remaining.as_millis().clamp(1, i32::MAX as u128) as i32
            }
            None => -1,
        };
        // Both borrowed descriptors remain live until the read guard is dropped.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as _, timeout) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if ready == 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if fds[1].revents != 0 {
            return Err(io::ErrorKind::ConnectionAborted.into());
        }
        if fds[0].revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if fds[0].revents & libc::POLLIN != 0 {
            break;
        }
    }
    match guard.read() {
        Ok(_) => {}
        Err(wayland_client::backend::WaylandError::Io(error))
            if error.kind() == io::ErrorKind::WouldBlock =>
        {
            return Ok(())
        }
        Err(error) => return Err(io::Error::other(error)),
    }
    queue.dispatch_pending(state).map_err(io::Error::other)?;
    Ok(())
}

use std::os::fd::AsFd;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::net::UnixStream, time::Duration};
    use wayland_client::{protocol::wl_callback, Connection, Dispatch, Proxy, QueueHandle};

    #[derive(Default)]
    struct State(bool);
    impl Dispatch<wl_callback::WlCallback, ()> for State {
        fn event(
            state: &mut Self,
            _: &wl_callback::WlCallback,
            _: wl_callback::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            state.0 = true;
        }
    }

    #[test]
    fn dispatch_handles_events_then_stop_timeout_and_disconnect_without_roundtrip() {
        let (client, mut compositor) = UnixStream::pair().unwrap();
        let connection = Connection::from_socket(client).unwrap();
        let mut queue = connection.new_event_queue();
        let mut state = State::default();
        let stop = crate::platform::linux::stop_pipe::StopPipePair::new().unwrap();
        let reader = stop.reader_fd();
        let callback = connection.display().sync(&queue.handle(), ());
        // Send a real wl_callback.done packet; the peer never answers another
        // sync request. A roundtrip-based dispatcher would block after this.
        for word in [callback.id().protocol_id(), 12u32 << 16, 1] {
            compositor.write_all(&word.to_ne_bytes()).unwrap();
        }
        dispatch_once(
            &mut queue,
            &mut state,
            reader,
            Some(Instant::now() + Duration::from_secs(1)),
        )
        .unwrap();
        assert!(state.0);
        assert_eq!(
            dispatch_once(
                &mut queue,
                &mut state,
                reader,
                Some(Instant::now() + Duration::from_millis(20))
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        stop.into_writer().trigger();
        assert_eq!(
            dispatch_once(&mut queue, &mut state, reader, None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::ConnectionAborted
        );
        unsafe { libc::close(reader) };
        drop(compositor);
        assert!(dispatch_once(
            &mut queue,
            &mut state,
            -1,
            Some(Instant::now() + Duration::from_secs(1))
        )
        .is_err());
    }
}
