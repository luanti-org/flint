//! `settingtypes.txt`: a line for each of a mod's own settings its code reads that isn't in one yet, following
//! `luanti_missing_settingtype`.

use std::collections::BTreeSet;
use std::path::Path;

use super::{Context, Files, append, display, package_name};
use crate::content_type::ContentType;
use crate::lint::luanti::missing_settingtype::{SettingRead, is_own_setting};
use crate::parsers::settingtypes_txt;

const DEFAULT_NOISE_PARAMS: &str = "0, 1, (250, 250, 250), 0, 3, 0.6, 2";

pub(super) fn generate(context: &Context, files: &mut Files) -> Result<(), String> {
    let dirs: Vec<&Path> = context.packages(ContentType::Mod).collect();
    for dir in dirs {
        let name = package_name(files, dir, ContentType::Mod)?;
        let path = dir.join(settingtypes_txt::FILE_NAME);

        let mut listed = BTreeSet::new();
        let outer = context.ancestors(dir).filter(|(_, k)| matches!(k, ContentType::Modpack | ContentType::Game));
        for settings_dir in std::iter::once(dir).chain(outer.map(|(d, _)| d.as_path())) {
            if let Some(text) = files.read(&settings_dir.join(settingtypes_txt::FILE_NAME))? {
                listed.extend(settingtypes_txt::parse(&text));
            }
        }

        let mut missing: Vec<&SettingRead> = Vec::new();
        for read in context.lua_in(dir).flat_map(|file| &file.settings) {
            if !is_own_setting(&name, &read.name) || listed.contains(&read.name) {
                continue;
            }
            match missing.iter_mut().find(|m| m.name == read.name) {
                // a read with a default says more than one without
                Some(m) if m.default.is_none() && read.default.is_some() => *m = read,
                Some(_) => {}
                None => missing.push(read),
            }
        }
        if missing.is_empty() {
            continue;
        }

        let mut lines = String::new();
        for read in missing {
            match line(&name, read) {
                Some(line) => lines.push_str(&line),
                None => println!(
                    "note: {}: add `{}` by hand, its possible flags can't be worked out from the code",
                    display(context.root, &path),
                    read.name
                ),
            }
        }
        if !lines.is_empty() {
            let text = files.read(&path)?.unwrap_or_default();
            files.write(path, append(&text, &lines));
        }
    }
    Ok(())
}

/// e.g. `mymod.max_speed (Max speed) float 1.5`, or `None` for flags, whose possible values aren't known
fn line(mod_name: &str, read: &SettingRead) -> Option<String> {
    let default = read.default.as_deref();
    let default = match read.kind {
        "bool" => default.filter(|d| matches!(*d, "true" | "false")).unwrap_or("false"),
        "int" => default.filter(|d| d.parse::<i64>().is_ok()).unwrap_or("0"),
        "float" => default.filter(|d| d.parse::<f64>().is_ok()).unwrap_or("0"),
        "v3f" => "(0, 0, 0)",
        "noise_params_3d" => DEFAULT_NOISE_PARAMS,
        "flags" => return None,
        _ => default.filter(|d| !d.contains('\n')).unwrap_or(""),
    };
    let line = format!("{} ({}) {} {default}", read.name, readable(mod_name, &read.name), read.kind);
    Some(format!("{}\n", line.trim_end()))
}

/// `mymod.max_speed` to `Max speed`
fn readable(mod_name: &str, setting: &str) -> String {
    let short = setting.strip_prefix(mod_name).map_or(setting, |rest| &rest[1..]);
    let words = short.replace(['_', '.'], " ");
    let mut chars = words.trim().chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::super::tests::{dir_with, file};

    #[test]
    fn missing_settings_are_added() {
        let code = r#"
            local speed = tonumber(core.settings:get("mymod.max_speed")) or 1.5
            local count = tonumber(core.settings:get("mymod_count")) or 3
            local on = core.settings:get_bool("mymod.on", true)
            local off = core.settings:get_bool("mymod.off")
            local unless = core.settings:get_bool("mymod.unless") ~= false
            local name = core.settings:get("mymod.name") or "bob"
            local spawn = core.settings:get_pos("mymod.spawn")
            local np = core.settings:get_np_group("mymod.noise")
            local flags = core.settings:get_flags("mymod.flags")
            local listed = core.settings:get("mymod.listed")
            local again = core.settings:get_bool("mymod.off", false)
            local other = core.settings:get("other.thing")
        "#;
        let dir = dir_with(
            "settingtypes",
            &[("mod.conf", "name = mymod\n"), ("init.lua", code), ("settingtypes.txt", "mymod.listed (Listed) string")],
        );
        assert_eq!(
            file(&dir, &[Target::Settingtypes], "settingtypes.txt").unwrap(),
            "mymod.listed (Listed) string\n\
             mymod.max_speed (Max speed) float 1.5\n\
             mymod_count (Count) int 3\n\
             mymod.on (On) bool true\n\
             mymod.off (Off) bool false\n\
             mymod.unless (Unless) bool true\n\
             mymod.name (Name) string bob\n\
             mymod.spawn (Spawn) v3f (0, 0, 0)\n\
             mymod.noise (Noise) noise_params_3d 0, 1, (250, 250, 250), 0, 3, 0.6, 2\n"
        );
    }

    #[test]
    fn settings_listed_by_the_game_are_fine() {
        let dir = dir_with(
            "settingtypes-game",
            &[
                ("game.conf", ""),
                ("settingtypes.txt", "a.x (X) string\n"),
                ("mods/a/mod.conf", "name = a\n"),
                ("mods/a/init.lua", "print(core.settings:get(\"a.x\"), core.settings:get(\"a.y\"))"),
            ],
        );
        assert_eq!(file(&dir, &[Target::Settingtypes], "mods/a/settingtypes.txt").unwrap(), "a.y (Y) string\n");
    }
}
