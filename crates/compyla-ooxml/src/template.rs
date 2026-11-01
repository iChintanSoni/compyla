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

        // Macros, anywhere (§10.4): a VBA project or its data, or an Excel 4.0 macro sheet, by part
        // name, content type or relationship type. Content types compare without case (RFC 2045).
        const MACRO_MARKS: [&str; 4] = ["vbaproject", "vbadata", "macrosheet", "macroenabled"];
        let macro_like = |text: &str| {
            let text = text.to_ascii_lowercase();
            MACRO_MARKS.iter().any(|mark| text.contains(mark))
        };
        for (name, data) in &template.parts {
            let mut macro_part =
                macro_like(name) || template.content_type(name).is_some_and(macro_like);
            if !macro_part && name.to_ascii_lowercase().ends_with(".rels") {
                // A relationship's type names what it points at; its target may be any URL.
                let rels = dom::parse(data).map_err(|_| {
                    Invalid(format!("has a malformed relationships part (`{name}`)"))
                })?;
                macro_part = rels
                    .children_named(PACKAGE_RELS, "Relationship")
                    .any(|rel| rel.attr("Type").is_some_and(macro_like));
            }
            if macro_part {
                return invalid(format!("is macro-enabled (`{name}`)"));
            }
        }
        // §10.4: no XML part may declare a DTD, whether or not anything reads it. UTF-16 parts are
        // transcoded first, so the check sees characters.
        for (name, data) in &template.parts {
            let lower = name.to_ascii_lowercase();
            let xml = lower.ends_with(".xml")
                || lower.ends_with(".rels")
                || template
                    .content_type(name)
                    .is_some_and(|t| media_type(t).ends_with("xml"));
            if !xml {
                continue;
            }
            let text = dom::utf8(data).map_err(|_| {
                Invalid(format!(
                    "has an XML part in an unsupported encoding (`{name}`)"
                ))
            })?;
            if contains_ignoring_case(&text, b"<!DOCTYPE") {
                return invalid(format!("has a DTD in `{name}`"));
            }
        }

        let main = template
            .relationships("")?
            .into_iter()
            .find(|r| r.kind.ends_with("/officeDocument") && !r.external)
            .map(|r| r.target)
            .ok_or_else(|| Invalid("has no main document part".into()))?;
        let content_type = media_type(template.content_type(&main).unwrap_or_default());
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

    /// The `[Content_Types].xml` defaults (extension, lowercased → content type).
    pub fn defaults(&self) -> impl Iterator<Item = (&str, &str)> {
        let mut defaults: Vec<(&str, &str)> = self
            .defaults
            .iter()
            .map(|(e, t)| (e.as_str(), t.as_str()))
            .collect();
        defaults.sort();
        defaults.into_iter()
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
            dom::Error::TooLarge => Invalid(format!(
                "has more than {} elements in `{name}`",
                dom::MAX_ELEMENTS
            )),
            dom::Error::Malformed(why) => Invalid(format!("has malformed XML in `{name}`: {why}")),
        })
    }

    /// The last element at `path` in a part, without building the rest of it (`dom::find_last`).
    pub fn find_last(&self, name: &str, path: &[(&str, &str)]) -> Result<Option<Element>, Invalid> {
        let Some(data) = self.part(name) else {
            return Ok(None);
        };
        dom::find_last(data, path).map_err(|error| match error {
            dom::Error::Dtd => Invalid(format!("has a DTD in `{name}`")),
            dom::Error::TooLarge => Invalid(format!(
                "has more than {} elements in `{name}`",
                dom::MAX_ELEMENTS
            )),
            dom::Error::Malformed(why) => Invalid(format!("has malformed XML in `{name}`: {why}")),
        })
    }

    /// The relationships of `source` (`""` for the package), with targets resolved to part names.
    /// Only a relationship without a `TargetMode`, or with exactly `Internal`, is internal; and a
    /// target that leaves the package, however written, is treated as external (§10.4).
    pub fn relationships(&self, source: &str) -> Result<Vec<Relationship>, Invalid> {
        let folder = source.rsplit_once('/').map_or("", |(folder, _)| folder);
        let Some(root) = self.xml(&relationships_part(source))? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        for rel in root.children_named(PACKAGE_RELS, "Relationship") {
            let target = rel.attr("Target").unwrap_or_default();
            let mut external = rel
                .attr("TargetMode")
                .is_some_and(|mode| mode != "Internal");
            let target = if external {
                target.to_string()
            } else {
                match resolve(folder, target).filter(|_| stays_inside(folder, target)) {
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

    /// The part `source` relates to by an internal relationship whose type's last segment is
    /// `kind`, compared without ASCII case, as the notes master is found (markdown-spec.md §8.8).
    pub fn related_by_type(&self, source: &str, kind: &str) -> Result<Option<String>, Invalid> {
        Ok(self
            .relationships(source)?
            .into_iter()
            .find(|r| {
                !r.external
                    && r.kind
                        .rsplit('/')
                        .next()
                        .is_some_and(|last| last.eq_ignore_ascii_case(kind))
            })
            .map(|r| r.target))
    }
}

/// A content type's media type, as content types compare: without case or parameters (RFC 2045).
pub fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// The relationships part of `source` (`""` for the package's own).
pub fn relationships_part(source: &str) -> String {
    match source.rsplit_once('/').unwrap_or(("", source)) {
        _ if source.is_empty() => "_rels/.rels".to_string(),
        ("", file) => format!("_rels/{file}.rels"),
        (folder, file) => format!("{folder}/_rels/{file}.rels"),
    }
}

/// Whether `haystack` contains `needle`, ASCII letters compared without case.
fn contains_ignoring_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// Whether an internal `target` stays in the package, however it's written: no scheme (`https:`,
/// `file:`, …), no `//host` or backslash, and no `..` past the root once percent-decoded.
fn stays_inside(folder: &str, target: &str) -> bool {
    let scheme = target.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
    });
    !scheme
        && !target.starts_with("//")
        && !target.contains('\\')
        && resolve(folder, &crate::read::percent_decoded(target)).is_some()
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

    #[test]
    fn targets_that_leave_the_package_however_written_are_external() {
        for target in [
            "https://tracker.example/x.png",
            "//tracker.example/x.png",
            "\\\\tracker.example\\share\\x.png",
            "media\\x.png",
            "%2E%2E/%2E%2E/%2E%2E/outside.png",
            "file:///C:/x.png",
        ] {
            assert!(!stays_inside("ppt/slideMasters", target), "{target}");
        }
        for target in ["../media/image1.png", "image%201.png", "#fragment"] {
            assert!(stays_inside("ppt/slideMasters", target), "{target}");
        }
    }
}
