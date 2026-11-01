# Bundled fonts

**Carlito**, the house stand-in for Calibri and Calibri Light (markdown-spec.md §3, architecture.md
D14), unmodified. Its licence reserves the name "Carlito", so a subset or a copy without outlines
would have to be renamed.

- Source: [google/fonts](https://github.com/google/fonts/tree/3dd78844021e948ceb633d1dcee3f7885561b5d9/ofl/carlito)
  at commit `3dd78844021e948ceb633d1dcee3f7885561b5d9`
- Copyright 2013 The Carlito Project Authors (https://github.com/googlefonts/carlito), with Reserved
  Font Name "Carlito"
- Licence: SIL Open Font License 1.1, in [OFL.txt](OFL.txt). Every distribution that bundles these
  files carries this notice and the licence.

| File | Bytes | SHA-256 |
|------|------:|---------|
| Carlito-Regular.ttf | 628,032 | `f6418f708baede9789daef5d458c0f53d2a888af9820e8062934e504fedc6595` |
| Carlito-Bold.ttf | 682,468 | `bb5d20f79b82599ec72983597437373a80f2d2085fa91fc144fd74e876a594db` |
| Carlito-Italic.ttf | 615,236 | `0b019225e58d702bfedcbd35c21696769f8ee115cb6343f84c2f240312450d1c` |
| Carlito-BoldItalic.ttf | 808,508 | `b32928186c119599e03ca6a1ffc680fdcb7fac95772f4b95d989cf6cd3861517` |
| OFL.txt | 4,424 | `58402f82a7c332a700294988fe7554fbb0a63a8d27ccc1ee3bbc640311990a00` |

`tests/fonts.rs` checks every file against its digest. To update, fetch the files from a newer
commit, update this table, and expect measurement's committed widths (`tests/golden.rs`) to move.
