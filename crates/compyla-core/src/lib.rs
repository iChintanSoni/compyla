//! Compyla's core: the parser, IR and validator (markdown-spec.md §1–§9). It performs no I/O
//! (architecture.md D11); hosts supply resolvers through the `compyla` facade.

pub mod diagnostic;
pub mod frontmatter;
pub mod ir;
pub mod ir_input;
pub mod locate;
pub mod parse;
pub mod prepass;
pub mod validate;

pub use diagnostic::{Diagnostic, Severity};
pub use ir::Document;
pub use parse::{ParseOutput, Sidecar, parse};
pub use validate::{Asset, Format};
