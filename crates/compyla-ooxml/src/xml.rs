//! Writing XML. Text and attribute values are always escaped, and characters XML 1.0 forbids are
//! replaced with U+FFFD, so no document text can become markup (markdown-spec.md §12.2).

/// An XML document under construction. Elements are written in the order the calls make them;
/// the caller keeps them balanced.
pub struct Writer {
    out: String,
}

impl Writer {
    /// A document starting with the XML declaration.
    pub fn new() -> Self {
        Self {
            out: "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n".into(),
        }
    }

    /// `<name attrs…>`.
    pub fn open(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.start(name, attrs);
        self.out.push('>');
    }

    /// `<name attrs…/>`.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.start(name, attrs);
        self.out.push_str("/>");
    }

    pub fn close(&mut self, name: &str) {
        self.out.push_str("</");
        self.out.push_str(name);
        self.out.push('>');
    }

    /// Escaped character data.
    pub fn text(&mut self, text: &str) {
        for c in text.chars() {
            match c {
                '&' => self.out.push_str("&amp;"),
                '<' => self.out.push_str("&lt;"),
                '>' => self.out.push_str("&gt;"),
                // A literal CR would be normalized away by XML parsers.
                '\r' => self.out.push_str("&#13;"),
                c => self.out.push(allowed(c)),
            }
        }
    }

    /// `<name attrs…>text</name>`.
    pub fn element(&mut self, name: &str, attrs: &[(&str, &str)], text: &str) {
        self.open(name, attrs);
        self.text(text);
        self.close(name);
    }

    pub fn finish(self) -> Vec<u8> {
        self.out.into_bytes()
    }

    fn start(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.out.push('<');
        self.out.push_str(name);
        for (key, value) in attrs {
            self.out.push(' ');
            self.out.push_str(key);
            self.out.push_str("=\"");
            for c in value.chars() {
                match c {
                    '&' => self.out.push_str("&amp;"),
                    '<' => self.out.push_str("&lt;"),
                    '>' => self.out.push_str("&gt;"),
                    '"' => self.out.push_str("&quot;"),
                    // Attribute-value normalization would turn these into spaces.
                    '\t' => self.out.push_str("&#9;"),
                    '\n' => self.out.push_str("&#10;"),
                    '\r' => self.out.push_str("&#13;"),
                    c => self.out.push(allowed(c)),
                }
            }
            self.out.push('"');
        }
    }
}

impl Default for Writer {
    fn default() -> Self {
        Self::new()
    }
}

/// `c`, or U+FFFD where XML 1.0 forbids it (markdown-spec.md §1.1).
fn allowed(c: char) -> char {
    match c as u32 {
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF => c,
        _ => '\u{FFFD}',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_attributes_are_escaped() {
        let mut xml = Writer::new();
        xml.open("a", &[("t", "\"<&>\"\n")]);
        xml.text("<b>&amp;\u{1}\r");
        xml.empty("c", &[]);
        xml.close("a");
        let out = String::from_utf8(xml.finish()).unwrap();
        assert!(out.ends_with(
            "<a t=\"&quot;&lt;&amp;&gt;&quot;&#10;\">&lt;b&gt;&amp;amp;\u{FFFD}&#13;<c/></a>"
        ));
    }
}
