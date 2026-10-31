//! The document-order walk (markdown-spec.md §9.1, §9.4): the sheets, the tables each holds, and the
//! range title, note and directives attached to each table.
//!
//! The body is walked as one stream in document order, entering sections, xlsx annotations, and the
//! bodies of ignored annotations and unknown directives. Every H1, wherever it is, starts a sheet.

use super::{excerpt, line_of};
use crate::diagnostic::Diagnostic;
use crate::ir::{Block, Directive, Paragraph, Position, Section, Table, plain_text};
use crate::validate::{DirectiveKind, at, directive};

/// The sheet limit (§12.1).
const MAX_SHEETS: usize = 1000;

pub struct Walk<'a> {
    /// The sheets that exist, in document order.
    pub sheets: Vec<RawSheet<'a>>,
    /// Every `anchor`, `table` and `columns` directive met, in document order; tables refer to
    /// them by index.
    pub directives: Vec<&'a Directive>,
    /// Whether each directive took effect on at least one table.
    pub applied: Vec<bool>,
}

pub struct RawSheet<'a> {
    /// The H1, or `None` for the default sheet.
    pub heading: Option<&'a Section>,
    pub tables: Vec<Attached<'a>>,
    /// Whether the sheet produced `xlsx_ignored_content` (a default sheet then exists, §9.1).
    ignored: bool,
}

/// A table and what attached to it.
pub struct Attached<'a> {
    pub table: &'a Table,
    pub title: Option<&'a Section>,
    pub note: Option<&'a Paragraph>,
    pub anchor: Option<usize>,
    pub excel: Option<usize>,
    pub columns: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    Anchor = 0,
    Table = 1,
    Columns = 2,
}

impl Slot {
    fn of(name: &str) -> Option<Slot> {
        match name {
            "anchor" => Some(Slot::Anchor),
            "table" => Some(Slot::Table),
            "columns" => Some(Slot::Columns),
            _ => None,
        }
    }
}

/// A wrapping annotation whose body is being walked.
struct Wrap {
    id: usize,
    slot: Slot,
    /// Cleared by an H1: an annotation never crosses a sheet boundary.
    active: bool,
    /// `anchor` and `table` apply to the first table they wrap only.
    used: bool,
}

pub fn walk<'a>(body: &'a [Block], out: &mut Vec<Diagnostic>) -> Walk<'a> {
    let mut walker = Walker {
        walk: Walk {
            sheets: vec![RawSheet {
                heading: None,
                tables: Vec::new(),
                ignored: false,
            }],
            directives: Vec::new(),
            applied: Vec::new(),
        },
        title: None,
        note: None,
        waiting: [None; 3],
        wraps: Vec::new(),
        stopped: false,
        out,
    };
    walker.blocks(body);
    walker.end_sheet();
    walker.walk
}

struct Walker<'a, 'o> {
    walk: Walk<'a>,
    /// A range title (H2) waiting for the next table.
    title: Option<&'a Section>,
    /// A note (paragraph) waiting for the next table.
    note: Option<&'a Paragraph>,
    /// Empty annotations waiting for the next table, by slot.
    waiting: [Option<usize>; 3],
    wraps: Vec<Wrap>,
    /// Set at the sheet limit: nothing further is processed (§12.1).
    stopped: bool,
    out: &'o mut Vec<Diagnostic>,
}

const MAPPING: &str = "xlsx maps `#` to sheets, `##` to range titles, tables to ranges and paragraphs to table notes.";

impl<'a> Walker<'a, '_> {
    fn blocks(&mut self, blocks: &'a [Block]) {
        for block in blocks {
            if self.stopped {
                return;
            }
            self.block(block);
        }
    }

    fn block(&mut self, block: &'a Block) {
        match block {
            Block::Section(section) => {
                let title = excerpt(&plain_text(&section.title));
                match section.level {
                    1 => {
                        if !self.start_sheet(section) {
                            return;
                        }
                    }
                    2 => {
                        if let Some(previous) = self.title.replace(section) {
                            self.ignored(
                                previous.position,
                                format!(
                                    "Range title `{}` is replaced by `{title}` before a table arrives; it is ignored.",
                                    excerpt(&plain_text(&previous.title))
                                ),
                                "Put a table under each `##` heading.",
                            );
                        }
                    }
                    level => self.ignored(
                        section.position,
                        format!(
                            "Heading `{title}` (level {level}) has no xlsx mapping; it is ignored, and the content under it is kept."
                        ),
                        MAPPING,
                    ),
                }
                self.blocks(&section.children);
            }
            Block::Paragraph(paragraph) => {
                if let Some(previous) = self.note.replace(paragraph) {
                    self.ignored(
                        previous.position,
                        "This paragraph is replaced as the next table's note by a later paragraph; it is ignored."
                            .to_string(),
                        "A table takes one note: merge the paragraphs, or put a table after each.",
                    );
                }
            }
            Block::Table(table) => self.table(table),
            Block::Directive(d) => self.directive(d),
            Block::Break(_) => {}
            Block::List(_) => self.ignored_block(block, "A list"),
            Block::Quote(_) => self.ignored_block(block, "A quote"),
            Block::Code(_) => self.ignored_block(block, "A code block"),
            Block::Media(_) => self.ignored_block(block, "An image"),
        }
    }

    /// An H1 ends the current sheet and starts another, unless the sheet limit is reached.
    fn start_sheet(&mut self, section: &'a Section) -> bool {
        self.end_sheet();
        if self.walk.sheets.len() >= MAX_SHEETS {
            let mut diagnostic = Diagnostic::new(
                "limit_exceeded",
                format!(
                    "The workbook has more than {MAX_SHEETS} sheets; the rest of the document was not processed."
                ),
            );
            if let Some(p) = section.position {
                diagnostic = diagnostic.at(p.line, Some(p.column));
            }
            self.out.push(diagnostic);
            self.stopped = true;
            return false;
        }
        self.walk.sheets.push(RawSheet {
            heading: Some(section),
            tables: Vec::new(),
            ignored: false,
        });
        true
    }

    /// At a sheet's end, whatever still waits for a table never attaches, and wrapping annotations
    /// stop applying. A default sheet with no table and no ignored content doesn't exist.
    fn end_sheet(&mut self) {
        if let Some(title) = self.title.take() {
            self.ignored(
                title.position,
                format!(
                    "Range title `{}` has no table after it in its sheet; it is ignored.",
                    excerpt(&plain_text(&title.title))
                ),
                "Put a table under the `##` heading, or remove it.",
            );
        }
        if let Some(note) = self.note.take() {
            self.ignored(
                note.position,
                "This paragraph has no table after it in its sheet, so it isn't a table note; it is ignored."
                    .to_string(),
                MAPPING,
            );
        }
        for slot in 0..self.waiting.len() {
            if let Some(id) = self.waiting[slot].take() {
                self.misplaced(id, "has no table after it in its sheet".to_string());
            }
        }
        for wrap in &mut self.wraps {
            wrap.active = false;
        }
        if let Some(sheet) = self.walk.sheets.last()
            && sheet.heading.is_none()
            && sheet.tables.is_empty()
            && !sheet.ignored
        {
            self.walk.sheets.pop();
        }
    }

    fn table(&mut self, table: &'a Table) {
        let attached = Attached {
            table,
            title: self.title.take(),
            note: self.note.take(),
            anchor: self.attach(Slot::Anchor, table),
            excel: self.attach(Slot::Table, table),
            columns: self.attach(Slot::Columns, table),
        };
        if let Some(sheet) = self.walk.sheets.last_mut() {
            sheet.tables.push(attached);
        }
    }

    /// The directive of `slot` that applies to `table`: the innermost wrapping one, else the waiting
    /// empty one. A wrapping annotation overrides a waiting empty one of the same name, which is
    /// consumed. A wrapping `anchor` or `table` applies to its first table only.
    fn attach(&mut self, slot: Slot, table: &Table) -> Option<usize> {
        let waiting = self.waiting[slot as usize].take();
        let mut chosen = None;
        let mut exhausted = None;
        for wrap in self
            .wraps
            .iter_mut()
            .rev()
            .filter(|w| w.active && w.slot == slot)
        {
            if slot == Slot::Columns {
                // `columns` applies to every table it wraps; the innermost wins.
                self.walk.applied[wrap.id] = true;
                chosen.get_or_insert(wrap.id);
            } else {
                if wrap.used {
                    exhausted = Some(wrap.id);
                } else {
                    wrap.used = true;
                    self.walk.applied[wrap.id] = true;
                    chosen = Some(wrap.id);
                }
                break;
            }
        }
        if let Some(id) = exhausted {
            let line = table
                .position
                .map(|p| format!(" at line {}", p.line))
                .unwrap_or_default();
            self.misplaced(
                id,
                format!("applies only to the first table it wraps, not to the table{line}"),
            );
        }
        if chosen.is_none()
            && let Some(id) = waiting
        {
            self.walk.applied[id] = true;
            chosen = Some(id);
        }
        chosen
    }

    fn directive(&mut self, d: &'a Directive) {
        let Some(slot) = Slot::of(&d.name) else {
            // Ignored containers (`notes`) are omitted with their children; ignored markers produce
            // nothing; ignored annotations and unknown directives render their children in place.
            if !matches!(
                directive(&d.name),
                Some((DirectiveKind::Container | DirectiveKind::Marker, _))
            ) {
                self.blocks(&d.children);
            }
            return;
        };
        let id = self.walk.directives.len();
        self.walk.directives.push(d);
        self.walk.applied.push(false);
        if d.children.is_empty() {
            if let Some(previous) = self.waiting[slot as usize].replace(id) {
                self.misplaced(
                    previous,
                    format!("is replaced by another `{}` before a table arrives", d.name),
                );
            }
            return;
        }
        self.wraps.push(Wrap {
            id,
            slot,
            active: true,
            used: false,
        });
        self.blocks(&d.children);
        self.wraps.pop();
        if !self.stopped && !self.walk.applied[id] {
            self.misplaced(id, "wraps no table in its sheet".to_string());
        }
    }

    fn ignored(&mut self, position: Option<Position>, message: String, hint: &str) {
        self.out.push(at(
            Diagnostic::new("xlsx_ignored_content", message).hint(hint),
            line_of(position),
        ));
        if let Some(sheet) = self.walk.sheets.last_mut() {
            sheet.ignored = true;
        }
    }

    fn ignored_block(&mut self, block: &Block, what: &str) {
        self.ignored(
            block.position(),
            format!("{what} has no xlsx mapping; it is ignored."),
            MAPPING,
        );
    }

    fn misplaced(&mut self, id: usize, why: String) {
        let d = self.walk.directives[id];
        let args = if d.args.is_empty() {
            String::new()
        } else {
            format!(" {}", excerpt(&d.args))
        };
        self.out.push(at(
            Diagnostic::new(
                "directive_misplaced",
                format!("`::: {}{args}` {why}; it is ignored.", d.name),
            )
            .hint("Put the directive directly before its table, or wrap the table in it."),
            line_of(d.position),
        ));
    }
}
