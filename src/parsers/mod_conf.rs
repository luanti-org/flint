//! `mod.conf`: settings file for a mod

use std::collections::BTreeSet;

use super::settings::Settings;

pub const FILE_NAME: &str = "mod.conf";

#[allow(dead_code)] // we dont use all yet
pub struct ModConf {
    pub name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub depends: Option<BTreeSet<String>>,
    pub optional_depends: Option<BTreeSet<String>>,
    pub author: Option<String>,
    /// Set by cdb only.
    pub release: Option<String>,
    pub textdomain: Option<String>,
}

pub fn parse(text: &str) -> ModConf {
    let settings = Settings::parse(text);
    let string = |key| settings.get(key).map(String::from);
    let list = |key| settings.get(key).map(parse_list);
    ModConf {
        name: string("name"),
        title: string("title"),
        description: string("description"),
        depends: list("depends"),
        optional_depends: list("optional_depends"),
        author: string("author"),
        release: string("release"),
        textdomain: string("textdomain"),
    }
}

fn parse_list(value: &str) -> BTreeSet<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

pub fn format_list(deps: &BTreeSet<String>) -> String {
    deps.iter().cloned().collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::{format_list, parse};
    use std::collections::BTreeSet;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn all_keys() {
        let conf = parse(
            "name = mymod\n\
             title = My Mod\n\
             description = Does things\n\
             depends = default, stairs\n\
             optional_depends = farming\n\
             author = someone\n\
             release = 1234\n\
             textdomain = mymod_td\n",
        );
        assert_eq!(conf.name.as_deref(), Some("mymod"));
        assert_eq!(conf.title.as_deref(), Some("My Mod"));
        assert_eq!(conf.description.as_deref(), Some("Does things"));
        assert_eq!(conf.depends, Some(set(&["default", "stairs"])));
        assert_eq!(conf.optional_depends, Some(set(&["farming"])));
        assert_eq!(conf.author.as_deref(), Some("someone"));
        assert_eq!(conf.release.as_deref(), Some("1234"));
        assert_eq!(conf.textdomain.as_deref(), Some("mymod_td"));
    }

    #[test]
    fn missing_keys_are_none() {
        let conf = parse("");
        assert!(conf.name.is_none());
        assert!(conf.description.is_none());
        assert!(conf.depends.is_none());
        assert!(conf.optional_depends.is_none());
    }

    #[test]
    fn list_tolerates_spacing_and_empty_items() {
        let conf = parse("depends = a,b ,  c,,\n");
        assert_eq!(conf.depends, Some(set(&["a", "b", "c"])));
    }

    #[test]
    fn empty_list_is_some_empty() {
        let conf = parse("depends =\n");
        assert_eq!(conf.depends, Some(BTreeSet::new()));
    }

    #[test]
    fn multiline_description() {
        let conf = parse("description = \"\"\"\nline one\nline two\n\"\"\"\n");
        assert_eq!(conf.description.as_deref(), Some("line one\nline two"));
    }

    #[test]
    fn format_list_is_sorted_and_comma_separated() {
        assert_eq!(format_list(&set(&["stairs", "default"])), "default, stairs");
        assert_eq!(format_list(&BTreeSet::new()), "");
    }
}
