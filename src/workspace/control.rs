use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub(crate) struct Cancellation {
    own: Arc<AtomicBool>,
    parents: Vec<Arc<AtomicBool>>,
    signals: Arc<Mutex<Vec<Weak<AtomicBool>>>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorkStopped {
    Cancelled,
    Deadline,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkControl {
    deadline: Option<Instant>,
    cancellation: Cancellation,
}

impl Cancellation {
    pub(crate) fn new() -> Self {
        Self {
            own: Arc::new(AtomicBool::new(false)),
            parents: Vec::new(),
            signals: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn cancel(&self) {
        self.own.store(true, Ordering::Relaxed);
        let mut signals = match self.signals.lock() {
            Ok(signals) => signals,
            Err(error) => error.into_inner(),
        };
        signals.retain(|signal| match signal.upgrade() {
            Some(signal) => {
                signal.store(true, Ordering::Release);
                true
            }
            None => false,
        });
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.own.load(Ordering::Relaxed)
            || self
                .parents
                .iter()
                .any(|signal| signal.load(Ordering::Relaxed))
    }

    pub(crate) fn register_signal(&self, signal: &Arc<AtomicBool>) {
        let mut signals = match self.signals.lock() {
            Ok(signals) => signals,
            Err(error) => error.into_inner(),
        };
        if self.is_cancelled() {
            signal.store(true, Ordering::Release);
            return;
        }

        signals.retain(|registered| registered.strong_count() > 0);

        signals.push(Arc::downgrade(signal));
    }

    pub(crate) fn with_parent(&self, parent: &Self) -> Self {
        let mut parents = self.parents.clone();
        parents.push(parent.own.clone());
        parents.extend(parent.parents.iter().cloned());

        Self {
            own: self.own.clone(),
            parents,
            signals: self.signals.clone(),
        }
    }
}

impl WorkControl {
    pub(crate) fn new(cancellation: Cancellation, timeout: Option<Duration>) -> Self {
        Self {
            deadline: timeout.and_then(|duration| Instant::now().checked_add(duration)),
            cancellation,
        }
    }

    pub(crate) fn checkpoint(&self) -> Result<(), WorkStopped> {
        self.check_cancellation()?;
        self.check_deadline()
    }

    pub(crate) fn checkpoint_periodic(
        &self,
        iteration: usize,
        interval: usize,
    ) -> Result<(), WorkStopped> {
        self.check_cancellation()?;

        if iteration.is_multiple_of(interval) {
            self.check_deadline()?;
        }

        Ok(())
    }

    fn check_cancellation(&self) -> Result<(), WorkStopped> {
        if self.cancellation.is_cancelled() {
            return Err(WorkStopped::Cancelled);
        }

        Ok(())
    }

    fn check_deadline(&self) -> Result<(), WorkStopped> {
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(WorkStopped::Deadline);
        }

        Ok(())
    }
}
