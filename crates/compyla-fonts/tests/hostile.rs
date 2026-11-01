//! Hostile font bytes and text (markdown-spec.md §12.1): reading and measuring never panic, and work
//! stays bounded. A font that doesn't parse is unreadable; one that parses in part measures as far
//! as it goes.

use std::time::{Duration, Instant};

use compyla_fonts::{Face, Features, Font, Fonts, Run, Style, Unreadable, measure};

const CARLITO: &[u8] = include_bytes!("../fonts/Carlito-Regular.ttf");
const SAMPLE: &str = "Hostile AVATAR ﬁ e\u{0301} 漢字 مرحبا שלום \u{0915}\u{094D}\u{0937} 1⁄2";

fn measure_with(face: &Face) -> i64 {
    let fonts = Fonts {
        latin: Font::Face(face),
        east_asian: Some(Font::Face(face)),
        complex: Some(Font::Face(face)),
    };
    measure(&Run {
        text: SAMPLE,
        fonts,
        size: 12 * 12_700,
        complex_size: 12 * 12_700,
        style: Style::Regular,
        features: Features {
            kerning: true,
            ..Features::default()
        },
        spacing: 0,
        language: None,
        rtl: None,
    })
}

/// A deterministic pseudo-random sequence (a 64-bit LCG), so failures reproduce.
fn lcg(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

#[test]
fn truncated_fonts_never_panic() {
    for len in (0..CARLITO.len()).step_by(4_093) {
        if let Ok(face) = Face::load(CARLITO[..len].to_vec(), "Carlito", Style::Regular) {
            measure_with(&face);
        }
    }
}

#[test]
fn corrupted_fonts_never_panic() {
    let mut seed = 7;
    for _ in 0..200 {
        let mut bytes = CARLITO.to_vec();
        for _ in 0..16 {
            let at = (lcg(&mut seed) as usize) % bytes.len();
            bytes[at] = lcg(&mut seed) as u8;
        }
        if let Ok(face) = Face::load(bytes, "Carlito", Style::Regular) {
            measure_with(&face);
        }
    }
}

#[test]
fn a_collection_declaring_more_faces_than_it_holds_isnt_a_font() {
    // `ttcf`, version 1.0, 0xFFFFFFFF faces, and offsets that point nowhere.
    let mut bytes = b"ttcf\x00\x01\x00\x00\xFF\xFF\xFF\xFF".to_vec();
    bytes.extend(std::iter::repeat_n(0xFFu8, 1 << 20));
    let started = Instant::now();
    assert_eq!(
        Face::load(bytes, "Anything", Style::Bold).unwrap_err(),
        Unreadable::NotAFont
    );
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_collection_of_many_broken_faces_is_scanned_in_linear_time() {
    // As many faces as the bytes hold, every offset pointing past the end.
    let faces: u32 = 1 << 18;
    let mut bytes = b"ttcf\x00\x01\x00\x00".to_vec();
    bytes.extend(faces.to_be_bytes());
    bytes.extend(std::iter::repeat_n(0xFFu8, faces as usize * 4));
    let started = Instant::now();
    assert!(Face::load(bytes, "Anything", Style::Bold).is_err());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn long_mixed_text_measures_in_linear_time() {
    let face = Face::load(CARLITO, "Carlito", Style::Regular).unwrap();
    let text = SAMPLE.repeat(4_000);
    let fonts = Fonts {
        latin: Font::Face(&face),
        east_asian: None,
        complex: None,
    };
    let started = Instant::now();
    let width = measure(&Run {
        text: &text,
        fonts,
        size: 12 * 12_700,
        complex_size: 12 * 12_700,
        style: Style::Regular,
        features: Features::default(),
        spacing: 0,
        language: None,
        rtl: None,
    });
    assert!(width > 0);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}
