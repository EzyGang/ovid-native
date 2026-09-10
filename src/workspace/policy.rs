use crate::workspace::{MutationContext, Workspace, WorkspaceError};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WorkspacePolicy {
    pub allow_fuzzy_replace: bool,
    pub fuzzy_replace_threshold: f64,
    pub max_read_bytes: u64,
    pub max_observation_file_bytes: u64,
    pub max_observation_entries: usize,
    pub max_observation_store_bytes: usize,
    pub create_parent_directories: bool,
}

impl Default for WorkspacePolicy {
    fn default() -> Self {
        Self {
            allow_fuzzy_replace: false,
            fuzzy_replace_threshold: 0.9,
            max_read_bytes: 4 * 1024 * 1024,
            max_observation_file_bytes: 64 * 1024 * 1024,
            max_observation_entries: 4096,
            max_observation_store_bytes: 8 * 1024 * 1024,
            create_parent_directories: false,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PolicyGeneration {
    pub policy: WorkspacePolicy,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct EditModeSelection {
    pub mode: String,
    pub generation: u64,
}

impl Workspace {
    pub(crate) fn policy(&self) -> Result<PolicyGeneration, WorkspaceError> {
        self.ensure_open()?;
        Ok(self.lock(&self.state.policy)?.clone())
    }

    pub(crate) fn set_policy(
        &self,
        policy: WorkspacePolicy,
    ) -> Result<PolicyGeneration, WorkspaceError> {
        let _coordinator = self.write_guard()?;

        self.ensure_open()?;
        let mut current = self.lock(&self.state.policy)?;
        if current.policy != policy {
            current.policy = policy;
            current.generation = current.generation.saturating_add(1);
        }
        Ok(current.clone())
    }

    pub(crate) fn edit_mode(&self) -> Result<EditModeSelection, WorkspaceError> {
        self.ensure_open()?;
        Ok(self.lock(&self.state.edit_mode)?.clone())
    }

    pub(crate) fn set_edit_mode(&self, mode: &str) -> Result<EditModeSelection, WorkspaceError> {
        let _coordinator = self.write_guard()?;

        self.ensure_open()?;
        if !self.lock(&self.state.registered_edit_modes)?.contains(mode) {
            return Err(WorkspaceError::EditMode(format!(
                "workspace edit mode is not registered: {mode}"
            )));
        }
        let mut current = self.lock(&self.state.edit_mode)?;
        if current.mode != mode {
            current.mode = mode.to_owned();
            current.generation = current.generation.saturating_add(1);
        }
        Ok(current.clone())
    }

    pub(crate) fn register_edit_mode(&self, mode: &str) -> Result<(), WorkspaceError> {
        self.ensure_open()?;
        if !mode.contains('.') || mode.starts_with('.') || mode.ends_with('.') {
            return Err(WorkspaceError::EditMode(format!(
                "custom workspace edit mode must be namespaced: {mode}"
            )));
        }
        self.lock(&self.state.registered_edit_modes)?
            .insert(mode.to_owned());
        Ok(())
    }

    pub(crate) fn supports_edit_mode(&self, mode: &str) -> Result<bool, WorkspaceError> {
        self.ensure_open()?;
        Ok(self.lock(&self.state.registered_edit_modes)?.contains(mode))
    }
    pub(crate) fn validate_mutation(
        &self,
        context: &MutationContext,
        expected_mode: &str,
    ) -> Result<(), WorkspaceError> {
        if context.mode != expected_mode {
            return Err(WorkspaceError::EditMode(format!(
                "captured edit mode does not match mutation: {} != {expected_mode}",
                context.mode
            )));
        }

        context.ensure_active()
    }
}
