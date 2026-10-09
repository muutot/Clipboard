use std::sync::{Arc, Mutex, Weak};

use clipboard_sync::cancellation::CancellationToken;

/// Root lifetime cancellation plus weak registrations of currently active runs.
/// Privacy toggles cancel runs, never the root or the auto worker, so switching
/// local-only off permits new work without reviving any cancelled transfer.
#[derive(Default)]
pub struct SyncCancellation(pub CancellationToken, Mutex<Vec<Weak<CancellationToken>>>);

impl SyncCancellation {
    /// Call while holding the config mutex, just like `cancel_active` on save.
    /// This makes policy snapshot + registration atomic against a privacy toggle.
    pub fn register(
        &self,
        local_only: bool,
        parent: &CancellationToken,
    ) -> Result<Arc<CancellationToken>, String> {
        if local_only {
            return Err("network access is disabled by local-only privacy mode".into());
        }
        let token = Arc::new(parent.child_token());
        token.check()?;
        let mut active = self
            .1
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.retain(|token| token.strong_count() > 0);
        active.push(Arc::downgrade(&token));
        Ok(token)
    }

    /// Must be called after a successful policy save, before dropping config.
    pub fn cancel_active(&self) {
        let mut active = self
            .1
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for token in active.drain(..).filter_map(|token| token.upgrade()) {
            token.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_cancels_every_active_entry_without_disabling_future_runs() {
        let policy = SyncCancellation::default();
        let worker = policy.0.child_token();
        let manual = policy.register(false, &policy.0).unwrap();
        let automatic = policy.register(false, &worker).unwrap();
        let materialize = policy.register(false, &policy.0).unwrap();
        let test = policy.register(false, &policy.0).unwrap();
        assert!(policy.register(true, &policy.0).is_err());
        policy.cancel_active();
        for token in [&manual, &automatic, &materialize, &test] {
            assert!(token.is_cancelled());
        }
        assert!(!worker.is_cancelled());
        let resumed = policy.register(false, &worker).unwrap();
        assert!(!resumed.is_cancelled());
        policy.0.cancel();
        assert!(resumed.is_cancelled());
        assert!(policy.register(false, &worker).is_err());
    }

    #[test]
    fn privacy_toggle_interrupts_an_active_s3_response_wait() {
        use crate::sync::v1::{ObjectStore, S3ObjectStore};
        use std::{io::Read, net::TcpListener, sync::mpsc, time::Duration};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (close_tx, close_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            ready_tx.send(()).unwrap();
            // Withhold headers to model a stalled S3 endpoint.
            let _ = close_rx.recv_timeout(Duration::from_secs(5));
        });
        let policy = SyncCancellation::default();
        let token = policy.register(false, &policy.0).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let run = std::thread::spawn(move || {
            let store = S3ObjectStore::new(
                format!("http://{address}"),
                "us-east-1",
                "test",
                "synthetic",
                "synthetic",
                "",
            )
            .unwrap();
            done_tx
                .send(token.run(|| store.head("v1/checkpoint.json")))
                .unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        policy.cancel_active();
        let result = done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("privacy change must interrupt HTTP wait");
        assert!(result.is_err());
        close_tx.send(()).unwrap();
        run.join().unwrap();
        server.join().unwrap();
    }
}
