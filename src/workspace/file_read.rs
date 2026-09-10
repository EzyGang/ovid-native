use std::{fs, path::Path};

use crate::workspace::{
    Cancellation, LineRange, NormalizedText, ReadExtent, RenderedLine, WorkControl, Workspace,
    WorkspaceError, WorkspaceFileRead, WorkspaceTextSerialization, line_hash, path, read_content,
    read_ranges,
};

impl Workspace {
    pub(crate) fn read_file(
        &self,
        path: &str,
        ranges: &[LineRange],
    ) -> Result<WorkspaceFileRead, WorkspaceError> {
        self.ensure_open()?;
        let path = path::normalize_relative(path)?;
        let policy = self.policy()?;
        let target = path::resolve_contained_file(self.root(), &path)?;
        let metadata = fs::metadata(&target)
            .map_err(|error| WorkspaceError::Read(format!("cannot inspect {path}: {error}")))?;
        let control = WorkControl::new(Cancellation::new(), None);
        let observation_limit = policy.policy.max_observation_file_bytes;
        let normalized_ranges = normalize_read_ranges(ranges, usize::MAX)?;
        if metadata.len() > observation_limit {
            return read_unobserved_file(
                &path,
                &target,
                &normalized_ranges,
                policy.policy.max_read_bytes,
                observation_limit,
                &control,
            );
        }

        let content = read_content(
            &target,
            ReadExtent::Complete {
                max_bytes: observation_limit,
            },
            &control,
        )?;
        if !content.complete {
            return read_unobserved_file(
                &path,
                &target,
                &normalized_ranges,
                policy.policy.max_read_bytes,
                observation_limit,
                &control,
            );
        }
        let total_bytes = content.total_bytes;
        let text = NormalizedText::decode(content.bytes)?;
        let total_lines = text.total_lines();
        let normalized_ranges = normalize_read_ranges(ranges, total_lines)?;
        let lines = bounded_render(&text, &normalized_ranges, policy.policy.max_read_bytes);
        let visible_ranges = ranges_from_rendered(&lines);
        let complete_presentation = total_lines == lines.len()
            && (total_lines == 0
                || visible_ranges
                    == vec![LineRange {
                        start: 1,
                        end: total_lines,
                    }]);
        let generation = self.file_generation(&path)?;
        let observation = Some(self.observations()?.record(
            &path,
            &text,
            generation,
            &lines,
            complete_presentation,
            (
                policy.policy.max_observation_entries,
                policy.policy.max_observation_store_bytes,
            ),
        )?);
        let serialization = Some(WorkspaceTextSerialization {
            bom: text.serialization.bom,
            line_ending: text.serialization.line_ending,
            terminal_newline: text.source.ends_with('\n'),
        });

        Ok(WorkspaceFileRead {
            path: path.to_owned(),
            observation,
            lines,
            total_lines,
            complete_presentation,
            editable: true,
            total_bytes,
            observation_limit,
            serialization,
        })
    }
}

fn read_unobserved_file(
    path: &str,
    target: &Path,
    ranges: &[LineRange],
    max_read_bytes: u64,
    observation_limit: u64,
    control: &WorkControl,
) -> Result<WorkspaceFileRead, WorkspaceError> {
    let selected = read_ranges::read_selected_text(target, ranges, max_read_bytes, control)?;
    let lines = selected
        .lines
        .into_iter()
        .map(|(number, text)| RenderedLine {
            number,
            short_hash: format!("{:02X}", line_hash::short_line_hash(text.as_bytes())),
            text,
        })
        .collect();

    Ok(WorkspaceFileRead {
        path: path.to_owned(),
        observation: None,
        lines,
        total_lines: selected.total_lines,
        complete_presentation: false,
        editable: false,
        total_bytes: selected.total_bytes,
        observation_limit,
        serialization: None,
    })
}

fn normalize_read_ranges(
    ranges: &[LineRange],
    total_lines: usize,
) -> Result<Vec<LineRange>, WorkspaceError> {
    if ranges.is_empty() {
        return Ok(if total_lines == 0 {
            Vec::new()
        } else {
            vec![LineRange {
                start: 1,
                end: total_lines,
            }]
        });
    }

    let mut normalized = ranges
        .iter()
        .map(|range| {
            if range.start == 0 || range.end < range.start {
                return Err(WorkspaceError::Read(
                    "workspace read ranges must be one-based and ordered".to_owned(),
                ));
            }
            Ok(LineRange {
                start: range.start,
                end: range.end.min(total_lines),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    normalized.retain(|range| range.start <= total_lines);
    normalized.sort_by_key(|range| range.start);
    let mut merged: Vec<LineRange> = Vec::new();
    for range in normalized {
        match merged.last_mut() {
            Some(previous) if range.start <= previous.end => {
                return Err(WorkspaceError::Read(
                    "workspace read ranges cannot overlap".to_owned(),
                ));
            }
            Some(previous) if range.start == previous.end.saturating_add(1) => {
                previous.end = range.end;
            }
            _ => merged.push(range),
        }
    }
    Ok(merged)
}

pub(super) fn bounded_render(
    text: &NormalizedText,
    ranges: &[LineRange],
    max_bytes: u64,
) -> Vec<RenderedLine> {
    let mut lines = Vec::new();
    let mut used = 0_u64;
    for range in ranges {
        for number in range.start..=range.end {
            let Some(source) = text.line(number) else {
                continue;
            };
            let line_bytes = source.len() as u64;
            if used.saturating_add(line_bytes) > max_bytes {
                return lines;
            }
            used = used.saturating_add(line_bytes);
            lines.push(RenderedLine {
                number,
                short_hash: format!("{:02X}", line_hash::short_line_hash(source.as_bytes())),
                text: source.to_owned(),
            });
        }
    }
    lines
}

fn ranges_from_rendered(lines: &[RenderedLine]) -> Vec<LineRange> {
    let mut ranges: Vec<LineRange> = Vec::new();
    for line in lines {
        match ranges.last_mut() {
            Some(range) if line.number == range.end.saturating_add(1) => range.end = line.number,
            _ => ranges.push(LineRange {
                start: line.number,
                end: line.number,
            }),
        }
    }
    ranges
}
