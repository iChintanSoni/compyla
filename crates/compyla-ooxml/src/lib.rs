//! OOXML packages for Compyla's renderers (architecture.md D12): a deterministic zip writer, XML
//! writing that can't be escaped from, and the package model. Output is reproducible byte for byte
//! (markdown-spec.md §13).

pub mod package;
pub mod xml;
pub mod zip;

pub use package::{Package, Properties, Relationships};
pub use xml::Writer;
