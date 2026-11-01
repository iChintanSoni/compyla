//! The CLI's font resolver (markdown-spec.md §10.3, C62): the lockfile `compyla.lock` pins each family
//! and style to a SHA-256, and the font cache holds each file under its digest. The resolver never
//! touches the network; `compyla fonts sync` (#23) fills the cache and writes the lockfile.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use compyla::FontStyle;
use sha2::{Digest, Sha256};

/// The lockfile's name.
const LOCKFILE: &str = "compyla.lock";

/// A lockfile, read: each family (ASCII-lowercase) and style's pinned SHA-256.
#[derive(Debug, Default)]
pub struct Lock {
    pins: HashMap<(String, FontStyle), String>,
}

/// The lockfile to use: the one `explicit` names, else the first `compyla.lock` in `directory` or a
/// directory above it.
pub fn lockfile(explicit: Option<&Path>, directory: &Path) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let directory = if directory.as_os_str().is_empty() {
        Path::new(".")
    } else {
        directory
    };
    let start = std::path::absolute(directory).ok()?;
    start
        .ancestors()
        .map(|dir| dir.join(LOCKFILE))
        .find(|path| path.is_file())
}

/// Reads a lockfile (`spec/lockfile.schema.json`).
pub fn read(path: &Path) -> Result<Lock, String> {
    let fail = |why: String| format!("{} can't be read: {why}", path.display());
    let text = std::fs::read_to_string(path).map_err(|e| fail(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| fail(e.to_string()))?;
    if json["lockfileVersion"] != 1 {
        return Err(fail("its `lockfileVersion` isn't 1".into()));
    }
    let fonts = json["fonts"]
        .as_array()
        .ok_or_else(|| fail("it has no `fonts` list".into()))?;
    let mut pins = HashMap::new();
    for (index, font) in fonts.iter().enumerate() {
        let entry = |key: &str| {
            font[key]
                .as_str()
                .ok_or_else(|| fail(format!("font {index} has no `{key}`")))
        };
        let family = entry("family")?;
        let style = match entry("style")? {
            "regular" => FontStyle::Regular,
            "bold" => FontStyle::Bold,
            "italic" => FontStyle::Italic,
            "boldItalic" => FontStyle::BoldItalic,
            other => return Err(fail(format!("font {index} has the style `{other}`"))),
        };
        let sha256 = entry("sha256")?;
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(fail(format!(
                "font {index}'s `sha256` isn't 64 lower-case hex digits"
            )));
        }
        pins.insert(
            (family.trim().to_ascii_lowercase(), style),
            sha256.to_string(),
        );
    }
    Ok(Lock { pins })
}

/// The font cache: `$COMPYLA_CACHE/fonts`, else the user's cache directory's `compyla/fonts`.
pub fn cache() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(root) = var("COMPYLA_CACHE") {
        return Some(root.join("fonts"));
    }
    let base = if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Caches")
    } else if cfg!(windows) {
        var("LOCALAPPDATA")?
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|home| home.join(".cache")))?
    };
    Some(base.join("compyla").join("fonts"))
}

impl Lock {
    /// The cached bytes the lockfile pins for `family` and `style`: the file named by the digest,
    /// read up to the font limit, and only if its bytes have that digest.
    pub fn resolve(&self, cache: &Path, family: &str, style: FontStyle) -> Option<Vec<u8>> {
        let sha256 = self
            .pins
            .get(&(family.trim().to_ascii_lowercase(), style))?;
        let mut bytes = Vec::new();
        std::fs::File::open(cache.join(sha256))
            .ok()?
            .take(compyla::FONT_SIZE_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        (&digest == sha256).then_some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("compyla-fonts-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_pinned_font_resolves_only_with_its_digest() {
        let dir = temp("pin");
        let font = b"font bytes".to_vec();
        let sha256: String = Sha256::digest(&font)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        std::fs::write(dir.join(&sha256), &font).unwrap();
        let lock_path = dir.join(LOCKFILE);
        std::fs::write(
            &lock_path,
            format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"Brand Sans","style":"regular","source":"file","sha256":"{sha256}"}}]}}"#
            ),
        )
        .unwrap();
        let nested = dir.join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            lockfile(None, &nested).unwrap(),
            std::path::absolute(&lock_path).unwrap()
        );
        let lock = read(&lock_path).unwrap();
        assert_eq!(
            lock.resolve(&dir, "brand sans", FontStyle::Regular),
            Some(font)
        );
        assert_eq!(lock.resolve(&dir, "Brand Sans", FontStyle::Bold), None);
        // Bytes whose digest doesn't match aren't the pinned font.
        std::fs::write(dir.join(&sha256), b"changed").unwrap();
        assert_eq!(lock.resolve(&dir, "Brand Sans", FontStyle::Regular), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_lockfile_is_an_error() {
        let dir = temp("bad");
        let path = dir.join(LOCKFILE);
        for text in [
            "not json",
            r#"{"lockfileVersion":2,"fonts":[]}"#,
            r#"{"lockfileVersion":1,"fonts":[{"family":"X","style":"Bold","sha256":"00"}]}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(read(&path).is_err(), "{text}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
