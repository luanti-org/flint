//! Settings format, used by multiple files

use std::collections::HashMap;

pub struct Settings {
    entries: HashMap<String, String>,
}

impl Settings {
    pub fn parse(text: &str) -> Self {
        let text = super::normalize(text);
        let mut entries = HashMap::new();
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let mut value = value.trim().to_string();
            if value == "\"\"\"" {
                value = lines
                    .by_ref()
                    .take_while(|l| l.trim() != "\"\"\"")
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            entries.insert(key.trim().to_string(), value);
        }
        Settings { entries }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

pub fn format_entry(key: &str, value: &str) -> String {
    if value.contains('\n') {
        format!("{key} = \"\"\"\n{value}\n\"\"\"\n")
    } else {
        format!("{key} = {value}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{Settings, format_entry};

    #[test]
    fn key_value() {
        let s = Settings::parse("foo = example text\nbar=baz\n");
        assert_eq!(s.get("foo"), Some("example text"));
        assert_eq!(s.get("bar"), Some("baz"));
        assert_eq!(s.get("missing"), None);
    }

    #[test]
    fn splits_on_first_equals() {
        let s = Settings::parse("expr = a = b\n");
        assert_eq!(s.get("expr"), Some("a = b"));
    }

    #[test]
    fn empty_value() {
        let s = Settings::parse("foo =\n");
        assert_eq!(s.get("foo"), Some(""));
    }

    #[test]
    fn skips_comments_blanks_and_junk() {
        let s = Settings::parse("# comment = no\n\n   \nnot a setting\nfoo = 1\n");
        assert_eq!(s.get("# comment"), None);
        assert_eq!(s.get("not a setting"), None);
        assert_eq!(s.get("foo"), Some("1"));
    }

    #[test]
    fn multiline_value() {
        let s = Settings::parse("bar = \"\"\"\nMultiline\nvalue\n\"\"\"\nafter = 1\n");
        assert_eq!(s.get("bar"), Some("Multiline\nvalue"));
        assert_eq!(s.get("after"), Some("1"));
    }

    #[test]
    fn unterminated_multiline_runs_to_end() {
        let s = Settings::parse("bar = \"\"\"\none\ntwo");
        assert_eq!(s.get("bar"), Some("one\ntwo"));
    }

    #[test]
    fn later_duplicate_wins() {
        let s = Settings::parse("foo = 1\nfoo = 2\n");
        assert_eq!(s.get("foo"), Some("2"));
    }

    #[test]
    fn crlf_and_bom() {
        let s = Settings::parse("\u{feff}foo = 1\r\nbar = 2\r\n");
        assert_eq!(s.get("foo"), Some("1"));
        assert_eq!(s.get("bar"), Some("2"));
    }

    #[test]
    fn format_single_line() {
        assert_eq!(format_entry("foo", "bar"), "foo = bar\n");
    }

    #[test]
    fn format_multiline() {
        assert_eq!(format_entry("foo", "a\nb"), "foo = \"\"\"\na\nb\n\"\"\"\n");
    }

    #[test]
    fn format_round_trips() {
        for value in ["single", "multi\nline\nvalue"] {
            let s = Settings::parse(&format_entry("key", value));
            assert_eq!(s.get("key"), Some(value));
        }
    }
}
