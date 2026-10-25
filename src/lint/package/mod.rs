//! lints on a package's files rather than its Lua, e.g. a legacy `depends.txt`, fixed by moving them to their
//! modern equivalents.

pub(crate) mod legacy_txt;
pub(crate) mod plural_forms;
pub(crate) mod translations;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use selene_lib::lints::Severity;
use selene_lib::CheckerConfig;

use crate::content_type::{self, ContentType};

pub struct Problem {
    pub path: PathBuf,
    pub message: String,
    pub fix: Option<Fix>,
}

#[derive(Default)]
pub struct Fix {
    writes: Vec<(PathBuf, String)>,
    migrations: Vec<(PathBuf, PathBuf)>,
}

impl Fix {
    fn write(&mut self, path: impl Into<PathBuf>, content: String) {
        self.writes.push((path.into(), content));
    }

    fn migrate(&mut self, from: impl Into<PathBuf>, into: impl Into<PathBuf>) {
        self.migrations.push((from.into(), into.into()));
    }

    pub fn apply(self) -> Result<(), String> {
        for (path, content) in &self.writes {
            fs::write(path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
            // migrations say where they went
            if !self.migrations.iter().any(|(_, into)| into == path) {
                println!("wrote {}", path.display());
            }
        }
        for (from, into) in self.migrations {
            fs::remove_file(&from).map_err(|e| format!("cannot remove {}: {e}", from.display()))?;
            println!("migrated {} into {}", from.display(), into.display());
        }
        Ok(())
    }
}

type Check = fn(&Path, ContentType) -> Result<Vec<Problem>, String>;

pub struct Rule {
    pub name: &'static str,
    pub severity: Severity,
    check: Check,
}

impl Rule {
    pub fn check(&self, dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
        (self.check)(dir, kind)
    }
}

/// in the order they're fixed, as later rules add to the conf files earlier ones create
const RULES: &[(&str, Check)] = &[
    (legacy_txt::MISSING_MOD_CONF, legacy_txt::missing_mod_conf),
    (legacy_txt::LEGACY_MODPACK_TXT, legacy_txt::modpack_txt),
    (legacy_txt::LEGACY_DESCRIPTION_TXT, legacy_txt::description_txt),
    (legacy_txt::LEGACY_DEPENDS_TXT, legacy_txt::depends_txt),
    (translations::NAME, translations::check),
];

/// build every package lint, rejecting options for them in `config`
pub fn rules(config: &mut CheckerConfig<toml::Value>) -> Result<Vec<Rule>, String> {
    RULES
        .iter()
        .map(|&(name, check)| {
            if config.config.remove(name).is_some() {
                return Err(format!("[linter.config] {name}: takes no options"));
            }
            let severity = config.lints.get(name).map_or(Severity::Warning, |v| v.to_severity());
            Ok(Rule { name, severity, check })
        })
        .collect()
}

pub fn packages(root: &Path) -> Result<Vec<(PathBuf, ContentType)>, String> {
    let mut found = Vec::new();
    match content_type::detect(root) {
        ContentType::Unknown => {}
        kind => collect_package(&mut found, root, kind)?,
    }
    Ok(found)
}

fn collect_package(found: &mut Vec<(PathBuf, ContentType)>, dir: &Path, kind: ContentType) -> Result<(), String> {
    found.push((dir.to_path_buf(), kind));
    match kind {
        ContentType::Modpack => collect_children(found, dir),
        ContentType::Game => collect_children(found, &dir.join("mods")),
        _ => Ok(()),
    }
}

/// Mods and modpacks inside a modpack or a game's `mods` dir.
fn collect_children(found: &mut Vec<(PathBuf, ContentType)>, dir: &Path) -> Result<(), String> {
    for child in list_dir(dir)? {
        if child.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        match content_type::detect(&child) {
            kind @ (ContentType::Mod | ContentType::Modpack) => collect_package(found, &child, kind)?,
            _ => {}
        }
    }
    Ok(())
}

/// Sorted entries of `dir`, or nothing if it doesn't exist. An empty path means the current dir.
pub(crate) fn list_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let read_path = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let entries = match fs::read_dir(read_path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", read_path.display())),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let name = entry.map_err(|e| format!("cannot read {}: {e}", read_path.display()))?.file_name();
        paths.push(dir.join(name));
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn read_optional(path: impl AsRef<Path>) -> Result<Option<String>, String> {
    let path = path.as_ref();
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::{packages, rules, Rule};
    use crate::content_type::ContentType;
    use selene_lib::CheckerConfig;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn dir_with(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-package-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn all_rules() -> Vec<Rule> {
        rules(&mut CheckerConfig::default()).unwrap()
    }

    fn lint(root: &Path, write: bool) -> Vec<(&'static str, bool)> {
        let mut found = Vec::new();
        for (dir, kind) in packages(root).unwrap() {
            for rule in all_rules() {
                for problem in rule.check(&dir, kind).unwrap() {
                    found.push((rule.name, problem.fix.is_some()));
                    if let Some(fix) = problem.fix.filter(|_| write) {
                        fix.apply().unwrap();
                    }
                }
            }
        }
        found
    }

    #[test]
    fn legacy_mod_is_migrated() {
        let dir = dir_with(
            "legacy-mod",
            &[
                ("init.lua", ""),
                ("description.txt", "A mod\n"),
                ("depends.txt", "default\nsfinv?\n"),
                ("locale/mymod.de.tr", "# textdomain: mymod\nHello=Hallo\n"),
            ],
        );
        let expected = [
            ("luanti_missing_mod_conf", true),
            ("luanti_legacy_description_txt", true),
            ("luanti_legacy_depends_txt", true),
            ("luanti_legacy_tr_translation", true),
        ];
        assert_eq!(lint(&dir, false), expected);
        assert_eq!(lint(&dir, true), expected);
        assert_eq!(lint(&dir, false), []);

        assert_eq!(
            fs::read_to_string(dir.join("mod.conf")).unwrap(),
            "name = flint-package-legacy-mod\ndescription = A mod\ndepends = default\noptional_depends = sfinv\n"
        );
        assert!(!dir.join("description.txt").exists() && !dir.join("depends.txt").exists());
        assert!(!dir.join("locale/mymod.de.tr").exists());
        assert!(fs::read_to_string(dir.join("locale/mymod.de.po")).unwrap().contains("msgstr \"Hallo\""));
    }

    #[test]
    fn mismatch_is_not_fixable() {
        let dir = dir_with(
            "mismatch",
            &[("mod.conf", "name = mymod\ndepends = default\n"), ("depends.txt", "stairs\n")],
        );
        assert_eq!(lint(&dir, true), [("luanti_legacy_depends_txt", false)]);
        assert!(dir.join("depends.txt").exists());
    }

    #[test]
    fn modpack_and_its_mods() {
        let dir = dir_with(
            "modpack",
            &[
                ("modpack.txt", ""),
                ("description.txt", "Pack\n"),
                ("a/init.lua", ""),
                ("a/mod.conf", "name = a\n"),
                ("a/description.txt", "A\n"),
                (".hidden/init.lua", ""),
            ],
        );
        assert_eq!(
            packages(&dir).unwrap(),
            [(dir.clone(), ContentType::Modpack), (dir.join("a"), ContentType::Mod)]
        );
        lint(&dir, true);
        assert_eq!(fs::read_to_string(dir.join("modpack.conf")).unwrap(), "description = Pack\n");
        assert_eq!(fs::read_to_string(dir.join("a/mod.conf")).unwrap(), "name = a\ndescription = A\n");
        assert!(!dir.join("modpack.txt").exists());
    }

    #[test]
    fn options_are_rejected() {
        let mut config: CheckerConfig<toml::Value> =
            toml::from_str("[config.luanti_legacy_depends_txt]\nx = 1\n").unwrap();
        assert!(rules(&mut config).is_err());
    }
}
