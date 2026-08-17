use std::collections::{HashMap, HashSet};

use crate::workspace::WorkspaceError;
use crate::workspace::content::NormalizedText;
use crate::workspace::observation_types::{
    Authorization, LineRange, ObservationReceipt, RenderedLine,
};
use crate::workspace::write::sha256;

#[derive(Debug)]
struct ObservationEntry {
    receipt: ObservationReceipt,
    line_digests: HashMap<usize, String>,
    retained_bytes: usize,
    last_used: u64,
}

#[derive(Clone, Debug)]
enum TagState {
    Digest(String),
    Collision,
}

#[derive(Debug, Default)]
pub(crate) struct ObservationLedger {
    entries: HashMap<(String, String), ObservationEntry>,
    tags: HashMap<(String, String), TagState>,
    collision_tombstones: HashSet<(String, String)>,
    retained_bytes: usize,
    clock: u64,
}

impl ObservationLedger {
    pub(crate) fn record(
        &mut self,
        path: &str,
        text: &NormalizedText,
        generation: u64,
        rendered: &[RenderedLine],
        complete_presentation: bool,
        retention: (usize, usize),
    ) -> Result<ObservationReceipt, WorkspaceError> {
        let (max_entries, max_bytes) = retention;
        let digest = sha256(text.source.as_bytes());
        let tag = digest[..4].to_ascii_uppercase();
        let tag_key = (path.to_owned(), tag.clone());
        self.register_tag(&tag_key, &digest);

        self.clock = self.clock.saturating_add(1);
        let key = (path.to_owned(), digest.clone());
        if let Some(entry) = self.entries.get_mut(&key) {
            self.retained_bytes = self.retained_bytes.saturating_sub(entry.retained_bytes);
            merge_rendered(entry, text, rendered);
            entry.receipt.visible_ranges = ranges_from_lines(entry.line_digests.keys().copied());
            entry.receipt.complete_presentation |= complete_presentation;
            entry.receipt.generation = generation;
            entry.last_used = self.clock;
            entry.retained_bytes = retained_size(entry);
            self.retained_bytes = self.retained_bytes.saturating_add(entry.retained_bytes);
        } else {
            let mut line_digests = HashMap::new();
            for line in rendered {
                if let Some(source) = text.line(line.number) {
                    line_digests.insert(line.number, sha256(source.as_bytes()));
                }
            }
            let receipt = ObservationReceipt {
                path: path.to_owned(),
                tag,
                content_sha256: digest,
                generation,
                visible_ranges: ranges_from_lines(line_digests.keys().copied()),
                complete_presentation,
            };
            let mut entry = ObservationEntry {
                receipt,
                line_digests,
                retained_bytes: 0,
                last_used: self.clock,
            };
            entry.retained_bytes = retained_size(&entry);
            self.retained_bytes = self.retained_bytes.saturating_add(entry.retained_bytes);
            self.entries.insert(key.clone(), entry);
        }

        self.evict(max_entries, max_bytes, &key);
        self.entries
            .get(&key)
            .map(|entry| entry.receipt.clone())
            .ok_or_else(|| {
                WorkspaceError::ObservationNotFound(
                    "workspace observation exceeded retention limits".to_owned(),
                )
            })
    }

    pub(crate) fn resolve(
        &mut self,
        path: &str,
        tag: &str,
        current_digest: &str,
    ) -> Result<Authorization, WorkspaceError> {
        let normalized_tag = tag.to_ascii_uppercase();
        let tag_key = (path.to_owned(), normalized_tag.clone());
        match self.tags.get(&tag_key) {
            Some(TagState::Collision) => {
                return Err(WorkspaceError::ObservationCollision(format!(
                    "workspace observation tag is ambiguous: {path}#{normalized_tag}"
                )));
            }
            Some(TagState::Digest(digest)) if digest == current_digest => (),
            Some(TagState::Digest(_)) => {
                return Err(WorkspaceError::Stale(format!(
                    "workspace observation is stale: {path}#{normalized_tag}"
                )));
            }
            None => {
                return Err(WorkspaceError::ObservationNotFound(format!(
                    "workspace observation was not retained: {path}#{normalized_tag}"
                )));
            }
        }

        self.authorization(path, current_digest)
    }
    pub(crate) fn resolve_tag(
        &mut self,
        path: &str,
        tag: &str,
    ) -> Result<Authorization, WorkspaceError> {
        let normalized_tag = tag.to_ascii_uppercase();
        let tag_key = (path.to_owned(), normalized_tag.clone());
        let digest = match self.tags.get(&tag_key) {
            Some(TagState::Collision) => {
                return Err(WorkspaceError::ObservationCollision(format!(
                    "workspace observation tag is ambiguous: {path}#{normalized_tag}"
                )));
            }
            Some(TagState::Digest(digest)) => digest.clone(),
            None => {
                return Err(WorkspaceError::ObservationNotFound(format!(
                    "workspace observation was not retained: {path}#{normalized_tag}"
                )));
            }
        };
        let key = (path.to_owned(), digest);
        let entry = self.entries.get_mut(&key).ok_or_else(|| {
            WorkspaceError::ObservationNotFound(format!(
                "workspace observation was evicted: {path}#{normalized_tag}"
            ))
        })?;

        self.clock = self.clock.saturating_add(1);
        entry.last_used = self.clock;
        Ok(Authorization::new(
            entry.receipt.clone(),
            entry.line_digests.clone(),
        ))
    }

    pub(crate) fn current(
        &mut self,
        path: &str,
        current_digest: &str,
    ) -> Result<Authorization, WorkspaceError> {
        self.authorization(path, current_digest)
    }

    fn authorization(&mut self, path: &str, digest: &str) -> Result<Authorization, WorkspaceError> {
        let tag = digest[..4].to_ascii_uppercase();
        let tag_key = (path.to_owned(), tag.clone());
        if self.collision_tombstones.contains(&tag_key) {
            return Err(WorkspaceError::ObservationCollision(format!(
                "workspace observation tag is ambiguous: {path}#{tag}"
            )));
        }

        let key = (path.to_owned(), digest.to_owned());
        let path_was_observed = self
            .entries
            .keys()
            .any(|(entry_path, _)| entry_path == path);
        let entry = match self.entries.get_mut(&key) {
            Some(entry) => entry,
            None if path_was_observed => {
                return Err(WorkspaceError::ObservedLineChanged(format!(
                    "workspace file changed after it was observed: {path}"
                )));
            }
            None => {
                return Err(WorkspaceError::ObservationNotFound(format!(
                    "workspace file must be read before mutation: {path}"
                )));
            }
        };

        self.clock = self.clock.saturating_add(1);
        entry.last_used = self.clock;
        Ok(Authorization::new(
            entry.receipt.clone(),
            entry.line_digests.clone(),
        ))
    }

    fn register_tag(&mut self, key: &(String, String), digest: &str) {
        match self.tags.get(key) {
            Some(TagState::Digest(existing)) if existing != digest => {
                self.tags.insert(key.clone(), TagState::Collision);
                self.collision_tombstones.insert(key.clone());
            }
            Some(_) => (),
            None => {
                self.tags
                    .insert(key.clone(), TagState::Digest(digest.to_owned()));
            }
        }
    }

    fn evict(&mut self, max_entries: usize, max_bytes: usize, protected: &(String, String)) {
        while self.entries.len() > max_entries || self.retained_bytes > max_bytes {
            let candidate = self
                .entries
                .iter()
                .filter(|(key, _)| *key != protected)
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone());
            let Some(key) = candidate else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.retained_bytes = self.retained_bytes.saturating_sub(entry.retained_bytes);
            }
        }
    }
}
fn merge_rendered(entry: &mut ObservationEntry, text: &NormalizedText, rendered: &[RenderedLine]) {
    for line in rendered {
        if let Some(source) = text.line(line.number) {
            entry
                .line_digests
                .insert(line.number, sha256(source.as_bytes()));
        }
    }
}

fn retained_size(entry: &ObservationEntry) -> usize {
    entry.receipt.path.len()
        + entry.receipt.tag.len()
        + entry.receipt.content_sha256.len()
        + entry.line_digests.len() * (std::mem::size_of::<usize>() + 64)
}

fn ranges_from_lines(lines: impl Iterator<Item = usize>) -> Vec<LineRange> {
    let mut lines = lines.collect::<Vec<_>>();
    lines.sort_unstable();
    let mut ranges: Vec<LineRange> = Vec::new();
    for line in lines {
        match ranges.last_mut() {
            Some(range) if line <= range.end.saturating_add(1) => range.end = line,
            _ => ranges.push(LineRange {
                start: line,
                end: line,
            }),
        }
    }
    ranges
}
