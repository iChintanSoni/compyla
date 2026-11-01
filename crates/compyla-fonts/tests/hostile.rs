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
    cmap_of(records, &[(u16::from(b'a'), glyph)])
}

/// A `cmap` with `records` Unicode encoding records, all pointing at one format 4 subtable that
/// maps each code point to its glyph (sorted by code point, all in the BMP).
fn cmap_of(records: u16, maps: &[(u16, u16)]) -> Vec<u8> {
    let mut out = 0u16.to_be_bytes().to_vec();
    out.extend(records.to_be_bytes());
    let subtable_at = 4 + 8 * u32::from(records);
    for _ in 0..records {
        out.extend(0u16.to_be_bytes()); // Unicode platform
        out.extend(3u16.to_be_bytes()); // BMP
        out.extend(subtable_at.to_be_bytes());
    }
    // Format 4: a segment per code point, then the closing 0xFFFF. The words are the format,
    // length, language, segment count × 2 and three search fields; the end codes, a pad, the start
    // codes (equal to the end codes), the deltas and the range offsets.
    let segments = maps.len() as u16 + 1;
    let ends: Vec<u16> = maps.iter().map(|&(c, _)| c).chain([0xFFFF]).collect();
    let mut words: Vec<u16> = vec![4, 16 + 8 * segments, 0, 2 * segments, 0, 0, 0];
    words.extend(&ends);
    words.push(0);
    words.extend(&ends);
    words.extend(maps.iter().map(|&(c, g)| g.wrapping_sub(c)).chain([1]));
    words.extend(std::iter::repeat_n(0, segments as usize));
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

#[test]
fn a_collection_with_huge_name_tables_is_read_quickly() {
    // 5,000 family-name records, each 60,000 bytes long, sharing one string: matching would decode
    // every one, so records whose length can't match aren't decoded, and a face listed 256 times
    // is read once.
    let records = 5_000u16;
    let length = 60_000u16;
    let mut name = 0u16.to_be_bytes().to_vec();
    name.extend(records.to_be_bytes());
    name.extend((6 + 12 * records).to_be_bytes());
    for _ in 0..records {
        for word in [3u16, 1, 0x409, 1, length, 0] {
            name.extend(word.to_be_bytes());
        }
    }
    name.extend(std::iter::repeat_n(0x41u8, length as usize));
    let faces = compyla_fonts::MAX_FACES;
    let first = 12 + 4 * faces;
    let mut collection = b"ttcf\x00\x01\x00\x00".to_vec();
    collection.extend(faces.to_be_bytes());
    for _ in 0..faces {
        collection.extend(first.to_be_bytes());
    }
    collection.extend(sfnt_at(
        first as usize,
        vec![(*b"cmap", cmap(1, 1)), (*b"name", name)],
    ));
    let started = Instant::now();
    assert!(Face::load(collection, "Brand Sans", Style::Bold).is_ok());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_mark_whose_base_no_face_maps_advances_nothing() {
    // A face mapping `b` and U+0301 to glyph 1 (600 of 1,000 units), whose GDEF classes it as a
    // base glyph, so shaped alone the acute would advance. Its base 漢 is unmapped.
    let mut gdef = Vec::new();
    for word in [1u16, 0, 12, 0, 0, 0, 1, 1, 1, 1] {
        gdef.extend(word.to_be_bytes()); // the header, then a format 1 class definition: glyph 1 → 1
    }
    let face = Face::load(
        sfnt(vec![
            (*b"cmap", cmap_of(1, &[(u16::from(b'b'), 1), (0x0301, 1)])),
            (*b"GDEF", gdef),
        ]),
        "Marks",
        Style::Regular,
    )
    .unwrap();
    assert!(face.maps('\u{0301}'));
    let size = 12 * 12_700;
    let run = Run {
        text: "b漢\u{0301}",
        fonts: Fonts {
            latin: Font::Face(&face),
            east_asian: None,
            complex: None,
        },
        size,
        complex_size: size,
        style: Style::Regular,
        features: Features::default(),
        spacing: 0,
        language: None,
        rtl: None,
    };
    // `b` at 600/1,000 em and 漢 at 1 em; the mark nothing.
    assert_eq!(measure(&run), 600 * size / 1000 + size);
}

/// `font` with its `hhea` ascender and descender set.
fn with_hhea(mut font: Vec<u8>, ascender: i16, descender: i16) -> Vec<u8> {
    let count = u16::from_be_bytes([font[4], font[5]]) as usize;
    let record = (0..count)
        .map(|i| 12 + 16 * i)
        .find(|&at| &font[at..at + 4] == b"hhea")
        .unwrap();
    let offset = u32::from_be_bytes(font[record + 8..record + 12].try_into().unwrap()) as usize;
    font[offset + 4..offset + 6].copy_from_slice(&ascender.to_be_bytes());
    font[offset + 6..offset + 8].copy_from_slice(&descender.to_be_bytes());
    font
}

/// A version 0 `OS/2` table with these `usWinAscent` and `usWinDescent`.
fn os2(ascent: u16, descent: u16) -> Vec<u8> {
    let mut table = vec![0u8; 78];
    table[74..76].copy_from_slice(&ascent.to_be_bytes());
    table[76..78].copy_from_slice(&descent.to_be_bytes());
    table
}

#[test]
fn ascent_and_descent_fall_back_where_a_face_lacks_them() {
    let load = |bytes: Vec<u8>| Face::load(bytes, "Odd", Style::Regular).unwrap();
    // The win metrics, read as unsigned even where they'd be negative as signed values.
    assert_eq!(
        load(sfnt(vec![(*b"OS/2", os2(900, 300))])).ascent_descent(),
        (900, 300)
    );
    assert_eq!(
        load(sfnt(vec![(*b"OS/2", os2(40_000, 33_000))])).ascent_descent(),
        (40_000, 33_000)
    );
    // Without an OS/2 table, hhea's ascender and its descender's magnitude.
    assert_eq!(
        load(with_hhea(sfnt(vec![]), 800, -200)).ascent_descent(),
        (800, 200)
    );
    assert_eq!(
        load(with_hhea(sfnt(vec![]), 800, i16::MIN)).ascent_descent(),
        (800, 32_767)
    );
    // Where both are 0, Carlito's, so a last line's height never divides by 0 (§3).
    assert_eq!(load(sfnt(vec![])).ascent_descent(), (1950, 550));
    assert_eq!(
        load(sfnt(vec![(*b"OS/2", os2(0, 0))])).ascent_descent(),
        (1950, 550)
    );
    assert_eq!(
        compyla_fonts::carlito(Style::Bold).ascent_descent(),
        (1950, 550)
    );
    assert_eq!(Font::Consolas.ascent_descent(), (1884, 514));
}

/// A `GSUB` whose `ccmp` feature runs a context lookup on glyph 1 that applies a multiple
/// substitution (1 → 1 1) and then itself again: on the first glyph only, so the text grows a glyph
/// at each level of recursion; or, `doubling`, on a pair of them and again on each of the pair, so
/// it doubles at each level.
fn looping_gsub(doubling: bool) -> Vec<u8> {
    let words =
        |values: &[u16]| -> Vec<u8> { values.iter().flat_map(|v| v.to_be_bytes()).collect() };
    // Lookup 0's subtable: a sequence context (format 3) of one glyph 1, or two, with its records
    // (sequence index, lookup index), and its coverage of glyph 1 after them.
    let context = match doubling {
        false => words(&[3, 1, 2, 16, 0, 1, 0, 0, 1, 1, 1]),
        true => words(&[3, 2, 3, 22, 22, 0, 1, 0, 0, 1, 0, 1, 1, 1]),
    };
    let lookup0 = 8 + context.len() as u16;
    let mut gsub = words(&[1, 0, 10, 30, 44]);
    // ScriptList (10): DFLT → a script whose default language system uses feature 0.
    gsub.extend(words(&[1]));
    gsub.extend(b"DFLT");
    gsub.extend(words(&[8, 4, 0, 0, 0xFFFF, 1, 0]));
    // FeatureList (30): ccmp → lookup 0.
    gsub.extend(words(&[1]));
    gsub.extend(b"ccmp");
    gsub.extend(words(&[8, 0, 1, 0]));
    // LookupList (44): two lookups.
    gsub.extend(words(&[2, 6, 6 + lookup0]));
    // Lookup 0: context (type 5).
    gsub.extend(words(&[5, 0, 1, 8]));
    gsub.extend(context);
    // Lookup 1: multiple substitution (type 2), glyph 1 → glyphs 1 and 1.
    gsub.extend(words(&[2, 0, 1, 8]));
    gsub.extend(words(&[1, 14, 1, 8]));
    gsub.extend(words(&[2, 1, 1]));
    gsub.extend(words(&[1, 1, 1]));
    gsub
}

#[test]
fn a_looping_substitution_is_shaped_only_to_the_bound() {
    // §12.1: shaping is bounded, and a run whose shaping reaches the bound is measured as shaped so
    // far. The shaper's own limits are that bound (rustybuzz 0.20's): 64 levels of nested lookups,
    // and output of 64 glyphs for each of the text's (at least 16,384), within 1,024 operations for
    // each (at least 16,384).
    let load = |gsub: Option<Vec<u8>>| {
        let mut tables = vec![(*b"cmap", cmap(1, 1))];
        tables.extend(gsub.map(|gsub| (*b"GSUB", gsub)));
        Face::load(sfnt(tables), "Looping", Style::Regular).unwrap()
    };
    let (plain, growing, doubling) = (
        load(None),
        load(Some(looping_gsub(false))),
        load(Some(looping_gsub(true))),
    );
    let width = |face: &Face| {
        measure(&Run {
            text: "aaaa",
            fonts: Fonts {
                latin: Font::Face(face),
                east_asian: None,
                complex: None,
            },
            size: 12 * 12_700,
            complex_size: 12 * 12_700,
            style: Style::Regular,
            features: Features::default(),
            spacing: 0,
            language: None,
            rtl: None,
        })
    };
    let started = Instant::now();
    let (plain, growing, doubling) = (width(&plain), width(&growing), width(&doubling));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    // Glyph 1 advances 600 of 1,000 units: 4 a's are 4 × 0.6 × 12 pt.
    let glyph = 600 * 12 * 12_700 / 1000;
    assert_eq!(plain, 4 * glyph);
    // A glyph more at each level stops at the 64th: 65 glyphs for each a.
    assert_eq!(growing, 4 * 65 * glyph);
    // Doubling at each level stops at the bound on work or output, long before 2^64 glyphs, and the
    // text measures the glyphs shaped so far.
    assert!(
        doubling > 4 * glyph && doubling <= 16_384 * glyph,
        "{}",
        doubling / glyph
    );
    assert_eq!(doubling % glyph, 0);
}
