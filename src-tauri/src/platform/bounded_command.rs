//! Small subprocess reads with bounded output, deadline and capture cancellation.
//! Pipes are read only when nonblocking/available; no reader thread can outlive us
//! waiting for a descendant that inherited stdout.
use std::{
    cell::RefCell,
    io::{self, Read},
    marker::PhantomData,
    process::{Child, ChildStdout, Command, Output, Stdio},
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone)]
struct Budget {
    deadline: Instant,
    stop: Arc<AtomicBool>,
    failed: bool,
}
thread_local! { static BUDGET: RefCell<Option<Budget>> = const { RefCell::new(None) }; }
pub struct CaptureBudget(Option<Budget>, PhantomData<Rc<()>>);
impl CaptureBudget {
    pub fn enter(stop: Arc<AtomicBool>) -> Self {
        Self(
            BUDGET.with(|slot| {
                slot.replace(Some(Budget {
                    deadline: Instant::now() + Duration::from_secs(3),
                    stop,
                    failed: false,
                }))
            }),
            PhantomData,
        )
    }
}
impl Drop for CaptureBudget {
    fn drop(&mut self) {
        BUDGET.with(|slot| slot.replace(self.0.take()));
    }
}
pub fn capture_aborted() -> bool {
    BUDGET.with(|slot| {
        slot.borrow().as_ref().is_some_and(|budget| {
            budget.failed || budget.stop.load(Ordering::SeqCst) || Instant::now() >= budget.deadline
        })
    })
}
pub(super) fn fail_capture() {
    BUDGET.with(|slot| {
        if let Some(budget) = slot.borrow_mut().as_mut() {
            budget.failed = true;
        }
    });
}

struct RunningChild(Child);
impl Drop for RunningChild {
    fn drop(&mut self) {
        // Unix helpers get a private process group, so a forked descendant
        // cannot keep the pipe open after timeout/cancellation.
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub trait BoundedCommandExt {
    fn bounded_output(&mut self, max_bytes: usize) -> io::Result<Output>;
}
impl BoundedCommandExt for Command {
    fn bounded_output(&mut self, max_bytes: usize) -> io::Result<Output> {
        if capture_aborted() {
            return Err(io::Error::other("capture cancelled or expired"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            self.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let deadline = Instant::now() + Duration::from_millis(600);
        let mut child = RunningChild(
            self.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?,
        );
        let mut stdout = child
            .0
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing stdout"))?;
        configure_pipe(&stdout)?;
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 32 * 1024];
        let mut eof = false;
        loop {
            if capture_aborted() || Instant::now() >= deadline {
                fail_capture();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "helper cancelled or timed out",
                ));
            }
            if !eof {
                let available = buffer
                    .len()
                    .min(max_bytes.saturating_sub(bytes.len()).saturating_add(1));
                match read_available(&mut stdout, &mut buffer[..available]) {
                    Ok(0) => eof = true,
                    Ok(count) => {
                        if count > max_bytes.saturating_sub(bytes.len()) {
                            fail_capture();
                            return Err(io::Error::other("helper output exceeds limit"));
                        }
                        bytes.extend_from_slice(&buffer[..count]);
                        continue;
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error),
                }
            }
            if eof {
                if let Some(status) = child.0.try_wait()? {
                    return Ok(Output {
                        status,
                        stdout: bytes,
                        stderr: Vec::new(),
                    });
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(unix)]
fn configure_pipe(pipe: &ChildStdout) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(unix)]
fn read_available(pipe: &mut ChildStdout, buffer: &mut [u8]) -> io::Result<usize> {
    pipe.read(buffer)
}

#[cfg(windows)]
fn configure_pipe(_: &ChildStdout) -> io::Result<()> {
    Ok(())
}
#[cfg(windows)]
fn read_available(pipe: &mut ChildStdout, buffer: &mut [u8]) -> io::Result<usize> {
    use std::os::windows::io::AsRawHandle;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            handle: *mut std::ffi::c_void,
            buffer: *mut std::ffi::c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            left: *mut u32,
        ) -> i32;
    }
    let mut available = 0;
    // The pipe handle is borrowed and remains live throughout this call.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(109) {
            Ok(0)
        } else {
            Err(error)
        };
    }
    if available == 0 {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    let count = buffer.len().min(available as usize);
    pipe.read(&mut buffer[..count])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    #[ignore = "subprocess fixture, invoked by bounded-command tests"]
    fn child_fixture() {
        match std::env::var("CLIPBOARD_HELPER_FIXTURE").as_deref() {
            Ok("stall") => std::thread::sleep(Duration::from_secs(5)),
            Ok("large") => {
                let _ = std::io::stdout().write_all(&vec![b'x'; 1024 * 1024]);
            }
            _ => println!("fixture payload"),
        }
    }
    fn fixture(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "platform::bounded_command::tests::child_fixture",
                "--ignored",
                "--nocapture",
            ])
            .env("CLIPBOARD_HELPER_FIXTURE", mode);
        command
    }
    #[test]
    fn output_success_overflow_timeout_and_stop_are_bounded() {
        let result = fixture("normal").bounded_output(4096).unwrap();
        assert!(result.status.success());
        assert!(String::from_utf8(result.stdout)
            .unwrap()
            .contains("fixture payload"));
        assert!(fixture("large").bounded_output(4096).is_err());
        let started = Instant::now();
        assert!(fixture("stall").bounded_output(4096).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        let stop = Arc::new(AtomicBool::new(false));
        let _budget = CaptureBudget::enter(stop.clone());
        let signal = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            stop.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        assert!(fixture("stall").bounded_output(4096).is_err());
        assert!(capture_aborted());
        assert!(started.elapsed() < Duration::from_secs(2));
        signal.join().unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn inherited_pipe_does_not_leave_a_reader_waiting_forever() {
        let started = Instant::now();
        assert!(Command::new("sh")
            .args(["-c", "sleep 10 & exit 0"])
            .bounded_output(4096)
            .is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
