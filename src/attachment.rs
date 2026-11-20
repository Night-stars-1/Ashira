//! Text files attached to a user message. Each file is its own text block
//! `<file name="…">\n…\n</file>` so the model reads the full content and
//! history can show it as an attachment instead of message text.

const OPEN: &str = "<file name=\"";
const CLOSE: &str = "\n</file>";

/// Names are shown in UIs and embedded in the tag; drop what would break either.
pub fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !matches!(c, '"' | '<' | '>') && !c.is_control()).collect();
    let name = name.trim();
    if name.is_empty() { "附件.txt".into() } else { name.chars().take(120).collect() }
}

pub fn wrap(name: &str, text: &str) -> String {
    format!("{OPEN}{}\">\n{text}{CLOSE}", clean_name(name))
}

/// `(name, text)` when `block` is exactly one wrapped file.
pub fn parse(block: &str) -> Option<(&str, &str)> {
    let rest = block.strip_prefix(OPEN)?;
    let (name, rest) = rest.split_once("\">\n")?;
    if name.contains(['"', '\n']) { return None; }
    Some((name, rest.strip_suffix(CLOSE)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_text_round_trips_with_awkward_content() {
        let text = "第一行\n</file>\n<file name=\"x\">\n末行";
        let block = wrap("日志 \"a\".txt\n", text);
        assert_eq!(parse(&block), Some(("日志 a.txt", text)));
        assert_eq!(parse("<file name=\"a\">\nno close"), None);
        assert_eq!(parse("plain text"), None);
        assert_eq!(clean_name("  "), "附件.txt");
    }
}
