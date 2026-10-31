//! The IR (markdown-spec.md §4), mirroring `spec/ir.schema.json`. Serializes to exactly the JSON the
//! schema defines: marks are present only when set, optional fields only when present.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const IR_VERSION: &str = "0.1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    #[serde(rename = "irVersion")]
    pub ir_version: String,
    pub meta: Map<String, Value>,
    pub body: Vec<Block>,
}

/// 1-based line; 1-based column in Unicode scalar values (markdown-spec.md §0.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Section(Section),
    Paragraph(Paragraph),
    Table(Table),
    List(List),
    Media(Media),
    Code(Code),
    Quote(Quote),
    Break(Break),
    Directive(Directive),
}

impl Block {
    pub fn position(&self) -> Option<Position> {
        match self {
            Block::Section(n) => n.position,
            Block::Paragraph(n) => n.position,
            Block::Table(n) => n.position,
            Block::List(n) => n.position,
            Block::Media(n) => n.position,
            Block::Code(n) => n.position,
            Block::Quote(n) => n.position,
            Block::Break(n) => n.position,
            Block::Directive(n) => n.position,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub level: u8,
    pub title: Vec<Run>,
    pub children: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub header: Vec<Cell>,
    pub align: Vec<Option<Align>>,
    pub rows: Vec<Vec<Cell>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    pub runs: Vec<Run>,
    /// The cell's source text: trimmed, `\|` unescaped (markdown-spec.md §4.6).
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct List {
    pub ordered: bool,
    /// Present only on an ordered list that doesn't start at 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u64>,
    pub items: Vec<ListItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListItem {
    pub children: Vec<Block>,
    /// `true`/`false` for a task-list item, `null` otherwise.
    pub checked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Media {
    pub src: String,
    pub alt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Code {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub children: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Break {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Directive {
    /// Lowercase (markdown-spec.md §1.3).
    pub name: String,
    pub args: String,
    pub children: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

/// A span of text with uniform marks (markdown-spec.md §4.4). `"\n"` in `text` is a hard break.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Run {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub code: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Run {
    /// A run of text with no marks and no link.
    pub fn plain(text: String) -> Run {
        Run {
            text,
            ..Run::default()
        }
    }

    fn same_marks(&self, other: &Run) -> bool {
        self.bold == other.bold
            && self.italic == other.italic
            && self.code == other.code
            && self.strike == other.strike
            && self.link == other.link
    }
}

/// The canonical form of a run list (markdown-spec.md §4.4): empty runs dropped, adjacent runs with
/// identical marks and link merged.
pub fn canonical_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    for run in runs {
        if run.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.same_marks(&run) => last.text.push_str(&run.text),
            _ => out.push(run),
        }
    }
    out
}

/// The plain text of a run list.
pub fn plain_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}
