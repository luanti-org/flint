//! `flint lint`: lint every `.lua` file in a mod with selene, plus Luanti specific lints on its Lua and files.
//! With `--write`, lints that can be fixed safely are.

use std::fs;
use std::io::IsTerminal;
use std::path::Path;

use codespan_reporting::diagnostic::{Diagnostic, Label, LabelStyle, Severity as CodespanSeverity};
use codespan_reporting::term::{self, termcolor::ColorChoice, termcolor::StandardStream};
use full_moon::LuaVersion;
use selene_lib::lints::{self, AstContext, Context, Severity};
use selene_lib::standard_library::{Field, FieldKind, StandardLibrary};
use selene_lib::{lint_exists, Checker, CheckerConfig};

use crate::annotate::{self, Level, Span};
use crate::parsers::{depends_txt, mod_conf};

mod filter;
pub(crate) mod fix;
mod luanti;
mod package;

/// with `--write`, fixes are applied and the file linted again until nothing changes, at most this many times
const MAX_FIX_PASSES: usize = 10;

const FIXABLE_NOTE: &str = "fixable with `flint lint --write`";

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

pub fn run(flint: &crate::config::Config, write: bool) -> Result<(), String> {
    let linter = checker(Path::new("."), flint.selene()?, &flint.globals()?)?;
    let files = flint.files(Path::new("."), crate::config::is_lua)?;

    let mut sources = codespan::Files::new();
    let color = if std::io::stderr().is_terminal() { ColorChoice::Auto } else { ColorChoice::Never };
    let stderr = StandardStream::stderr(color);
    let term_config = term::Config::default();
    let emit = |sources: &codespan::Files<String>, diagnostic: &Diagnostic<codespan::FileId>| {
        annotate_diagnostic(sources, diagnostic);
        term::emit(&mut stderr.lock(), &term_config, sources, diagnostic).map_err(|e| e.to_string())
    };

    let (mut errors, mut warnings, mut fixes) = (0, 0, 0);
    let mut count = |severity: Severity| match severity {
        Severity::Allow => None,
        Severity::Error => {
            errors += 1;
            Some(CodespanSeverity::Error)
        }
        Severity::Warning => {
            warnings += 1;
            Some(CodespanSeverity::Warning)
        }
    };

    for path in &files {
        let mut code = match fs::read_to_string(path) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("error: {}: cannot read: {e}", path.display());
                annotate::emit(Level::Error, path, None, "flint lint", &format!("cannot read: {e}"));
                count(Severity::Error);
                continue;
            }
        };
        let owner = luanti::ModInfo::find(path);

        if write {
            let (fixed, applied) = fix_code(&linter, code.clone(), owner.as_ref());
            if applied > 0 {
                fs::write(path, &fixed).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                println!("fixed {applied} issues in {}", path.display());
                fixes += applied;
                code = fixed;
            }
        }
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
                    count(Severity::Error);
                }
                continue;
            }
        };

        let mut findings = linter.test_on(sources.source(file_id), &ast, owner.as_ref());
        findings.sort_by_key(|f| f.diagnostic.start_position());
        for finding in findings {
            let Some(severity) = count(finding.severity) else { continue };
            let mut diagnostic = finding.diagnostic.into_codespan_diagnostic(file_id, severity);
            if finding.fix.is_some() {
                diagnostic.notes.push(FIXABLE_NOTE.to_owned());
            }
            emit(&sources, &diagnostic)?;
        }
    }

    for (dir, kind) in package::packages(Path::new(""))? {
        for rule in &linter.package {
            if rule.severity == Severity::Allow {
                continue;
            }
            let problems = match rule.check(&dir, kind) {
                Ok(problems) => problems,
                Err(e) => {
                    eprintln!("error: {e}");
                    annotate::emit(Level::Error, &dir, None, rule.name, &e);
                    count(Severity::Error);
                    continue;
                }
            };
            for problem in problems {
                let fixable = problem.fix.is_some();
                if let Some(fix) = problem.fix.filter(|_| write) {
                    match fix.apply() {
                        Ok(()) => fixes += 1,
                        Err(e) => {
                            eprintln!("error: {e}");
                            annotate::emit(Level::Error, &problem.path, None, rule.name, &e);
                            count(Severity::Error);
                        }
                    }
                    continue;
                }
                let Some(severity) = count(rule.severity) else { continue };
                let text = fs::read_to_string(&problem.path).unwrap_or_default();
                let file_id = sources.add(problem.path.display().to_string(), text);
                let mut diagnostic = Diagnostic::new(severity)
                    .with_code(rule.name)
                    .with_message(problem.message)
                    .with_labels(vec![Label::primary(file_id, 0..0)]);
                if fixable {
                    diagnostic.notes.push(FIXABLE_NOTE.to_owned());
                }
                emit(&sources, &diagnostic)?;
            }
        }
    }

    if write {
        println!("fixed {fixes} issues");
    }
    println!("{errors} errors, {warnings} warnings in {} files", files.len());
    match errors {
        0 => Ok(()),
        n => Err(format!("lint found {n} errors")),
    }
}

/// `code` with every fixable lint fixed, and how many fixes were applied
fn fix_code(linter: &Linter, mut code: String, owner: Option<&luanti::ModInfo>) -> (String, usize) {
    let mut applied = 0;
    for _ in 0..MAX_FIX_PASSES {
        let Ok(ast) = parse(&code) else { break };
        let edits: Vec<fix::Edit> = linter
            .test_on(&code, &ast, owner)
            .into_iter()
            .filter(|f| f.severity != Severity::Allow)
            .filter_map(|f| f.fix)
            .flatten()
            .collect();
        let (fixed, count) = fix::apply(&code, edits);
        // never keep a fix that breaks the file
        if count == 0 || fixed == code || parse(&fixed).is_err() {
            break;
        }
        code = fixed;
        applied += count;
    }
    (code, applied)
}

/// report `diagnostic` as a GitHub Actions annotation, at its primary label
fn annotate_diagnostic(sources: &codespan::Files<String>, diagnostic: &Diagnostic<codespan::FileId>) {
    if !annotate::enabled() {
        return;
    }
    let level = match diagnostic.severity {
        CodespanSeverity::Bug | CodespanSeverity::Error => Level::Error,
        CodespanSeverity::Warning => Level::Warning,
        CodespanSeverity::Note | CodespanSeverity::Help => Level::Notice,
    };
    let Some(label) = diagnostic.labels.iter().find(|l| l.style == LabelStyle::Primary) else {
        return;
    };
    let location = |byte: usize| sources.location(label.file_id, byte as u32).ok();
    let span = location(label.range.start).zip(location(label.range.end)).map(|(start, end)| Span {
        line: start.line.to_usize() + 1,
        column: start.column.to_usize() + 1,
        end_line: end.line.to_usize() + 1,
        end_column: end.column.to_usize() + 1,
    });

    let mut message = diagnostic.message.clone();
    for text in std::iter::once(&label.message).chain(&diagnostic.notes) {
        if !text.is_empty() {
            message.push('\n');
            message.push_str(text);
        }
    }
    let title = diagnostic.code.as_deref().unwrap_or("flint lint");
    let path = Path::new(sources.name(label.file_id));
    annotate::emit(level, path, span.as_ref(), title, &message);
}

/// a lint found in Lua code, with the edits that fix it if it can be fixed
struct Finding {
    diagnostic: lints::Diagnostic,
    severity: Severity,
    fix: Option<Vec<fix::Edit>>,
}

struct Linter {
    selene: Checker<toml::Value>,
    luanti: Vec<Box<dyn luanti::Rule>>,
    package: Vec<package::Rule>,
    context: Context,
    invalid_filter: Severity,
}

impl Linter {
    fn test_on(&self, code: &str, ast: &full_moon::ast::Ast, owner: Option<&luanti::ModInfo>) -> Vec<Finding> {
        let filters = filter::Filters::new(ast, |lint| {
            lint_exists(lint) || self.luanti.iter().any(|rule| rule.name() == lint)
        });
        let hidden = filter::hide_from_selene(code, ast).and_then(|code| parse(&code).ok());
        let mut findings: Vec<Finding> = self
            .selene
            .test_on(hidden.as_ref().unwrap_or(ast))
            .into_iter()
            .map(|d| Finding { diagnostic: d.diagnostic, severity: d.severity, fix: None })
            .collect();

        let ast_context = AstContext::from_ast(ast);
        for rule in &self.luanti {
            let severity = rule.severity();
            findings.extend(
                rule.pass(ast, &self.context, &ast_context, owner)
                    .into_iter()
                    .map(|found| Finding { diagnostic: found.diagnostic, severity, fix: found.fix }),
            );
        }
        filters.apply(findings, self.invalid_filter)
    }
}

/// build a linter for the package in `dir`, using `config`, mod globals, and the configured `globals`
fn checker(dir: &Path, mut config: CheckerConfig<toml::Value>, globals: &[String]) -> Result<Linter, String> {
    let mut std = StandardLibrary::from_name("lua51").expect("selene is missing lua51");
    // selene wrongly requires a second argument, `table.getn` only takes the table
    if let Some(Field { field_kind: FieldKind::Function(getn), .. }) = std.globals.get_mut("table.getn") {
        getn.arguments.truncate(1);
    }
    let mods = mod_globals(dir)?;
    let extra = mods.iter().chain(globals).map(String::as_str);
    for name in LUANTI_GLOBALS.iter().copied().chain(extra) {
        std.globals
            .insert(name.to_string(), Field::from_field_kind(FieldKind::Any));
    }
    let luanti = luanti::rules(&mut config)?;
    let package = package::rules(&mut config)?;
    let context = Context { standard_library: std.clone(), user_set_standard_library: None };
    let invalid_filter = config.lints.get(filter::INVALID).map_or(Severity::Error, |v| v.to_severity());
    let selene = Checker::new(config, std).map_err(|e| e.to_string())?;
    Ok(Linter { selene, luanti, package, context, invalid_filter })
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
    use super::{checker, fix_code, luanti, mod_globals, parse};
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
        let flint = config::load(dir).unwrap();
        let owner = luanti::ModInfo::find(&dir.join("init.lua"));
        checker(dir, flint.selene().unwrap(), &flint.globals().unwrap())
            .unwrap()
            .test_on(code, &ast, owner.as_ref())
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
    fn configured_globals_are_defined() {
        let dir = dir_with("configured-globals", &[(FILE_NAME, "[linter]
globals = [\"mylib\", \"OTHER\"]
")]);
        assert_eq!(lint(&dir, "mylib.foo(OTHER)
print(nope)"), [("undefined_variable", Severity::Error)]);
    }

    #[test]
    fn globals_must_be_strings() {
        let dir = dir_with("bad-globals", &[(FILE_NAME, "[linter]
globals = \"mylib\"
")]);
        assert!(config::load(&dir).is_err());
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
    fn settings_must_be_in_settingtypes() {
        let code = "print(core.settings:get(\"mymod.a\"), core.settings:get(\"mymod.b\"), core.settings:get(\"mymod.c\"))";
        let dir = dir_with(
            "settingtypes",
            &[
                ("game.conf", ""),
                ("settingtypes.txt", "mymod.a (A) string\n"),
                ("mods/mymod/mod.conf", "name = mymod\n"),
                ("mods/mymod/init.lua", ""),
                ("mods/mymod/settingtypes.txt", "mymod.b (B) string\n"),
            ],
        );
        assert_eq!(lint(&dir.join("mods/mymod"), code), [("luanti_missing_settingtype", Severity::Warning)]);
        let owner = luanti::ModInfo::find(&dir.join("mods/mymod/init.lua")).unwrap();
        assert_eq!(owner.name, "mymod");
        assert_eq!(owner.settings.into_iter().collect::<Vec<_>>(), ["mymod.a", "mymod.b"]);
    }

    /// `code` fixed as if it were in `dir`, and how many fixes were applied
    fn fixed(dir: &Path, code: &str) -> (String, usize) {
        let flint = config::load(dir).unwrap();
        let linter = checker(dir, flint.selene().unwrap(), &flint.globals().unwrap()).unwrap();
        let owner = luanti::ModInfo::find(&dir.join("init.lua"));
        fix_code(&linter, code.to_string(), owner.as_ref())
    }

    #[test]
    fn fixes_are_applied() {
        let dir = dir_with("fix", &[]);
        assert_eq!(fixed(&dir, "minetest.log(minetest)\n"), ("core.log(core)\n".to_string(), 2));
        assert_eq!(fixed(&dir, "core.log(1)\n"), ("core.log(1)\n".to_string(), 0));
    }

    #[test]
    fn deprecated_functions_are_fixed() {
        let dir = dir_with("fix-deprecated", &[("mod.conf", "name = a\nmin_minetest_version = 5.11\n"), ("init.lua", "")]);
        let fix = |code: &str| fixed(&dir, code).0;
        assert_eq!(fix("p:add_player_velocity(p:get_player_velocity())"), "p:add_velocity(p:get_velocity())");
        assert_eq!(fix("p:set_look_pitch(1)\np:set_look_yaw(2)"), "p:set_look_vertical(1)\np:set_look_horizontal(2)");
        assert_eq!(fix("print(core.get_node_group(n, g))\nlocal f = core.get_node_group"), "print(core.get_item_group(n, g))\nlocal f = core.get_item_group");
        assert_eq!(fix("print(core.setting_get_pos(\"a\"))"), "print(core.settings:get_pos(\"a\"))");
        assert_eq!(fix("local f = core.setting_get_pos"), "local f = core.setting_get_pos");
        for code in ["print(p:get_look_pitch(), p:get_look_yaw(), o:get_entity_name())", "print(core.get_perlin(np))"] {
            assert_eq!(fix(code), code);
        }
    }

    #[test]
    fn replacements_newer_than_the_mod_are_not_fixed() {
        let velocity = "p:add_player_velocity(p:get_player_velocity())";
        let setting = "print(core.setting_get_pos(\"a\"))";
        let fix = |name: &str, conf: &str, code: &str| {
            let dir = dir_with(name, &[("mod.conf", conf), ("init.lua", "")]);
            fixed(&dir, code).0
        };
        assert_eq!(fix("version-none", "name = a\n", velocity), velocity);
        assert_eq!(fix("version-5.3", "name = a\nmin_minetest_version = 5.3\n", velocity), velocity);
        assert_eq!(fix("version-5.4", "name = a\nmin_minetest_version = 5.4\n", velocity), "p:add_velocity(p:get_velocity())");
        assert_eq!(fix("version-5.10", "name = a\nmin_minetest_version = 5.10\n", setting), setting);
        assert_eq!(fix("version-old", "name = a\n", "p:set_look_pitch(1)"), "p:set_look_vertical(1)");
    }

    #[test]
    fn replacements_newer_than_the_mod_are_not_reported() {
        let code = "p:add_player_velocity(v)";
        let notes = |name: &str, conf: &str| {
            let dir = dir_with(name, &[("mod.conf", conf), ("init.lua", code)]);
            let flint = config::load(&dir).unwrap();
            let owner = luanti::ModInfo::find(&dir.join("init.lua"));
            checker(&dir, flint.selene().unwrap(), &flint.globals().unwrap())
                .unwrap()
                .test_on(code, &parse(code).unwrap(), owner.as_ref())
                .into_iter()
                .filter(|f| f.diagnostic.code == "luanti_deprecated_add_player_velocity")
                .map(|f| f.diagnostic.notes)
                .collect::<Vec<_>>()
        };
        assert_eq!(notes("report-5.3", "name = a\nmin_minetest_version = 5.3\n"), Vec::<Vec<String>>::new());
        assert_eq!(notes("report-5.4", "name = a\nmin_minetest_version = 5.4\n"), [Vec::<String>::new()]);
        let unknown = notes("report-none", "name = a\n");
        assert_eq!(unknown.len(), 1);
        assert!(unknown[0][0].contains("set `min_minetest_version = 5.4`"), "{unknown:?}");
    }

    #[test]
    fn get_perlin_is_fixed_only_for_new_enough_engines() {
        let code = "print(core.get_perlin(np))";
        let renamed = "print(core.get_value_noise(np))";
        let with_conf = |name: &str, conf: &str| {
            let dir = dir_with(name, &[("mod.conf", conf), ("init.lua", "")]);
            fixed(&dir, code).0
        };
        assert_eq!(with_conf("perlin-none", "name = a\n"), code);
        assert_eq!(with_conf("perlin-old", "name = a\nmin_minetest_version = 5.11\n"), code);
        assert_eq!(with_conf("perlin-new", "name = a\nmin_minetest_version = 5.12\n"), renamed);
        assert_eq!(with_conf("perlin-patch", "name = a\nmin_minetest_version = 5.12.1\n"), renamed);
        assert_eq!(with_conf("perlin-major", "name = a\nmin_minetest_version = 6.0\n"), renamed);
        assert_eq!(with_conf("perlin-bad", "name = a\nmin_minetest_version = latest\n"), code);
    }

    #[test]
    fn min_version_is_the_highest_of_the_mod_and_its_game() {
        let dir = dir_with(
            "min-version",
            &[
                ("game.conf", "min_minetest_version = 5.12\n"),
                ("mods/pack/modpack.conf", "min_minetest_version = 5.4\n"),
                ("mods/pack/a/mod.conf", "name = a\nmin_minetest_version = 5.10\n"),
                ("mods/pack/a/init.lua", ""),
                ("mods/b/mod.conf", "name = b\nmin_minetest_version = 5.13\n"),
                ("mods/b/init.lua", ""),
            ],
        );
        let min = |path: &str| luanti::ModInfo::find(&dir.join(path)).unwrap().min_version;
        assert_eq!(min("mods/pack/a/init.lua"), Some(luanti::Version(5, 12)));
        assert_eq!(min("mods/b/init.lua"), Some(luanti::Version(5, 13)));
    }

    #[test]
    fn env_is_fixed() {
        let dir = dir_with("fix-env", &[]);
        let fix = |code: &str| fixed(&dir, code).0;
        assert_eq!(fix("print(core.env:get_node(pos))"), "print(core.get_node(pos))");
        assert_eq!(fix("print(core.env.registered_nodes, core[\"env\"][\"x\"])"), "print(core.registered_nodes, core[\"x\"])");
        assert_eq!(fix("local env = core.env\nprint(env)"), "local env = core.env\nprint(env)");
        assert_eq!(fixed(&dir, "print(minetest.env:get_node(pos))"), ("print(core.get_node(pos))".to_string(), 2));
    }

    #[test]
    fn physics_override_is_fixed() {
        let dir = dir_with("fix-physics", &[]);
        let fix = |code: &str| fixed(&dir, code).0;
        assert_eq!(fix("p:set_physics_override(1.5, nil, g)"), "p:set_physics_override({ speed = 1.5, gravity = g })");
        assert_eq!(fix("p:set_physics_override(2)"), "p:set_physics_override({ speed = 2 })");
        for code in ["p:set_physics_override(1, f())", "local function f(...) p:set_physics_override(1, ...) end", "p:set_physics_override(1, 1, 1, true)"] {
            assert_eq!(fix(code), code);
        }
    }

    #[test]
    fn allowed_lints_are_not_fixed() {
        let dir = dir_with("fix-filtered", &[]);
        let code = "-- flint: allow(luanti_minetest_global)\nminetest.log(1)\nminetest.log(2)\n";
        let expected = "-- flint: allow(luanti_minetest_global)\nminetest.log(1)\ncore.log(2)\n";
        assert_eq!(fixed(&dir, code), (expected.to_string(), 1));

        let dir = dir_with("fix-allowed", &[(FILE_NAME, "[linter.lints]\nluanti_minetest_global = \"allow\"\n")]);
        assert_eq!(fixed(&dir, "minetest.log(1)\n").1, 0);
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
