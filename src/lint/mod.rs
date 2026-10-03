//! `flint lint`: lint every `.lua` file in a mod with selene, plus Luanti specific lints.

use std::fs;
use std::io::IsTerminal;
use std::path::Path;

use codespan_reporting::diagnostic::{Diagnostic, Label, Severity as CodespanSeverity};
use codespan_reporting::term::{self, termcolor::ColorChoice, termcolor::StandardStream};
use full_moon::LuaVersion;
use selene_lib::lints::{AstContext, Context, Severity};
use selene_lib::standard_library::{Field, FieldKind, StandardLibrary};
use selene_lib::{lint_exists, Checker, CheckerConfig, CheckerDiagnostic};

use crate::parsers::{depends_txt, mod_conf};

mod filter;
mod luanti;

/// globals provided by Luanti and LuaJIT on top of Lua 5.1
const LUANTI_GLOBALS: &[&str] = &[
    "core",
    "minetest",
    "vector",
    "dump",
    "dump2",
    "DIR_DELIM",
    "INIT",
    "AreaStore",
    "ItemStack",
    "PcgRandom",
    "PerlinNoise",
    "PerlinNoiseMap",
    "PseudoRandom",
    "Raycast",
    "SecureRandom",
    "Settings",
    "ValueNoise",
    "ValueNoiseMap",
    "VoxelArea",
    "VoxelManip",
    "bit",
    "jit",
    "math.factorial",
    "math.hypot",
    "math.round",
    "math.sign",
    "string.split",
    "string.trim",
    "table.copy",
    "table.indexof",
    "table.insert_all",
    "table.key_value_swap",
    "table.shuffle",
];

pub fn run(flint: &crate::config::Config) -> Result<(), String> {
    let checker = checker(Path::new("."), flint.selene()?)?;
    let files = flint.files(Path::new("."))?;

    let mut sources = codespan::Files::new();
    let color = if std::io::stderr().is_terminal() { ColorChoice::Auto } else { ColorChoice::Never };
    let stderr = StandardStream::stderr(color);
    let term_config = term::Config::default();
    let emit = |sources: &codespan::Files<String>, diagnostic: &Diagnostic<codespan::FileId>| {
        term::emit(&mut stderr.lock(), &term_config, sources, diagnostic).map_err(|e| e.to_string())
    };

    let (mut errors, mut warnings) = (0, 0);
    for path in &files {
        let code = match fs::read_to_string(path) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {}: cannot read: {e}", path.display());
                errors += 1;
                continue;
            }
        };
        let file_id = sources.add(path.display().to_string(), code);

        let ast = match parse(sources.source(file_id)) {
            Ok(ast) => ast,
            Err(parse_errors) => {
                for e in parse_errors {
                    let (start, end) = e.range();
                    let diagnostic = Diagnostic::error()
                        .with_code("parse_error")
                        .with_message(e.error_message())
                        .with_labels(vec![Label::primary(file_id, start.bytes()..end.bytes())]);
                    emit(&sources, &diagnostic)?;
                    errors += 1;
                }
                continue;
            }
        };

        let mut diagnostics = checker.test_on(sources.source(file_id), &ast);
        diagnostics.sort_by_key(|d| d.diagnostic.start_position());
        for d in diagnostics {
            let severity = match d.severity {
                Severity::Allow => continue,
                Severity::Error => {
                    errors += 1;
                    CodespanSeverity::Error
                }
                Severity::Warning => {
                    warnings += 1;
                    CodespanSeverity::Warning
                }
            };
            emit(&sources, &d.diagnostic.into_codespan_diagnostic(file_id, severity))?;
        }
    }

    println!("{errors} errors, {warnings} warnings in {} files", files.len());
    match errors {
        0 => Ok(()),
        n => Err(format!("lint found {n} errors")),
    }
}

struct Linter {
    selene: Checker<toml::Value>,
    luanti: Vec<Box<dyn luanti::Rule>>,
    context: Context,
    invalid_filter: Severity,
}

impl Linter {
    fn test_on(&self, code: &str, ast: &full_moon::ast::Ast) -> Vec<CheckerDiagnostic> {
        let filters = filter::Filters::new(ast, |lint| {
            lint_exists(lint) || self.luanti.iter().any(|rule| rule.name() == lint)
        });
        let hidden = filter::hide_from_selene(code, ast).and_then(|code| parse(&code).ok());
        let mut diagnostics = self.selene.test_on(hidden.as_ref().unwrap_or(ast));

        let ast_context = AstContext::from_ast(ast);
        for rule in &self.luanti {
            let severity = rule.severity();
            diagnostics.extend(
                rule.pass(ast, &self.context, &ast_context)
                    .into_iter()
                    .map(|diagnostic| CheckerDiagnostic { diagnostic, severity }),
            );
        }
        filters.apply(diagnostics, self.invalid_filter)
    }
}

/// build a linter for the package in `dir`, using `config` and mod globals
fn checker(dir: &Path, mut config: CheckerConfig<toml::Value>) -> Result<Linter, String> {
    let mut std = StandardLibrary::from_name("lua51").expect("selene is missing lua51");
    // selene wrongly requires a second argument, `table.getn` only takes the table
    if let Some(Field { field_kind: FieldKind::Function(getn), .. }) = std.globals.get_mut("table.getn") {
        getn.arguments.truncate(1);
    }
    let mods = mod_globals(dir)?;
    for name in LUANTI_GLOBALS.iter().copied().chain(mods.iter().map(String::as_str)) {
        std.globals
            .insert(name.to_string(), Field::from_field_kind(FieldKind::Any));
    }
    let luanti = luanti::rules(&mut config)?;
    let context = Context { standard_library: std.clone(), user_set_standard_library: None };
    let invalid_filter = config.lints.get(filter::INVALID).map_or(Severity::Error, |v| v.to_severity());
    let selene = Checker::new(config, std).map_err(|e| e.to_string())?;
    Ok(Linter { selene, luanti, context, invalid_filter })
}

pub(crate) fn parse(code: &str) -> Result<full_moon::ast::Ast, Vec<full_moon::Error>> {
    full_moon::parse_fallible(code, LuaVersion::luajit()).into_result()
}

// allow mods that are depending on as globals
fn mod_globals(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    collect_mod_globals(dir, &mut names)?;
    names.sort();
    names.dedup();
    Ok(names)
}

fn collect_mod_globals(dir: &Path, names: &mut Vec<String>) -> Result<(), String> {
    if let Ok(text) = fs::read_to_string(dir.join(mod_conf::FILE_NAME)) {
        let conf = mod_conf::parse(&text);
        names.extend(conf.name);
        names.extend(conf.depends.into_iter().flatten());
        names.extend(conf.optional_depends.into_iter().flatten());
    }
    if let Ok(text) = fs::read_to_string(dir.join(depends_txt::FILE_NAME)) {
        let deps = depends_txt::parse(&text);
        names.extend(deps.depends);
        names.extend(deps.optional_depends);
    }

    // allow a mods name as a global
    if dir.join("init.lua").is_file() {
        let dir = dir.canonicalize().map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        names.extend(dir.file_name().map(|n| n.to_string_lossy().into_owned()));
    }

    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        if entry.file_type().is_ok_and(|t| t.is_dir()) && !entry.file_name().to_string_lossy().starts_with('.') {
            collect_mod_globals(&entry.path(), names)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{checker, mod_globals, parse};
    use crate::config::{self, FILE_NAME};
    use selene_lib::lints::Severity;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn dir_with(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-lint-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// lint `code` as if it were in `dir`, returning the non-allowed lint codes and severities
    fn lint(dir: &Path, code: &str) -> Vec<(&'static str, Severity)> {
        let ast = parse(code).unwrap_or_else(|e| panic!("parse failed: {e:?}"));
        let config = config::load(dir).unwrap().selene().unwrap();
        checker(dir, config)
            .unwrap()
            .test_on(code, &ast)
            .into_iter()
            .filter(|d| d.severity != Severity::Allow)
            .map(|d| (d.diagnostic.code, d.severity))
            .collect()
    }

    #[test]
    fn parses_luajit_syntax() {
        assert!(parse("local a, b, c = 0xFFULL, -5LL, 3i").is_ok());
        assert!(parse("for i = 1, 3 do goto continue ::continue:: end").is_ok());
        assert!(parse("local x = ").is_err());
    }

    #[test]
    fn luanti_globals_are_defined() {
        let dir = dir_with("luanti-globals", &[]);
        let code = r#"
            core.log(dump(vector.new(1, 2, 3)), DIR_DELIM)
            core.register_node("a:b", { drop = ItemStack("a:b"), area = VoxelArea, s = Settings })
            print(table.copy({}), string.split("a,b", ","), string.trim(" a "), bit.band(1, 2))
        "#;
        assert_eq!(lint(&dir, code), []);
    }

    #[test]
    fn unknown_global_is_an_error() {
        let dir = dir_with("unknown-global", &[]);
        assert_eq!(lint(&dir, "print(default.foo)"), [("undefined_variable", Severity::Error)]);
    }

    #[test]
    fn deprecated_std_is_a_warning() {
        let dir = dir_with("deprecated", &[]);
        assert_eq!(lint(&dir, "print(table.getn({}))"), [("deprecated", Severity::Warning)]);
    }

    #[test]
    fn collects_mod_names_and_dependencies() {
        let dir = dir_with(
            "mod-globals",
            &[
                ("mymod/mod.conf", "name = mymod
depends = default
optional_depends = sfinv, creative
"),
                ("mymod/init.lua", ""),
                ("legacy/depends.txt", "farming
stairs?
"),
                ("legacy/init.lua", ""),
                ("legacy/textures/readme.txt", ""),
                (".hidden/mod.conf", "name = hidden
"),
            ],
        );
        assert_eq!(
            mod_globals(&dir).unwrap(),
            ["creative", "default", "farming", "legacy", "mymod", "sfinv", "stairs"]
        );
    }

    #[test]
    fn mod_globals_are_defined() {
        let dir = dir_with(
            "mod-globals-defined",
            &[("mod.conf", "name = mymod
depends = default
"), ("init.lua", "")],
        );
        assert_eq!(lint(&dir, "mymod = {}
mymod.node = default.node_sound_stone_defaults()"), []);
    }

    #[test]
    fn config_overrides_severity() {
        let dir = dir_with(
            "config",
            &[(FILE_NAME, "[linter.lints]
undefined_variable = \"allow\"
unused_variable = \"deny\"
")],
        );
        assert_eq!(lint(&dir, "print(nope)
local x = 1"), [("unused_variable", Severity::Error)]);
    }

    #[test]
    fn filter_covers_the_next_statement() {
        let dir = dir_with("filter-next", &[]);
        let code = "-- flint: allow(unused_variable)
local a = 1
local b = 2
";
        assert_eq!(lint(&dir, code), [("unused_variable", Severity::Warning)]);
    }

    #[test]
    fn filter_covers_same_line_and_enclosing_statement() {
        let dir = dir_with("filter-around", &[]);
        assert_eq!(lint(&dir, "local a = 1 -- flint: allow(unused_variable)
local b = 2
").len(), 1);
        assert_eq!(lint(&dir, "local t = {
	-- flint: allow(undefined_variable)
	nope,
}
print(t)
"), []);
    }

    #[test]
    fn filter_covers_whole_function_and_innermost_wins() {
        let dir = dir_with("filter-nested", &[]);
        let code = "
            -- flint: allow(unused_variable)
            local function f()
                local a = 1
                -- flint: deny(unused_variable)
                local b = 2
            end
        ";
        assert_eq!(lint(&dir, code), [("unused_variable", Severity::Error)]);
    }

    #[test]
    fn selene_comments_are_filters_for_luanti_lints() {
        let dir = dir_with("filter-selene", &[]);
        let code = "-- selene: allow(luanti_legacy_physics_override)
core.get_player_by_name(\"a\"):set_physics_override(1, 1, 1)
";
        assert_eq!(lint(&dir, code), []);
    }

    #[test]
    fn global_filter_covers_the_file() {
        let dir = dir_with("filter-global", &[]);
        assert_eq!(lint(&dir, "--# flint: allow(undefined_variable)
print(a)
print(b)
"), []);
        assert_eq!(
            lint(&dir, "print(a)
--# flint: allow(undefined_variable)
print(b)
"),
            [("undefined_variable", Severity::Error), ("undefined_variable", Severity::Error), ("invalid_lint_filter", Severity::Error)]
        );
    }

    #[test]
    fn unknown_lint_in_filter_is_an_error() {
        let dir = dir_with("filter-unknown", &[]);
        assert_eq!(lint(&dir, "-- flint: allow(nope)
print(1)
"), [("invalid_lint_filter", Severity::Error)]);
    }

    #[test]
    fn luanti_lints_run_and_are_configurable() {
        let code = "core.get_player_by_name(\"a\"):set_physics_override(1, 1, 1)";
        let dir = dir_with("luanti-lint", &[]);
        assert_eq!(lint(&dir, code), [("luanti_legacy_physics_override", Severity::Error)]);

        let dir = dir_with(
            "luanti-lint-config",
            &[(FILE_NAME, "[linter.lints]
luanti_legacy_physics_override = \"warn\"
")],
        );
        assert_eq!(lint(&dir, code), [("luanti_legacy_physics_override", Severity::Warning)]);

        let dir = dir_with(
            "luanti-lint-allow",
            &[(FILE_NAME, "[linter.lints]
luanti_legacy_physics_override = \"allow\"
")],
        );
        assert_eq!(lint(&dir, code), []);
    }
}
