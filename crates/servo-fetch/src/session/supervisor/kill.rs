//! Revocable kill authority: separates kill initiation from wait/reap ownership.

use std::sync::{Arc, Mutex};

pub(super) trait TerminationTarget {
    fn terminate_now(&self) -> std::io::Result<()>;
}

#[derive(Debug)]
pub(super) struct KillAuthority<T> {
    target: Mutex<Option<Arc<T>>>,
}

impl<T: TerminationTarget> KillAuthority<T> {
    pub(super) fn new() -> Self {
        Self {
            target: Mutex::new(None),
        }
    }

    pub(super) fn grant(&self, target: Arc<T>) {
        let previous = self
            .target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(target);
        debug_assert!(previous.is_none(), "kill authority must only be granted once");
    }

    pub(super) fn terminate_now(&self) {
        let result = {
            let target = self.target.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            target.as_ref().map(|target| target.terminate_now())
        };
        if let Some(Err(error)) = result {
            tracing::warn!(%error, "failed to terminate worker process tree from cancellation port");
        }
    }

    pub(super) fn revoke(&self) -> Option<Arc<T>> {
        self.target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crossbeam_channel::{Receiver, Sender};

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum AuthorityEvent {
        TerminationFinished,
        Revoked,
    }

    #[derive(Debug)]
    struct BlockingTarget {
        signal_started: Sender<()>,
        release_signal: Receiver<()>,
        events: Sender<AuthorityEvent>,
    }

    impl TerminationTarget for BlockingTarget {
        fn terminate_now(&self) -> std::io::Result<()> {
            self.signal_started.send(()).unwrap();
            self.release_signal.recv().unwrap();
            self.events.send(AuthorityEvent::TerminationFinished).unwrap();
            Ok(())
        }
    }

    #[derive(Debug)]
    struct CountingTarget(AtomicUsize);

    impl TerminationTarget for CountingTarget {
        fn terminate_now(&self) -> std::io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn signal_finishes_before_revocation() {
        let (signal_started, signal_started_rx) = crossbeam_channel::bounded(1);
        let (release_signal, release_signal_rx) = crossbeam_channel::bounded(1);
        let (events, event_rx) = crossbeam_channel::unbounded();
        let authority = Arc::new(KillAuthority::new());
        authority.grant(Arc::new(BlockingTarget {
            signal_started,
            release_signal: release_signal_rx,
            events: events.clone(),
        }));

        let signal_authority = Arc::clone(&authority);
        let signal = std::thread::spawn(move || signal_authority.terminate_now());
        signal_started_rx.recv().unwrap();

        let revoke_authority = Arc::clone(&authority);
        let revoke = std::thread::spawn(move || {
            revoke_authority.revoke();
            events.send(AuthorityEvent::Revoked).unwrap();
        });
        release_signal.send(()).unwrap();

        assert_eq!(event_rx.recv().unwrap(), AuthorityEvent::TerminationFinished);
        assert_eq!(event_rx.recv().unwrap(), AuthorityEvent::Revoked);
        signal.join().unwrap();
        revoke.join().unwrap();
    }

    #[test]
    fn force_after_revoke_returns_while_reap_is_blocked() {
        let authority = Arc::new(KillAuthority::new());
        let target = Arc::new(CountingTarget(AtomicUsize::new(0)));
        authority.grant(Arc::clone(&target));
        authority.revoke();

        let (reap_started, reap_started_rx) = crossbeam_channel::bounded(1);
        let (release_reap, release_reap_rx) = crossbeam_channel::bounded(1);
        let reaped_target = Arc::clone(&target);
        let reap = std::thread::spawn(move || {
            reap_started.send(()).unwrap();
            release_reap_rx.recv().unwrap();
            drop(reaped_target);
        });
        reap_started_rx.recv().unwrap();

        authority.terminate_now();
        assert_eq!(target.0.load(Ordering::SeqCst), 0);

        release_reap.send(()).unwrap();
        reap.join().unwrap();
    }
}
