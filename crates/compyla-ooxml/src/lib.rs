//! OOXML packages for Compyla's renderers (architecture.md D12): a deterministic zip writer, XML
//! writing that can't be escaped from, and the package model. Output is reproducible byte for byte
//! (markdown-spec.md §13).

pub mod dom;
pub mod fonts;
pub mod inspect;
pub mod layouts;
pub mod package;
pub mod read;
pub mod sanitize;
pub mod template;
pub mod text;
pub mod theme;
pub mod xml;
pub mod zip;

pub use inspect::inspect;
pub use package::{Package, Properties, Relationships};
pub use template::{Kind, Template};
pub use xml::Writer;
