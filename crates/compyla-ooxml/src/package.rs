//! The OPC package (ECMA-376 Part 2): parts with content types, relationships between them, and the
//! document properties every package carries.

use crate::xml::Writer;
use crate::zip;

pub const RELATIONSHIPS: &str = "application/vnd.openxmlformats-package.relationships+xml";
const CORE_PROPERTIES: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const EXTENDED_PROPERTIES: &str =
    "application/vnd.openxmlformats-officedocument.extended-properties+xml";

pub const OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
pub const HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const CORE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const EXTENDED_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";

/// Every timestamp a package carries (markdown-spec.md §13).
pub const TIMESTAMP: &str = "1980-01-01T00:00:00Z";

/// A package under construction. Parts are written in the order they are added, after
/// `[Content_Types].xml`.
#[derive(Default)]
pub struct Package {
    parts: Vec<(String, String, Vec<u8>)>,
}

impl Package {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a part. `name` has no leading `/`.
    pub fn add(&mut self, name: impl Into<String>, content_type: &str, data: Vec<u8>) {
        self.parts.push((name.into(), content_type.into(), data));
    }

    /// Adds the relationships part of `source` (`""` for the package itself).
    pub fn add_relationships(&mut self, source: &str, relationships: &Relationships) {
        let name = match source.rsplit_once('/') {
            Some((folder, file)) => format!("{folder}/_rels/{file}.rels"),
            None if source.is_empty() => "_rels/.rels".to_string(),
            None => format!("_rels/{source}.rels"),
        };
        self.add(name, RELATIONSHIPS, relationships.to_xml());
    }

    /// The package's bytes: a content-type override for every part, then the parts.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut types = Writer::new();
        types.open(
            "Types",
            &[(
                "xmlns",
                "http://schemas.openxmlformats.org/package/2006/content-types",
            )],
        );
        types.empty(
            "Default",
            &[("Extension", "rels"), ("ContentType", RELATIONSHIPS)],
        );
        types.empty(
            "Default",
            &[("Extension", "xml"), ("ContentType", "application/xml")],
        );
        for (name, content_type, _) in &self.parts {
            if content_type != RELATIONSHIPS {
                types.empty(
                    "Override",
                    &[
                        ("PartName", &format!("/{name}")),
                        ("ContentType", content_type),
                    ],
                );
            }
        }
        types.close("Types");
        let mut entries = vec![("[Content_Types].xml".to_string(), types.finish())];
        entries.extend(
            self.parts
                .iter()
                .map(|(name, _, data)| (name.clone(), data.clone())),
        );
        zip::write(&entries)
    }
}

/// A part's relationships, with ids `rId1`, `rId2`, … in the order they are added.
#[derive(Default)]
pub struct Relationships {
    items: Vec<(String, String, bool)>,
}

impl Relationships {
    pub fn new() -> Self {
        Self::default()
    }

    /// A relationship to another part, by its path relative to the source part's folder.
    pub fn add(&mut self, kind: &str, target: &str) -> String {
        self.push(kind, target, false)
    }

    /// A relationship to an external resource, such as a hyperlink's URL.
    pub fn add_external(&mut self, kind: &str, target: &str) -> String {
        self.push(kind, target, true)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    fn push(&mut self, kind: &str, target: &str, external: bool) -> String {
        self.items.push((kind.into(), target.into(), external));
        format!("rId{}", self.items.len())
    }

    /// The relationships part's XML.
    pub fn to_xml(&self) -> Vec<u8> {
        let mut xml = Writer::new();
        xml.open(
            "Relationships",
            &[(
                "xmlns",
                "http://schemas.openxmlformats.org/package/2006/relationships",
            )],
        );
        for (index, (kind, target, external)) in self.items.iter().enumerate() {
            let id = format!("rId{}", index + 1);
            let mut attrs = vec![("Id", id.as_str()), ("Type", kind), ("Target", target)];
            if *external {
                attrs.push(("TargetMode", "External"));
            }
            xml.empty("Relationship", &attrs);
        }
        xml.close("Relationships");
        xml.finish()
    }
}

/// Document properties, written only from the document's `meta` (markdown-spec.md C44): a title or
/// author it lacks is not written, and nothing comes from the renderer or the environment.
#[derive(Debug, Default, Clone)]
pub struct Properties {
    pub title: Option<String>,
    pub creator: Option<String>,
}

/// Adds the core and extended properties parts, and the package relationships to them and to the
/// main part (`main`, with its relationship type `OFFICE_DOCUMENT`).
pub fn add_package_parts(package: &mut Package, main: &str, properties: &Properties) {
    let mut rels = Relationships::new();
    rels.add(OFFICE_DOCUMENT, main);
    rels.add(CORE_RELATIONSHIP, "docProps/core.xml");
    rels.add(EXTENDED_RELATIONSHIP, "docProps/app.xml");
    package.add_relationships("", &rels);

    let mut core = Writer::new();
    core.open(
        "cp:coreProperties",
        &[
            (
                "xmlns:cp",
                "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
            ),
            ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
            ("xmlns:dcterms", "http://purl.org/dc/terms/"),
            ("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"),
        ],
    );
    if let Some(title) = &properties.title {
        core.element("dc:title", &[], title);
    }
    if let Some(creator) = &properties.creator {
        core.element("dc:creator", &[], creator);
    }
    for stamp in ["dcterms:created", "dcterms:modified"] {
        core.element(stamp, &[("xsi:type", "dcterms:W3CDTF")], TIMESTAMP);
    }
    core.close("cp:coreProperties");
    package.add("docProps/core.xml", CORE_PROPERTIES, core.finish());

    let mut app = Writer::new();
    app.open(
        "Properties",
        &[(
            "xmlns",
            "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties",
        )],
    );
    app.element("Application", &[], "Compyla");
    app.close("Properties");
    package.add("docProps/app.xml", EXTENDED_PROPERTIES, app.finish());
}
