//! parsers for luanti stuff

pub mod depends_txt;
pub mod description_txt;
pub mod mod_conf;
pub mod settings;

fn normalize(text: &str) -> String {
    text.trim_start_matches('\u{feff}').replace("\r\n", "\n")
}

#[cfg(test)]
mod tests {
    use super::normalize;

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
