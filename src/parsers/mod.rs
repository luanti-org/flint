//! parsers for luanti stuff

#[cfg(feature = "convert")]
pub mod b3d;
pub mod depends_txt;
pub mod description_txt;
pub mod mod_conf;
pub mod settings;
pub mod settingtypes_txt;
pub mod tr;

fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n")
}

pub fn edit_normalized(text: &str, edit: impl FnOnce(String) -> String) -> String {
    let edited = edit(normalize(text));
    let edited = if text.contains("\r\n") { edited.replace('\n', "\r\n") } else { edited };
    if text.starts_with('\u{feff}') { format!("\u{feff}{edited}") } else { edited }
}

#[cfg(test)]
mod tests {
    use super::{edit_normalized, normalize};

    #[test]
    fn edits_keep_bom_and_crlf() {
        let add = |text: String| text + "c\n";
        assert_eq!(edit_normalized("\u{feff}a\r\nb\r\n", add), "\u{feff}a\r\nb\r\nc\r\n");
        assert_eq!(edit_normalized("a\nb\n", add), "a\nb\nc\n");
    }

    #[test]
    fn strips_bom() {
        assert_eq!(normalize("\u{feff}hello"), "hello");
    }

    #[test]
    fn converts_crlf() {
        assert_eq!(normalize("a\r\nb\r\n"), "a\nb\n");
    }

    #[test]
    fn leaves_plain_text_alone() {
        assert_eq!(normalize("a\nb"), "a\nb");
    }
}
