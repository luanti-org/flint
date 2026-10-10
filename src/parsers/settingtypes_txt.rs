//! `settingtypes.txt`: settings a mod, modpack or game shows in the settings menu

use std::collections::BTreeSet;

pub const FILE_NAME: &str = "settingtypes.txt";

pub fn parse(text: &str) -> BTreeSet<String> {
    super::normalize(text)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('['))
        .filter_map(|line| line.split_whitespace().next())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn names() {
        let text = "\
# a comment
[Category]
[*Subcategory]
mymod.enabled (Enabled) bool true

    mymod_speed (Speed) float 1.5 0 10
mymod.name (Name) string
";
        assert_eq!(parse(text).into_iter().collect::<Vec<_>>(), ["mymod.enabled", "mymod.name", "mymod_speed"]);
    }

    #[test]
    fn crlf_and_bom() {
        assert_eq!(parse("\u{feff}a (A) bool true\r\nb (B) int 1\r\n").len(), 2);
    }
}
