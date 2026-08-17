use std::collections::HashMap;

use crate::workspace::line_hash::short_line_hash;
use crate::workspace::workflows::load_current;
use crate::workspace::{ObservationReceipt, RenderedLine, Workspace, WorkspaceError};

impl Workspace {
    pub(crate) fn observe_source_lines(
        &self,
        path: &str,
        claims: &[(usize, String)],
        spans: &[(usize, usize, usize, usize)],
        complete_presentation: bool,
    ) -> Result<(ObservationReceipt, Vec<RenderedLine>), WorkspaceError> {
        let policy = self.policy()?;
        let (_, text) = load_current(self, path, &policy.policy)?;
        let mut claimed = HashMap::new();
        for (line, value) in claims {
            if *line == 0 {
                return Err(WorkspaceError::ObservationNotFound(format!(
                    "source evidence has an invalid line number: {path}:0"
                )));
            }
            match claimed.insert(*line, value.as_str()) {
                Some(existing) if existing != value => {
                    return Err(WorkspaceError::ObservationNotFound(format!(
                        "source evidence has conflicting duplicate lines: {path}:{line}"
                    )));
                }
                _ => (),
            }
        }
        validate_spans(path, &text, &claimed, spans)?;
        if complete_presentation
            && (claimed.len() != text.total_lines()
                || (1..=text.total_lines()).any(|line| !claimed.contains_key(&line)))
        {
            return Err(WorkspaceError::ObservationNotFound(format!(
                "complete source evidence does not cover the current file: {path}"
            )));
        }

        let mut lines = claimed
            .into_iter()
            .map(|(number, value)| {
                let current = text.line(number).ok_or_else(|| {
                    WorkspaceError::Stale(format!(
                        "source evidence line no longer exists: {path}:{number}"
                    ))
                })?;
                if current != value {
                    return Err(WorkspaceError::Stale(format!(
                        "source evidence changed before presentation: {path}:{number}"
                    )));
                }
                Ok(RenderedLine {
                    number,
                    short_hash: format!("{:02X}", short_line_hash(current.as_bytes())),
                    text: current.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, WorkspaceError>>()?;
        lines.sort_unstable_by_key(|line| line.number);
        let generation = self.file_generation(path)?;
        let receipt = self.observations()?.record(
            path,
            &text,
            generation,
            &lines,
            complete_presentation,
            (
                policy.policy.max_observation_entries,
                policy.policy.max_observation_store_bytes,
            ),
        )?;
        Ok((receipt, lines))
    }
}

fn validate_spans(
    path: &str,
    text: &crate::workspace::content::NormalizedText,
    claimed: &HashMap<usize, &str>,
    spans: &[(usize, usize, usize, usize)],
) -> Result<(), WorkspaceError> {
    let serialized = text.serialize();
    let source = std::str::from_utf8(&serialized)
        .map_err(|_| WorkspaceError::Encoding("workspace file is not valid UTF-8".to_owned()))?;
    for &(start_line, start_byte, end_line, end_byte) in spans {
        let valid_order = start_line > 0
            && end_line >= start_line
            && end_byte >= start_byte
            && source.is_char_boundary(start_byte)
            && source.is_char_boundary(end_byte);
        let start_bounds = text.serialized_line_bounds(start_line);
        let end_bounds = text.serialized_line_bounds(end_line);
        let valid_boundaries = start_bounds.zip(end_bounds).is_some_and(|(start, end)| {
            (start.contains(&start_byte) || start_byte == start.end)
                && (end.contains(&end_byte) || end_byte == end.end)
        });
        let complete_claims = (start_line..=end_line).all(|line| claimed.contains_key(&line));
        if !valid_order || !valid_boundaries || !complete_claims {
            return Err(WorkspaceError::ObservationNotFound(format!(
                "source evidence has an invalid UTF-8 span: {path}:{start_line}"
            )));
        }
    }
    Ok(())
}
