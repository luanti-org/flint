//! `description.txt`: mod description before mod.conf

pub const FILE_NAME: &str = "description.txt";

pub fn parse(text: &str) -> String {
    super::normalize(text).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(parse("  A cool mod \n\n"), "A cool mod");
    }

    #[test]
    fn keeps_inner_newlines() {
        assert_eq!(parse("line one\r\nline two\r\n"), "line one\nline two");
    }

    #[test]
    fn strips_bom() {
        assert_eq!(parse("\u{feff}desc"), "desc");
    }

    #[test]
    fn empty_file() {
        assert_eq!(parse(""), "");
        assert_eq!(parse(" \n \n"), "");
    }
}
