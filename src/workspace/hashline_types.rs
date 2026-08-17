use std::collections::HashMap;

use crate::workspace::content::NormalizedText;

#[derive(Clone, Debug)]
pub(crate) struct HashlineOperation {
    pub kind: String,
    pub start: Option<usize>,
    pub start_hash: Option<String>,
    pub end: Option<usize>,
    pub end_hash: Option<String>,
    pub body: Vec<String>,
    pub register: Option<String>,
    pub destination: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct HashlineSection {
    pub path: String,
    pub tag: String,
    pub operations: Vec<HashlineOperation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HashlineRegister {
    pub lines: Vec<String>,
    pub trailing_newline: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum HashlineContent {
    Body(Vec<String>),
    Register(Option<String>),
}

#[derive(Clone, Debug)]
pub(crate) enum ResolvedHashlineOperation {
    Put {
        gap: usize,
        remove: Option<(usize, usize)>,
        content: HashlineContent,
        order: usize,
    },
    Cut {
        start: usize,
        end: usize,
        register: Option<String>,
        order: usize,
    },
}

#[derive(Debug)]
pub(crate) struct PreparedHashlineSection {
    pub file: HashlineFilePlan,
    pub operations: Vec<ResolvedHashlineOperation>,
}

#[derive(Debug)]
pub(crate) struct HashlineFilePlan {
    pub path: String,
    pub destination: Option<String>,
    pub remove: bool,
    pub identity: crate::workspace::FileIdentity,
    pub target: std::path::PathBuf,
    pub current: NormalizedText,
    pub final_source: String,
    pub changed_range: Option<crate::workspace::LineRange>,
}

#[derive(Debug)]
pub(crate) struct HashlinePlan {
    pub files: Vec<HashlineFilePlan>,
    pub named_registers: HashMap<String, HashlineRegister>,
}
