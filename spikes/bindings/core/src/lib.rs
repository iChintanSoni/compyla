//! Stand-in for compyla-core in toolchain check 3: pure (no I/O), depends on the real parser
//! (comrak), and is compiled unchanged into the Node, Python and WASM hosts.

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, parse_document};

/// Parses markdown and returns a JSON summary: the number of top-level blocks and the first
/// heading's text. Deterministic, allocation-only, no I/O.
pub fn summarize(source: &str) -> String {
    let arena = Arena::new();
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    let root = parse_document(&arena, source, &options);

    let blocks = root.children().count();
    let first_heading = root.descendants().find_map(|node| {
        if !matches!(node.data.borrow().value, NodeValue::Heading(_)) {
            return None;
        }
        let mut text = String::new();
        for child in node.descendants() {
            if let NodeValue::Text(t) = &child.data.borrow().value {
                text.push_str(t);
            }
        }
        Some(text)
    });

    serde_json::json!({ "blocks": blocks, "firstHeading": first_heading }).to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn summarizes() {
        assert_eq!(super::summarize("# Hello\n\ntext\n"), r#"{"blocks":2,"firstHeading":"Hello"}"#);
    }
}
