//! `flint.toml`: project configuration, loosely following biome.js.
//!
//! * `[linter]` is passed to selene
//! * `[formatter]` is passed to StyLua
//! * `[vcs]` controls whether VCS ignore files are respected
//! * `[files]` controls which files are operated on

use std::fs;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use selene_lib::CheckerConfig;
use serde::Deserialize;

pub const FILE_NAME: &str = "flint.toml";

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    linter: toml::Table,
    formatter: toml::Table,
    pub vcs: Vcs,
    pub files: Files,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Vcs {
    pub enabled: bool,
    pub client_kind: ClientKind,
    /// unlike biome, on by default
    pub use_ignore_file: bool,
}

impl Default for Vcs {
    fn default() -> Self {
        Vcs { enabled: true, client_kind: ClientKind::Git, use_ignore_file: true }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    Git,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Files {
    pub includes: Vec<String>,
}

impl Default for Files {
    fn default() -> Self {
        Files { includes: vec!["**/*.lua".to_string()] }
    }
}

pub fn load(dir: &Path) -> Result<Config, String> {
    let config: Config = match fs::read_to_string(dir.join(FILE_NAME)) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{FILE_NAME}: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => return Err(format!("cannot read {FILE_NAME}: {e}")),
    };
    config.stylua()?;
    config.selene()?;
    config.includes()?;
    Ok(config)
}

impl Config {
    /// StyLua config from `[formatter]`, always using LuaJIT syntax
    pub fn stylua(&self) -> Result<stylua_lib::Config, String> {
        if self.formatter.contains_key("syntax") {
            return Err(format!("{FILE_NAME}: [formatter]: `syntax` is not supported, flint always uses LuaJIT"));
        }
        let config: stylua_lib::Config = toml::Value::Table(self.formatter.clone())
            .try_into()
            .map_err(|e| format!("{FILE_NAME}: [formatter]: {e}"))?;
        // luajit == 5.1 + extensions
        Ok(stylua_lib::Config { syntax: stylua_lib::LuaVersion::LuaJIT, ..config })
    }

    /// selene config from `[linter]`
    pub fn selene(&self) -> Result<CheckerConfig<toml::Value>, String> {
        let config: CheckerConfig<toml::Value> = toml::Value::Table(self.linter.clone())
            .try_into()
            .map_err(|e| format!("{FILE_NAME}: [linter]: {e}"))?;
        if config.std.is_some() {
            return Err(format!("{FILE_NAME}: [linter]: `std` is not supported, flint provides the Luanti globals"));
        }
        if !config.exclude.is_empty() {
            return Err(format!("{FILE_NAME}: [linter]: `exclude` is not supported, use `includes` in [files]"));
        }
        Ok(config)
    }

    fn includes(&self) -> Result<Includes, String> {
        let mut set = GlobSetBuilder::new();
        let mut negated = Vec::new();
        for pattern in &self.files.includes {
            let (glob, negate) = match pattern.strip_prefix('!') {
                Some(glob) => (glob, true),
                None => (pattern.as_str(), false),
            };
            let glob = GlobBuilder::new(glob)
                .literal_separator(true)
                .build()
                .map_err(|e| format!("{FILE_NAME}: [files]: {e}"))?;
            set.add(glob);
            negated.push(negate);
        }
        let set = set.build().map_err(|e| format!("{FILE_NAME}: [files]: {e}"))?;
        Ok(Includes { set, negated })
    }

    pub fn files(&self, root: &Path) -> Result<Vec<PathBuf>, String> {
        let includes = self.includes()?;
        let use_ignore_file = self.vcs.enabled && self.vcs.use_ignore_file;
        let walker = WalkBuilder::new(root)
            .standard_filters(use_ignore_file)
            .hidden(true)
            .require_git(false)
            .build();

        let mut files = Vec::new();
        for entry in walker {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let relative = entry.path().strip_prefix(root).unwrap_or(entry.path());
            if includes.is_match(relative) {
                files.push(entry.into_path());
            }
        }
        files.sort();
        Ok(files)
    }
}

struct Includes {
    set: GlobSet,
    negated: Vec<bool>,
}

impl Includes {
    fn is_match(&self, path: &Path) -> bool {
        self.set.matches(path).into_iter().max().is_some_and(|i| !self.negated[i])
    }
}

#[cfg(test)]
mod tests {
    use super::{load, FILE_NAME};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn dir_with(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-config-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files(dir: &Path) -> Vec<String> {
        load(dir)
            .unwrap()
            .files(dir)
            .unwrap()
            .iter()
            .map(|p| p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn defaults_without_config() {
        let dir = dir_with("defaults", &[]);
        let config = load(&dir).unwrap();
        assert!(config.vcs.enabled && config.vcs.use_ignore_file);
        assert_eq!(config.files.includes, ["**/*.lua"]);
        assert_eq!(config.stylua().unwrap().syntax, stylua_lib::LuaVersion::LuaJIT);
    }

    #[test]
    fn example_config_is_valid() {
        let dir = dir_with("example", &[(FILE_NAME, include_str!("../example.flint.toml"))]);
        load(&dir).unwrap();
    }

    #[test]
    fn collects_lua_files_skipping_hidden() {
        let dir = dir_with(
            "collect",
            &[("init.lua", ""), ("src/a.lua", ""), ("readme.txt", ""), (".hidden/b.lua", "")],
        );
        assert_eq!(files(&dir), ["init.lua", "src/a.lua"]);
    }

    #[test]
    fn respects_gitignore_by_default() {
        let dir = dir_with("gitignore", &[(".gitignore", "vendor/\n"), ("init.lua", ""), ("vendor/lib.lua", "")]);
        assert_eq!(files(&dir), ["init.lua"]);
    }

    #[test]
    fn ignore_file_can_be_disabled() {
        let files_with = |name, vcs| {
            let dir = dir_with(
                name,
                &[(FILE_NAME, vcs), (".gitignore", "vendor/\n"), ("init.lua", ""), ("vendor/lib.lua", "")],
            );
            files(&dir)
        };
        assert_eq!(files_with("no-ignore-file", "[vcs]\nuse_ignore_file = false\n"), ["init.lua", "vendor/lib.lua"]);
        assert_eq!(files_with("vcs-disabled", "[vcs]\nenabled = false\n"), ["init.lua", "vendor/lib.lua"]);
    }

    #[test]
    fn includes_override_defaults() {
        let dir = dir_with(
            "includes",
            &[
                (FILE_NAME, "[files]\nincludes = [\"**/*.lua\", \"!vendor/**\", \"vendor/keep.lua\", \"*.luau\"]\n"),
                ("init.lua", ""),
                ("x.luau", ""),
                ("vendor/lib.lua", ""),
                ("vendor/keep.lua", ""),
            ],
        );
        assert_eq!(files(&dir), ["init.lua", "vendor/keep.lua", "x.luau"]);
    }

    #[test]
    fn formatter_is_passed_to_stylua() {
        let dir = dir_with("formatter", &[(FILE_NAME, "[formatter]\nindent_type = \"Spaces\"\ncolumn_width = 80\n")]);
        let stylua = load(&dir).unwrap().stylua().unwrap();
        assert_eq!(stylua.column_width, 80);
        assert_eq!(stylua.indent_type, stylua_lib::IndentType::Spaces);
        assert_eq!(stylua.syntax, stylua_lib::LuaVersion::LuaJIT);
    }

    #[test]
    fn invalid_config_is_an_error() {
        for (name, text) in [
            ("unknown-section", "[nope]\n"),
            ("unknown-vcs-key", "[vcs]\nnope = true\n"),
            ("bad-client", "[vcs]\nclient_kind = \"svn\"\n"),
            ("bad-formatter", "[formatter]\ncolumn_width = \"wide\"\n"),
            ("bad-linter", "[linter.lints]\nunused_variable = \"loud\"\n"),
            ("linter-std", "[linter]\nstd = \"lua51\"\n"),
            ("formatter-syntax", "[formatter]\nsyntax = \"Lua51\"\n"),
            ("bad-glob", "[files]\nincludes = [\"[\"]\n"),
        ] {
            let dir = dir_with(name, &[(FILE_NAME, text)]);
            let err = load(&dir).err().unwrap_or_else(|| panic!("{name} should be rejected"));
            assert!(err.starts_with(FILE_NAME), "{name}: {err}");
        }
    }
}
