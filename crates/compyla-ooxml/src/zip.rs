//! A deterministic zip writer (markdown-spec.md §13). Entries are written in the order given, with
//! the fixed timestamp 1980-01-01 00:00 (the earliest zip can store), DEFLATE from `miniz_oxide`
//! at a fixed level, and no field that depends on the platform, the clock or the environment, so
//! the same entries always give the same bytes.

/// The DOS date of 1980-01-01 (year 0 since 1980, month 1, day 1); the time is 00:00:00.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;
/// Version 2.0: DEFLATE.
const VERSION: u16 = 20;
const DEFLATE: u16 = 8;
const LEVEL: u8 = 6;

/// Writes `entries` (name, data) as a zip archive.
///
/// # Panics
///
/// If the archive would need Zip64: more than 65,535 entries, or 4 GiB. Rendered packages are far
/// smaller, since input is limited (markdown-spec.md §12.1).
pub fn write(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    let small = |n: usize| u32::try_from(n).expect("zip entry over 4 GiB");
    for (name, data) in entries {
        let compressed = miniz_oxide::deflate::compress_to_vec(data, LEVEL);
        let crc = crc32fast::hash(data);
        let offset = small(out.len());
        let name_length = u16::try_from(name.len()).expect("zip entry name over 64 KiB");

        out.extend(0x0403_4b50u32.to_le_bytes());
        header(
            &mut out,
            crc,
            small(compressed.len()),
            small(data.len()),
            name_length,
        );
        out.extend(name.as_bytes());
        out.extend(&compressed);

        central.extend(0x0201_4b50u32.to_le_bytes());
        // Version made by: 2.0, MS-DOS, so no Unix permissions or other host fields.
        central.extend(VERSION.to_le_bytes());
        header(
            &mut central,
            crc,
            small(compressed.len()),
            small(data.len()),
            name_length,
        );
        central.extend(0u16.to_le_bytes()); // comment length
        central.extend(0u16.to_le_bytes()); // disk number
        central.extend(0u16.to_le_bytes()); // internal attributes
        central.extend(0u32.to_le_bytes()); // external attributes
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let count = u16::try_from(entries.len()).expect("over 65,535 zip entries");
    let central_offset = small(out.len());
    let central_size = small(central.len());
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // this disk
    out.extend(0u16.to_le_bytes()); // the central directory's disk
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(central_size.to_le_bytes());
    out.extend(central_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment length
    out
}

/// The fields a local header and a central directory record share, from "version needed" to
/// "extra field length".
fn header(out: &mut Vec<u8>, crc: u32, compressed: u32, size: u32, name_length: u16) {
    out.extend(VERSION.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // flags: ASCII names, sizes known in advance
    out.extend(DEFLATE.to_le_bytes());
    out.extend(DOS_TIME.to_le_bytes());
    out.extend(DOS_DATE.to_le_bytes());
    out.extend(crc.to_le_bytes());
    out.extend(compressed.to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(name_length.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // extra field length
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn entries() -> Vec<(String, Vec<u8>)> {
        vec![
            ("[Content_Types].xml".into(), b"<Types/>".to_vec()),
            ("empty".into(), Vec::new()),
            ("xl/big.xml".into(), "<row/>".repeat(10_000).into_bytes()),
        ]
    }

    #[test]
    fn archives_read_back_in_order() {
        let bytes = write(&entries());
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(archive.len(), 3);
        for (index, (name, data)) in entries().iter().enumerate() {
            let mut file = archive.by_index(index).unwrap();
            assert_eq!(file.name().unwrap(), name.as_str());
            let mut read = Vec::new();
            file.read_to_end(&mut read).unwrap();
            assert_eq!(&read, data);
        }
    }

    #[test]
    fn the_same_entries_give_the_same_bytes() {
        assert_eq!(write(&entries()), write(&entries()));
    }
}
