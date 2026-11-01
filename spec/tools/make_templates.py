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


EXTERNAL = ' TargetMode="External"'


def rels(*items):
    body = "".join(
        f'<Relationship Id="{i}" Type="{OFFICE}/{t}" Target="{target}"{EXTERNAL if ext else ""}/>'
        for i, t, target, ext in items
    )
    return f'{DECL}<Relationships xmlns="{RELS}">{body}</Relationships>'


def package_rels(main):
    return rels(("rId1", "officeDocument", main, False))


def add_rels(parts, name, *items):
    """Adds relationships to `parts[name]`, creating it if needed; a type is a URI or the last
    segment of an Office one."""
    body = "".join(
        f'<Relationship Id="{i}" Type="{t if t.startswith("http") else OFFICE + "/" + t}" Target="{target}"{EXTERNAL if ext else ""}/>'
        for i, t, target, ext in items)
    parts[name] = parts.get(name, rels()).replace("</Relationships>", body + "</Relationships>")


def theme(major, minor, odd=False, accent1="2F5BEA"):
    colors = {"dk1": '<a:sysClr val="windowText" lastClr="000000"/>', "lt1": '<a:sysClr val="window" lastClr="FFFFFF"/>',
              "dk2": '<a:srgbClr val="1f2a44"/>', "lt2": '<a:srgbClr val="E8ECF3"/>',
              **{f"accent{i}": f'<a:srgbClr val="{c}"/>' for i, c in enumerate([accent1, "00A88F", "F2A900", "D64545", "7A4FD1", "4A5568"], 1)},
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


def pptx(path, main_type="presentation", extra=(), repeat_ids=1, odd=False,
         fonts=("Brand Display", "Brand Sans"), accent1="2F5BEA", patch=None):
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
        ("ppt/theme/theme1.xml", theme(*fonts, odd, accent1)),
        ("ppt/fonts/font1.fntdata", b"\x00" * 64),
        *extra,
    ]
    write(path, patch(entries) if patch else entries)


def active_content(entries):
    """Active and external content in the parts a deck keeps (§10.4, C58, C59): on the master, a
    linked picture, a hyperlink, two hyperlinks §4.5 refuses (one behind a leading space), an action
    that runs a program, and a picture linked through a hyperlink relationship; on a layout, an
    embedded OLE object (with its alternate and preview), a linked one, an ActiveX control, an
    Office add-in, an animation of the embedded object and a legacy VML drawing; on another layout,
    an action that runs a macro; and external relationships nothing refers to, from the
    presentation (one with its target mode in lower case) and the theme."""
    parts = dict(entries)
    mc = "http://schemas.openxmlformats.org/markup-compatibility/2006"
    ole_uri = "http://schemas.openxmlformats.org/presentationml/2006/ole"

    def frame(id, ole):
        return (f'<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="{id}" name="Object {id}"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>'
                '<p:xfrm><a:off x="838200" y="4000000"/><a:ext cx="1270000" cy="952500"/></p:xfrm>'
                f'<a:graphic><a:graphicData uri="{ole_uri}">{ole}</a:graphicData></a:graphic></p:graphicFrame>')

    picture = ('<p:pic><p:nvPicPr><p:cNvPr id="10" name="Tracker"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>'
               '<p:blipFill><a:blip r:link="rId20"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
               '<p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="12700" cy="12700"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>')
    def linking(id, name, hlink):
        return (f'<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}">{hlink}</p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr>'
                f'<p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>{name}</a:t></a:r></a:p></p:txBody></p:sp>')

    link = linking(11, "Site", '<a:hlinkClick r:id="rId21"/>')
    # A hyperlink §4.5 refuses, an action that runs a program, and a picture linked through a
    # relationship of the hyperlink type that no hyperlink refers to.
    refused = linking(13, "Help", '<a:hlinkClick r:id="rId22"/>')
    program = linking(14, "Run", '<a:hlinkClick r:id="rId23" action="ppaction://program"/>')
    spoof = picture.replace('id="10" name="Tracker"', 'id="12" name="Spoof"').replace('r:link="rId20"', 'r:link="rId24"')
    # A refused destination behind a leading space, as a browser reads it (§4.5, C59).
    spaced = linking(15, "Space", '<a:hlinkClick r:id="rId25"/>')
    master = "ppt/slideMasters/slideMaster1.xml"
    parts[master] = parts[master].replace("</p:spTree></p:cSld>", picture + link + refused + program + spoof + spaced + "</p:spTree></p:cSld>", 1)
    add_rels(parts, "ppt/slideMasters/_rels/slideMaster1.xml.rels",
             ("rId20", "image", "https://tracker.example/pixel.png", True),
             ("rId21", "hyperlink", "https://example.com/", True),
             ("rId22", "hyperlink", "ms-msdt:/id PCWDiagnostic", True),
             ("rId23", "hyperlink", "file:///C:/Windows/System32/calc.exe", True),
             ("rId24", "hyperlink", "https://tracker.example/spoof.png", True),
             ("rId25", "hyperlink", " file://evil.example/share/x", True))

    embedded = (f'<mc:AlternateContent xmlns:mc="{mc}"><mc:Choice xmlns:v="urn:schemas-microsoft-com:vml" Requires="v">'
                + frame(20, '<p:oleObj name="Document" r:id="rId10" imgW="1270000" imgH="952500" progId="Word.Document.12"><p:embed/></p:oleObj>')
                + '</mc:Choice><mc:Fallback>'
                + frame(20, '<p:oleObj name="Document" r:id="rId10" imgW="1270000" imgH="952500" progId="Word.Document.12"><p:embed/>'
                        '<p:pic><p:nvPicPr><p:cNvPr id="0" name=""/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId11"/>'
                        '<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="838200" y="4000000"/><a:ext cx="1270000" cy="952500"/></a:xfrm>'
                        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:oleObj>')
                + '</mc:Fallback></mc:AlternateContent>')
    linked = frame(21, '<p:oleObj name="Budget" r:id="rId12" imgW="1270000" imgH="952500" progId="Excel.Sheet.12"><p:link updateAutomatic="1"/></p:oleObj>')
    controls = (f'<p:controls><mc:AlternateContent xmlns:mc="{mc}"><mc:Choice xmlns:v="urn:schemas-microsoft-com:vml" Requires="v">'
                '<p:control spid="1025" name="Button" r:id="rId13" imgW="914400" imgH="457200"/></mc:Choice><mc:Fallback>'
                '<p:control name="Button" r:id="rId13" imgW="914400" imgH="457200"><p:pic><p:nvPicPr><p:cNvPr id="22" name="Button"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>'
                '<p:blipFill><a:blip r:embed="rId14"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm>'
                '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:control></mc:Fallback></mc:AlternateContent></p:controls>')
    timing = ('<p:timing><p:tnLst><p:par><p:cTn id="1" dur="indefinite" restart="never" nodeType="tmRoot"><p:childTnLst>'
              '<p:set><p:cBhvr><p:cTn id="2" dur="1" fill="hold"/><p:tgtEl><p:spTgt spid="20"/></p:tgtEl><p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr>'
              '<p:to><p:strVal val="visible"/></p:to></p:set></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>')
    # An Office add-in, loaded from a file share.
    web_ns = "http://schemas.microsoft.com/office/webextensions/webextension/2010/11"
    addin = ('<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="23" name="Add-in"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr>'
             '<p:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></p:xfrm>'
             f'<a:graphic><a:graphicData uri="{web_ns}"><we:webextensionref xmlns:we="{web_ns}" r:id="rId16"/></a:graphicData></a:graphic></p:graphicFrame>')
    layout = "ppt/slideLayouts/slideLayout2.xml"
    parts[layout] = (parts[layout].replace("</p:spTree></p:cSld>", embedded + linked + addin + "</p:spTree>" + controls + "</p:cSld>" + timing, 1))
    add_rels(parts, "ppt/slideLayouts/_rels/slideLayout2.xml.rels",
             ("rId10", "package", "../embeddings/Microsoft_Word_Document.docx", False),
             ("rId11", "image", "../media/preview.png", False),
             ("rId12", "oleObject", "file:///C:/Finance/budget.xlsx", True),
             ("rId13", "control", "../activeX/activeX1.xml", False),
             ("rId14", "image", "../media/control.png", False),
             ("rId15", "vmlDrawing", "../drawings/vmlDrawing1.vml", False),
             ("rId16", "http://schemas.microsoft.com/office/2011/relationships/webextension", "../webextensions/webextension1.xml", False))
    parts["ppt/webextensions/webextension1.xml"] = (
        f'{DECL}<we:webextension xmlns:we="{web_ns}" id="{{A0B1C2D3-0000-4000-8000-000000000001}}">'
        '<we:reference id="wa000000001" version="1.0.0.0" store="\\\\evil.example\\addins" storeType="Filesystem"/>'
        '<we:alternateReferences/><we:properties/><we:bindings/><we:snapshot/></we:webextension>')
    # An action that runs a macro, spelled with a character reference, in a layout whose only
    # relationship is its master: every kept part is read, and an action is found however written.
    other = "ppt/slideLayouts/slideLayout3.xml"
    parts[other] = parts[other].replace("</p:spTree></p:cSld>", linking(24, "Macro", '<a:hlinkClick r:id="" action="&#112;paction://macro?name=Run"/>') + "</p:spTree></p:cSld>", 1)
    # Legacy VML, as Office writes it: not well-formed XML, and never read, since it goes unparsed.
    parts["ppt/drawings/vmlDrawing1.vml"] = b'<xml xmlns:v="urn:schemas-microsoft-com:vml"><v:shape id="_x0000_s1025"><br></v:shape></xml>'
    parts["ppt/embeddings/Microsoft_Word_Document.docx"] = b"An embedded document, not opened by any fixture.\n"
    parts["ppt/media/preview.png"] = b"\x89PNG\r\n\x1a\n preview"
    parts["ppt/media/control.png"] = b"\x89PNG\r\n\x1a\n control"
    parts["ppt/activeX/activeX1.xml"] = (f'{DECL}<ax:ocx xmlns:ax="http://schemas.microsoft.com/office/2006/activeX" xmlns:r="{OFFICE}" '
                                         'ax:classid="{D7053240-CE69-11CD-A777-00DD01143C57}" ax:persistence="persistStorage" r:id="rId1"/>')
    add_rels(parts, "ppt/activeX/_rels/activeX1.xml.rels",
             ("rId1", "http://schemas.microsoft.com/office/2006/relationships/activeXControlBinary", "activeX1.bin", False))
    parts["ppt/activeX/activeX1.bin"] = b"\x00" * 32

    add_rels(parts, "ppt/_rels/presentation.xml.rels", ("rId9", "image", "https://tracker.example/deck.png", True))
    # A target mode spelled in lower case is still external.
    parts["ppt/_rels/presentation.xml.rels"] = parts["ppt/_rels/presentation.xml.rels"].replace("</Relationships>", (
        f'<Relationship Id="rId8" Type="{OFFICE}/image" Target="https://tracker.example/lower.png" TargetMode="external"/></Relationships>'))
    add_rels(parts, "ppt/theme/_rels/theme1.xml.rels", ("rId1", "image", "https://tracker.example/theme.png", True))
    parts["[Content_Types].xml"] = parts["[Content_Types].xml"].replace("</Types>", (
        '<Default Extension="bin" ContentType="application/vnd.ms-office.activeX"/>'
        '<Default Extension="docx" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document"/>'
        '<Default Extension="png" ContentType="image/png"/>'
        '<Default Extension="vml" ContentType="application/vnd.openxmlformats-officedocument.vmlDrawing"/>'
        '<Override PartName="/ppt/activeX/activeX1.xml" ContentType="application/vnd.ms-office.activeX+xml"/>'
        '<Override PartName="/ppt/webextensions/webextension1.xml" ContentType="application/vnd.ms-office.webextension+xml"/></Types>'))
    return list(parts.items())


def template_slides(entries):
    """Sample slides of the template's own, and kept parts that still relate to them (§8.8, §10.4,
    C60): three slides the presentation lists, the second with a picture and the last two in a
    custom show, the first without a content type of its own and the third's typed in another case
    and with a parameter, and the second with a notes page; a notes master; on the master, a button
    that jumps to the first slide, a picture of the first through an image relationship, a
    relationship to the second that nothing refers to, one to the second's notes page, and an
    external one of the slide type; on a layout, a text link that jumps to the third with a sound, a
    picture of the third, and a button that jumps to the next slide, which names none."""
    parts = dict(entries)

    def shape(id, name, hlink="", run=""):
        return (f'<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}">{hlink}</p:cNvPr><p:cNvSpPr/><p:nvPr/></p:nvSpPr>'
                f'<p:spPr/><p:txBody><a:bodyPr/><a:p><a:r>{run}<a:t>{name}</a:t></a:r></a:p></p:txBody></p:sp>')

    def slide(title, *shapes):
        heading = (f'<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>'
                   f'<p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:t>{title}</a:t></a:r></a:p></p:txBody></p:sp>')
        return (f'{DECL}<p:sld xmlns:a="{A}" xmlns:r="{OFFICE}" xmlns:p="{P}"><p:cSld>{tree(heading, *shapes)}</p:cSld>'
                '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>')

    def image(id, name, rid):
        return (f'<p:pic><p:nvPicPr><p:cNvPr id="{id}" name="{name}" descr="{name}"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>'
                f'<p:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
                '<p:spPr><a:xfrm><a:off x="838200" y="1825625"/><a:ext cx="1270000" cy="952500"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>')

    picture = image(3, "Sample picture", "rId2")
    for number, (title, layout, shapes) in enumerate([("Welcome", 1, ()), ("Sample", 2, (picture,)), ("Appendix", 3, ())], 1):
        parts[f"ppt/slides/slide{number}.xml"] = slide(title, *shapes)
        add_rels(parts, f"ppt/slides/_rels/slide{number}.xml.rels", ("rId1", "slideLayout", f"../slideLayouts/slideLayout{layout}.xml", False))
    add_rels(parts, "ppt/slides/_rels/slide2.xml.rels", ("rId2", "image", "../media/sample.png", False),
             ("rId3", "notesSlide", "../notesSlides/notesSlide2.xml", False))
    parts["ppt/media/sample.png"] = b"\x89PNG\r\n\x1a\n sample"

    notes_master = "ppt/notesMasters/notesMaster1.xml"
    parts[notes_master] = (f'{DECL}<p:notesMaster xmlns:a="{A}" xmlns:r="{OFFICE}" xmlns:p="{P}">'
                           f'<p:cSld>{tree(placeholder(2, "Notes Placeholder 1", "body", 1))}</p:cSld>'
                           '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
                           '</p:notesMaster>')
    add_rels(parts, "ppt/notesMasters/_rels/notesMaster1.xml.rels", ("rId1", "theme", "../theme/theme2.xml", False))
    parts["ppt/theme/theme2.xml"] = theme("Brand Display", "Brand Sans")
    notes = (f'{DECL}<p:notes xmlns:a="{A}" xmlns:r="{OFFICE}" xmlns:p="{P}">'
             f'<p:cSld>{tree(placeholder(2, "Notes Placeholder 1", "body", 1))}</p:cSld>'
             '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>')
    parts["ppt/notesSlides/notesSlide2.xml"] = notes.replace("<a:p/>", "<a:p><a:r><a:t>Sample notes</a:t></a:r></a:p>")
    add_rels(parts, "ppt/notesSlides/_rels/notesSlide2.xml.rels",
             ("rId1", "notesMaster", "../notesMasters/notesMaster1.xml", False),
             ("rId2", "slide", "../slides/slide2.xml", False))

    main = "ppt/presentation.xml"
    parts[main] = (parts[main]
                   .replace("</p:sldMasterIdLst>", '</p:sldMasterIdLst><p:notesMasterIdLst><p:notesMasterId r:id="rId7"/></p:notesMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId4"/><p:sldId id="257" r:id="rId5"/><p:sldId id="258" r:id="rId6"/></p:sldIdLst>', 1)
                   .replace("</p:embeddedFontLst>", '</p:embeddedFontLst><p:custShowLst><p:custShow name="Short" id="0"><p:sldLst><p:sld r:id="rId5"/><p:sld r:id="rId6"/></p:sldLst></p:custShow></p:custShowLst>', 1))
    add_rels(parts, "ppt/_rels/presentation.xml.rels", *[(f"rId{n + 3}", "slide", f"slides/slide{n}.xml", False) for n in (1, 2, 3)],
             ("rId7", "notesMaster", "notesMasters/notesMaster1.xml", False))

    master = "ppt/slideMasters/slideMaster1.xml"
    home = shape(30, "Home", '<a:hlinkClick r:id="rId30" action="ppaction://hlinksldjump"/>')
    thumbnail = image(32, "Thumbnail", "rId32")
    parts[master] = parts[master].replace("</p:spTree></p:cSld>", home + thumbnail + "</p:spTree></p:cSld>", 1)
    add_rels(parts, "ppt/slideMasters/_rels/slideMaster1.xml.rels",
             ("rId30", "slide", "../slides/slide1.xml", False),
             ("rId31", "slide", "../slides/slide2.xml", False),
             ("rId32", "image", "../slides/slide1.xml", False),
             ("rId33", "slide", "https://example.com/deck/slide1", True),
             ("rId34", "notesSlide", "../notesSlides/notesSlide2.xml", False))

    layout = "ppt/slideLayouts/slideLayout2.xml"
    appendix = shape(30, "Appendix", run='<a:rPr lang="en-US"><a:hlinkClick r:id="rId2" action="ppaction://hlinksldjump"><a:snd r:embed="rId3" name="click.wav"/></a:hlinkClick></a:rPr>')
    following = shape(31, "Next", '<a:hlinkClick r:id="" action="ppaction://hlinkshowjump?jump=nextslide"/>')
    preview = image(32, "Preview", "rId4")
    parts[layout] = parts[layout].replace("</p:spTree></p:cSld>", appendix + following + preview + "</p:spTree></p:cSld>", 1)
    add_rels(parts, "ppt/slideLayouts/_rels/slideLayout2.xml.rels",
             ("rId2", "slide", "../slides/slide3.xml", False),
             ("rId3", "audio", "../media/click.wav", False),
             ("rId4", "image", "../slides/slide3.xml", False))
    parts["ppt/media/click.wav"] = b"RIFF\x04\x00\x00\x00WAVE"

    parts["[Content_Types].xml"] = parts["[Content_Types].xml"].replace("</Types>", (
        '<Default Extension="png" ContentType="image/png"/><Default Extension="wav" ContentType="audio/wav"/>'
        + f'<Override PartName="/ppt/slides/slide2.xml" ContentType="{PML}.slide+xml"/>'
        + f'<Override PartName="/ppt/slides/slide3.xml" ContentType="{PML}.Slide+xml; charset=utf-8"/>'
        + f'<Override PartName="/ppt/notesMasters/notesMaster1.xml" ContentType="{PML}.notesMaster+xml"/>'
        + f'<Override PartName="/ppt/notesSlides/notesSlide2.xml" ContentType="{PML}.notesSlide+xml"/>'
        + '<Override PartName="/ppt/theme/theme2.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>'
        + "</Types>"))
    return list(parts.items())


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
    # Theme tokens for a workbook (§9.2): fonts any viewer has, and an accent white text doesn't read
    # on, so a striped table takes its dark header instead.
    pptx(ROOT / "render/120-xlsx-theme-tokens/orange.potx", main_type="template",
         fonts=("Georgia", "Verdana"), accent1="C55A11")
    # Active and external content a deck mustn't carry over from its template (§10.4).
    pptx(ROOT / "render/121-pptx-template-active-content/brand.potx", main_type="template",
         patch=active_content)
    # Sample slides of the template's own, which its master and a layout still jump to (§8.8).
    pptx(ROOT / "render/122-pptx-template-slide-links/brand.potx", main_type="template",
         patch=template_slides)


if __name__ == "__main__":
    main()
