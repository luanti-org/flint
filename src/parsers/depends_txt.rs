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
