//! `luanti_minetest_domain` searches package text files for minetest.net to update, lua handled by lua rule

use std::fs;
use std::path::{Path, PathBuf};

use super::{Fix, Problem, list_dir};
use crate::content_type::{self, ContentType};
use crate::lint::luanti::minetest_domain::{NEW, OLD, domains};

pub use crate::lint::luanti::minetest_domain::NAME;

/// safe files to operate on
const EXTENSIONS: &[&str] = &[
    "conf", "txt", "md", "markdown", "rst", "html", "htm", "po", "pot", "tr", "json", "yml", "yaml", "toml", "cfg",
    "ini", "xml",
];

pub(super) fn check(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    let mut problems = Vec::new();
    for path in text_files(dir, dir, kind)? {
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let found = domains(&text);
        if found.is_empty() {
            continue;
        }
        let lines: Vec<String> = found.iter().map(|&at| (text[..at].matches('\n').count() + 1).to_string()).collect();
        let mut fixed = text.clone();
        for &at in found.iter().rev() {
            fixed.replace_range(at..at + OLD.len(), NEW);
        }
        let mut fix = Fix::default();
        fix.write(&path, fixed);
        let plural = if lines.len() > 1 { "s" } else { "" };
        let message = format!("use `{NEW}` instead of `{OLD}`, on line{plural} {}", lines.join(", "));
        problems.push(Problem { path, message, fix: Some(fix) });
    }
    Ok(problems)
}

fn text_files(dir: &Path, root: &Path, kind: ContentType) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for path in list_dir(dir)? {
        if path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        if path.is_dir() {
            if !is_child_package(&path, root, kind) {
                found.extend(text_files(&path, root, kind)?);
            }
        } else if path.extension().is_none_or(|ext| EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))) {
            found.push(path);
        }
    }
    Ok(found)
}

fn is_child_package(dir: &Path, root: &Path, kind: ContentType) -> bool {
    let parent = match kind {
        ContentType::Modpack => root.to_path_buf(),
        ContentType::Game => root.join("mods"),
        _ => return false,
    };
    dir.parent() == Some(parent.as_path()) && matches!(content_type::detect(dir), ContentType::Mod | ContentType::Modpack)
}

#[cfg(test)]
mod tests {
    use super::check;
    use crate::content_type::ContentType;
    use std::fs;
    use std::path::PathBuf;

    fn dir_with(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-minetest-domain-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn text_files_are_checked_and_fixed() {
        let dir = dir_with(
            "mod",
            &[
                ("mod.conf", b"name = a\nurl = https://content.minetest.net/packages/a/\n"),
                ("settingtypes.txt", b"# see https://wiki.minetest.net/a\na.b (B) bool true\n"),
                ("README.md", b"[docs](https://minetest.net) and https://notminetest.net\n\nminetest.net\n"),
                ("LICENSE", b"https://forum.minetest.net\n"),
                ("init.lua", b"-- https://minetest.net\n"),
                ("textures/a.png", b"\x89PNG minetest.net"),
                (".git/config", b"url = https://minetest.net\n"),
            ],
        );
        let problems = check(&dir, ContentType::Mod).unwrap();
        let found: Vec<_> = problems.iter().map(|p| (p.path.strip_prefix(&dir).unwrap().to_path_buf(), p.message.clone())).collect();
        assert_eq!(
            found,
            [
                (PathBuf::from("LICENSE"), "use `luanti.org` instead of `minetest.net`, on line 1".to_owned()),
                (PathBuf::from("README.md"), "use `luanti.org` instead of `minetest.net`, on lines 1, 3".to_owned()),
                (PathBuf::from("mod.conf"), "use `luanti.org` instead of `minetest.net`, on line 2".to_owned()),
                (PathBuf::from("settingtypes.txt"), "use `luanti.org` instead of `minetest.net`, on line 1".to_owned()),
            ]
        );
        for problem in problems {
            problem.fix.unwrap().apply().unwrap();
        }
        assert_eq!(fs::read_to_string(dir.join("mod.conf")).unwrap(), "name = a\nurl = https://content.luanti.org/packages/a/\n");
        assert_eq!(
            fs::read_to_string(dir.join("README.md")).unwrap(),
            "[docs](https://luanti.org) and https://notminetest.net\n\nluanti.org\n"
        );
        assert!(check(&dir, ContentType::Mod).unwrap().is_empty());
    }

    #[test]
    fn mods_inside_are_left_to_their_own_check() {
        let dir = dir_with(
            "game",
            &[
                ("game.conf", b"title = G\nurl = https://minetest.net\n"),
                ("mods/a/mod.conf", b"name = a\nurl = https://minetest.net\n"),
                ("mods/readme.txt", b"https://minetest.net\n"),
            ],
        );
        let paths = |dir: &PathBuf, kind| check(dir, kind).unwrap().into_iter().map(|p| p.path).collect::<Vec<_>>();
        assert_eq!(paths(&dir, ContentType::Game), [dir.join("game.conf"), dir.join("mods/readme.txt")]);
        assert_eq!(paths(&dir.join("mods/a"), ContentType::Mod), [dir.join("mods/a/mod.conf")]);
    }
}
