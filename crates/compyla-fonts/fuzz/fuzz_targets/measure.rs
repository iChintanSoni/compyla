//! Font bytes and text from the fuzzer: reading them and measuring must never panic or hang.
#![no_main]

use compyla_fonts::{Face, Features, Font, Fonts, Ligatures, Run, Style, measure};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The first byte says how many of the next bytes are text; the rest is the font.
    let Some((&split, rest)) = data.split_first() else {
        return;
    };
    let (text, font) = rest.split_at(usize::from(split).min(rest.len()));
    let text = String::from_utf8_lossy(text);
    let Ok(face) = Face::load(font.to_vec(), "Fuzz", Style::Regular) else {
        return;
    };
    let fonts = Fonts {
        latin: Font::Face(&face),
        east_asian: Some(Font::Face(&face)),
        complex: Some(Font::Face(&face)),
    };
    measure(&Run {
        text: &text,
        fonts,
        size: 152_400,
        complex_size: 152_400,
        style: Style::Regular,
        features: Features {
            kerning: true,
            ligatures: Ligatures {
                standard: true,
                contextual: true,
                historical: true,
                discretional: true,
            },
            contextual_alternates: true,
        },
        spacing: 0,
        language: Some("en"),
        rtl: None,
    });
});
