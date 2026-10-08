//! Cooperative cancellation shared by synchronous engine work and async HTTP I/O.
use std::{cell::RefCell, marker::PhantomData, rc::Rc};

pub const CANCELLED: &str = "sync cancelled";
#[derive(Clone, Default, Debug)]
pub struct CancellationToken(pub(crate) tokio_util::sync::CancellationToken);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.cancel();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    pub fn child_token(&self) -> Self {
        Self(self.0.child_token())
    }
    pub fn check(&self) -> Result<(), String> {
        if self.is_cancelled() {
            Err(CANCELLED.into())
        } else {
            Ok(())
        }
    }
    /// The engine is synchronous and stays on one blocking thread. Nested scopes
    /// restore their predecessor even on panic; tokens never bleed into another run.
    pub fn run<T>(&self, run: impl FnOnce() -> T) -> T {
        let previous = CURRENT.with(|current| current.replace(self.clone()));
        let _scope = Scope {
            previous,
            _not_send: PhantomData,
        };
        run()
    }
}
thread_local! { static CURRENT: RefCell<CancellationToken> = RefCell::new(CancellationToken::default()); }
struct Scope {
    previous: CancellationToken,
    _not_send: PhantomData<Rc<()>>,
}
impl Drop for Scope {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            current.replace(self.previous.clone());
        });
    }
}
pub(crate) fn current() -> CancellationToken {
    CURRENT.with(|current| current.borrow().clone())
}
pub(crate) fn check() -> Result<(), String> {
    current().check()
}
pub(crate) fn sleep(duration: std::time::Duration) -> Result<(), String> {
    let token = current();
    crate::transport::runtime().block_on(async {
        tokio::select! { biased;
            _ = token.0.cancelled() => Err(CANCELLED.into()),
            _ = tokio::time::sleep(duration) => Ok(()),
        }
    })
}

/// Reports cancellation as an iterator error, including after the terminal read,
/// so repository batch transactions roll back instead of committing a prefix.
pub fn checked_batches<T>(
    mut batches: impl Iterator<Item = Result<T, String>>,
) -> impl Iterator<Item = Result<T, String>> {
    let token = current();
    let mut finished = false;
    std::iter::from_fn(move || {
        if finished {
            return None;
        }
        if let Err(error) = token.check() {
            finished = true;
            return Some(Err(error));
        }
        let next = batches.next();
        if let Err(error) = token.check() {
            finished = true;
            return Some(Err(error));
        }
        if next.is_none() {
            finished = true;
        }
        next
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_restore_on_panic_and_cancelled_batches_never_look_complete() {
        let token = CancellationToken::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            token.run(|| {
                token.cancel();
                panic!("test unwind");
            })
        }));
        assert!(result.is_err());
        assert!(check().is_ok());
        token.run(|| {
            let mut batches = checked_batches(std::iter::once(Ok(1)));
            assert_eq!(batches.next().unwrap().unwrap_err(), CANCELLED);
            assert!(batches.next().is_none());
        });
    }
}
