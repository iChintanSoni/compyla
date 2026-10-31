"""Generates the template packages of spec/conformance/inspect (and any other suite that needs a
template), so every binary fixture can be reviewed as source here. Standard library only:

    python3 -I spec/tools/make_templates.py

Each package is written with fixed timestamps, in a fixed entry order. The packages are synthetic,
written for these fixtures; none is copied from an Office application.
"""

import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "conformance"

CT = "http://schemas.openxmlformats.org/package/2006/content-types"
RELS = "http://schemas.openxmlformats.org/package/2006/relationships"
OFFICE = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
P = "http://schemas.openxmlformats.org/presentationml/2006/main"
W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
X = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'


def write(path, entries, method=zipfile.ZIP_DEFLATED):
    path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(path, "w") as archive:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = method
            archive.writestr(info, data.encode() if isinstance(data, str) else data)


def content_types(overrides, defaults=()):
    items = [f'<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>',
             f'<Default Extension="xml" ContentType="application/xml"/>']
    items += [f'<Default Extension="{e}" ContentType="{t}"/>' for e, t in defaults]
    items += [f'<Override PartName="/{p}" ContentType="{t}"/>' for p, t in overrides]
    return f'{DECL}<Types xmlns="{CT}">{"".join(items)}</Types>'


def rels(*items):
    body = "".join(
        f'<Relationship Id="{i}" Type="{OFFICE}/{t}" Target="{target}"{" TargetMode=\"External\"" if ext else ""}/>'
        for i, t, target, ext in items
    )
    return f'{DECL}<Relationships xmlns="{RELS}">{body}</Relationships>'


def package_rels(main):
    return rels(("rId1", "officeDocument", main, False))


def theme(major, minor, odd=False):
    colors = {"dk1": '<a:sysClr val="windowText" lastClr="000000"/>', "lt1": '<a:sysClr val="window" lastClr="FFFFFF"/>',
              "dk2": '<a:srgbClr val="1f2a44"/>', "lt2": '<a:srgbClr val="E8ECF3"/>',
              **{f"accent{i}": f'<a:srgbClr val="{c}"/>' for i, c in enumerate(["2F5BEA", "00A88F", "F2A900", "D64545", "7A4FD1", "4A5568"], 1)},
              "hlink": '<a:srgbClr val="0563C1"/>', "folHlink": '<a:srgbClr val="954F72"/>'}
    scheme = "".join(f"<a:{k}>{v}</a:{k}>" for k, v in colors.items())
    if odd:  # a slot outside the theme's twelve, which a report must not echo
        scheme += '<a:extraSlot><a:srgbClr val="123456"/></a:extraSlot>'
    fonts = f'<a:majorFont><a:latin typeface="{major}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>' \
            f'<a:minorFont><a:latin typeface="{minor}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>'
    return (f'{DECL}<a:theme xmlns:a="{A}" name="Fixture"><a:themeElements>'
            f'<a:clrScheme name="Fixture">{scheme}</a:clrScheme>'
            f'<a:fontScheme name="Fixture">{fonts}</a:fontScheme>'
            f'<a:fmtScheme name="Fixture"><a:fillStyleLst/><a:lnStyleLst/><a:effectStyleLst/><a:bgFillStyleLst/></a:fmtScheme>'
            f'</a:themeElements></a:theme>')


# ---------------------------------------------------------------- pptx

PML = "application/vnd.openxmlformats-officedocument.presentationml"


def placeholder(id, name, kind=None, idx=None):
    ph = "<p:ph" + (f' type="{kind}"' if kind else "") + (f' idx="{idx}"' if idx is not None else "") + "/>"
    return (f'<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr>'
            f'<p:spPr/><p:txBody><a:bodyPr/><a:p/></p:txBody></p:sp>')


def tree(*shapes):
    return ('<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>'
            + "".join(shapes) + "</p:spTree>")


def layout(name, kind, *shapes):
    return (f'{DECL}<p:sldLayout xmlns:a="{A}" xmlns:r="{OFFICE}" xmlns:p="{P}" type="{kind}">'
            f'<p:cSld name="{name}">{tree(*shapes)}</p:cSld></p:sldLayout>')


def pptx(path, main_type="presentation", extra=(), repeat_ids=1, odd=False):
    layouts = [
        ("Title Slide", "title", placeholder(2, "Title 1", "ctrTitle"),
         placeholder(3, "Subtitle 2", "notAType" if odd else "subTitle", 1)),
        ("Titel und Inhalt", "obj", placeholder(2, "Titel 1", "title"), placeholder(3, "Inhaltsplatzhalter 2", None, 1),
         placeholder(4, "Datumsplatzhalter 3", "dt", 10)),
        ("Blank", "blank"),
    ]
    master = (f'{DECL}<p:sldMaster xmlns:a="{A}" xmlns:r="{OFFICE}" xmlns:p="{P}">'
              f'<p:cSld>{tree(placeholder(2, "Title Placeholder 1", "title"))}</p:cSld>'
              '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
              '<p:sldLayoutIdLst>' + "".join(f'<p:sldLayoutId id="{2147483649 + i}" r:id="rId{i + 1}"/>' for i in range(len(layouts))) * repeat_ids
              + '</p:sldLayoutIdLst><p:txStyles><p:titleStyle><a:lvl1pPr><a:defRPr><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>'
              '<p:bodyStyle><a:lvl1pPr><a:defRPr><a:latin typeface="Source Serif 4"/></a:defRPr></a:lvl1pPr></p:bodyStyle><p:otherStyle/></p:txStyles></p:sldMaster>')
    master_rels = rels(*[(f"rId{i + 1}", "slideLayout", f"../slideLayouts/slideLayout{i + 1}.xml", False) for i in range(len(layouts))],
                       (f"rId{len(layouts) + 1}", "theme", "../theme/theme1.xml", False))
    presentation = (f'{DECL}<p:presentation xmlns:a="{A}" xmlns:r="{OFFICE}" xmlns:p="{P}">'
                    '<p:sldMasterIdLst>' + '<p:sldMasterId id="2147483648" r:id="rId1"/>' * repeat_ids + '</p:sldMasterIdLst>'
                    '<p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/>'
                    '<p:embeddedFontLst><p:embeddedFont><p:font typeface="Brand Sans"/><p:regular r:id="rId3"/></p:embeddedFont></p:embeddedFontLst>'
                    '</p:presentation>')
    entries = [
        ("[Content_Types].xml", content_types(
            [("ppt/presentation.xml", f"{PML}.{main_type}.main+xml"),
             ("ppt/slideMasters/slideMaster1.xml", f"{PML}.slideMaster+xml"),
             ("ppt/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml")]
            + [(f"ppt/slideLayouts/slideLayout{i + 1}.xml", f"{PML}.slideLayout+xml") for i in range(len(layouts))],
            defaults=[("fntdata", "application/x-fontdata")])),
        ("_rels/.rels", package_rels("ppt/presentation.xml")),
        ("ppt/presentation.xml", presentation),
        ("ppt/_rels/presentation.xml.rels", rels(("rId1", "slideMaster", "slideMasters/slideMaster1.xml", False),
                                                  ("rId2", "theme", "theme/theme1.xml", False),
                                                  ("rId3", "font", "fonts/font1.fntdata", False))),
        ("ppt/slideMasters/slideMaster1.xml", master),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", master_rels),
        *[(f"ppt/slideLayouts/slideLayout{i + 1}.xml", layout(*spec)) for i, spec in enumerate(layouts)],
        *[(f"ppt/slideLayouts/_rels/slideLayout{i + 1}.xml.rels", rels(("rId1", "slideMaster", "../slideMasters/slideMaster1.xml", False)))
          for i in range(len(layouts))],
        ("ppt/theme/theme1.xml", theme("Brand Display", "Brand Sans", odd)),
        ("ppt/fonts/font1.fntdata", b"\x00" * 64),
        *extra,
    ]
    write(path, entries)


# ---------------------------------------------------------------- docx

WML = "application/vnd.openxmlformats-officedocument.wordprocessingml"


def docx(path, styles=True, theme_part=True, document_body=None, main_type="template", extra=()):
    style_xml = (f'{DECL}<w:styles xmlns:w="{W}">'
                 '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi"/></w:rPr></w:rPrDefault></w:docDefaults>'
                 '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
                 '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/></w:style>'
                 '<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code Block"/><w:rPr><w:rFonts w:ascii="Brand Mono" w:hAnsi="Brand Mono"/></w:rPr></w:style>'
                 '<w:style w:type="character" w:styleId="Strong"><w:name w:val="Strong"/></w:style>'
                 '<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/></w:style>'
                 '<w:style w:type="numbering" w:styleId="NoList"><w:name w:val="No List"/></w:style>'
                 '</w:styles>')
    font_table = (f'{DECL}<w:fonts xmlns:w="{W}" xmlns:r="{OFFICE}">'
                  '<w:font w:name="Brand Serif"><w:embedRegular r:id="rId1"/></w:font><w:font w:name="Brand Mono"/></w:fonts>')
    body = document_body if document_body is not None else (
        '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1134" w:bottom="1440" w:left="1134" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>')
    document = f'{DECL}<w:document xmlns:w="{W}" xmlns:r="{OFFICE}"><w:body>{body}</w:body></w:document>'
    overrides = [("word/document.xml", f"{WML}.{main_type}.main+xml"), ("word/fontTable.xml", f"{WML}.fontTable+xml")]
    doc_rels = [("rId1", "fontTable", "fontTable.xml", False)]
    entries = [("word/document.xml", document), ("word/fontTable.xml", font_table)]
    if styles:
        overrides.append(("word/styles.xml", f"{WML}.styles+xml"))
        doc_rels.append(("rId2", "styles", "styles.xml", False))
        entries.append(("word/styles.xml", style_xml))
    if theme_part:
        overrides.append(("word/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"))
        doc_rels.append(("rId3", "theme", "theme/theme1.xml", False))
        entries.append(("word/theme/theme1.xml", theme("Brand Serif", "Brand Sans")))
    write(path, [("[Content_Types].xml", content_types(overrides + [(n, t) for n, t, _ in extra])),
                 ("_rels/.rels", package_rels("word/document.xml")),
                 ("word/_rels/document.xml.rels", rels(*doc_rels)), *entries, *[(n, d) for n, _, d in extra]])


# ---------------------------------------------------------------- xlsx

def xltx(path):
    SML = "application/vnd.openxmlformats-officedocument.spreadsheetml"
    workbook = f'{DECL}<workbook xmlns="{X}" xmlns:r="{OFFICE}"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>'
    styles = f'{DECL}<styleSheet xmlns="{X}"><fonts count="1"><font><sz val="11"/><name val="Brand Grid"/></font></fonts></styleSheet>'
    sheet = f'{DECL}<worksheet xmlns="{X}"><sheetData/></worksheet>'
    write(path, [
        ("[Content_Types].xml", content_types([("xl/workbook.xml", f"{SML}.template.main+xml"),
                                               ("xl/worksheets/sheet1.xml", f"{SML}.worksheet+xml"),
                                               ("xl/styles.xml", f"{SML}.styles+xml"),
                                               ("xl/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml")])),
        ("_rels/.rels", package_rels("xl/workbook.xml")),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", rels(("rId1", "worksheet", "worksheets/sheet1.xml", False),
                                            ("rId2", "styles", "styles.xml", False),
                                            ("rId3", "theme", "theme/theme1.xml", False))),
        ("xl/worksheets/sheet1.xml", sheet),
        ("xl/styles.xml", styles),
        ("xl/theme/theme1.xml", theme("Brand Display", "Brand Sans")),
    ])


def main():
    inspect = ROOT / "inspect"
    pptx(inspect / "101-pptx-layouts/template.potx", main_type="template")
    docx(inspect / "102-docx-styles-page/template.dotx")
    xltx(inspect / "103-xlsx-theme/template.xltx")
    docx(inspect / "104-docx-no-theme-no-styles/template.docx", styles=False, theme_part=False, document_body="", main_type="document")
    pptx(inspect / "105-invalid-macro/template.pptm", extra=[("ppt/vbaProject.bin", b"\x00" * 32)])
    write(inspect / "106-invalid-dtd/template.docx", [
        ("[Content_Types].xml", content_types([("word/document.xml", f"{WML}.document.main+xml")])),
        ("_rels/.rels", package_rels("word/document.xml")),
        ("word/document.xml", f'{DECL}<!DOCTYPE w:document [<!ENTITY x "x">]><w:document xmlns:w="{W}"><w:body/></w:document>'),
    ])
    write(inspect / "107-invalid-path/template.docx", [
        ("[Content_Types].xml", content_types([("word/document.xml", f"{WML}.document.main+xml")])),
        ("_rels/.rels", package_rels("word/document.xml")),
        ("word/document.xml", f'{DECL}<w:document xmlns:w="{W}"><w:body/></w:document>'),
        ("../outside.xml", "<x/>"),
    ])
    write(inspect / "108-invalid-bomb/template.docx", [
        ("[Content_Types].xml", content_types([("word/document.xml", f"{WML}.document.main+xml")])),
        ("_rels/.rels", package_rels("word/document.xml")),
        ("word/document.xml", f'{DECL}<w:document xmlns:w="{W}"><w:body/></w:document>'),
        ("word/padding.xml", "<x>" + " " * (8 * 1024 * 1024) + "</x>"),
    ])
    (inspect / "109-invalid-not-a-package").mkdir(parents=True, exist_ok=True)
    (inspect / "109-invalid-not-a-package/template.docx").write_bytes(b"This is plain text, not a package.\n")
    (inspect / "110-invalid-encrypted").mkdir(parents=True, exist_ok=True)
    (inspect / "110-invalid-encrypted/template.docx").write_bytes(bytes.fromhex("D0CF11E0A1B11AE1") + b"\x00" * 504)
    # A DTD in a UTF-16 part, where a byte search for "<!DOCTYPE" finds nothing.
    hidden = '<?xml version="1.0" encoding="UTF-16"?><!DOCTYPE x [<!ENTITY e "e">]><x>&e;</x>'
    docx(inspect / "111-invalid-utf16-dtd/template.docx", main_type="document",
         extra=[("customXml/item1.xml", "application/xml", b"\xff\xfe" + hidden.encode("utf-16-le"))])
    # A VBA project behind a lowercase content type and an unusual name.
    docx(inspect / "112-invalid-vba-content-type/template.docx", main_type="document",
         extra=[("word/project.bin", "application/vnd.ms-office.vbaproject", b"\x00" * 32)])
    # Masters and layouts listed many times over: each is read and reported once. Also a color slot
    # outside the twelve, and a placeholder type outside ST_PlaceholderType.
    pptx(inspect / "113-pptx-repeated-ids/template.pptx", repeat_ids=500, odd=True)
    # A body with more elements than a part's tree may have: the final section is still found. The
    # padding, which doesn't compress, keeps the package under 100:1. A zero page width is ignored.
    body = "<w:p/>" * 600_000 + '<w:sectPr><w:pgSz w:w="0" w:h="16838"/></w:sectPr>'
    seed, padding = 12345, bytearray()
    for _ in range(120_000):
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        padding.append(seed >> 16 & 0xFF)
    docx(inspect / "114-docx-large-body/template.docx", main_type="document", document_body=body,
         extra=[("word/media/padding.bin", "application/octet-stream", bytes(padding))])

    # Supplied templates for lint and convert (§8.5, §10.4): a localized layout, bound by name or
    # by type, and house layouts the template lacks; then templates of the wrong kind, and one
    # without a theme or styles.
    pptx(ROOT / "lint/139-pptx-template-binding/brand.potx", main_type="template")
    pptx(ROOT / "render/119-pptx-supplied-template/brand.potx", main_type="template")
    docx(ROOT / "lint/141-pptx-template-of-another-kind/brand.dotx")
    xltx(ROOT / "lint/142-xlsx-workbook-template/brand.xltx")
    docx(ROOT / "lint/143-docx-template-without-theme/brand.docx", styles=False, theme_part=False,
         document_body="", main_type="document")


if __name__ == "__main__":
    main()
