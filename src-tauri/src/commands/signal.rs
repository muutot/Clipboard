use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

pub fn stop_signal_requested(receiver: &mpsc::Receiver<()>) -> bool {
    matches!(
        receiver.try_recv(),
        Ok(()) | Err(mpsc::TryRecvError::Disconnected)
    )
}

pub fn wait_for_stop(
    receiver: &mpsc::Receiver<()>,
    stop_flag: &AtomicBool,
    duration: Duration,
) -> bool {
    if stop_flag.load(Ordering::SeqCst) {
        return true;
    }
    match receiver.recv_timeout(duration) {
        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => true,
        Err(mpsc::RecvTimeoutError::Timeout) => stop_flag.load(Ordering::SeqCst),
    }
}
