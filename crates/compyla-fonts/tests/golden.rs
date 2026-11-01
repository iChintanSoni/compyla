//! Measurement is the same in every host (markdown-spec.md §13, C62): these widths are committed,
//! and the test runs natively and on wasm32 (`wasm32-wasip1`, under Node's WASI), so a host that
//! measured differently would fail here.

use compyla_fonts::{Features, Font, Fonts, Ligatures, Measurer, Run, Style, carlito, measure};

const PT: i64 = 12_700;

fn carlito_fonts(style: Style) -> Fonts<'static> {
    Fonts {
        latin: Font::Face(carlito(style)),
        east_asian: None,
        complex: None,
    }
}

fn run<'a>(text: &'a str, fonts: Fonts<'a>, size_pt: i64) -> Run<'a> {
    Run {
        text,
        fonts,
        size: size_pt * PT,
        complex_size: size_pt * PT,
        style: Style::Regular,
        features: Features::default(),
        spacing: 0,
        language: None,
        rtl: None,
    }
}

const KERNED: Features = Features {
    kerning: true,
    ligatures: Ligatures {
        standard: false,
        contextual: false,
        historical: false,
        discretional: false,
    },
    contextual_alternates: true,
};

fn cases() -> Vec<(&'static str, Run<'static>)> {
    let prose = "The quick brown fox jumps over the lazy dog, AVATAR WAVE To 1⁄2 office.";
    let mut cases = Vec::new();
    for (name, style) in [
        ("regular", Style::Regular),
        ("bold", Style::Bold),
        ("italic", Style::Italic),
        ("bold italic", Style::BoldItalic),
    ] {
        let plain = Run {
            style,
            ..run(prose, carlito_fonts(style), 18)
        };
        cases.push((name, plain));
        cases.push((
            name,
            Run {
                features: KERNED,
                ..plain
            },
        ));
    }
    let latin = carlito_fonts(Style::Regular);
    cases.push((
        "ligatures",
        Run {
            features: Features {
                ligatures: Ligatures {
                    standard: true,
                    contextual: true,
                    historical: false,
                    discretional: false,
                },
                ..KERNED
            },
            ..run("office affinity flow ﬁ", latin, 24)
        },
    ));
    cases.push(("cjk unmapped", run("会議の議題 Agenda 2026", latin, 20)));
    cases.push((
        "arabic unmapped rtl",
        Run {
            rtl: Some(true),
            ..run("مرحبا بالعالم 123", latin, 20)
        },
    ));
    cases.push((
        "hebrew complex size",
        Run {
            complex_size: 28 * PT,
            ..run("שלום world", latin, 14)
        },
    ));
    cases.push((
        "marks",
        run("e\u{0301} q\u{0301} n\u{0303} A\u{0308}\u{0323}", latin, 16),
    ));
    cases.push((
        "spaces",
        run("a\u{00A0}b\u{2009}c\u{3000}d\u{200B}e\u{00AD}f", latin, 16),
    ));
    cases.push((
        "spacing",
        Run {
            spacing: 2 * PT,
            ..run("Tracked text", latin, 16)
        },
    ));
    cases.push((
        "language",
        Run {
            language: Some("tr"),
            ..run("İstanbul fi", latin, 16)
        },
    ));
    cases.push((
        "consolas",
        run(
            "fn main() { println!(\"漢\"); }",
            Fonts {
                latin: Font::Consolas,
                east_asian: None,
                complex: None,
            },
            14,
        ),
    ));
    // 10.5 pt: OOXML sizes are hundredths of a point, so EMU sizes needn't be whole points.
    cases.push((
        "fractional size",
        Run {
            size: 133_350,
            complex_size: 133_350,
            ..run("Fractional 10.5 pt", latin, 0)
        },
    ));
    cases
}

/// The widths, in EMU, in the order of `cases`.
const EXPECTED: &[i64] = &[
    6919391, // regular
    6802524, // regular, kerned
    7085595, // bold
    6970179, // bold, kerned
    6866036, // italic
    6746825, // italic, kerned
    7052220, // bold italic
    6938813, // bold italic, kerned
    2454771, // ligatures
    2681261, // cjk unmapped
    2025052, // arabic unmapped rtl
    1168461, // hebrew complex size
    570011,  // marks
    849610,  // spaces
    1329332, // spacing
    817562,  // language
    2817192, // consolas
    949597,  // fractional size
];

#[test]
fn measurement_matches_the_committed_widths() {
    let got: Vec<i64> = cases().iter().map(|(_, run)| measure(run)).collect();
    let names: Vec<&str> = cases().iter().map(|(name, _)| *name).collect();
    let listing: String = got
        .iter()
        .zip(&names)
        .map(|(width, name)| format!("    {width}, // {name}\n"))
        .collect();
    assert_eq!(got, EXPECTED, "\nwidths now:\n{listing}");
}

#[test]
fn a_measurer_keeps_its_faces_and_measures_alike() {
    // One measurer across every case, in order and then in reverse, gives the committed widths:
    // the faces it keeps from one run don't change the next.
    let cases = cases();
    let mut measurer = Measurer::new();
    let forward: Vec<i64> = cases.iter().map(|(_, run)| measurer.measure(run)).collect();
    assert_eq!(forward, EXPECTED);
    let mut backward: Vec<i64> = cases
        .iter()
        .rev()
        .map(|(_, run)| measurer.measure(run))
        .collect();
    backward.reverse();
    assert_eq!(backward, EXPECTED);
}

#[test]
fn a_measurer_parses_each_face_once() {
    let regular = run("Kept", carlito_fonts(Style::Regular), 12);
    let bold = run("Kept", carlito_fonts(Style::Bold), 12);
    let mut measurer = Measurer::new();
    measurer.measure(&regular);
    assert_eq!(measurer.faces(), 1);
    // The second face joins the first, which isn't parsed again.
    measurer.measure(&bold);
    measurer.measure(&regular);
    assert_eq!(measurer.faces(), 2);
}
