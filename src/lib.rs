use parking_lot::Mutex;
use std::{
    future::Future,
    ops::DerefMut,
    pin::Pin,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    task::{Poll, Waker},
};

/// A deadman switch that can be used to detect when it is dropped.
///
/// # Example
///
/// ```rust
/// use async_deadman::Deadman;
///
/// #[tokio::main] // does not have to be tokio
/// async fn main() {
///     let (deadman, receiver) = Deadman::new();
///
///     tokio::spawn(async move {
///         receiver.await;
///         println!("Deadman was dropped");
///     });
///
///     drop(deadman);
/// }
/// ```
#[must_use]
pub struct Deadman {
    state: Arc<DeadmanState>,
}

impl Deadman {
    pub fn new() -> (Self, DeadmanReceiver) {
        let state = Arc::new(DeadmanState {
            wakers: Mutex::new(Vec::new()),
            is_dead: AtomicBool::new(false),
        });

        (
            Self {
                state: state.clone(),
            },
            DeadmanReceiver { state },
        )
    }

    pub fn release(self) {
        drop(self);
    }
}

impl Drop for Deadman {
    fn drop(&mut self) {
        self.state.is_dead.store(true, atomic::Ordering::Relaxed);
        let wakers = std::mem::take(self.state.wakers.lock().deref_mut());

        for waker in wakers {
            waker.wake();
        }
    }
}

struct DeadmanState {
    wakers: Mutex<Vec<Waker>>,
    is_dead: AtomicBool,
}

#[derive(Clone)]
pub struct DeadmanReceiver {
    state: Arc<DeadmanState>,
}

impl Future for DeadmanReceiver {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let inner = Pin::into_inner(self);

        if inner.state.is_dead.load(atomic::Ordering::Relaxed) {
            Poll::Ready(())
        } else {
            inner.state.wakers.lock().push(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::{select, sync::oneshot};

    use super::Deadman;

    #[tokio::test]
    async fn test_deadman() {
        let (deadman, receiver) = Deadman::new();
        let (other_sender, other_receiver) = oneshot::channel();

        tokio::spawn(async move {
            select! {
                _ = receiver => {},
                _ = other_receiver => {
                    assert!(false, "Deadman was not released");
                },
            }
        });

        drop(deadman);

        let _ = other_sender.send(());
    }

    #[tokio::test]
    async fn test_deadman_not_released() {
        let (deadman, receiver) = Deadman::new();
        let (other_sender, other_receiver) = oneshot::channel();

        tokio::spawn(async move {
            select! {
                _ = other_receiver => {},
                _ = receiver => {
                    assert!(false, "Deadman was released");
                },
            }
        });

        std::mem::forget(deadman);

        let _ = other_sender.send(());
    }
}
