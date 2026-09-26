//! `depends.txt`: old mod deps before mod.conf

use std::collections::BTreeSet;

pub const FILE_NAME: &str = "depends.txt";

pub struct DependsTxt {
    pub depends: BTreeSet<String>,
    pub optional_depends: BTreeSet<String>,
}

pub fn parse(text: &str) -> DependsTxt {
    let mut depends = BTreeSet::new();
    let mut optional_depends = BTreeSet::new();
    for line in super::normalize(text).lines() {
        let dep: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(name) = dep.strip_suffix('?') {
            if !name.is_empty() {
                optional_depends.insert(name.to_string());
            }
        } else if !dep.is_empty() {
            depends.insert(dep);
        }
    }
    DependsTxt { depends, optional_depends }
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn set(items: &[&str]) -> std::collections::BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn splits_hard_and_optional() {
        let d = parse("default\nfarming?\nmobs?\n");
        assert_eq!(d.depends, set(&["default"]));
        assert_eq!(d.optional_depends, set(&["farming", "mobs"]));
    }

    #[test]
    fn ignores_blank_lines() {
        let d = parse("\n\ndefault\n\n  \n");
        assert_eq!(d.depends, set(&["default"]));
        assert!(d.optional_depends.is_empty());
    }

    #[test]
    fn strips_all_whitespace() {
        let d = parse("  de fault \t\nfarming ?\n");
        assert_eq!(d.depends, set(&["default"]));
        assert_eq!(d.optional_depends, set(&["farming"]));
    }

    #[test]
    fn lone_question_mark_is_ignored() {
        let d = parse("?\n");
        assert!(d.depends.is_empty());
        assert!(d.optional_depends.is_empty());
    }

    #[test]
    fn deduplicates() {
        let d = parse("default\ndefault\n");
        assert_eq!(d.depends, set(&["default"]));
    }

    #[test]
    fn empty_file() {
        let d = parse("");
        assert!(d.depends.is_empty());
        assert!(d.optional_depends.is_empty());
    }
}
