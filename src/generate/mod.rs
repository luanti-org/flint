//! `flint generate`: create or update mod package files

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use selene_lib::lints::AstContext;

use crate::annotate::{self, Level};
use crate::content_type::ContentType;
use crate::lint::luanti::missing_settingtype::{self, SettingRead};
use crate::lint::package::{self, legacy_txt, read_optional};
use crate::parsers::settings::Settings;

mod api_versions;
mod conf;
mod repo;
mod settingtypes;
mod strings;
mod translations;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Target {
    #[value(name = "game.conf")]
    GameConf,
    #[value(name = "modpack.conf")]
    ModpackConf,
    #[value(name = "mod.conf")]
    ModConf,
    #[value(name = "texture_pack.conf")]
    TexturePackConf,
    #[value(name = "settingtypes.txt")]
    Settingtypes,
    #[value(name = "pot", alias = "po")]
    Pot,
    #[value(name = ".cdb.json")]
    CdbJson,
    #[value(name = ".luacheckrc")]
    Luacheckrc,
    #[value(name = ".gitattributes")]
    Gitattributes,
}

impl Target {
    fn name(self) -> String {
        self.to_possible_value().expect("no targets are skipped").get_name().to_owned()
    }

    fn applies(self, kind: ContentType, root: bool) -> bool {
        match self {
            Target::ModConf | Target::Settingtypes => kind == ContentType::Mod,
            Target::ModpackConf => kind == ContentType::Modpack,
            Target::GameConf => kind == ContentType::Game,
            Target::TexturePackConf => kind == ContentType::TexturePack,
            Target::Pot => matches!(kind, ContentType::Mod | ContentType::TexturePack),
            Target::CdbJson | Target::Gitattributes => root,
            Target::Luacheckrc => root && kind != ContentType::TexturePack,
        }
    }
}

pub fn run(flint: &crate::config::Config, targets: &[Target], check: bool) -> Result<(), String> {
    let (files, failures) = plan(flint, Path::new(""), targets)?;

    let mut outdated = 0;
    for (path, content) in &files.changed {
        let existing = read_optional(path)?;
        if existing.as_deref() == Some(content.as_str()) {
            continue;
        }
        let (verb, done) = if existing.is_some() { ("update", "updated") } else { ("create", "created") };
        if check {
            println!("would {verb} {}", path.display());
            annotate::emit(Level::Error, path, None, "flint generate", &format!("would {verb}, run `flint generate`"));
            outdated += 1;
            continue;
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        println!("{done} {}", path.display());
    }

    if failures > 0 {
        return Err(format!("{failures} Lua files could not be read or parsed, so weren't generated from"));
    }
    if outdated > 0 {
        return Err(format!("{outdated} files are out of date"));
    }
    Ok(())
}

fn plan(flint: &crate::config::Config, root: &Path, targets: &[Target]) -> Result<(Files, usize), String> {
    let packages = package::packages(root)?;
    let Some(&(_, root_kind)) = packages.first() else {
        return Err("no mod, modpack, game or texture pack found".to_owned());
    };
    let applies = |target: Target| packages.iter().any(|(dir, kind)| target.applies(*kind, dir == root));
    let mut targets = targets.to_vec();
    if targets.is_empty() {
        targets = Target::value_variants().iter().copied().filter(|&t| applies(t)).collect();
    } else if let Some(target) = targets.iter().find(|&&t| !applies(t)) {
        return Err(format!("`{}` doesn't apply to a {}", target.name(), kind_name(root_kind)));
    }
    targets.sort();
    targets.dedup();

    let (lua, failures) = scan(flint, root)?;
    let context = Context { root, packages, lua, flint };
    let mut files = Files::default();
    for target in targets {
        match target {
            Target::ModConf | Target::ModpackConf | Target::GameConf | Target::TexturePackConf => {
                conf::generate(&context, &mut files, target)?
            }
            Target::Settingtypes => settingtypes::generate(&context, &mut files)?,
            Target::Pot => translations::generate(&context, &mut files)?,
            Target::CdbJson => repo::cdb_json(&context, &mut files)?,
            Target::Luacheckrc => repo::luacheckrc(&context, &mut files)?,
            Target::Gitattributes => repo::gitattributes(&context, &mut files)?,
        }
    }
    Ok((files, failures))
}

fn kind_name(kind: ContentType) -> &'static str {
    match kind {
        ContentType::Game => "game",
        ContentType::Mod => "mod",
        ContentType::Modpack => "modpack",
        ContentType::TexturePack => "texture pack",
        ContentType::Unknown => "unknown package",
    }
}

#[derive(Default)]
struct Files {
    changed: BTreeMap<PathBuf, String>,
}

impl Files {
    fn read(&self, path: &Path) -> Result<Option<String>, String> {
        match self.changed.get(path) {
            Some(text) => Ok(Some(text.clone())),
            None => read_optional(path),
        }
    }

    fn write(&mut self, path: PathBuf, content: String) {
        self.changed.insert(path, content);
    }
}

struct LuaFile {
    path: PathBuf,
    requirements: Vec<api_versions::Requirement>,
    settings: Vec<SettingRead>,
    strings: Vec<strings::Translatable>,
    modpaths: Vec<String>,
}

struct Context<'a> {
    root: &'a Path,
    packages: Vec<(PathBuf, ContentType)>,
    lua: Vec<LuaFile>,
    flint: &'a crate::config::Config,
}

impl Context<'_> {
    fn packages(&self, kind: ContentType) -> impl Iterator<Item = &Path> {
        self.packages.iter().filter(move |(_, k)| *k == kind).map(|(dir, _)| dir.as_path())
    }

    fn lua_under<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a LuaFile> {
        self.lua.iter().filter(move |file| file.path.starts_with(dir))
    }

    fn lua_in<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a LuaFile> {
        self.lua_under(dir).filter(move |file| {
            let owner = self.packages.iter().map(|(d, _)| d).filter(|d| file.path.starts_with(d)).max_by_key(|d| d.as_os_str().len());
            owner.is_some_and(|owner| owner == dir)
        })
    }

    fn ancestors<'a>(&'a self, dir: &'a Path) -> impl Iterator<Item = &'a (PathBuf, ContentType)> {
        let mut ancestors: Vec<_> = self.packages.iter().filter(|(d, _)| d != dir && dir.starts_with(d)).collect();
        ancestors.sort_by_key(|(d, _)| std::cmp::Reverse(d.as_os_str().len()));
        ancestors.into_iter()
    }
}

fn scan(flint: &crate::config::Config, root: &Path) -> Result<(Vec<LuaFile>, usize), String> {
    let walk_root = if root.as_os_str().is_empty() { Path::new(".") } else { root };
    let mut files = Vec::new();
    let mut failures = 0;
    for path in flint.files(walk_root, crate::config::is_lua)? {
        let path = root.join(path.strip_prefix(walk_root).unwrap_or(&path));
        let code = match fs::read_to_string(&path) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {}: cannot read: {e}", path.display());
                annotate::emit(Level::Error, &path, None, "flint generate", &format!("cannot read: {e}"));
                failures += 1;
                continue;
            }
        };
        let Ok(ast) = crate::lint::parse(&code) else {
            eprintln!("error: {}: cannot parse, see `flint lint`", path.display());
            annotate::emit(Level::Error, &path, None, "flint generate", "cannot parse, see `flint lint`");
            failures += 1;
            continue;
        };
        let ast_context = AstContext::from_ast(&ast);
        files.push(LuaFile {
            requirements: api_versions::requirements(&ast, &ast_context),
            settings: missing_settingtype::reads(&ast, &ast_context),
            strings: strings::extract(&ast, &ast_context),
            modpaths: conf::modpaths(&ast, &ast_context),
            path,
        });
    }
    Ok((files, failures))
}

fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn package_name(files: &Files, dir: &Path, kind: ContentType) -> Result<String, String> {
    let conf = kind.conf_file().filter(|_| kind != ContentType::Game);
    if let Some(conf) = conf
        && let Some(name) = files.read(&dir.join(conf))?.and_then(|text| Settings::parse(&text).get("name").map(str::to_owned))
        && !name.is_empty()
    {
        return Ok(name);
    }
    legacy_txt::folder_name(dir)
}

fn append(text: &str, lines: &str) -> String {
    crate::parsers::edit_normalized(text, |mut text| {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text + lines
    })
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::{Target, plan};
    use crate::config;
    use std::fs;
    use std::path::{Path, PathBuf};

    pub(super) fn dir_with(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-generate-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(super) fn generated(dir: &Path, targets: &[Target]) -> Vec<(String, String)> {
        let flint = config::load(dir).unwrap();
        let (files, failures) = plan(&flint, dir, targets).unwrap();
        assert_eq!(failures, 0);
        files
            .changed
            .into_iter()
            .filter(|(path, content)| fs::read_to_string(path).ok().as_ref() != Some(content))
            .map(|(path, content)| (super::display(dir, &path), content))
            .collect()
    }

    pub(super) fn file(dir: &Path, targets: &[Target], name: &str) -> Option<String> {
        generated(dir, targets).into_iter().find(|(path, _)| path == name).map(|(_, content)| content)
    }

    fn paths(dir: &Path, targets: &[Target]) -> Vec<String> {
        generated(dir, targets).into_iter().map(|(path, _)| path).collect()
    }

    #[test]
    fn everything_that_applies_by_default() {
        let dir = dir_with(
            "all-mod",
            &[("mod.conf", "name = mymod\n"), ("init.lua", "local S = core.get_translator(\"mymod\")\nprint(S(\"Hi\"), core.settings:get(\"mymod.x\"))\n")],
        );
        assert_eq!(
            paths(&dir, &[]),
            [".cdb.json", ".gitattributes", ".luacheckrc", "locale/mymod.pot", "mod.conf", "settingtypes.txt"]
        );
    }

    #[test]
    fn nested_packages_are_generated_for() {
        let dir = dir_with(
            "all-game",
            &[
                ("game.conf", "title = My Game\n"),
                ("mods/a/mod.conf", "name = a\n"),
                ("mods/a/init.lua", "print(core.settings:get(\"a.x\"))"),
                ("mods/pack/modpack.conf", "name = pack\n"),
                ("mods/pack/b/init.lua", ""),
            ],
        );
        let paths = paths(&dir, &[Target::ModConf, Target::Settingtypes]);
        assert_eq!(paths, ["mods/a/mod.conf", "mods/a/settingtypes.txt", "mods/pack/b/mod.conf"]);
    }

    #[test]
    fn targets_that_dont_apply_are_errors() {
        let dir = dir_with("not-applicable", &[("mod.conf", "name = mymod\n"), ("init.lua", "")]);
        let flint = config::load(&dir).unwrap();
        let err = plan(&flint, &dir, &[Target::GameConf]).err().unwrap();
        assert_eq!(err, "`game.conf` doesn't apply to a mod");
        let empty = dir_with("not-a-package", &[]);
        assert!(plan(&flint, &empty, &[]).is_err());
    }

    #[test]
    fn up_to_date_files_are_unchanged() {
        let dir = dir_with("up-to-date", &[("mod.conf", "name = mymod\ntitle = Mymod\n"), ("init.lua", "")]);
        assert_eq!(paths(&dir, &[Target::ModConf, Target::Settingtypes]), Vec::<String>::new());
    }
}
