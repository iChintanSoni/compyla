//! Image dimensions, read from the headers of the formats the profile accepts (markdown-spec.md
//! §4.7): PNG, JPEG and GIF.

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

#[cfg(test)]
mod tests {
    use super::*;

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
