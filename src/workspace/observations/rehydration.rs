use crate::workspace::line_hash::short_line_hash;
use crate::workspace::path::normalize_relative;
use crate::workspace::workflows::load_current;
use crate::workspace::{RenderedLine, Workspace, WorkspaceError, sha256};

use super::{ObservationLedger, retained_size};

/// Outcome of one persisted receipt rehydration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RehydrationStatus {
    /// The ledger now authorizes the given lines of the unchanged file.
    Restored,
    /// The file changed, is unreadable, or no longer has a given line. Nothing was recorded.
    Stale,
    /// Recording would evict a retained observation. Nothing was recorded.
    Skipped,
}

impl Workspace {
    pub(crate) fn rehydrate_observation(
        &self,
        path: &str,
        content_sha256: &str,
        line_numbers: &[usize],
        complete_presentation: bool,
    ) -> Result<RehydrationStatus, WorkspaceError> {
        self.ensure_open()?;
        let policy = self.policy()?.policy;
        let Ok(path) = normalize_relative(path) else {
            return Ok(RehydrationStatus::Stale);
        };
        let Ok((_, text)) = load_current(self, &path, &policy) else {
            return Ok(RehydrationStatus::Stale);
        };
        if sha256(text.source.as_bytes()) != content_sha256 {
            return Ok(RehydrationStatus::Stale);
        }

        let mut numbers = line_numbers.to_vec();
        numbers.sort_unstable();
        numbers.dedup();
        let lines = numbers
            .into_iter()
            .map(|number| {
                text.line(number).map(|source| RenderedLine {
                    number,
                    short_hash: format!("{:02X}", short_line_hash(source.as_bytes())),
                    text: source.to_owned(),
                })
            })
            .collect::<Option<Vec<_>>>();
        let Some(lines) = lines else {
            return Ok(RehydrationStatus::Stale);
        };

        let retention = (
            policy.max_observation_entries,
            policy.max_observation_store_bytes,
        );
        let generation = self.file_generation(&path)?;
        let mut ledger = self.observations()?;
        if !ledger.has_room(&path, content_sha256, &lines, retention) {
            return Ok(RehydrationStatus::Skipped);
        }
        let complete = complete_presentation && lines.len() == text.total_lines();
        ledger.record(&path, &text, generation, &lines, complete, retention)?;
        Ok(RehydrationStatus::Restored)
    }
}

impl ObservationLedger {
    fn has_room(
        &self,
        path: &str,
        digest: &str,
        rendered: &[RenderedLine],
        retention: (usize, usize),
    ) -> bool {
        let (max_entries, max_bytes) = retention;
        let key = (path.to_owned(), digest.to_owned());
        let (entries, replaced_bytes, kept_lines) = match self.entries.get(&key) {
            Some(entry) => (
                self.entries.len(),
                entry.retained_bytes,
                entry
                    .line_digests
                    .keys()
                    .filter(|number| {
                        rendered
                            .binary_search_by_key(*number, |line| line.number)
                            .is_err()
                    })
                    .count(),
            ),
            None => (self.entries.len().saturating_add(1), 0, 0),
        };
        let bytes = self
            .retained_bytes
            .saturating_sub(replaced_bytes)
            .saturating_add(retained_size(path, kept_lines + rendered.len()));
        entries <= max_entries && bytes <= max_bytes
    }
}
