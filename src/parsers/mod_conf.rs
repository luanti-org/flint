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
