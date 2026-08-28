use std::collections::BTreeMap;

use crate::workspace::WorkspaceDirectoryRead;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceDirectoryTreeLine {
    pub depth: usize,
    pub name: String,
    pub kind: String,
    pub omitted_entries: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceDirectoryTreeRead {
    pub path: String,
    pub lines: Vec<WorkspaceDirectoryTreeLine>,
    pub child_limit: usize,
    pub scanned_entries: usize,
    pub limited_directories: usize,
    pub omitted_entries: usize,
    pub scan_truncated: bool,
}

pub(crate) fn build_directory_tree(
    listing: WorkspaceDirectoryRead,
    child_limit: usize,
) -> WorkspaceDirectoryTreeRead {
    let mut root = DirectoryNode::root();
    for entry in &listing.entries {
        let relative = relative_entry_path(&listing.path, &entry.path);
        if !relative.is_empty() {
            root.insert(relative, &entry.kind);
        }
    }

    let mut lines = Vec::new();
    let mut limits = TreeLimits::default();
    render_children(&root, 0, true, child_limit, &mut lines, &mut limits);

    WorkspaceDirectoryTreeRead {
        child_limit,
        path: listing.path,
        lines,
        scanned_entries: listing.entries.len(),
        limited_directories: limits.directories,
        omitted_entries: limits.entries,
        scan_truncated: listing.truncated,
    }
}

#[derive(Debug)]
struct DirectoryNode {
    name: String,
    kind: String,
    children: BTreeMap<String, Self>,
}

impl DirectoryNode {
    fn root() -> Self {
        Self {
            name: String::new(),
            kind: "directory".to_owned(),
            children: BTreeMap::new(),
        }
    }

    fn insert(&mut self, path: &str, kind: &str) {
        let mut parts = path.split('/').peekable();
        let mut node = self;
        while let Some(part) = parts.next() {
            let last = parts.peek().is_none();
            let child = node
                .children
                .entry(part.to_owned())
                .or_insert_with(|| Self {
                    name: part.to_owned(),
                    kind: if last { kind } else { "directory" }.to_owned(),
                    children: BTreeMap::new(),
                });
            if last {
                child.kind = kind.to_owned();
            }
            node = child;
        }
    }
}

#[derive(Default)]
struct TreeLimits {
    directories: usize,
    entries: usize,
}

fn relative_entry_path<'a>(root: &str, entry: &'a str) -> &'a str {
    if root == "." {
        return entry;
    }
    entry
        .strip_prefix(root)
        .and_then(|relative| relative.strip_prefix('/'))
        .unwrap_or(entry)
}

fn render_children(
    node: &DirectoryNode,
    depth: usize,
    root: bool,
    child_limit: usize,
    lines: &mut Vec<WorkspaceDirectoryTreeLine>,
    limits: &mut TreeLimits,
) {
    if !root && node.children.len() > child_limit {
        let omitted = node.children.len() - child_limit;
        for child in node.children.values().take(child_limit - 1) {
            render_node(child, depth, child_limit, lines, limits);
        }
        lines.push(WorkspaceDirectoryTreeLine {
            depth,
            name: String::new(),
            kind: "omitted".to_owned(),
            omitted_entries: Some(omitted),
        });
        if let Some(child) = node.children.values().next_back() {
            render_node(child, depth, child_limit, lines, limits);
        }
        limits.directories += 1;
        limits.entries += omitted;
        return;
    }

    for child in node.children.values() {
        render_node(child, depth, child_limit, lines, limits);
    }
}

fn render_node(
    node: &DirectoryNode,
    depth: usize,
    child_limit: usize,
    lines: &mut Vec<WorkspaceDirectoryTreeLine>,
    limits: &mut TreeLimits,
) {
    lines.push(WorkspaceDirectoryTreeLine {
        depth,
        name: node.name.clone(),
        kind: node.kind.clone(),
        omitted_entries: None,
    });
    if node.kind == "directory" {
        render_children(node, depth + 1, false, child_limit, lines, limits);
    }
}
