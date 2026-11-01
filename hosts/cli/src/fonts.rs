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

/// Reads a lockfile, which must match `spec/lockfile.schema.json` and list a family (compared by
/// its ASCII-lowercase form) and style at most once (§10.3).
pub fn read(path: &Path) -> Result<Lock, String> {
    let fail = |why: String| format!("{} can't be read: {why}", path.display());
    let text = std::fs::read_to_string(path).map_err(|e| fail(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| fail(e.to_string()))?;
    let only = |value: &serde_json::Value, keys: &[&str], what: &str| match value.as_object() {
        None => Err(fail(format!("{what} isn't an object"))),
        Some(object) => match object.keys().find(|key| !keys.contains(&key.as_str())) {
            Some(key) => Err(fail(format!("{what} has the unknown key `{key}`"))),
            None => Ok(()),
        },
    };
    only(&json, &["lockfileVersion", "fonts"], "it")?;
    // JSON Schema's `const 1` takes 1.0 too.
    if json["lockfileVersion"].as_f64() != Some(1.0) {
        return Err(fail("its `lockfileVersion` isn't 1".into()));
    }
    let fonts = json["fonts"]
        .as_array()
        .ok_or_else(|| fail("it has no `fonts` list".into()))?;
    let mut pins = HashMap::new();
    for (index, font) in fonts.iter().enumerate() {
        only(
            font,
            &["family", "style", "source", "from", "sha256"],
            &format!("font {index}"),
        )?;
        let entry = |key: &str| {
            font[key]
                .as_str()
                .ok_or_else(|| fail(format!("font {index} has no `{key}`")))
        };
        let family = entry("family")?;
        if family.is_empty() {
            return Err(fail(format!("font {index}'s `family` is empty")));
        }
        let style = match entry("style")? {
            "regular" => FontStyle::Regular,
            "bold" => FontStyle::Bold,
            "italic" => FontStyle::Italic,
            "boldItalic" => FontStyle::BoldItalic,
            other => return Err(fail(format!("font {index} has the style `{other}`"))),
        };
        if !matches!(entry("source")?, "file" | "google") {
            return Err(fail(format!(
                "font {index}'s `source` isn't `file` or `google`"
            )));
        }
        if !font["from"].is_null() && !font["from"].is_string() {
            return Err(fail(format!("font {index}'s `from` isn't a string")));
        }
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
        if pins
            .insert((family.to_ascii_lowercase(), style), sha256.to_string())
            .is_some()
        {
            return Err(fail(format!(
                "font {index} lists `{family}` and its style again"
            )));
        }
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
        // A relative `XDG_CACHE_HOME` is ignored, as the XDG Base Directory spec says.
        var("XDG_CACHE_HOME")
            .filter(|path| path.is_absolute())
            .or_else(|| var("HOME").map(|home| home.join(".cache")))?
    };
    Some(base.join("compyla").join("fonts"))
}

impl Lock {
    /// The cached bytes the lockfile pins for `family` and `style`: the file named by the digest,
    /// read up to the font limit, and only if its bytes have that digest.
    pub fn resolve(&self, cache: &Path, family: &str, style: FontStyle) -> Option<Vec<u8>> {
        let sha256 = self.pins.get(&(family.to_ascii_lowercase(), style))?;
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
        let digest = "0".repeat(64);
        for text in [
            "not json",
            r#"{"lockfileVersion":2,"fonts":[]}"#,
            r#"{"lockfileVersion":1,"fonts":[{"family":"X","style":"Bold","sha256":"00"}]}"#,
            &format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"X","style":"regular","sha256":"{digest}"}}]}}"#
            ),
            &format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"X","style":"regular","source":"web","sha256":"{digest}"}}]}}"#
            ),
            &format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"","style":"regular","source":"file","sha256":"{digest}"}}]}}"#
            ),
            &format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"X","style":"regular","source":"file","sha256":"{digest}","extra":1}}]}}"#
            ),
            &format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"X","style":"regular","source":"file","from":7,"sha256":"{digest}"}}]}}"#
            ),
            // The same family and style twice, compared without ASCII case.
            &format!(
                r#"{{"lockfileVersion":1,"fonts":[{{"family":"Brand","style":"regular","source":"file","sha256":"{digest}"}},{{"family":"BRAND","style":"regular","source":"google","sha256":"{digest}"}}]}}"#
            ),
            r#"{"lockfileVersion":1,"fonts":[],"extra":true}"#,
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(read(&path).is_err(), "{text}");
        }
        // `const 1` takes 1.0; other styles of the same family are other entries.
        std::fs::write(
            &path,
            format!(
                r#"{{"lockfileVersion":1.0,"fonts":[{{"family":"Brand","style":"regular","source":"file","from":"brand.ttf","sha256":"{digest}"}},{{"family":"brand","style":"bold","source":"google","sha256":"{digest}"}}]}}"#
            ),
        )
        .unwrap();
        assert_eq!(read(&path).unwrap().pins.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
