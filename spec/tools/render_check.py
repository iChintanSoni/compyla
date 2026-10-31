"""Render conformance checker (spec/conformance/README.md, "Render suite").

Extracts the normative projection from a rendered docx, pptx, xlsx or pdf file, and compares it
with a fixture's expected output.

    render_check.py project <format> <file>                print the projection as JSON
    render_check.py check   <format> <expected.json> <file>  compare; exit 1 on mismatch

Run through uv, which supplies the pinned dependencies:
    uv run --no-project --with lxml==6.0.2 --with pypdf==6.1.1 python -I spec/tools/render_check.py ...
"""

import json
import re
import sys
import zipfile
from posixpath import dirname, join, normpath

from lxml import etree

NS = {
    "w": "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "x": "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "rel": "http://schemas.openxmlformats.org/package/2006/relationships",
    "cp": "http://schemas.openxmlformats.org/package/2006/metadata/core-properties",
    "dc": "http://purl.org/dc/elements/1.1/",
}

# Built-in spreadsheet number formats that appear without a <numFmt> record.
BUILTIN_NUMFMTS = {0: "General", 1: "0", 2: "0.00", 3: "#,##0", 4: "#,##0.00", 9: "0%", 10: "0.00%", 14: "mm-dd-yy", 49: "@"}


def xml(package, part):
    return etree.fromstring(package.read(part))


def rels(package, part):
    """Relationship id → (target part or URL, external?) for a part."""
    path = join(dirname(part), "_rels", part.rsplit("/", 1)[-1] + ".rels")
    if path not in package.namelist():
        return {}
    out = {}
    for rel in xml(package, path).findall("rel:Relationship", NS):
        external = rel.get("TargetMode") == "External"
        target = rel.get("Target")
        out[rel.get("Id")] = (target if external else normpath(join(dirname(part), target)), external)
    return out


def core_properties(package):
    if "docProps/core.xml" not in package.namelist():
        return {}
    core = xml(package, "docProps/core.xml")
    out = {}
    for key, path in (("title", "dc:title"), ("creator", "dc:creator")):
        node = core.find(path, NS)
        if node is not None and node.text:
            out[key] = node.text
    return out


# ---------------------------------------------------------------- docx


def run_text(element):
    """Text of the runs under an element: w:t, w:tab and line-break w:br inside w:r only, so tab-stop
    definitions in w:pPr/w:tabs never count."""
    parts = []
    for run in element.iter(f"{{{NS['w']}}}r"):
        for node in run:
            tag = etree.QName(node).localname
            if tag == "t":
                parts.append(node.text or "")
            elif tag == "tab":
                parts.append("\t")
            elif tag == "br" and node.get(f"{{{NS['w']}}}type") not in ("page", "column"):
                parts.append("\n")
    return "".join(parts)


def project_docx(package):
    document = xml(package, "word/document.xml")
    styles = xml(package, "word/styles.xml") if "word/styles.xml" in package.namelist() else None
    names, default = {}, "normal"
    if styles is not None:
        for style in styles.findall("w:style", NS):
            name = style.find("w:name", NS)
            if name is not None:
                names[style.get(f"{{{NS['w']}}}styleId")] = name.get(f"{{{NS['w']}}}val").lower()
            if style.get(f"{{{NS['w']}}}type") == "paragraph" and style.get(f"{{{NS['w']}}}default") == "1" and name is not None:
                default = name.get(f"{{{NS['w']}}}val").lower()
    links = rels(package, "word/document.xml")
    body = document.find("w:body", NS)

    paragraphs, rules = [], 0
    for paragraph in body.findall("w:p", NS):
        text = run_text(paragraph)
        style_ref = paragraph.find("w:pPr/w:pStyle", NS)
        style = names.get(style_ref.get(f"{{{NS['w']}}}val"), style_ref.get(f"{{{NS['w']}}}val").lower()) if style_ref is not None else default
        if text.strip():
            paragraphs.append({"style": style, "text": text})
        elif paragraph.find("w:pPr/w:pBdr/w:bottom", NS) is not None:
            rules += 1

    tables = []
    for table in body.findall("w:tbl", NS):
        tables.append([
            ["\n".join(run_text(p) for p in cell.findall("w:p", NS)) for cell in row.findall("w:tc", NS)]
            for row in table.findall("w:tr", NS)
        ])

    hyperlinks = []
    for link in document.iter(f"{{{NS['w']}}}hyperlink"):
        rid = link.get(f"{{{NS['r']}}}id")
        target = links[rid][0] if rid in links else "#" + (link.get(f"{{{NS['w']}}}anchor") or "")
        hyperlinks.append({"text": run_text(link), "target": target})

    page_breaks = len(document.xpath("//w:br[@w:type='page']", namespaces=NS)) + len(document.xpath("//w:pageBreakBefore", namespaces=NS))
    return {
        "paragraphs": paragraphs,
        "tables": tables,
        "hyperlinks": hyperlinks,
        "pageBreaks": page_breaks,
        "horizontalRules": rules,
        "properties": core_properties(package),
    }


# ---------------------------------------------------------------- pptx


def shape_paragraphs(shape):
    """Non-empty paragraph texts; a:br inside a paragraph is a line break."""
    out = []
    for paragraph in shape.iter(f"{{{NS['a']}}}p"):
        parts = []
        for node in paragraph:
            tag = etree.QName(node).localname
            if tag in ("r", "fld"):
                parts.extend(t.text or "" for t in node.iter(f"{{{NS['a']}}}t"))
            elif tag == "br":
                parts.append("\n")
        text = "".join(parts)
        if text.strip():
            out.append(text)
    return out


def project_pptx(package):
    presentation = xml(package, "ppt/presentation.xml")
    pres_rels = rels(package, "ppt/presentation.xml")
    slides = []
    for slide_id in presentation.findall("p:sldIdLst/p:sldId", NS):
        part = pres_rels[slide_id.get(f"{{{NS['r']}}}id")][0]
        slide = xml(package, part)
        slide_rels = rels(package, part)
        layout, notes = None, None
        for target, external in slide_rels.values():
            if external:
                continue
            if "/slideLayouts/" in target:
                layout = xml(package, target).find("p:cSld", NS).get("name")
            if "/notesSlides/" in target:
                notes_xml = xml(package, target)
                body = [s for s in notes_xml.iter(f"{{{NS['p']}}}sp") if s.find(".//p:ph[@type='body']", NS) is not None]
                text = "\n".join(t for s in body for t in shape_paragraphs(s))
                notes = text or None
        title, texts = None, []
        # Shapes and graphic frames (tables) in shape-tree order, including inside groups.
        for shape in slide.iter(f"{{{NS['p']}}}sp", f"{{{NS['p']}}}graphicFrame"):
            placeholder = shape.find(".//p:nvPr/p:ph", NS)
            kind = placeholder.get("type") if placeholder is not None else None
            lines = shape_paragraphs(shape)
            if kind in ("title", "ctrTitle"):
                title = "\n".join(lines) or None
            elif kind not in ("sldNum", "dt", "ftr"):
                texts.extend(lines)
        slides.append({"layout": layout, "title": title, "texts": texts, "notes": notes})
    return {"slides": slides, "properties": core_properties(package)}


# ---------------------------------------------------------------- xlsx


def project_xlsx(package):
    workbook = xml(package, "xl/workbook.xml")
    wb_rels = rels(package, "xl/workbook.xml")
    shared = []
    if "xl/sharedStrings.xml" in package.namelist():
        for item in xml(package, "xl/sharedStrings.xml").findall("x:si", NS):
            shared.append("".join(t.text or "" for t in item.iter(f"{{{NS['x']}}}t")))
    formats = []
    if "xl/styles.xml" in package.namelist():
        styles = xml(package, "xl/styles.xml")
        custom = {int(f.get("numFmtId")): f.get("formatCode") for f in styles.findall("x:numFmts/x:numFmt", NS)}
        for xf in styles.findall("x:cellXfs/x:xf", NS):
            fmt_id = int(xf.get("numFmtId", "0"))
            formats.append(custom.get(fmt_id, BUILTIN_NUMFMTS.get(fmt_id, str(fmt_id))))

    sheets, tables = [], []
    for sheet in workbook.findall("x:sheets/x:sheet", NS):
        part = wb_rels[sheet.get(f"{{{NS['r']}}}id")][0]
        data = xml(package, part)
        cells = {}
        for cell in data.iter(f"{{{NS['x']}}}c"):
            kind = cell.get("t", "n")
            entry = {}
            formula = cell.find("x:f", NS)
            value = cell.find("x:v", NS)
            if formula is not None:
                entry["formula"] = formula.text or ""
            elif kind == "s" and value is not None:
                entry["value"], entry["type"] = shared[int(value.text)], "s"
            elif kind == "inlineStr":
                entry["value"], entry["type"] = "".join(t.text or "" for t in cell.iter(f"{{{NS['x']}}}t")), "s"
            elif kind == "str" and value is not None:
                entry["value"], entry["type"] = value.text, "s"
            elif kind == "b" and value is not None:
                entry["value"], entry["type"] = value.text == "1", "b"
            elif value is not None:
                number = float(value.text)
                entry["value"], entry["type"] = (int(number) if number.is_integer() else number), "n"
            else:
                continue
            style = int(cell.get("s", "0"))
            entry["numFmt"] = formats[style] if style < len(formats) else "General"
            cells[cell.get("r")] = entry
        sheets.append({"name": sheet.get("name"), "cells": cells})
        for target, external in rels(package, part).values():
            if not external and "/tables/" in target:
                table = xml(package, target)
                style = table.find("x:tableStyleInfo", NS)
                tables.append({"name": table.get("name"), "ref": table.get("ref"), "style": style.get("name") if style is not None else None})
    return {"sheets": sheets, "tables": tables, "properties": core_properties(package)}


# ---------------------------------------------------------------- pdf


def project_pdf(path):
    from pypdf import PdfReader

    reader = PdfReader(path)
    text = " ".join(" ".join((page.extract_text() or "").split()) for page in reader.pages)

    def flatten(items):
        for item in items:
            if isinstance(item, list):
                yield from flatten(item)
            else:
                yield str(item.title)

    links = []
    for page in reader.pages:
        for annot in page.get("/Annots") or []:
            action = annot.get_object().get("/A")
            action = action.get_object() if action is not None else None
            if action and action.get("/URI"):
                uri = str(action["/URI"].get_object() if hasattr(action["/URI"], "get_object") else action["/URI"])
                if not links or links[-1] != uri:  # one link split across lines is several annotations
                    links.append(uri)
    box = reader.pages[0].mediabox if reader.pages else None
    meta = reader.metadata or {}
    return {
        "pages": len(reader.pages),
        "pageSize": [round(float(box.width), 2), round(float(box.height), 2)] if box else None,
        "text": text,
        "outline": list(flatten(reader.outline)),
        "links": links,
        "metadata": {k: str(v.get_object() if hasattr(v, "get_object") else v) for k, v in (("title", meta.get("/Title")), ("author", meta.get("/Author"))) if v},
    }


# ---------------------------------------------------------------- package assertions


def check_package(package, spec, problems):
    names = set(package.namelist())
    for part in spec.get("partsPresent", []):
        if part not in names:
            problems.append(f"package: missing part {part}")
    for pattern in spec.get("partsAbsent", []):
        hits = [n for n in names if re.fullmatch(pattern, n)]
        if hits:
            problems.append(f"package: forbidden part(s) {hits}")
    for assertion in spec.get("xpath", []):
        got = xml(package, assertion["part"]).xpath(assertion["expr"], namespaces=NS)
        if isinstance(got, float) and got.is_integer():
            got = int(got)
        if got != assertion["equals"]:
            problems.append(f"package: xpath {assertion['expr']} on {assertion['part']} = {got!r}, want {assertion['equals']!r}")


# ---------------------------------------------------------------- matching (README, "Matching")


def subsequence(want, got):
    it = iter(got)
    return all(any(item == g for g in it) for item in want)


def compare(fmt, want, got, problems):
    for key, expected in want.items():
        if key == "package":
            continue
        actual = got.get(key)
        if fmt == "docx" and key == "paragraphs":
            ok = subsequence(expected, actual)
        elif fmt == "pptx" and key == "slides":
            ok = len(expected) == len(actual) and all(
                all(e.get(f) == a.get(f) for f in ("layout", "title", "notes") if f in e) and subsequence(e.get("texts", []), a["texts"])
                for e, a in zip(expected, actual)
            )
        elif fmt == "xlsx" and key == "sheets":
            ok = [s["name"] for s in expected] == [s["name"] for s in actual] and all(
                all(addr in a["cells"] and all(a["cells"][addr].get(f) == v for f, v in fields.items()) for addr, fields in e.get("cells", {}).items())
                for e, a in zip(expected, actual)
            )
        elif fmt == "pdf" and key == "text":
            ok = subsequence_text(expected, actual)
        elif fmt == "pdf" and key == "pageSize":
            ok = actual is not None and all(abs(a - e) <= 0.5 for a, e in zip(actual, expected))
        else:
            ok = expected == actual
        if not ok:
            problems.append(f"{key}: got {json.dumps(actual, ensure_ascii=False)}, want {json.dumps(expected, ensure_ascii=False)}")


def subsequence_text(fragments, text):
    position = 0
    for fragment in fragments:
        normalized = " ".join(fragment.split())
        found = text.find(normalized, position)
        if found < 0:
            return False
        position = found + len(normalized)
    return True


def project(fmt, path):
    if fmt == "pdf":
        return project_pdf(path)
    with zipfile.ZipFile(path) as package:
        return {"docx": project_docx, "pptx": project_pptx, "xlsx": project_xlsx}[fmt](package)


def main(argv):
    if argv[1] == "project":
        print(json.dumps(project(argv[2], argv[3]), indent=2, ensure_ascii=False))
        return 0
    fmt, expected_path, path = argv[2], argv[3], argv[4]
    want = json.load(open(expected_path, encoding="utf-8"))["output"]
    problems = []
    compare(fmt, want, project(fmt, path), problems)
    if "package" in want and fmt != "pdf":
        with zipfile.ZipFile(path) as package:
            check_package(package, want["package"], problems)
    for problem in problems:
        print(problem)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
