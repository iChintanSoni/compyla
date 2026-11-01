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

/// Measures `SAMPLE` repeated `reps` times in Carlito, and how long that took.
fn time_measuring(reps: usize) -> Duration {
    let face = Face::load(CARLITO, "Carlito", Style::Regular).unwrap();
    let text = SAMPLE.repeat(reps);
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
    started.elapsed()
}

#[test]
fn long_mixed_text_measures_in_linear_time() {
    // Four times the text takes about four times as long; a quadratic path would take sixteen.
    let small = time_measuring(500);
    let large = time_measuring(2_000);
    assert!(
        large < small * 8 + Duration::from_millis(200),
        "{small:?} for 500, {large:?} for 2,000"
    );
}

/// A minimal TrueType font: `head`, `hhea`, `maxp`, `hmtx` and the tables given, in tag order, for
/// shapes no real font has.
fn sfnt(extra: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    sfnt_at(0, extra)
}

/// `sfnt`, to be placed `base` bytes into a file: table offsets count from the file's start, as in
/// a collection.
fn sfnt_at(base: usize, extra: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes()); // units per em
    let mut hhea = vec![0u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[34..36].copy_from_slice(&2u16.to_be_bytes()); // horizontal metrics
    let mut maxp = 0x0000_5000u32.to_be_bytes().to_vec();
    maxp.extend(2u16.to_be_bytes()); // glyphs
    let hmtx = [500u16, 0, 600, 0]
        .iter()
        .flat_map(|v| v.to_be_bytes())
        .collect();
    let mut tables = vec![
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"maxp", maxp),
    ];
    tables.extend(extra);
    tables.sort_by_key(|(tag, _)| *tag);
    let count = tables.len() as u16;
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend(count.to_be_bytes());
    out.extend([0u8; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend(tag);
        out.extend([0u8; 4]); // checksum
        out.extend(((base + offset) as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        body.extend(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend(body);
    out
}

/// A `cmap` with `records` Unicode encoding records, all pointing at one format 4 subtable that
/// maps `a` to `glyph`.
fn cmap(records: u16, glyph: u16) -> Vec<u8> {
    let mut out = 0u16.to_be_bytes().to_vec();
    out.extend(records.to_be_bytes());
    let subtable_at = 4 + 8 * u32::from(records);
    for _ in 0..records {
        out.extend(0u16.to_be_bytes()); // Unicode platform
        out.extend(3u16.to_be_bytes()); // BMP
        out.extend(subtable_at.to_be_bytes());
    }
    // Format 4: two segments, `a`..`a` and the closing 0xFFFF.
    let delta = glyph.wrapping_sub(u16::from(b'a'));
    let words: [u16; 16] = [
        4,
        32,
        0,
        4,
        2,
        0,
        0, // format, length, language, segCountX2, search fields
        u16::from(b'a'),
        0xFFFF, // end codes
        0,      // reserved pad
        u16::from(b'a'),
        0xFFFF, // start codes
        delta,
        1, // deltas
        0,
        0, // range offsets
    ];
    out.extend(words.iter().flat_map(|w| w.to_be_bytes()));
    out
}

#[test]
fn a_character_mapped_to_notdef_isnt_mapped() {
    let face = Face::load(sfnt(vec![(*b"cmap", cmap(1, 0))]), "Zero", Style::Regular).unwrap();
    assert!(!face.maps('a'));
    let mapped = Face::load(sfnt(vec![(*b"cmap", cmap(1, 1))]), "One", Style::Regular).unwrap();
    assert!(mapped.maps('a'));
    // So `a` falls back to Carlito instead of measuring nothing.
    let fonts = Fonts {
        latin: Font::Face(&face),
        east_asian: None,
        complex: None,
    };
    let run = Run {
        text: "a",
        fonts,
        size: 12 * 12_700,
        complex_size: 12 * 12_700,
        style: Style::Regular,
        features: Features::default(),
        spacing: 0,
        language: None,
        rtl: None,
    };
    let carlito = Run {
        fonts: Fonts {
            latin: Font::Face(compyla_fonts::carlito(Style::Regular)),
            east_asian: None,
            complex: None,
        },
        ..run
    };
    assert_eq!(measure(&run), measure(&carlito));
}

#[test]
fn fonts_past_the_table_and_record_limits_arent_fonts() {
    let records = sfnt(vec![(*b"cmap", cmap(u16::MAX, 1))]);
    assert_eq!(
        Face::load(records, "Many", Style::Regular).unwrap_err(),
        Unreadable::NotAFont
    );
    let within = sfnt(vec![(*b"cmap", cmap(compyla_fonts::MAX_CMAP_RECORDS, 1))]);
    assert!(Face::load(within, "Some", Style::Regular).is_ok());
    // 600 tables of 4 bytes each.
    let many: Vec<([u8; 4], Vec<u8>)> = (0..600u16)
        .map(|n| {
            let tag = [b'z', b'y', (n >> 8) as u8 + b'a', (n & 0xFF) as u8];
            (tag, vec![0; 4])
        })
        .collect();
    assert_eq!(
        Face::load(sfnt(many), "Many", Style::Regular).unwrap_err(),
        Unreadable::NotAFont
    );
}

#[test]
fn a_collection_past_the_face_limit_isnt_a_font() {
    let collection = |faces: u32| {
        let mut out = b"ttcf\x00\x01\x00\x00".to_vec();
        out.extend(faces.to_be_bytes());
        let first = 12 + 4 * faces;
        for _ in 0..faces {
            out.extend(first.to_be_bytes()); // every face is the same one
        }
        out.extend(sfnt_at(first as usize, vec![(*b"cmap", cmap(1, 1))]));
        out
    };
    assert!(Face::load(collection(compyla_fonts::MAX_FACES), "X", Style::Regular).is_ok());
    assert_eq!(
        Face::load(
            collection(compyla_fonts::MAX_FACES + 1),
            "X",
            Style::Regular
        )
        .unwrap_err(),
        Unreadable::NotAFont
    );
}
