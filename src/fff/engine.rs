use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fff_search::{FFFMode, FilePicker, FilePickerOptions, SharedFilePicker, SharedFrecency};
use pyo3::prelude::*;

use crate::fff::FffError;
use crate::fff::types::{FffConfig, FffLimits, NativeFffIndexStatus};
use crate::workspace::Workspace;

#[derive(Debug)]
pub(crate) struct FffEngineState {
    pub workspace: Arc<Workspace>,
    pub picker: SharedFilePicker,
    pub frecency: SharedFrecency,
    pub config: FffConfig,
    pub limits: FffLimits,
    started: AtomicBool,
    indexed_revision: AtomicU64,
    closed: AtomicBool,
    startup_lock: Mutex<()>,
}

#[pyclass(frozen, skip_from_py_object)]
#[derive(Clone, Debug)]
pub(crate) struct NativeFffEngine {
    pub(crate) inner: Arc<FffEngineState>,
}

impl NativeFffEngine {
    pub(crate) fn new(
        workspace: Arc<Workspace>,
        config: FffConfig,
        limits: FffLimits,
    ) -> Result<Self, FffError> {
        validate_config(&config, &limits)?;
        Ok(Self {
            inner: Arc::new(FffEngineState {
                workspace,
                picker: SharedFilePicker::default(),
                frecency: SharedFrecency::default(),
                config,
                limits,
                started: AtomicBool::new(false),
                closed: AtomicBool::new(false),
                indexed_revision: AtomicU64::new(u64::MAX),
                startup_lock: Mutex::new(()),
            }),
        })
    }
}

impl FffEngineState {
    pub(crate) fn start(&self) -> Result<NativeFffIndexStatus, FffError> {
        self.ensure_open()?;
        let _startup_guard = self
            .startup_lock
            .lock()
            .map_err(|_| FffError::Startup("FFF startup lock is poisoned".to_owned()))?;

        if !self.started.load(Ordering::Acquire) {
            let options = FilePickerOptions {
                base_path: self.workspace.root().to_string_lossy().into_owned(),
                enable_mmap_cache: self.config.enable_mmap_cache,
                enable_content_indexing: self.config.enable_content_indexing,
                mode: FFFMode::Ai,
                cache_budget: None,
                watch: self.config.watch,
                follow_symlinks: false,
                enable_fs_root_scanning: false,
                enable_home_dir_scanning: false,
            };
            FilePicker::new_with_shared_state(self.picker.clone(), self.frecency.clone(), options)
                .map_err(|error| FffError::Startup(error.to_string()))?;
            self.started.store(true, Ordering::Release);
        }

        self.status()
    }

    pub(crate) fn wait_ready(
        &self,
        timeout_seconds: f64,
    ) -> Result<NativeFffIndexStatus, FffError> {
        self.start()?;
        let timeout = Duration::from_secs_f64(timeout_seconds);

        if !self.picker.wait_for_indexing_complete(timeout) {
            return Err(FffError::IndexNotReady);
        }
        if self.config.watch && !self.picker.wait_for_watcher(timeout) {
            return Err(FffError::IndexNotReady);
        }

        self.status()
    }

    pub(crate) fn status(&self) -> Result<NativeFffIndexStatus, FffError> {
        if self.closed.load(Ordering::Acquire) {
            return Ok((
                "closed".to_owned(),
                0,
                false,
                false,
                self.config.enable_content_indexing,
            ));
        }
        if !self.started.load(Ordering::Acquire) {
            return Ok((
                "new".to_owned(),
                0,
                false,
                self.config.watch,
                self.config.enable_content_indexing,
            ));
        }

        let guard = self
            .picker
            .read()
            .map_err(|error| FffError::Runtime(error.to_string()))?;
        let picker = guard
            .as_ref()
            .ok_or_else(|| FffError::Runtime("FFF picker is unavailable".to_owned()))?;
        let progress = picker.get_scan_progress();
        let complete = !progress.is_scanning && progress.is_warmup_complete;
        let state = if complete { "ready" } else { "indexing" };

        Ok((
            state.to_owned(),
            progress.scanned_files_count,
            complete,
            self.config.watch,
            self.config.enable_content_indexing,
        ))
    }

    pub(crate) fn rescan(&self) -> Result<NativeFffIndexStatus, FffError> {
        self.ensure_started()?;
        self.picker
            .trigger_full_rescan_async(&self.frecency)
            .map_err(|error| FffError::Runtime(error.to_string()))?;
        self.indexed_revision.store(u64::MAX, Ordering::Release);
        self.status()
    }

    pub(crate) fn close(&self) -> Result<(), FffError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        self.picker.cancel();
        self.picker.shutdown_watches_and_wait();
        let mut guard = self
            .picker
            .write()
            .map_err(|error| FffError::Runtime(error.to_string()))?;
        if let Some(picker) = guard.as_mut() {
            picker.stop_background_monitor();
        }
        guard.take();

        Ok(())
    }

    pub(crate) fn ensure_started(&self) -> Result<(), FffError> {
        self.ensure_open()?;
        self.start()?;
        let revision = self.workspace.revision();
        if self.indexed_revision.load(Ordering::Acquire) != revision {
            self.picker
                .trigger_full_rescan_async(&self.frecency)
                .map_err(|error| FffError::Runtime(error.to_string()))?;
        }
        let status = self.wait_ready(self.config.initial_scan_timeout_seconds)?;
        if status.0 != "ready" {
            return Err(FffError::IndexNotReady);
        }

        self.indexed_revision.store(revision, Ordering::Release);
        Ok(())
    }

    pub(crate) fn ensure_open(&self) -> Result<(), FffError> {
        if self.closed.load(Ordering::Acquire) || !self.workspace.ensure_open() {
            return Err(FffError::Closed);
        }

        Ok(())
    }
}

impl Drop for FffEngineState {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn validate_config(config: &FffConfig, limits: &FffLimits) -> Result<(), FffError> {
    let positive = config.initial_scan_timeout_seconds.is_finite()
        && config.initial_scan_timeout_seconds > 0.0
        && config.search_timeout_seconds.is_finite()
        && config.search_timeout_seconds > 0.0
        && limits.max_search_timeout_seconds.is_finite()
        && limits.max_search_timeout_seconds > 0.0;
    if !positive {
        return Err(FffError::Configuration(
            "FFF timeouts must be finite and positive".to_owned(),
        ));
    }

    Ok(())
}
