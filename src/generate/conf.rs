//! handles package conf management

use std::path::Path;

use full_moon::ast::{self, Call, Index, Prefix, Suffix};
use full_moon::visitors::Visitor;
use selene_lib::lints::AstContext;

use super::{Context, Files, Target, display, package_name, valid_name};
use crate::content_type::ContentType;
use crate::lint::luanti::{self, Version};
use crate::lint::package::read_optional;
use crate::parsers::settings::{Settings, set_entry};
use crate::parsers::{depends_txt, mod_conf};

pub(super) fn generate(context: &Context, files: &mut Files, target: Target) -> Result<(), String> {
    let kind = match target {
        Target::ModConf => ContentType::Mod,
        Target::ModpackConf => ContentType::Modpack,
        Target::GameConf => ContentType::Game,
        Target::TexturePackConf => ContentType::TexturePack,
        _ => unreachable!("{target:?} isn't a conf"),
    };
    let dirs: Vec<&Path> = context.packages(kind).collect();
    for dir in dirs {
        generate_one(context, files, dir, kind)?;
    }
    Ok(())
}

fn generate_one(context: &Context, files: &mut Files, dir: &Path, kind: ContentType) -> Result<(), String> {
    let path = dir.join(kind.conf_file().expect("packages have a conf"));
    let shown = display(context.root, &path);
    let original = files.read(&path)?.unwrap_or_default();
    let conf = Settings::parse(&original);
    let mut text = original.clone();

    let name = package_name(files, dir, kind)?;
    if kind != ContentType::Game && conf.get("name").is_none_or(str::is_empty) {
        match valid_name(&name) {
            true => text = set_entry(&text, "name", &name),
            false if kind == ContentType::TexturePack => {}
            false => {
                return Err(format!(
                    "{shown}: the folder name `{name}` can't be the name, which may only have a-z, 0-9 and _, set `name`"
                ));
            }
        }
    }
    if conf.get("title").is_none_or(|t| t.trim().is_empty()) {
        let title = match conf.get("name").filter(|n| kind == ContentType::Game && !n.is_empty()) {
            Some(title) => title.to_owned(),
            None => title_from(&name),
        };
        text = set_entry(&text, "title", &title);
    }
    if conf.get("description").is_none_or(|d| d.trim().is_empty()) {
        println!("note: {shown} has no `description`, add one to describe it in the main menu and ContentDB");
    }

    if kind == ContentType::Mod {
        text = optional_depends(context, dir, &name, &shown, text)?;
    }
    if kind != ContentType::TexturePack {
        text = min_version(context, files, dir, &shown, &conf, text)?;
    }
    if text != original {
        files.write(path, text);
    }
    Ok(())
}

fn title_from(name: &str) -> String {
    name.split(['_', ' ', '-'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn optional_depends(context: &Context, dir: &Path, name: &str, shown: &str, text: String) -> Result<String, String> {
    let conf = mod_conf::parse(&text);
    let mut depends = conf.depends.unwrap_or_default();
    let mut optional = conf.optional_depends.unwrap_or_default();
    let legacy = read_optional(dir.join(depends_txt::FILE_NAME))?.map(|text| depends_txt::parse(&text));
    if let Some(legacy) = &legacy {
        depends.extend(legacy.depends.iter().cloned());
        depends.extend(legacy.optional_depends.iter().cloned());
    }
    let mut added = Vec::new();
    for file in context.lua_in(dir) {
        for modname in &file.modpaths {
            if modname != name && valid_name(modname) && !depends.contains(modname) && optional.insert(modname.clone()) {
                added.push(format!("`{modname}` ({})", display(context.root, &file.path)));
            }
        }
    }
    if added.is_empty() {
        return Ok(text);
    }
    if legacy.is_some() {
        println!(
            "note: {shown}: not adding {} to `optional_depends` while there's a `depends.txt`, run `flint modernize` first",
            added.join(", ")
        );
        return Ok(text);
    }
    println!("{shown}: optional_depends += {}, checked for with `core.get_modpath`", added.join(", "));
    Ok(set_entry(&text, "optional_depends", &mod_conf::format_list(&optional)))
}

fn min_version(
    context: &Context,
    files: &Files,
    dir: &Path,
    shown: &str,
    conf: &Settings,
    text: String,
) -> Result<String, String> {
    let mut needed: Option<(&Path, &super::api_versions::Requirement)> = None;
    for file in context.lua_under(dir) {
        for requirement in &file.requirements {
            if needed.is_none_or(|(_, n)| requirement.version > n.version) {
                needed = Some((&file.path, requirement));
            }
        }
    }
    let Some((file, needed)) = needed else { return Ok(text) };

    let mut current = conf.get("min_minetest_version").and_then(Version::parse);
    for (ancestor, kind) in context.ancestors(dir) {
        let Some(conf) = kind.conf_file().filter(|_| matches!(kind, ContentType::Modpack | ContentType::Game)) else {
            continue;
        };
        let text = files.read(&ancestor.join(conf))?.unwrap_or_default();
        current = current.max(Settings::parse(&text).get("min_minetest_version").and_then(Version::parse));
    }
    if current >= Some(needed.version) {
        return Ok(text);
    }
    println!(
        "{shown}: min_minetest_version = {}, for `{}` at {}:{}",
        needed.version,
        needed.what,
        display(context.root, file),
        needed.line
    );
    Ok(set_entry(&text, "min_minetest_version", &needed.version.to_string()))
}

/// mods named in `core.get_modpath("name")`
pub(super) fn modpaths(ast: &ast::Ast, ast_context: &AstContext) -> Vec<String> {
    let mut visitor = ModpathVisitor { ast_context, found: Vec::new() };
    visitor.visit_ast(ast);
    visitor.found
}

struct ModpathVisitor<'a> {
    ast_context: &'a AstContext,
    found: Vec<String>,
}

impl Visitor for ModpathVisitor<'_> {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        let Prefix::Name(namespace) = call.prefix() else { return };
        let [Suffix::Index(Index::Dot { name, .. }), Suffix::Call(Call::AnonymousCall(args))] =
            call.suffixes().collect::<Vec<_>>()[..]
        else {
            return;
        };
        if name.token().to_string() != "get_modpath" || luanti::luanti_namespace(namespace, self.ast_context).is_none() {
            return;
        }
        let token = match &**args {
            ast::FunctionArgs::Parentheses { arguments, .. } if arguments.len() == 1 => match arguments.iter().next() {
                Some(ast::Expression::String(token)) => token,
                _ => return,
            },
            ast::FunctionArgs::String(token) => token,
            _ => return,
        };
        if let Some(modname) = super::strings::lua_string(token) {
            self.found.push(modname);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::super::tests::{dir_with, file, generated};
    use super::title_from;

    #[test]
    fn titles() {
        assert_eq!(title_from("my_mod"), "My Mod");
        assert_eq!(title_from("mymod"), "Mymod");
        assert_eq!(title_from("a__b-c"), "A B C");
    }

    #[test]
    fn new_mod_conf_from_folder_name() {
        let dir = dir_with("conf-new", &[("modpack.conf", "name = pack\n"), ("my_mod/init.lua", "")]);
        assert_eq!(file(&dir, &[Target::ModConf], "my_mod/mod.conf").unwrap(), "name = my_mod\ntitle = My Mod\n");
    }

    #[test]
    fn invalid_folder_name_is_an_error() {
        let dir = dir_with("conf-bad-name", &[("My Mod/init.lua", ""), ("modpack.conf", "name = pack\ntitle = Pack\n")]);
        let flint = crate::config::load(&dir).unwrap();
        let err = super::super::plan(&flint, &dir, &[Target::ModConf]).err().unwrap();
        assert!(err.contains("`My Mod`"), "{err}");
    }

    #[test]
    fn existing_keys_are_kept() {
        let conf = "# my mod\nname = mymod\ntitle = Custom\ndescription = Does things\ndepends = default\n";
        let dir = dir_with("conf-keep", &[("mod.conf", conf), ("init.lua", "")]);
        assert_eq!(generated(&dir, &[Target::ModConf]), []);
    }

    #[test]
    fn min_version_is_raised_to_what_the_code_needs() {
        let code = "core.register_on_authplayer(f)\nlocal h = core.sha256(\"x\")\nlocal g = core.encode_png(1, 1, {})\n";
        let conf = |name: &str, conf: &str| {
            let dir = dir_with(name, &[("mod.conf", conf), ("init.lua", code)]);
            file(&dir, &[Target::ModConf], "mod.conf")
        };
        assert_eq!(conf("version-none", "name = a\ntitle = A\n").unwrap(), "name = a\ntitle = A\nmin_minetest_version = 5.9\n");
        assert_eq!(
            conf("version-lower", "name = a\nmin_minetest_version = 5.4\ntitle = A\n").unwrap(),
            "name = a\nmin_minetest_version = 5.9\ntitle = A\n"
        );
        assert_eq!(conf("version-higher", "name = a\ntitle = A\nmin_minetest_version = 5.10\n"), None);
    }

    #[test]
    fn min_version_covered_by_the_game_isnt_repeated() {
        let dir = dir_with(
            "version-game",
            &[
                ("game.conf", "title = G\n"),
                ("mods/a/mod.conf", "name = a\ntitle = A\n"),
                ("mods/a/init.lua", "core.sha256(\"x\")"),
                ("mods/b/mod.conf", "name = b\ntitle = B\n"),
                ("mods/b/init.lua", "core.encode_png(1, 1, {})"),
            ],
        );
        assert_eq!(generated(&dir, &[Target::GameConf, Target::ModConf]), [("game.conf".to_owned(), "title = G\nmin_minetest_version = 5.9\n".to_owned())]);
        // without the game's conf, each mod gets its own
        let mods = generated(&dir, &[Target::ModConf]);
        assert_eq!(mods.len(), 2);
        assert!(mods[1].1.ends_with("min_minetest_version = 5.5\n"), "{mods:?}");
    }

    #[test]
    fn game_title_comes_from_deprecated_name() {
        let dir = dir_with("game-name", &[("game.conf", "name = My Game\n")]);
        assert_eq!(file(&dir, &[Target::GameConf], "game.conf").unwrap(), "name = My Game\ntitle = My Game\n");
    }

    #[test]
    fn get_modpath_becomes_optional_depends() {
        let code = "if core.get_modpath(\"default\") then end\nif core.get_modpath(\"stairs\") and core.get_modpath(\"mymod\") then end\nlocal p = core.get_modpath(core.get_current_modname())";
        let dir = dir_with(
            "optional-depends",
            &[("mod.conf", "name = mymod\ntitle = M\ndepends = stairs\noptional_depends = sfinv\n"), ("init.lua", code)],
        );
        assert_eq!(
            file(&dir, &[Target::ModConf], "mod.conf").unwrap(),
            "name = mymod\ntitle = M\ndepends = stairs\noptional_depends = default, sfinv\n"
        );
    }

    #[test]
    fn texture_pack_conf() {
        let dir = dir_with("Texture Pack", &[("texture_pack.conf", "")]);
        assert_eq!(file(&dir, &[Target::TexturePackConf], "texture_pack.conf").unwrap(), "title = Flint Generate Texture Pack\n");
    }
}
