//! Authenticated loopback IPC for the non-Windows single-instance guard.
//! The guard owns the endpoint file; no clipboard data crosses this channel.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Serialize, Deserialize)]
struct Endpoint {
    pid: u32,
    port: u16,
    token: String,
}

pub struct LocalWake {
    endpoint_path: PathBuf,
    token: String,
    listener: Option<TcpListener>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LocalWake {
    /// Called only after acquiring the instance lock, so a stale endpoint may
    /// be replaced without disturbing a live owner's channel.
    pub fn bind(project: &Path, pid: u32) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let endpoint_path = project.join("instance-wake.json");
        match fs::remove_file(&endpoint_path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&endpoint_path)?;
        let endpoint = Endpoint {
            pid,
            port: listener.local_addr()?.port(),
            token: token.clone(),
        };
        file.write_all(&serde_json::to_vec(&endpoint)?)?;
        file.sync_all()?;
        Ok(Self {
            endpoint_path,
            token,
            listener: Some(listener),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
        })
    }

    pub fn start<F: Fn() + Send + 'static>(&mut self, callback: F) -> io::Result<()> {
        if self.thread.is_some() {
            return Ok(());
        }
        let listener = self
            .listener
            .as_ref()
            .ok_or_else(|| io::Error::other("wake listener closed"))?
            .try_clone()?;
        let stop = Arc::clone(&self.stop);
        let token = self.token.clone();
        self.thread = Some(
            thread::Builder::new()
                .name("single-instance-wake".into())
                .spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        match listener.accept() {
                            Ok((mut stream, peer)) if peer.ip().is_loopback() => {
                                // Bound total time, including fragmented TCP packets.
                                let mut bytes = [0; 32];
                                if read_token(&mut stream, &mut bytes).is_ok()
                                    && bytes == token.as_bytes()
                                    && !stop.load(Ordering::SeqCst)
                                {
                                    callback();
                                }
                            }
                            Ok(_) => {}
                            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                                thread::sleep(Duration::from_millis(25))
                            }
                            Err(_) => break,
                        }
                    }
                })?,
        );
        Ok(())
    }

    pub fn notify(project: &Path, pid: u32) -> bool {
        let result = (|| -> Option<()> {
            let file = fs::File::open(project.join("instance-wake.json")).ok()?;
            let endpoint: Endpoint = serde_json::from_reader(file.take(4096)).ok()?;
            if endpoint.pid != pid || endpoint.token.len() != 32 {
                return None;
            }
            let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, endpoint.port).into();
            let mut stream =
                TcpStream::connect_timeout(&address, Duration::from_millis(200)).ok()?;
            stream
                .set_write_timeout(Some(Duration::from_millis(200)))
                .ok()?;
            stream.write_all(endpoint.token.as_bytes()).ok()?;
            Some(())
        })();
        result.is_some()
    }
}

fn read_token(stream: &mut TcpStream, bytes: &mut [u8; 32]) -> io::Result<()> {
    // Accepted sockets can inherit O_NONBLOCK on BSD/macOS.
    stream.set_nonblocking(false)?;
    let deadline = Instant::now() + Duration::from_millis(100);
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "wake token deadline"))?;
        stream.set_read_timeout(Some(remaining))?;
        let count = stream.read(&mut bytes[offset..])?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        offset += count;
    }
    Ok(())
}

impl Drop for LocalWake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.listener.take();
        // Do not remove a replacement owner's metadata during late teardown.
        if fs::read(&self.endpoint_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Endpoint>(&bytes).ok())
            .is_some_and(|endpoint| endpoint.token == self.token)
        {
            let _ = fs::remove_file(&self.endpoint_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    #[test]
    fn wake_is_authenticated_stoppable_and_removes_endpoint() {
        let project = std::env::temp_dir().join(format!("clipboard-wake-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&project).unwrap();
        let mut wake = LocalWake::bind(&project, 42).unwrap();
        let (tx, rx) = mpsc::channel();
        wake.start(move || {
            let _ = tx.send(());
        })
        .unwrap();
        assert!(!LocalWake::notify(&project, 43));
        let endpoint: Endpoint =
            serde_json::from_slice(&fs::read(&wake.endpoint_path).unwrap()).unwrap();
        let mut wrong = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        wrong.write_all(&[b'x'; 32]).unwrap();
        assert!(rx.recv_timeout(Duration::from_millis(150)).is_err());
        let mut fragmented = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port)).unwrap();
        fragmented
            .write_all(&endpoint.token.as_bytes()[..10])
            .unwrap();
        thread::sleep(Duration::from_millis(10));
        fragmented
            .write_all(&endpoint.token.as_bytes()[10..])
            .unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&wake.endpoint_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o077,
                0
            );
        }
        assert!(LocalWake::notify(&project, 42));
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(wake);
        assert!(!project.join("instance-wake.json").exists());
        assert!(!LocalWake::notify(&project, 42));
        fs::remove_dir(&project).unwrap();
    }
}
