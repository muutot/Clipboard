//! Bounded, process-local progress for the three user-visible long operations.
use clipboard_sync::cancellation::CancellationToken;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::Duration,
};

pub const CANCELLED: &str = "operation cancelled";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Tags,
    Backup,
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationStatus {
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Cancelled,
}
impl OperationStatus {
    fn active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationPhase {
    Preparing,
    Scanning,
    Applying,
    Creating,
    Validating,
    Restoring,
    Transferring,
    Discovering,
    Uploading,
    Downloading,
    Compacting,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationSnapshot {
    pub id: String,
    pub kind: OperationKind,
    pub status: OperationStatus,
    pub phase: OperationPhase,
    pub completed: u64,
    pub total: Option<u64>,
}
struct Slot {
    snapshot: OperationSnapshot,
    cancellation: CancellationToken,
}
#[derive(Default)]
struct State {
    closing: bool,
    slots: HashMap<OperationKind, Slot>,
}
#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    idle: Condvar,
}
#[derive(Clone, Default)]
pub struct BackgroundOperations(Arc<Inner>);

fn lock(inner: &Inner) -> MutexGuard<'_, State> {
    inner
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
impl BackgroundOperations {
    pub fn start(
        &self,
        kind: OperationKind,
        parent: &CancellationToken,
    ) -> Result<Operation, String> {
        let mut state = lock(&self.0);
        if state.closing {
            return Err("application is shutting down".into());
        }
        if state
            .slots
            .get(&kind)
            .is_some_and(|slot| slot.snapshot.status.active())
        {
            return Err("operation already in progress".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let cancellation = parent.child_token();
        state.slots.insert(
            kind,
            Slot {
                snapshot: OperationSnapshot {
                    id: id.clone(),
                    kind,
                    status: OperationStatus::Running,
                    phase: OperationPhase::Preparing,
                    completed: 0,
                    total: None,
                },
                cancellation: cancellation.clone(),
            },
        );
        Ok(Operation {
            registry: self.clone(),
            kind,
            id,
            cancellation,
            finished: false,
        })
    }
    pub fn snapshot(&self, kind: OperationKind) -> Option<OperationSnapshot> {
        lock(&self.0)
            .slots
            .get(&kind)
            .map(|slot| slot.snapshot.clone())
    }
    /// An old window must never cancel a newer run of the same operation.
    pub fn cancel(&self, kind: OperationKind, id: &str) -> bool {
        let mut state = lock(&self.0);
        if let Some(slot) = state.slots.get_mut(&kind) {
            if slot.snapshot.id == id && slot.snapshot.status.active() {
                slot.snapshot.status = OperationStatus::Cancelling;
                slot.cancellation.cancel();
                return true;
            }
        }
        false
    }
    pub fn shutdown(&self, timeout: Duration) -> bool {
        let mut state = lock(&self.0);
        state.closing = true;
        for slot in state
            .slots
            .values_mut()
            .filter(|s| s.snapshot.status.active())
        {
            slot.snapshot.status = OperationStatus::Cancelling;
            slot.cancellation.cancel();
        }
        let (state, _) = self
            .0
            .idle
            .wait_timeout_while(state, timeout, |state| {
                state
                    .slots
                    .values()
                    .any(|slot| slot.snapshot.status.active())
            })
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        !state
            .slots
            .values()
            .any(|slot| slot.snapshot.status.active())
    }
}

pub struct Operation {
    registry: BackgroundOperations,
    kind: OperationKind,
    id: String,
    cancellation: CancellationToken,
    finished: bool,
}
impl Operation {
    pub fn token(&self) -> &CancellationToken {
        &self.cancellation
    }
    pub fn check(&self) -> Result<(), String> {
        if self.cancellation.is_cancelled() {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }
    pub fn progress(
        &self,
        phase: OperationPhase,
        completed: u64,
        total: Option<u64>,
    ) -> Result<(), String> {
        self.check()?;
        if let Some(slot) = lock(&self.registry.0).slots.get_mut(&self.kind) {
            if slot.snapshot.id != self.id {
                return Err("operation is no longer current".into());
            }
            slot.snapshot.phase = phase;
            slot.snapshot.completed = completed;
            slot.snapshot.total = total;
        }
        Ok(())
    }
    pub fn finish<T, E>(mut self, result: Result<T, E>) -> Result<T, E> {
        // Completed commits can win the cancellation race and must still report success.
        self.set_status(if result.is_ok() {
            OperationStatus::Succeeded
        } else if self.cancellation.is_cancelled() {
            OperationStatus::Cancelled
        } else {
            OperationStatus::Failed
        });
        self.finished = true;
        result
    }
    fn set_status(&self, status: OperationStatus) {
        if let Some(slot) = lock(&self.registry.0).slots.get_mut(&self.kind) {
            if slot.snapshot.id == self.id {
                slot.snapshot.status = status;
            }
        }
        self.registry.0.idle.notify_all();
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        if !self.finished {
            self.set_status(if self.cancellation.is_cancelled() {
                OperationStatus::Cancelled
            } else {
                OperationStatus::Failed
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_scoped_and_stale_ids_cannot_cancel_the_next_run() {
        let registry = BackgroundOperations::default();
        let root = CancellationToken::default();
        let first = registry.start(OperationKind::Sync, &root).unwrap();
        let old = registry.snapshot(OperationKind::Sync).unwrap().id;
        assert!(registry.start(OperationKind::Sync, &root).is_err());
        assert!(registry.cancel(OperationKind::Sync, &old));
        assert_eq!(first.check().unwrap_err(), CANCELLED);
        assert!(!root.is_cancelled());
        first.finish(Err::<(), _>(CANCELLED)).unwrap_err();
        let next = registry.start(OperationKind::Sync, &root).unwrap();
        assert!(!registry.cancel(OperationKind::Sync, &old));
        assert!(next.check().is_ok());
        next.finish(Ok::<_, String>(())).unwrap();
        assert_eq!(
            registry.snapshot(OperationKind::Sync).unwrap().status,
            OperationStatus::Succeeded
        );
    }
    #[test]
    fn completion_wins_a_late_cancel_and_unwinding_releases_the_slot() {
        let registry = BackgroundOperations::default();
        let root = CancellationToken::default();
        let operation = registry.start(OperationKind::Tags, &root).unwrap();
        registry.cancel(OperationKind::Tags, &operation.id);
        operation.finish(Ok::<_, String>(())).unwrap();
        assert_eq!(
            registry.snapshot(OperationKind::Tags).unwrap().status,
            OperationStatus::Succeeded
        );
        drop(registry.start(OperationKind::Tags, &root).unwrap());
        assert_eq!(
            registry.snapshot(OperationKind::Tags).unwrap().status,
            OperationStatus::Failed
        );
    }
    #[test]
    fn shutdown_cancels_waits_and_rejects_late_start() {
        let registry = BackgroundOperations::default();
        let root = CancellationToken::default();
        let operation = registry.start(OperationKind::Backup, &root).unwrap();
        let worker = std::thread::spawn(move || {
            while operation.check().is_ok() {
                std::thread::yield_now();
            }
            drop(operation);
        });
        assert!(registry.shutdown(Duration::from_secs(2)));
        worker.join().unwrap();
        assert!(registry.start(OperationKind::Backup, &root).is_err());
    }
}
