"""Generates house.potx, the house template the pptx renderer uses when no template is supplied
(architecture.md D12; markdown-spec.md §8.5). Standard library only:

    python3 -I crates/compyla-pptx/house/make_house.py

The template is written for Compyla, not copied from an Office application. Geometry follows the
standard 16:9 placeholder grid; fonts are the theme fonts Office and LibreOffice (as Carlito) share.
Every entry gets the same timestamp, in a fixed order, so the file is reproducible.
"""

import zipfile
from pathlib import Path

OUT = Path(__file__).resolve().parent / "house.potx"

A = "http://schemas.openxmlformats.org/drawingml/2006/main"
P = "http://schemas.openxmlformats.org/presentationml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
RELS = "http://schemas.openxmlformats.org/package/2006/relationships"
PML = "application/vnd.openxmlformats-officedocument.presentationml"
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
NS = f'xmlns:a="{A}" xmlns:r="{R}" xmlns:p="{P}"'

# The house palette, shared with the xlsx header fill (lt2) and hyperlink color.
COLORS = [("dk1", "1F2A44"), ("lt1", "FFFFFF"), ("dk2", "1F2A44"), ("lt2", "E8ECF3"),
          ("accent1", "2F5BEA"), ("accent2", "00A88F"), ("accent3", "F2A900"), ("accent4", "D64545"),
          ("accent5", "7A4FD1"), ("accent6", "4A5568"), ("hlink", "0563C1"), ("folHlink", "954F72")]
MAJOR, MINOR = "Calibri Light", "Calibri"

# 16:9, in EMU.
WIDTH, HEIGHT = 12192000, 6858000


def rels(*items):
    body = "".join(f'<Relationship Id="rId{i}" Type="{R}/{t}" Target="{target}"/>' for i, (t, target) in enumerate(items, 1))
    return f'{DECL}<Relationships xmlns="{RELS}">{body}</Relationships>'


def theme(name):
    scheme = "".join(f'<a:{k}><a:srgbClr val="{v}"/></a:{k}>' for k, v in COLORS)
    fonts = (f'<a:majorFont><a:latin typeface="{MAJOR}"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
             f'<a:minorFont><a:latin typeface="{MINOR}"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>')
    solid = '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>'
    line = lambda w: f'<a:ln w="{w}" cap="flat" cmpd="sng" algn="ctr">{solid}<a:prstDash val="solid"/><a:miter lim="800000"/></a:ln>'
    fmt = (f'<a:fmtScheme name="Compyla"><a:fillStyleLst>{solid}{solid}{solid}</a:fillStyleLst>'
           f'<a:lnStyleLst>{line(6350)}{line(12700)}{line(19050)}</a:lnStyleLst>'
           '<a:effectStyleLst>' + '<a:effectStyle><a:effectLst/></a:effectStyle>' * 3 + '</a:effectStyleLst>'
           f'<a:bgFillStyleLst>{solid}{solid}{solid}</a:bgFillStyleLst></a:fmtScheme>')
    return (f'{DECL}<a:theme xmlns:a="{A}" name="{name}"><a:themeElements>'
            f'<a:clrScheme name="Compyla">{scheme}</a:clrScheme><a:fontScheme name="Compyla">{fonts}</a:fontScheme>{fmt}'
            '</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>')


def xfrm(x, y, cx, cy):
    return f'<a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'


def shape(id, name, ph, geometry=None, body="", text="<a:p><a:endParaRPr lang=\"en-US\"/></a:p>", anchor=None):
    """A placeholder shape: `ph` is the inside of p:ph's attributes, e.g. 'type="title"'."""
    spPr = f"<p:spPr>{geometry}</p:spPr>" if geometry else "<p:spPr/>"
    bodyPr = f'<a:bodyPr anchor="{anchor}"/>' if anchor else "<a:bodyPr/>"
    return (f'<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr>'
            f'<p:nvPr><p:ph {ph}/></p:nvPr></p:nvSpPr>{spPr}<p:txBody>{bodyPr}<a:lstStyle>{body}</a:lstStyle>{text}</p:txBody></p:sp>')


def tree(*shapes):
    return ('<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
            '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
            + "".join(shapes) + "</p:spTree>")


def level(n, size, bullet=True, indent=228600, margin=None, bold=False, italic=False):
    margin = margin if margin is not None else 228600 * n
    bu = '<a:buFont typeface="Arial"/><a:buChar char="&#8226;"/>' if bullet else "<a:buNone/>"
    attrs = f' b="1"' if bold else ""
    attrs += f' i="1"' if italic else ""
    return (f'<a:lvl{n}pPr marL="{margin}" indent="{-indent if bullet else 0}" algn="l" defTabSz="914400">'
            f'<a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPts val="1000"/></a:spcBef>{bu}'
            f'<a:defRPr sz="{size}" kern="1200"{attrs}><a:solidFill><a:schemeClr val="tx1"/></a:solidFill>'
            '<a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvlNpPr>').replace("lvlNpPr", f"lvl{n}pPr")


def plain(size, align="l", bold=False, italic=False):
    """A level-1 style with no bullet, for subtitles, captions, headings and quotes."""
    attrs = (' b="1"' if bold else "") + (' i="1"' if italic else "")
    return (f'<a:lvl1pPr marL="0" indent="0" algn="{align}"><a:buNone/><a:defRPr sz="{size}"{attrs}/></a:lvl1pPr>')


# Geometry: the standard 16:9 grid.
TITLE = xfrm(838200, 365125, 10515600, 1325563)
BODY = xfrm(838200, 1825625, 10515600, 4351338)
DATE = xfrm(838200, 6356350, 2743200, 365125)
FOOTER = xfrm(4038600, 6356350, 4114800, 365125)
NUMBER = xfrm(8610600, 6356350, 2743200, 365125)


def footers(start):
    return [shape(start, "Date Placeholder", 'type="dt" sz="half" idx="10"'),
            shape(start + 1, "Footer Placeholder", 'type="ftr" sz="quarter" idx="11"'),
            shape(start + 2, "Slide Number Placeholder", 'type="sldNum" sz="quarter" idx="12"')]


def master():
    shapes = [
        shape(2, "Title Placeholder 1", 'type="title"', TITLE, anchor="ctr"),
        shape(3, "Text Placeholder 2", 'type="body" idx="1"', BODY),
        shape(4, "Date Placeholder 3", 'type="dt" sz="half" idx="10"', DATE),
        shape(5, "Footer Placeholder 4", 'type="ftr" sz="quarter" idx="11"', FOOTER),
        shape(6, "Slide Number Placeholder 5", 'type="sldNum" sz="quarter" idx="12"', NUMBER,
              text='<a:p><a:pPr algn="r"/><a:fld id="{B6F15528-21DE-4FAA-801E-634DDDAF4B2B}" type="slidenum"><a:rPr lang="en-US" sz="1200"/><a:t>&#8249;#&#8250;</a:t></a:fld><a:endParaRPr lang="en-US"/></a:p>'),
    ]
    title_style = ('<p:titleStyle><a:lvl1pPr algn="l" defTabSz="914400" rtl="0" eaLnBrk="1" latinLnBrk="0" hangingPunct="1">'
                   '<a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:spcBef><a:spcPct val="0"/></a:spcBef><a:buNone/>'
                   '<a:defRPr sz="4000" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill>'
                   '<a:latin typeface="+mj-lt"/><a:ea typeface="+mj-ea"/><a:cs typeface="+mj-cs"/></a:defRPr></a:lvl1pPr></p:titleStyle>')
    sizes = [2800, 2400, 2000, 1800, 1800, 1800, 1800, 1800, 1800]
    body_style = "<p:bodyStyle>" + "".join(level(n, sizes[n - 1], margin=228600 + 457200 * (n - 1)) for n in range(1, 10)) + "</p:bodyStyle>"
    other_style = '<p:otherStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr></p:otherStyle>'
    ids = "".join(f'<p:sldLayoutId id="{2147483649 + i}" r:id="rId{i + 1}"/>' for i in range(len(LAYOUTS)))
    return (f'{DECL}<p:sldMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>{tree(*shapes)}</p:cSld>'
            '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
            f'<p:sldLayoutIdLst>{ids}</p:sldLayoutIdLst><p:txStyles>{title_style}{body_style}{other_style}</p:txStyles></p:sldMaster>')


# The house set (markdown-spec.md §8.5), with each layout's OOXML type and placeholders.
LAYOUTS = [
    ("Title Slide", "title", [
        shape(2, "Title 1", 'type="ctrTitle"', xfrm(1524000, 1122363, 9144000, 2387600),
              '<a:lvl1pPr algn="ctr"><a:defRPr sz="6000"/></a:lvl1pPr>', anchor="b"),
        shape(3, "Subtitle 2", 'type="subTitle" idx="1"', xfrm(1524000, 3602038, 9144000, 1655762), plain(2400, "ctr")),
    ]),
    ("Title and Content", "obj", [
        shape(2, "Title 1", 'type="title"'),
        shape(3, "Content Placeholder 2", 'idx="1"'),
    ]),
    ("Section Header", "secHead", [
        shape(2, "Title 1", 'type="title"', xfrm(831850, 1709738, 10515600, 2852737),
              '<a:lvl1pPr><a:defRPr sz="6000"/></a:lvl1pPr>', anchor="b"),
        shape(3, "Text Placeholder 2", 'type="body" idx="1"', xfrm(831850, 4589463, 10515600, 1500187), plain(2400)),
    ]),
    ("Two Content", "twoObj", [
        shape(2, "Title 1", 'type="title"'),
        shape(3, "Content Placeholder 2", 'sz="half" idx="1"', xfrm(838200, 1825625, 5181600, 4351338)),
        shape(4, "Content Placeholder 3", 'sz="half" idx="2"', xfrm(6172200, 1825625, 5181600, 4351338)),
    ]),
    ("Comparison", "twoTxTwoObj", [
        shape(2, "Title 1", 'type="title"', xfrm(839788, 365125, 10515600, 1325563)),
        shape(3, "Text Placeholder 2", 'type="body" idx="1"', xfrm(839788, 1681163, 5157787, 823912), plain(2400, bold=True), anchor="b"),
        shape(4, "Content Placeholder 3", 'sz="half" idx="2"', xfrm(839788, 2505075, 5157787, 3684588)),
        shape(5, "Text Placeholder 4", 'type="body" sz="quarter" idx="3"', xfrm(6172200, 1681163, 5183188, 823912), plain(2400, bold=True), anchor="b"),
        shape(6, "Content Placeholder 5", 'sz="quarter" idx="4"', xfrm(6172200, 2505075, 5183188, 3684588)),
    ]),
    ("Title Only", "titleOnly", [
        shape(2, "Title 1", 'type="title"'),
    ]),
    ("Blank", "blank", []),
    ("Content with Caption", "objTx", [
        shape(2, "Title 1", 'type="title"', xfrm(839788, 457200, 3932237, 1600200),
              '<a:lvl1pPr><a:defRPr sz="3200"/></a:lvl1pPr>', anchor="b"),
        shape(3, "Content Placeholder 2", 'idx="1"', xfrm(5183188, 987425, 6172200, 4873625)),
        shape(4, "Text Placeholder 3", 'type="body" sz="half" idx="2"', xfrm(839788, 2057400, 3932237, 3811588), plain(1600)),
    ]),
    ("Picture with Caption", "picTx", [
        shape(2, "Title 1", 'type="title"', xfrm(839788, 457200, 3932237, 1600200),
              '<a:lvl1pPr><a:defRPr sz="3200"/></a:lvl1pPr>', anchor="b"),
        shape(3, "Picture Placeholder 2", 'type="pic" idx="1"', xfrm(5183188, 987425, 6172200, 4873625), plain(3200)),
        shape(4, "Text Placeholder 3", 'type="body" sz="half" idx="2"', xfrm(839788, 2057400, 3932237, 3811588), plain(1600)),
    ]),
    ("Quote", "cust", [
        shape(2, "Title 1", 'type="title"'),
        shape(3, "Text Placeholder 2", 'type="body" idx="1"', xfrm(1219200, 1825625, 9753600, 3657600), plain(3200, "ctr", italic=True), anchor="ctr"),
    ]),
]


def layout(name, kind, shapes):
    preserve = ' preserve="1"' if kind == "cust" else ""
    return (f'{DECL}<p:sldLayout {NS} type="{kind}"{preserve}><p:cSld name="{name}">{tree(*shapes, *footers(len(shapes) + 2))}</p:cSld>'
            '<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>')


def notes_master():
    shapes = [
        shape(2, "Header Placeholder 1", 'type="hdr" sz="quarter"', xfrm(0, 0, 2971800, 458788)),
        shape(3, "Date Placeholder 2", 'type="dt" idx="1"', xfrm(3884613, 0, 2971800, 458788)),
        ('<p:sp><p:nvSpPr><p:cNvPr id="4" name="Slide Image Placeholder 3"/><p:cNvSpPr><a:spLocks noGrp="1" noRot="1" noChangeAspect="1"/></p:cNvSpPr>'
         '<p:nvPr><p:ph type="sldImg" idx="2"/></p:nvPr></p:nvSpPr><p:spPr>' + xfrm(685800, 1143000, 5486400, 3086100)
         + '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/><a:ln w="12700"><a:solidFill><a:prstClr val="black"/></a:solidFill></a:ln></p:spPr></p:sp>'),
        shape(5, "Notes Placeholder 4", 'type="body" sz="quarter" idx="3"', xfrm(685800, 4400550, 5486400, 3600450)),
        shape(6, "Footer Placeholder 5", 'type="ftr" sz="quarter" idx="4"', xfrm(0, 8685213, 2971800, 458787)),
        shape(7, "Slide Number Placeholder 6", 'type="sldNum" sz="quarter" idx="5"', xfrm(3884613, 8685213, 2971800, 458787)),
    ]
    style = "".join(f'<a:lvl{n}pPr marL="{457200 * (n - 1)}" algn="l" defTabSz="914400"><a:defRPr sz="1200" kern="1200"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/><a:ea typeface="+mn-ea"/><a:cs typeface="+mn-cs"/></a:defRPr></a:lvl{n}pPr>' for n in range(1, 10))
    return (f'{DECL}<p:notesMaster {NS}><p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg>{tree(*shapes)}</p:cSld>'
            '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
            f'<p:notesStyle>{style}</p:notesStyle></p:notesMaster>')


def main():
    n = len(LAYOUTS)
    presentation = (f'{DECL}<p:presentation {NS} saveSubsetFonts="1">'
                    '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>'
                    '<p:notesMasterIdLst><p:notesMasterId r:id="rId2"/></p:notesMasterIdLst>'
                    f'<p:sldSz cx="{WIDTH}" cy="{HEIGHT}"/><p:notesSz cx="6858000" cy="9144000"/>'
                    '<p:defaultTextStyle><a:defPPr><a:defRPr lang="en-US"/></a:defPPr></p:defaultTextStyle></p:presentation>')
    types = [("ppt/presentation.xml", f"{PML}.template.main+xml"),
             ("ppt/slideMasters/slideMaster1.xml", f"{PML}.slideMaster+xml"),
             *[(f"ppt/slideLayouts/slideLayout{i}.xml", f"{PML}.slideLayout+xml") for i in range(1, n + 1)],
             ("ppt/notesMasters/notesMaster1.xml", f"{PML}.notesMaster+xml"),
             ("ppt/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"),
             ("ppt/theme/theme2.xml", "application/vnd.openxmlformats-officedocument.theme+xml"),
             ("ppt/presProps.xml", f"{PML}.presProps+xml"),
             ("ppt/viewProps.xml", f"{PML}.viewProps+xml"),
             ("ppt/tableStyles.xml", f"{PML}.tableStyles+xml")]
    content_types = (f'{DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
                     '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
                     '<Default Extension="xml" ContentType="application/xml"/>'
                     + "".join(f'<Override PartName="/{p}" ContentType="{t}"/>' for p, t in types) + "</Types>")
    package_rels = (f'{DECL}<Relationships xmlns="{RELS}"><Relationship Id="rId1" Type="{R}/officeDocument" Target="ppt/presentation.xml"/></Relationships>')
    entries = [
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", package_rels),
        ("ppt/presentation.xml", presentation),
        ("ppt/_rels/presentation.xml.rels", rels(("slideMaster", "slideMasters/slideMaster1.xml"),
                                                  ("notesMaster", "notesMasters/notesMaster1.xml"),
                                                  ("theme", "theme/theme1.xml"),
                                                  ("presProps", "presProps.xml"),
                                                  ("viewProps", "viewProps.xml"),
                                                  ("tableStyles", "tableStyles.xml"))),
        ("ppt/slideMasters/slideMaster1.xml", master()),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels",
         rels(*[("slideLayout", f"../slideLayouts/slideLayout{i}.xml") for i in range(1, n + 1)], ("theme", "../theme/theme1.xml"))),
        *[(f"ppt/slideLayouts/slideLayout{i}.xml", layout(*LAYOUTS[i - 1])) for i in range(1, n + 1)],
        *[(f"ppt/slideLayouts/_rels/slideLayout{i}.xml.rels", rels(("slideMaster", "../slideMasters/slideMaster1.xml"))) for i in range(1, n + 1)],
        ("ppt/notesMasters/notesMaster1.xml", notes_master()),
        ("ppt/notesMasters/_rels/notesMaster1.xml.rels", rels(("theme", "../theme/theme2.xml"))),
        ("ppt/theme/theme1.xml", theme("Compyla")),
        ("ppt/theme/theme2.xml", theme("Compyla Notes")),
        ("ppt/presProps.xml", f'{DECL}<p:presentationPr {NS}/>'),
        ("ppt/viewProps.xml", f'{DECL}<p:viewPr {NS}><p:normalViewPr><p:restoredLeft sz="15620"/><p:restoredTop sz="94660"/></p:normalViewPr><p:gridSpacing cx="76200" cy="76200"/></p:viewPr>'),
        # Medium Style 2 - Accent 1, built into every Office application.
        ("ppt/tableStyles.xml", f'{DECL}<a:tblStyleLst xmlns:a="{A}" def="{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}"/>'),
    ]
    with zipfile.ZipFile(OUT, "w") as archive:
        for name, data in entries:
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)


if __name__ == "__main__":
    main()
