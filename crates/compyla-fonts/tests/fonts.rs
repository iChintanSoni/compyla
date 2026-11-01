//! The bundled fonts are the files fonts/README.md pins, unmodified (architecture.md D14).

use sha2::{Digest, Sha256};

const PINNED: [(&str, &[u8], &str); 5] = [
    (
        "Carlito-Regular.ttf",
        include_bytes!("../fonts/Carlito-Regular.ttf"),
        "f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595",
    ),
    (
        "Carlito-Bold.ttf",
        include_bytes!("../fonts/Carlito-Bold.ttf"),
        "bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db",
    ),
    (
        "Carlito-Italic.ttf",
        include_bytes!("../fonts/Carlito-Italic.ttf"),
        "0b019225e58d702bfedcbd35c21696769f8ee115cb6343f84c2f240312450d1c",
    ),
    (
        "Carlito-BoldItalic.ttf",
        include_bytes!("../fonts/Carlito-BoldItalic.ttf"),
        "b32928186c119599e03ca6a1ffc680fdcb7fac95772f4b95d989cf6cd3861517",
    ),
    (
        "OFL.txt",
        include_bytes!("../fonts/OFL.txt"),
        "58402f82a7c332a700294988fe7554fbb0a63a8d27ccc1ee3bbc640311990a00",
    ),
];

#[test]
fn bundled_files_match_their_pinned_digests() {
    for (name, bytes, pinned) in PINNED {
        let digest: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(digest, pinned, "{name}");
    }
}

#[test]
fn the_licence_reserves_the_name() {
    let licence = std::str::from_utf8(PINNED[4].1).unwrap();
    assert!(licence.contains("Reserved Font Name \"Carlito\""));
    assert!(licence.contains("SIL OPEN FONT LICENSE Version 1.1"));
}
