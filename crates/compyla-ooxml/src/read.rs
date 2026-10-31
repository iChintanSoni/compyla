//! Reading zip archives of untrusted origin (markdown-spec.md §10.4, §12.1). Every limit is
//! enforced on the bytes themselves, never on what headers claim: a DEFLATE stream that would pass
//! the uncompressed limit stops there.

use std::collections::HashSet;

/// The template package limits of markdown-spec.md §12.1.
pub const MAX_COMPRESSED: usize = 50 * 1024 * 1024;
pub const MAX_UNCOMPRESSED: usize = 256 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 10_000;
pub const MAX_RATIO: usize = 100;

/// Why an archive was refused. The text completes "The template …".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

fn refuse<T>(reason: impl Into<String>) -> Result<T, Refused> {
    Err(Refused(reason.into()))
}

/// The entries of a zip archive, by name, in central-directory order.
pub fn entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Refused> {
    if bytes.len() > MAX_COMPRESSED {
        return refuse("is larger than 50 MiB");
    }
    // An encrypted Office file is an OLE compound file wrapping the package.
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return refuse("is encrypted or password-protected");
    }
    let end = end_record(bytes).ok_or_else(|| Refused("isn't a zip package".into()))?;
    let count = usize::from(u16_at(bytes, end + 10)?);
    let size = u32_at(bytes, end + 12)? as usize;
    let offset = u32_at(bytes, end + 16)? as usize;
    if count == 0xFFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
        return refuse("uses Zip64, which a template never needs");
    }
    if count > MAX_ENTRIES {
        return refuse(format!("has more than {MAX_ENTRIES} entries"));
    }
    let mut out = Vec::with_capacity(count);
    let mut names = HashSet::new();
    let (mut at, mut total) = (offset, 0usize);
    // What the archive may inflate to: the lesser of the size limit and the ratio limit. Enforced
    // while inflating, so overlapping or repeated entries can't make it allocate more. Computed
    // in u64: on 32-bit hosts the product overflows usize.
    let budget =
        (MAX_RATIO as u64 * bytes.len().max(1) as u64).min(MAX_UNCOMPRESSED as u64) as usize;
    for _ in 0..count {
        if u32_at(bytes, at)? != 0x0201_4b50 {
            return refuse("has a damaged central directory");
        }
        let flags = u16_at(bytes, at + 8)?;
        let method = u16_at(bytes, at + 10)?;
        let compressed = u32_at(bytes, at + 20)? as usize;
        let name_length = usize::from(u16_at(bytes, at + 28)?);
        let extra_length = usize::from(u16_at(bytes, at + 30)?);
        let comment_length = usize::from(u16_at(bytes, at + 32)?);
        let local = u32_at(bytes, at + 42)? as usize;
        let name = slice(bytes, at + 46, name_length)?;
        at += 46 + name_length + extra_length + comment_length;

        let name = std::str::from_utf8(name)
            .map_err(|_| Refused("has an entry name that isn't UTF-8".into()))?
            .to_string();
        check_name(&name)?;
        if !names.insert(name.to_ascii_lowercase()) {
            return refuse(format!("has the entry `{name}` twice"));
        }
        if flags & 1 != 0 {
            return refuse("is encrypted or password-protected");
        }
        if name.ends_with('/') {
            continue; // a folder entry
        }
        // The local header repeats the name and has its own extra field.
        if u32_at(bytes, local)? != 0x0403_4b50 {
            return refuse("has a damaged entry");
        }
        let local_name = usize::from(u16_at(bytes, local + 26)?);
        let local_extra = usize::from(u16_at(bytes, local + 28)?);
        // A reader that walks local headers must see the same name.
        if slice(bytes, local + 30, local_name)? != name.as_bytes() {
            return refuse(format!("has an entry whose names disagree (`{name}`)"));
        }
        let data = slice(bytes, local + 30 + local_name + local_extra, compressed)?;
        let left = budget - total;
        let content =
            match method {
                0 if data.len() <= left => data.to_vec(),
                0 => return refuse(over(budget)),
                8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, left).map_err(
                    |error| match error.status {
                        miniz_oxide::inflate::TINFLStatus::HasMoreOutput => Refused(over(budget)),
                        _ => Refused("has an entry that doesn't decompress".into()),
                    },
                )?,
                _ => return refuse("uses a compression method other than DEFLATE"),
            };
        total += content.len();
        out.push((name, content));
    }
    Ok(out)
}

/// The refusal for inflating past `budget`.
fn over(budget: usize) -> String {
    if budget < MAX_UNCOMPRESSED {
        "compresses more than 100:1".into()
    } else {
        "is larger than 256 MiB uncompressed".into()
    }
}

/// §10.4: no absolute paths or `..` segments, however written. Part names are URIs (OPC), so they
/// are checked percent-decoded; a segment of dots, or with trailing dots or spaces (which Windows
/// drops), counts as `..`; backslashes, drive letters, controls and empty segments are refused.
fn check_name(name: &str) -> Result<(), Refused> {
    let decoded = percent_decoded(name);
    let segments: Vec<&str> = decoded.trim_end_matches('/').split('/').collect();
    let unsafe_name = decoded.starts_with('/')
        || decoded.contains(['\\', ':'])
        || decoded.chars().any(char::is_control)
        || segments
            .iter()
            .any(|segment| segment.is_empty() || segment.trim_end_matches(['.', ' ']).is_empty());
    if unsafe_name {
        return refuse(format!("has an entry with an unsafe path (`{name}`)"));
    }
    Ok(())
}

pub(crate) fn percent_decoded(name: &str) -> String {
    let bytes = name.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(high), Some(low)) = (
                bytes.get(i + 1).copied().and_then(hex),
                bytes.get(i + 2).copied().and_then(hex),
            )
        {
            out.push((high * 16 + low) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The end of central directory record: the last signature in the final 64 KiB + 22 bytes.
fn end_record(bytes: &[u8]) -> Option<usize> {
    let floor = bytes.len().saturating_sub(65_535 + 22);
    (floor..bytes.len().saturating_sub(21))
        .rev()
        .find(|&at| bytes[at..at + 4] == [0x50, 0x4b, 0x05, 0x06])
}

fn slice(bytes: &[u8], at: usize, length: usize) -> Result<&[u8], Refused> {
    at.checked_add(length)
        .and_then(|end| bytes.get(at..end))
        .ok_or_else(|| Refused("is truncated".into()))
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, Refused> {
    let b = slice(bytes, at, 2)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, Refused> {
    let b = slice(bytes, at, 4)?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip;

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let owned: Vec<(String, Vec<u8>)> = entries
            .iter()
            .map(|(n, d)| (n.to_string(), d.to_vec()))
            .collect();
        zip::write(&owned)
    }

    #[test]
    fn reads_what_the_writer_wrote() {
        let bytes = archive(&[("a.xml", b"<a/>"), ("b/c.xml", &[b'x'; 1000])]);
        let read = entries(&bytes).unwrap();
        assert_eq!(read[0], ("a.xml".to_string(), b"<a/>".to_vec()));
        assert_eq!(read[1].1.len(), 1000);
    }

    #[test]
    fn refuses_unsafe_and_hostile_archives() {
        let reason = |bytes: &[u8]| entries(bytes).unwrap_err().0;
        assert_eq!(reason(b"not a zip"), "isn't a zip package");
        for name in [
            "../evil.xml",
            "/abs.xml",
            "a/%2e%2e/b.xml",
            "a/.. /b.xml",
            "a/.../b.xml",
            "a\\b.xml",
            "C:/x.xml",
            "a//b.xml",
            "a\u{0}.xml",
        ] {
            assert!(
                reason(&archive(&[(name, b"x")])).contains("unsafe path"),
                "{name}"
            );
        }
        assert!(reason(&archive(&[("a.xml", b"x"), ("A.xml", b"y")])).contains("twice"));
        // A bomb: 1 MiB of zeros compresses about 1000:1.
        assert!(reason(&archive(&[("bomb.xml", &vec![0; 1 << 20])])).contains("100:1"));
        let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        ole.extend([0; 100]);
        assert!(reason(&ole).contains("encrypted"));
        // A large archive's ratio limit, computed without overflow on 32-bit hosts.
        let mut big = archive(&[("a.xml", b"<a/>")]);
        big.extend(vec![0; 43_000_000]);
        assert!(entries(&big).is_err(), "trailing bytes hide the end record");
        let mut truncated = archive(&[("a.xml", &[b'x'; 100])]);
        truncated.drain(10..40);
        assert!(entries(&truncated).is_err());
    }
}
