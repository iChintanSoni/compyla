//! Image dimensions, read from the headers of the formats the profiles accept (markdown-spec.md
//! §4.7): PNG, JPEG and GIF.

use std::borrow::Cow;

/// The image limits of markdown-spec.md §12.1.
pub const MAX_ENCODED: usize = 50 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 100_000_000;

/// Width and height in pixels, or `None` when the header can't be read.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |at: usize| Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
    let be32 = |at: usize| {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let size = |w: u32, h: u32| (w > 0 && h > 0).then_some((w, h));
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        // IHDR is the first chunk: width and height at bytes 16 and 20.
        return size(be32(16)?, be32(20)?);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        let le16 = |at: usize| Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
        return size(u32::from(le16(6)?), u32::from(le16(8)?));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // Walk the segments to the first start-of-frame marker.
        let mut at = 2;
        while at + 4 <= bytes.len() {
            if bytes[at] != 0xFF {
                return None;
            }
            let marker = bytes[at + 1];
            if marker == 0xFF {
                at += 1; // fill byte
                continue;
            }
            let length = usize::from(be16(at + 2)?);
            let frame = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if frame {
                return size(u32::from(be16(at + 7)?), u32::from(be16(at + 5)?));
            }
            if length < 2 {
                return None;
            }
            at += 2 + length;
        }
    }
    None
}

/// A JPEG's EXIF orientation (1–8; 1 is upright), from its first APP1 segment.
pub fn orientation(bytes: &[u8]) -> u8 {
    orientation_tag(bytes)
        .and_then(|tag| u8::try_from(tag.value).ok())
        .filter(|o| (1..=8).contains(o))
        .unwrap_or(1)
}

/// `bytes` with its EXIF orientation set to 1 (as stored). A renderer that turns the picture itself
/// writes this copy, so a viewer that also applies the tag doesn't turn it twice, and one that
/// ignores it agrees.
pub fn as_stored(bytes: &[u8]) -> Cow<'_, [u8]> {
    match orientation_tag(bytes) {
        Some(tag) if tag.value != 1 => {
            let mut copy = bytes.to_vec();
            let one = if tag.little {
                1u16.to_le_bytes()
            } else {
                1u16.to_be_bytes()
            };
            copy[tag.at..tag.at + 2].copy_from_slice(&one);
            Cow::Owned(copy)
        }
        _ => Cow::Borrowed(bytes),
    }
}

/// An orientation tag's value, the offset of its two bytes in the file, and their byte order.
struct Tag {
    value: u16,
    at: usize,
    little: bool,
}

fn orientation_tag(bytes: &[u8]) -> Option<Tag> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let be16 = |at: usize| Some(u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
    let mut at = 2;
    while at + 4 <= bytes.len() && bytes[at] == 0xFF {
        let marker = bytes[at + 1];
        let length = usize::from(be16(at + 2)?);
        if marker == 0xDA || length < 2 {
            return None; // start of scan: no EXIF before it
        }
        let segment = bytes.get(at + 4..at + 2 + length)?;
        if marker == 0xE1 && segment.starts_with(b"Exif\0\0") {
            let mut tag = exif_orientation(&segment[6..])?;
            tag.at += at + 4 + 6;
            return Some(tag);
        }
        at += 2 + length;
    }
    None
}

/// The orientation tag (0x0112) of a TIFF structure's first IFD, its offset relative to `tiff`.
fn exif_orientation(tiff: &[u8]) -> Option<Tag> {
    let little = match tiff.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let b = [*tiff.get(at)?, *tiff.get(at + 1)?];
        Some(if little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |at: usize| {
        let b = [
            *tiff.get(at)?,
            *tiff.get(at + 1)?,
            *tiff.get(at + 2)?,
            *tiff.get(at + 3)?,
        ];
        Some(if little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let entries = usize::from(u16_at(ifd)?);
    (0..entries).find_map(|i| {
        let entry = ifd + 2 + 12 * i;
        // A SHORT is left-justified in the entry's value field.
        (u16_at(entry)? == 0x0112).then(|| {
            u16_at(entry + 8).map(|value| Tag {
                value,
                at: entry + 8,
                little,
            })
        })?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A JPEG start with an EXIF segment giving `orientation`, in either byte order.
    fn exif_jpeg(orientation: u16, little: bool) -> Vec<u8> {
        let mut tiff: Vec<u8> = if little {
            b"II*\0".to_vec()
        } else {
            b"MM\0*".to_vec()
        };
        let put16 = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let put32 = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        tiff.extend(put32(8));
        tiff.extend(put16(1));
        tiff.extend(put16(0x0112));
        tiff.extend(put16(3));
        tiff.extend(put32(1));
        tiff.extend(put16(orientation));
        tiff.extend([0, 0]);
        tiff.extend(put32(0));
        let mut segment = b"Exif\0\0".to_vec();
        segment.extend(tiff);
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE1];
        jpeg.extend(((segment.len() + 2) as u16).to_be_bytes());
        jpeg.extend(segment);
        jpeg.extend([0xFF, 0xDA, 0x00, 0x02]);
        jpeg
    }

    #[test]
    fn reads_exif_orientation() {
        assert_eq!(orientation(&exif_jpeg(6, true)), 6);
        assert_eq!(orientation(&exif_jpeg(8, false)), 8);
        assert_eq!(orientation(&exif_jpeg(42, true)), 1);
        assert_eq!(orientation(b"\x89PNG"), 1);
        assert_eq!(orientation(&[0xFF, 0xD8, 0xFF, 0xE1, 0x00]), 1);
        // A value past a byte isn't truncated into range: 0x0106 isn't 6.
        assert_eq!(orientation(&exif_jpeg(0x0106, false)), 1);
    }

    #[test]
    fn the_copy_written_is_as_stored() {
        for little in [true, false] {
            let turned = exif_jpeg(6, little);
            let copy = as_stored(&turned);
            assert_eq!(orientation(&copy), 1);
            assert_eq!(copy.len(), turned.len());
            let differing = copy.iter().zip(&turned).filter(|(a, b)| a != b).count();
            assert_eq!(differing, 1, "only the value's low byte changes");
        }
        let upright = exif_jpeg(1, true);
        assert!(matches!(as_stored(&upright), Cow::Borrowed(_)));
        assert!(matches!(as_stored(b"\x89PNG"), Cow::Borrowed(_)));
    }

    #[test]
    fn reads_png_gif_and_jpeg_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(640u32.to_be_bytes());
        png.extend(480u32.to_be_bytes());
        assert_eq!(dimensions(&png), Some((640, 480)));
        let gif = b"GIF89a\x20\x00\x10\x00".to_vec();
        assert_eq!(dimensions(&gif), Some((32, 16)));
        // SOI, an APP0 segment, then SOF0 with height 200 and width 300.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend([0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0xC8, 0x01, 0x2C, 0x03]);
        assert_eq!(dimensions(&jpeg), Some((300, 200)));
        assert_eq!(dimensions(b"nope"), None);
        assert_eq!(dimensions(&[0xFF, 0xD8, 0xFF]), None);
    }
}
