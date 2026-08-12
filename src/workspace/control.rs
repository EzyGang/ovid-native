use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub(crate) struct Cancellation {
    cancelled: Arc<AtomicBool>,
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
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
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
