//! Hostile input (markdown-spec.md §12.1): inputs that once took quadratic time. Each takes at most
//! a few seconds in a debug build; the bound is generous so that only a return to quadratic
//! behaviour fails. The tests run one at a time, so the time each measures is its own: side by side
//! on a small CI machine, they slowed each other past the bound.

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const BOUND: Duration = Duration::from_secs(10);

/// Held for the whole of each test.
fn serial() -> MutexGuard<'static, ()> {
    static SERIAL: Mutex<()> = Mutex::new(());
    // A failed test poisons it, and the others still run.
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lint_within_bound(
    name: &str,
    source: &str,
    format: compyla::Format,
) -> Vec<compyla::Diagnostic> {
    let options = compyla::Options {
        format: Some(format),
        strict: false,
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint(source, &options, &compyla::no_assets).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed < BOUND, "{name} took {elapsed:?}");
    diagnostics
}

#[test]
fn repeated_headings_take_linear_time() {
    let _serial = serial();
    // Each duplicate's slug once searched its suffixes from -1.
    let source = "# a\n\n".repeat(40_000);
    lint_within_bound("duplicate headings", &source, compyla::Format::Docx);
}

#[test]
fn many_directive_options_take_linear_time() {
    let _serial = serial();
    let keys: Vec<String> = (0..80_000).map(|i| format!("k{i}")).collect();
    let source = format!("::: table {}\n| A |\n|---|\n| 1 |\n:::\n", keys.join(" "));
    lint_within_bound("many options", &source, compyla::Format::Xlsx);
}

#[test]
fn a_long_line_of_inline_nodes_takes_linear_time() {
    let _serial = serial();
    // Character columns were once recomputed from the start of the line for every node.
    let line: Vec<String> = (0..12_000).map(|i| format!("*é{i}*")).collect();
    let source = format!("{}\n", line.join(" "));
    lint_within_bound("long line", &source, compyla::Format::Docx);
}

#[test]
fn leading_notes_take_linear_time() {
    let _serial = serial();
    let source = format!(
        "# T\n\n{}{}",
        "::: notes\n:::\n".repeat(40_000),
        "::: layout Blank\n:::\n".repeat(40_000)
    );
    lint_within_bound("leading notes", &source, compyla::Format::Pptx);
}

#[test]
fn diagnostics_are_capped() {
    let _serial = serial();
    let source = format!(
        "::: columns {}\n| A |\n|---|\n| 1 |\n:::\n",
        "X,".repeat(100_000)
    );
    let diagnostics = lint_within_bound("many diagnostics", &source, compyla::Format::Xlsx);
    assert_eq!(diagnostics.len(), 1001);
    assert_eq!(diagnostics.last().unwrap().code, "diagnostics_truncated");
}

#[test]
fn a_huge_ir_position_doesnt_overflow() {
    let _serial = serial();
    let ir = serde_json::json!({
        "irVersion": "0.1",
        "meta": {},
        "body": [{
            "type": "table",
            "position": { "line": 4_294_967_295u64, "column": 1 },
            "header": [{ "runs": [{ "text": "A" }], "raw": "A" }],
            "align": [null],
            "rows": [[{ "runs": [{ "text": "=SUM(" }], "raw": "=SUM(" }]]
        }]
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
        ..Default::default()
    };
    let diagnostics = compyla::lint_ir(&ir.to_string(), &options, &compyla::no_assets).unwrap();
    assert!(diagnostics.iter().any(|d| d.code == "formula_error"));
}

#[test]
#[cfg(feature = "xlsx")]
fn a_blocked_convert_reports_its_error_past_the_cap() {
    let _serial = serial();
    // 1,100 warnings sort before the error, which the cap still keeps (§11.2).
    let source = format!(
        "# S\n\n{}| A |\n|---|\n| {} |\n",
        "### h\n\n".repeat(1_100),
        "x".repeat(32_800)
    );
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
        ..Default::default()
    };
    let result = compyla::convert(&source, &options, &compyla::no_assets).unwrap();
    assert!(result.bytes.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.code == "xlsx_limit_exceeded")
    );
    assert_eq!(result.diagnostics.len(), 1001);
    let lint = compyla::lint(&source, &options, &compyla::no_assets).unwrap();
    assert!(lint.iter().any(|d| d.severity == compyla::Severity::Error));
}

#[test]
#[cfg(feature = "xlsx")]
fn many_excel_tables_convert() {
    let _serial = serial();
    // Each Excel table is a package part: more than 65,535 parts need Zip64.
    let source = format!(
        "# S\n\n{}",
        "::: table\n| A |\n|---|\n:::\n\n".repeat(70_000)
    );
    let options = compyla::Options {
        format: Some(compyla::Format::Xlsx),
        strict: false,
        ..Default::default()
    };
    let start = Instant::now();
    let result = compyla::convert(&source, &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND * 3, "took {:?}", start.elapsed());
    let bytes = result.bytes.expect("converted");
    let archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert!(archive.len() > 70_000);
}

#[test]
#[cfg(feature = "pptx")]
fn a_picture_naming_one_link_in_many_attributes_converts() {
    let _serial = serial();
    // Every attribute of one blip names the same external link; each goes, in one linear pass
    // (§10.4, §12.1), not one search of the others per attribute.
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let attributes: String = (0..60_000).map(|i| format!(r#" r:l{i}="rIdX""#)).collect();
    let picture = format!(
        r#"<p:pic><p:nvPicPr><p:cNvPr id="90" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip{attributes}/></p:blipFill><p:spPr/></p:pic>"#
    );
    let link = format!(
        r#"<Relationship Id="rIdX" Type="{R}/image" Target="https://tracker.example/x.png" TargetMode="External"/></Relationships>"#
    );
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND)
        .unwrap()
        .into_iter()
        .map(|(name, data)| {
            let text = String::from_utf8_lossy(&data);
            let data = match name.as_str() {
                "ppt/slideMasters/slideMaster1.xml" => text
                    .replacen(
                        "</p:spTree></p:cSld>",
                        &format!("{picture}</p:spTree></p:cSld>"),
                        1,
                    )
                    .into_bytes(),
                "ppt/slideMasters/_rels/slideMaster1.xml.rels" => {
                    text.replacen("</Relationships>", &link, 1).into_bytes()
                }
                _ => data,
            };
            (name, data)
        })
        .collect();
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(compyla_ooxml::zip::write(&entries)),
        ..Default::default()
    };
    let start = Instant::now();
    let result = compyla::convert("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
    assert!(result.bytes.is_some());
    assert_eq!(
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == "template_content_removed")
            .count(),
        1
    );
}

#[test]
#[cfg(feature = "pptx")]
fn a_template_full_of_active_content_converts() {
    let _serial = serial();
    // Each linked picture and OLE object is found, removed and reported in one pass (§10.4); the
    // reports are capped like any others (§11.2).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let n = 8_000;
    let mut shapes = String::new();
    let mut rels = String::new();
    for i in 0..n {
        shapes.push_str(&format!(
            r#"<p:pic><p:nvPicPr><p:cNvPr id="{}" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:link="rIdL{i}"/></p:blipFill><p:spPr/></p:pic><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{}" name=""/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/presentationml/2006/ole"><p:oleObj r:id="rIdO{i}"><p:embed/></p:oleObj></a:graphicData></a:graphic></p:graphicFrame>"#,
            100 + 2 * i,
            101 + 2 * i
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rIdL{i}" Type="{R}/image" Target="https://tracker.example/{i}.png" TargetMode="External"/><Relationship Id="rIdO{i}" Type="{R}/oleObject" Target="../embeddings/o{i}.bin"/>"#
        ));
    }
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(BRAND)
        .unwrap()
        .into_iter()
        .map(|(name, data)| {
            let text = String::from_utf8_lossy(&data);
            let data = match name.as_str() {
                "ppt/slideMasters/slideMaster1.xml" => text
                    .replacen(
                        "</p:spTree></p:cSld>",
                        &format!("{shapes}</p:spTree></p:cSld>"),
                        1,
                    )
                    .into_bytes(),
                "ppt/slideMasters/_rels/slideMaster1.xml.rels" => text
                    .replacen("</Relationships>", &format!("{rels}</Relationships>"), 1)
                    .into_bytes(),
                _ => data,
            };
            (name, data)
        })
        .collect();
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(compyla_ooxml::zip::write(&entries)),
        ..Default::default()
    };
    let start = Instant::now();
    let result = compyla::convert(
        "# A\n\n---\n\n## B\n\nText.\n",
        &options,
        &compyla::no_assets,
    )
    .unwrap();
    assert!(start.elapsed() < BOUND, "took {:?}", start.elapsed());
    assert!(result.bytes.is_some());
    let codes: Vec<&str> = result.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes.len(), 1_001);
    // The template's two unresolved fonts sort first (§11.2), then removals fill the cap.
    assert_eq!(codes[..2], ["font_unresolved"; 2]);
    assert!(
        codes[2..1_000]
            .iter()
            .all(|&c| c == "template_content_removed")
    );
    assert_eq!(codes[1_000], "diagnostics_truncated");
}

/// A template's entries with `edit` applied to the named parts.
fn edited(package: &[u8], edit: impl Fn(&str, Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let entries: Vec<(String, Vec<u8>)> = compyla_ooxml::read::entries(package)
        .unwrap()
        .into_iter()
        .map(|(name, data)| {
            let data = edit(&name, data);
            (name, data)
        })
        .collect();
    compyla_ooxml::zip::write(&entries)
}

fn font_unresolved(diagnostics: &[compyla::Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.code == "font_unresolved")
        .count()
}

#[test]
fn a_hostile_embedded_font_is_unresolved_not_a_panic() {
    let _serial = serial();
    // lint/145's template, whose obfuscated Brand Serif now de-obfuscates to a collection claiming
    // billions of faces (§3, §12.1).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/145-docx-embedded-fonts/brand.dotx");
    let key: Vec<u8> = (0..16)
        .map(|i| {
            let hex = "1F2E3D4C5B6A47899A0BC1D2E3F40516";
            u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()
        })
        .rev()
        .collect();
    let mut forged = b"ttcf\x00\x01\x00\x00\xFF\xFF\xFF\xFF".to_vec();
    forged.extend(std::iter::repeat_n(0xFFu8, 1 << 16));
    for (i, byte) in forged.iter_mut().take(32).enumerate() {
        *byte ^= key[i % 16];
    }
    let template = edited(BRAND, |name, data| {
        if name == "word/fonts/font1.odttf" {
            forged.clone()
        } else {
            data
        }
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Docx),
        template: Some(template),
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
    // Brand Serif, refused, and the defaults' three.
    assert_eq!(font_unresolved(&diagnostics), 4, "{diagnostics:?}");
}

#[test]
fn a_font_resolver_returning_hostile_bytes_is_bounded() {
    let _serial = serial();
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    let huge = vec![0u8; compyla::FONT_SIZE_LIMIT + 1];
    let fonts = |family: &str, _style| match family {
        "Brand Display" => Some(huge.clone()),
        _ => Some(b"\x00\x01\x00\x00garbage".to_vec()),
    };
    let resolvers = compyla::Resolvers {
        assets: &compyla::no_assets,
        templates: None,
        fonts: Some(&fonts),
    };
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(BRAND.to_vec()),
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint("# A\n", &options, resolvers).unwrap();
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
    assert_eq!(font_unresolved(&diagnostics), 2, "{diagnostics:?}");
}

#[test]
fn a_template_naming_thousands_of_families_is_linear_and_capped() {
    let _serial = serial();
    // A master naming 50,000 distinct typefaces: each is checked once, in linear time, and the
    // diagnostics stop at the cap (§11.2).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    let runs: String = (0..50_000)
        .map(|i| {
            format!(r#"<a:r><a:rPr><a:latin typeface="Family {i}"/></a:rPr><a:t>x</a:t></a:r>"#)
        })
        .collect();
    let shape = format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="90" name="Many"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p>{runs}</a:p></p:txBody></p:sp>"#
    );
    let template = edited(BRAND, |name, data| {
        if name == "ppt/slideMasters/slideMaster1.xml" {
            String::from_utf8(data)
                .unwrap()
                .replacen(
                    "</p:spTree></p:cSld>",
                    &format!("{shape}</p:spTree></p:cSld>"),
                    1,
                )
                .into_bytes()
        } else {
            data
        }
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(template),
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
    assert_eq!(font_unresolved(&diagnostics), compyla_core::diagnostic::CAP);
    assert_eq!(diagnostics.last().unwrap().code, "diagnostics_truncated");
}

#[test]
fn a_template_embedding_thousands_of_fonts_is_linear() {
    let _serial = serial();
    // 30,000 embedded fonts and a master naming each of them in lower case: each family is looked
    // up once among the embedded, not compared with each, which took 24 s here (§3, §12.1).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    let embedded: String = (0..30_000)
        .map(|i| {
            format!(
                r#"<p:embeddedFont><p:font typeface="Embedded {i}"/><p:regular r:id="rId3"/></p:embeddedFont>"#
            )
        })
        .collect();
    let runs: String = (0..30_000)
        .map(|i| {
            format!(r#"<a:r><a:rPr><a:latin typeface="embedded {i}"/></a:rPr><a:t>x</a:t></a:r>"#)
        })
        .collect();
    let template = edited(BRAND, |name, data| {
        let text = String::from_utf8(data).unwrap();
        match name {
            "ppt/presentation.xml" => text.replacen("</p:embeddedFontLst>", &format!("{embedded}</p:embeddedFontLst>"), 1),
            "ppt/slideMasters/slideMaster1.xml" => text.replacen(
                "</p:spTree></p:cSld>",
                &format!(r#"<p:sp><p:nvSpPr><p:cNvPr id="90" name="Many"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p>{runs}</a:p></p:txBody></p:sp></p:spTree></p:cSld>"#),
                1,
            ),
            _ => text,
        }
        .into_bytes()
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(template),
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
    // Only the template's own Brand Display and Source Serif 4.
    assert_eq!(font_unresolved(&diagnostics), 2, "{diagnostics:?}");
}

#[test]
fn an_oversized_embedded_font_is_refused_by_its_size() {
    let _serial = serial();
    // lint/145's template, whose plain Brand Sans is now 65 MiB: runs of 32 bytes, which compress
    // within the template's limits (§12.1).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/145-docx-embedded-fonts/brand.dotx");
    let mut state = 1u32;
    let huge: Vec<u8> = (0..(65 << 20) / 32)
        .flat_map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            [(state >> 24) as u8; 32]
        })
        .collect();
    // Written with the fastest deflate: the engine's own writer takes seconds over 65 MiB in a
    // debug build.
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let fast = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(1));
    for (name, data) in compyla_ooxml::read::entries(BRAND).unwrap() {
        writer.start_file(name.as_str(), fast).unwrap();
        let data = if name == "word/fonts/font2.ttf" {
            &huge
        } else {
            &data
        };
        std::io::Write::write_all(&mut writer, data).unwrap();
    }
    let template = writer.finish().unwrap().into_inner();
    let options = compyla::Options {
        format: Some(compyla::Format::Docx),
        template: Some(template),
        ..Default::default()
    };
    let start = Instant::now();
    let diagnostics = compyla::lint("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
    let refused: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code == "font_unresolved" && d.message.contains("larger than 64 MiB"))
        .collect();
    assert_eq!(refused.len(), 1, "{diagnostics:?}");
    assert!(refused[0].message.contains("Brand Sans"));
    // Brand Sans, and the defaults' three.
    assert_eq!(font_unresolved(&diagnostics), 4, "{diagnostics:?}");
}

#[test]
fn a_layout_and_master_of_many_placeholders_are_read_in_linear_time() {
    let _serial = serial();
    // 12,000 date placeholders on the master and 12,000 content ones on a layout: each content
    // placeholder finds the master's it inherits from by its type and family at once (§8.6), not
    // by a search of every date placeholder, which took 24 s here.
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    let many = |kind: &str| -> String {
        (0..12_000)
            .map(|i| {
                format!(
                    r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name=""/><p:cNvSpPr/><p:nvPr><p:ph type="{kind}" idx="{}"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr marL="1"/></a:lstStyle><a:p/></p:txBody></p:sp>"#,
                    100 + i,
                    100 + i
                )
            })
            .collect()
    };
    let (dates, contents) = (many("dt"), many("obj"));
    let template = edited(BRAND, |name, data| {
        let text = String::from_utf8(data).unwrap();
        match name {
            "ppt/slideMasters/slideMaster1.xml" => {
                text.replacen("</p:spTree>", &format!("{dates}</p:spTree>"), 1)
            }
            "ppt/slideLayouts/slideLayout2.xml" => {
                text.replacen("</p:spTree>", &format!("{contents}</p:spTree>"), 1)
            }
            _ => text,
        }
        .into_bytes()
    });
    let options = compyla::Options {
        format: Some(compyla::Format::Pptx),
        template: Some(template),
        ..Default::default()
    };
    let start = Instant::now();
    compyla::lint("# A\n", &options, &compyla::no_assets).unwrap();
    assert!(start.elapsed() < BOUND, "{:?}", start.elapsed());
}

#[test]
fn masters_sharing_a_large_theme_read_it_once() {
    use compyla_core::validate::pptx::text::TemplateText;
    let _serial = serial();
    // 150 masters, each with a layout, all related to one theme padded with 250,000 elements: the
    // text model reads the theme once, not once a master (§8.6, §12.1).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    const MASTERS: usize = 150;
    let mut seed = 1u64;
    let padding: String = (0..250_000)
        .map(|_| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            format!(r#"<a:x v="{}"/>"#, seed >> 40)
        })
        .collect();
    let entries = compyla_ooxml::read::entries(BRAND).unwrap();
    let master = String::from_utf8(
        entries
            .iter()
            .find(|(n, _)| n == "ppt/slideMasters/slideMaster1.xml")
            .unwrap()
            .1
            .clone(),
    )
    .unwrap();
    let start = master.find("<p:sldLayoutIdLst>").unwrap();
    let end = master.find("</p:sldLayoutIdLst>").unwrap();
    let one_layout = format!(
        r#"{}<p:sldLayoutIdLst><p:sldLayoutId id="2147483900" r:id="rId1"/>{}"#,
        &master[..start],
        &master[end..]
    );
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const PML: &str = "application/vnd.openxmlformats-officedocument.presentationml";
    let mut ids = String::new();
    let mut rels = String::new();
    let mut types = String::new();
    let mut extra: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..MASTERS {
        ids.push_str(&format!(
            r#"<p:sldMasterId id="{}" r:id="rIdM{i}"/>"#,
            2_147_484_000 + i
        ));
        rels.push_str(&format!(
            r#"<Relationship Id="rIdM{i}" Type="{REL}/slideMaster" Target="slideMasters/many{i}.xml"/>"#
        ));
        types.push_str(&format!(
            r#"<Override PartName="/ppt/slideMasters/many{i}.xml" ContentType="{PML}.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/many{i}.xml" ContentType="{PML}.slideLayout+xml"/>"#
        ));
        extra.push((
            format!("ppt/slideMasters/many{i}.xml"),
            one_layout.clone().into_bytes(),
        ));
        extra.push((
            format!("ppt/slideMasters/_rels/many{i}.xml.rels"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{REL}/slideLayout" Target="../slideLayouts/many{i}.xml"/><Relationship Id="rId2" Type="{REL}/theme" Target="../theme/theme1.xml"/></Relationships>"#
            )
            .into_bytes(),
        ));
        extra.push((
            format!("ppt/slideLayouts/many{i}.xml"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="{REL}" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld name="Many {i}"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld></p:sldLayout>"#
            )
            .into_bytes(),
        ));
    }
    let mut all: Vec<(String, Vec<u8>)> = entries
        .into_iter()
        .map(|(name, data)| {
            let text = String::from_utf8(data).unwrap_or_default();
            let text = match name.as_str() {
                "ppt/theme/theme1.xml" => {
                    text.replacen("</a:theme>", &format!("{padding}</a:theme>"), 1)
                }
                "ppt/presentation.xml" => text.replacen(
                    "</p:sldMasterIdLst>",
                    &format!("{ids}</p:sldMasterIdLst>"),
                    1,
                ),
                "ppt/_rels/presentation.xml.rels" => {
                    text.replacen("</Relationships>", &format!("{rels}</Relationships>"), 1)
                }
                "[Content_Types].xml" => text.replacen("</Types>", &format!("{types}</Types>"), 1),
                _ => return (name, text.into_bytes()),
            };
            (name, text.into_bytes())
        })
        .collect();
    all.extend(extra);
    let bytes = compyla_ooxml::zip::write(&all);
    let template = compyla_ooxml::template::Template::read(&bytes).unwrap();
    let layouts = compyla_ooxml::layouts::read(&template).unwrap();
    assert!(layouts.len() > MASTERS);
    let model = compyla_ooxml::text::Model::new(&template, &layouts);
    let started = Instant::now();
    for layout in 0..layouts.len() {
        assert_eq!(model.title(layout).levels[0].run.latin, "Brand Display");
    }
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
    // One theme part, read once: counted, so a build of any speed would show a second read.
    assert_eq!(model.themes_read(), 1);
}

#[test]
fn many_slots_of_a_large_layout_are_read_in_linear_time() {
    use compyla_core::validate::pptx::text::TemplateText;
    let _serial = serial();
    // A layout of 30,000 content placeholders, each asked for: the layout is read once into what
    // each placeholder sets, so each slot is found at once, not by reading the layout again
    // (§8.6, §12.1).
    const BRAND: &[u8] =
        include_bytes!("../../../spec/conformance/lint/139-pptx-template-binding/brand.potx");
    const SLOTS: usize = 30_000;
    let contents: String = (0..SLOTS)
        .map(|i| {
            format!(
                r#"<p:sp><p:nvSpPr><p:cNvPr id="{}" name=""/><p:cNvSpPr/><p:nvPr><p:ph idx="{}"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:lstStyle/><a:p/></p:txBody></p:sp>"#,
                100 + i,
                100 + i
            )
        })
        .collect();
    let bytes = edited(BRAND, |name, data| {
        let text = String::from_utf8(data).unwrap();
        match name {
            "ppt/slideLayouts/slideLayout2.xml" => {
                text.replacen("</p:spTree>", &format!("{contents}</p:spTree>"), 1)
            }
            _ => text,
        }
        .into_bytes()
    });
    let template = compyla_ooxml::template::Template::read(&bytes).unwrap();
    let layouts = compyla_ooxml::layouts::read(&template).unwrap();
    let layout = layouts
        .iter()
        .position(|l| l.name == "Titel und Inhalt")
        .unwrap();
    let model = compyla_ooxml::text::Model::new(&template, &layouts);
    let started = Instant::now();
    for slot in 0..100 {
        assert!(model.content(layout, slot).is_some());
    }
    // The layout and its master, each read once.
    assert_eq!(model.parts_read(), 2);
    for slot in 100..SLOTS {
        assert!(model.content(layout, slot).is_some());
    }
    assert!(started.elapsed() < BOUND, "{:?}", started.elapsed());
}
