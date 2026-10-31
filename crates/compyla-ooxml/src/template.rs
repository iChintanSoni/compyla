//! Template packages (markdown-spec.md §10.4, §10.5): an untrusted package, read and checked once,
//! then queried part by part. Its kind comes from the main part's content type, never from a file
//! name.

use std::collections::HashMap;

use crate::dom::{self, CONTENT_TYPES, Element, PACKAGE_RELS};
use crate::read;

/// The package kinds a template can be (§10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Docx,
    Pptx,
    Xlsx,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Docx => "docx",
            Kind::Pptx => "pptx",
            Kind::Xlsx => "xlsx",
        }
    }
}

/// Why a template is `template_invalid` (§10.4). The text completes "The template …".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(pub String);

impl From<read::Refused> for Invalid {
    fn from(refused: read::Refused) -> Self {
        Invalid(refused.0)
    }
}

fn invalid<T>(reason: impl Into<String>) -> Result<T, Invalid> {
    Err(Invalid(reason.into()))
}

/// A relationship of a part: a resolved part name, or an external target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub external: bool,
}

/// A checked template package.
#[derive(Debug)]
pub struct Template {
    pub kind: Kind,
    /// The main part (`word/document.xml`, `ppt/presentation.xml`, `xl/workbook.xml`, or wherever
    /// the package relationship points).
    pub main: String,
    parts: Vec<(String, Vec<u8>)>,
    by_name: HashMap<String, usize>,
    defaults: HashMap<String, String>,
    overrides: HashMap<String, String>,
}

/// Main-part content types: the accepted kinds, then the macro-enabled ones.
const MAIN_TYPES: [(&str, Kind); 6] = [
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        Kind::Docx,
    ),
    (
        "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml",
        Kind::Docx,
    ),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
        Kind::Pptx,
    ),
    (
        "application/vnd.openxmlformats-officedocument.presentationml.template.main+xml",
        Kind::Pptx,
    ),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
        Kind::Xlsx,
    ),
    (
        "application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml",
        Kind::Xlsx,
    ),
];

impl Template {
    /// Reads and checks a package (§10.4, §12.1).
    pub fn read(bytes: &[u8]) -> Result<Template, Invalid> {
        let parts = read::entries(bytes)?;
        let by_name: HashMap<String, usize> = parts
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (name.to_ascii_lowercase(), index))
            .collect();
        let mut template = Template {
            kind: Kind::Docx,
            main: String::new(),
            parts,
            by_name,
            defaults: HashMap::new(),
            overrides: HashMap::new(),
        };

        let types = template.xml("[Content_Types].xml")?.ok_or_else(|| {
            Invalid("has no [Content_Types].xml, so it isn't an OOXML package".into())
        })?;
        if !types.is(CONTENT_TYPES, "Types") {
            return invalid("has a malformed [Content_Types].xml");
        }
        for child in &types.children {
            let content_type = child.attr("ContentType").unwrap_or_default().to_string();
            if child.is(CONTENT_TYPES, "Default") {
                let extension = child
                    .attr("Extension")
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                template.defaults.insert(extension, content_type);
            } else if child.is(CONTENT_TYPES, "Override") {
                let part = child.attr("PartName").unwrap_or_default();
                let part = part.trim_start_matches('/').to_ascii_lowercase();
                template.overrides.insert(part, content_type);
            }
        }

        // Macros, anywhere: a VBA project part, by name or by content type.
        let macro_part = template.parts.iter().any(|(name, _)| {
            name.to_ascii_lowercase().ends_with("vbaproject.bin")
                || template
                    .content_type(name)
                    .is_some_and(|t| t.contains("vbaProject"))
        });
        if macro_part {
            return invalid("is macro-enabled (it contains a VBA project)");
        }
        // §10.4: no XML part may declare a DTD, whether or not anything reads it.
        for (name, data) in &template.parts {
            let xml = name.ends_with(".xml")
                || name.ends_with(".rels")
                || template
                    .content_type(name)
                    .is_some_and(|t| t.ends_with("xml"));
            if xml && data.windows(9).any(|w| w == b"<!DOCTYPE") {
                return invalid(format!("has a DTD in `{name}`"));
            }
        }

        let main = template
            .relationships("")?
            .into_iter()
            .find(|r| r.kind.ends_with("/officeDocument") && !r.external)
            .map(|r| r.target)
            .ok_or_else(|| Invalid("has no main document part".into()))?;
        let content_type = template.content_type(&main).unwrap_or_default().to_string();
        if content_type.contains("macroEnabled") {
            return invalid("is macro-enabled");
        }
        template.kind = MAIN_TYPES
            .iter()
            .find(|(t, _)| *t == content_type)
            .map(|(_, kind)| *kind)
            .ok_or_else(|| {
                Invalid("isn't a docx, dotx, pptx, potx, xlsx or xltx package".into())
            })?;
        if template.part(&main).is_none() {
            return invalid("is missing its main document part");
        }
        template.main = main;
        Ok(template)
    }

    pub fn part(&self, name: &str) -> Option<&[u8]> {
        self.by_name
            .get(&name.to_ascii_lowercase())
            .map(|&index| self.parts[index].1.as_slice())
    }

    /// Every part, in package order.
    pub fn parts(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.parts.iter().map(|(n, d)| (n.as_str(), d.as_slice()))
    }

    pub fn content_type(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.overrides
            .get(&name)
            .or_else(|| {
                let extension = name.rsplit_once('.').map(|(_, e)| e)?;
                self.defaults.get(extension)
            })
            .map(String::as_str)
    }

    /// A part's XML, or `None` when the package has no such part.
    pub fn xml(&self, name: &str) -> Result<Option<Element>, Invalid> {
        let Some(data) = self.part(name) else {
            return Ok(None);
        };
        dom::parse(data).map(Some).map_err(|error| match error {
            dom::Error::Dtd => Invalid(format!("has a DTD in `{name}`")),
            dom::Error::Malformed(why) => Invalid(format!("has malformed XML in `{name}`: {why}")),
        })
    }

    /// The relationships of `source` (`""` for the package), with targets resolved to part names.
    /// A target that leaves the package is treated as external.
    pub fn relationships(&self, source: &str) -> Result<Vec<Relationship>, Invalid> {
        let (folder, file) = match source.rsplit_once('/') {
            Some((folder, file)) => (folder, file),
            None => ("", source),
        };
        let rels = if source.is_empty() {
            "_rels/.rels".to_string()
        } else if folder.is_empty() {
            format!("_rels/{file}.rels")
        } else {
            format!("{folder}/_rels/{file}.rels")
        };
        let Some(root) = self.xml(&rels)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for rel in root.children_named(PACKAGE_RELS, "Relationship") {
            let target = rel.attr("Target").unwrap_or_default();
            let mut external = rel.attr("TargetMode") == Some("External");
            let target = if external {
                target.to_string()
            } else {
                match resolve(folder, target) {
                    Some(part) => part,
                    None => {
                        external = true;
                        target.to_string()
                    }
                }
            };
            out.push(Relationship {
                id: rel.attr("Id").unwrap_or_default().to_string(),
                kind: rel.attr("Type").unwrap_or_default().to_string(),
                target,
                external,
            });
        }
        Ok(out)
    }

    /// The first relationship of `source` whose type ends with `/kind`, as a part name.
    pub fn related(&self, source: &str, kind: &str) -> Result<Option<String>, Invalid> {
        let suffix = format!("/{kind}");
        Ok(self
            .relationships(source)?
            .into_iter()
            .find(|r| r.kind.ends_with(&suffix) && !r.external)
            .map(|r| r.target))
    }
}

/// A relationship target relative to `folder`, as a part name; `None` if it leaves the package.
fn resolve(folder: &str, target: &str) -> Option<String> {
    let target = target.split('#').next().unwrap_or_default();
    let mut segments: Vec<&str> = if let Some(absolute) = target.strip_prefix('/') {
        absolute.split('/').collect()
    } else {
        folder
            .split('/')
            .filter(|s| !s.is_empty())
            .chain(target.split('/'))
            .collect()
    };
    let mut out: Vec<&str> = Vec::new();
    for segment in segments.drain(..) {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            segment => out.push(segment),
        }
    }
    Some(out.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_inside_the_package() {
        assert_eq!(
            resolve("ppt", "slides/slide1.xml").as_deref(),
            Some("ppt/slides/slide1.xml")
        );
        assert_eq!(
            resolve("ppt/slideLayouts", "../slideMasters/slideMaster1.xml").as_deref(),
            Some("ppt/slideMasters/slideMaster1.xml")
        );
        assert_eq!(
            resolve("", "/word/document.xml").as_deref(),
            Some("word/document.xml")
        );
        assert_eq!(resolve("ppt", "../../etc/passwd"), None);
    }
}
