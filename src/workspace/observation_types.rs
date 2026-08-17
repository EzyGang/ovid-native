use std::collections::{HashMap, HashSet};

use crate::workspace::WorkspaceError;
use crate::workspace::content::NormalizedText;
use crate::workspace::write::sha256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LineRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RenderedLine {
    pub number: usize,
    pub short_hash: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ObservationReceipt {
    pub path: String,
    pub tag: String,
    pub content_sha256: String,
    pub generation: u64,
    pub visible_ranges: Vec<LineRange>,
    pub complete_presentation: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Authorization {
    pub receipt: ObservationReceipt,
    line_digests: HashMap<usize, String>,
}

impl Authorization {
    pub(super) fn new(receipt: ObservationReceipt, line_digests: HashMap<usize, String>) -> Self {
        Self {
            receipt,
            line_digests,
        }
    }

    pub(crate) fn require_lines(
        &self,
        path: &str,
        text: &NormalizedText,
        lines: &HashSet<usize>,
    ) -> Result<(), WorkspaceError> {
        for line in lines {
            let retained = self.line_digests.get(line).ok_or_else(|| {
                WorkspaceError::UnseenLine(format!(
                    "workspace line was not observed: {path}:{line}"
                ))
            })?;
            let current = text.line(*line).ok_or_else(|| {
                WorkspaceError::ObservedLineChanged(format!(
                    "observed workspace line no longer exists: {path}:{line}"
                ))
            })?;
            if *retained != sha256(current.as_bytes()) {
                return Err(WorkspaceError::ObservedLineChanged(format!(
                    "observed workspace line changed: {path}:{line}"
                )));
            }
        }

        Ok(())
    }

    pub(crate) fn require_complete(&self, path: &str) -> Result<(), WorkspaceError> {
        if !self.receipt.complete_presentation {
            return Err(WorkspaceError::UnseenLine(format!(
                "complete workspace file observation is required: {path}"
            )));
        }

        Ok(())
    }
}
