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
        Ok(Authorization {
            receipt: entry.receipt.clone(),
            line_digests: entry.line_digests.clone(),
        })
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

pub(crate) fn short_line_hash(input: &[u8]) -> u8 {
    (xxh32(input) & 0xff) as u8
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

fn xxh32(input: &[u8]) -> u32 {
    const PRIME1: u32 = 2_654_435_761;
    const PRIME2: u32 = 2_246_822_519;
    const PRIME3: u32 = 3_266_489_917;
    const PRIME4: u32 = 668_265_263;
    const PRIME5: u32 = 374_761_393;

    let mut index = 0;
    let mut hash = if input.len() >= 16 {
        let mut v1 = PRIME1.wrapping_add(PRIME2);
        let mut v2 = PRIME2;
        let mut v3 = 0;
        let mut v4 = 0_u32.wrapping_sub(PRIME1);
        while index <= input.len() - 16 {
            v1 = xxh_round(v1, read_u32(&input[index..]));
            v2 = xxh_round(v2, read_u32(&input[index + 4..]));
            v3 = xxh_round(v3, read_u32(&input[index + 8..]));
            v4 = xxh_round(v4, read_u32(&input[index + 12..]));
            index += 16;
        }
        v1.rotate_left(1)
            .wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12))
            .wrapping_add(v4.rotate_left(18))
    } else {
        PRIME5
    };
    hash = hash.wrapping_add(input.len() as u32);
    while index + 4 <= input.len() {
        hash = hash
            .wrapping_add(read_u32(&input[index..]).wrapping_mul(PRIME3))
            .rotate_left(17)
            .wrapping_mul(PRIME4);
        index += 4;
    }
    while index < input.len() {
        hash = hash
            .wrapping_add(u32::from(input[index]).wrapping_mul(PRIME5))
            .rotate_left(11)
            .wrapping_mul(PRIME1);
        index += 1;
    }
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(PRIME2);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(PRIME3);
    hash ^ (hash >> 16)
}

fn xxh_round(value: u32, input: u32) -> u32 {
    value
        .wrapping_add(input.wrapping_mul(2_246_822_519))
        .rotate_left(13)
        .wrapping_mul(2_654_435_761)
}

fn read_u32(input: &[u8]) -> u32 {
    u32::from_le_bytes([input[0], input[1], input[2], input[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xxh32_matches_reference_vectors() {
        assert_eq!(xxh32(b""), 0x02cc_5d05);
        assert_eq!(xxh32(b"a"), 0x550d_7456);
        assert_eq!(xxh32(b"hello"), 0xfb00_77f9);
    }
}
