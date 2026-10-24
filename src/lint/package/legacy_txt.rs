//! `description.txt`, `depends.txt` and `modpack.txt`, which belong in the package's `.conf` file, and mods
//! without a `mod.conf`.

use std::path::{Path, PathBuf};

use super::{Fix, Problem, read_optional};
use crate::content_type::ContentType;
use crate::parsers::settings::{Settings, format_entry};
use crate::parsers::{depends_txt, description_txt, mod_conf};

pub const MISSING_MOD_CONF: &str = "luanti_missing_mod_conf";
pub const LEGACY_MODPACK_TXT: &str = "luanti_legacy_modpack_txt";
pub const LEGACY_DESCRIPTION_TXT: &str = "luanti_legacy_description_txt";
pub const LEGACY_DEPENDS_TXT: &str = "luanti_legacy_depends_txt";

const DESCRIPTION_TXT: &str = description_txt::FILE_NAME;
const DEPENDS_TXT: &str = depends_txt::FILE_NAME;
/// Legacy modpack marker, its contents are ignored by the engine.
const MODPACK_TXT: &str = "modpack.txt";
const MODPACK_CONF: &str = "modpack.conf";

pub(super) fn missing_mod_conf(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    let conf_path = dir.join(mod_conf::FILE_NAME);
    if kind != ContentType::Mod || conf_path.is_file() {
        return Ok(Vec::new());
    }
    let mut fix = Fix::default();
    fix.write(conf_path, format_entry("name", &folder_name(dir)?));
    Ok(vec![Problem {
        path: dir.join("init.lua"),
        message: format!("mod has no `{}`", mod_conf::FILE_NAME),
        fix: Some(fix),
    }])
}

pub(super) fn modpack_txt(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    let marker = dir.join(MODPACK_TXT);
    if kind != ContentType::Modpack || !marker.is_file() {
        return Ok(Vec::new());
    }
    let conf_path = dir.join(MODPACK_CONF);
    let mut fix = Fix::default();
    if !conf_path.is_file() {
        fix.write(&conf_path, String::new());
    }
    fix.migrate(&marker, conf_path);
    Ok(vec![Problem {
        path: marker,
        message: format!("`{MODPACK_TXT}` is deprecated, use `{MODPACK_CONF}`"),
        fix: Some(fix),
    }])
}

pub(super) fn description_txt(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    let Some(conf_name) = kind.conf_file() else {
        return Ok(Vec::new());
    };
    let path = dir.join(DESCRIPTION_TXT);
    let Some(desc) = read_optional(&path)?.map(|s| description_txt::parse(&s)) else {
        return Ok(Vec::new());
    };
    let conf_path = dir.join(conf_name);
    let conf_text = read_optional(&conf_path)?;
    let conf = Settings::parse(conf_text.as_deref().unwrap_or(""));
    let additions = match conf.get("description") {
        Some(existing) if existing.trim() != desc => {
            return Ok(vec![Problem {
                path,
                message: format!(
                    "`{DESCRIPTION_TXT}` is deprecated, and does not match `description` in `{}`",
                    conf_path.display()
                ),
                fix: None,
            }]);
        }
        Some(_) => String::new(),
        None if desc.is_empty() => String::new(),
        None => format_entry("description", &desc),
    };
    Ok(vec![Problem {
        message: format!("`{DESCRIPTION_TXT}` is deprecated, use `description` in `{conf_name}`"),
        fix: Some(move_into_conf(dir, kind, &path, conf_path, conf_text, &additions)?),
        path,
    }])
}

pub(super) fn depends_txt(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    let path = dir.join(DEPENDS_TXT);
    if kind != ContentType::Mod {
        return Ok(Vec::new());
    }
    let Some(txt) = read_optional(&path)?.map(|s| depends_txt::parse(&s)) else {
        return Ok(Vec::new());
    };
    let conf_path = dir.join(mod_conf::FILE_NAME);
    let conf_text = read_optional(&conf_path)?;
    let conf = mod_conf::parse(conf_text.as_deref().unwrap_or(""));
    let pairs = [
        ("depends", &conf.depends, &txt.depends),
        ("optional_depends", &conf.optional_depends, &txt.optional_depends),
    ];
    let mut additions = String::new();
    for (key, existing, deps) in pairs {
        match existing {
            Some(existing) if existing != deps => {
                return Ok(vec![Problem {
                    path,
                    message: format!(
                        "`{DEPENDS_TXT}` is deprecated, and does not match `{key}` in `{}`",
                        conf_path.display()
                    ),
                    fix: None,
                }]);
            }
            Some(_) => {}
            None if deps.is_empty() => {}
            None => additions.push_str(&format_entry(key, &mod_conf::format_list(deps))),
        }
    }
    Ok(vec![Problem {
        message: format!(
            "`{DEPENDS_TXT}` is deprecated, use `depends` and `optional_depends` in `{}`",
            mod_conf::FILE_NAME
        ),
        fix: Some(move_into_conf(dir, kind, &path, conf_path, conf_text, &additions)?),
        path,
    }])
}

fn move_into_conf(
    dir: &Path,
    kind: ContentType,
    from: &Path,
    conf_path: PathBuf,
    conf_text: Option<String>,
    additions: &str,
) -> Result<Fix, String> {
    let mut fix = Fix::default();
    if conf_text.is_none() || !additions.is_empty() {
        let mut out = match conf_text {
            Some(text) => text,
            None if kind == ContentType::Mod => format_entry("name", &folder_name(dir)?),
            None => String::new(),
        };
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(additions);
        fix.write(&conf_path, out);
    }
    fix.migrate(from, conf_path);
    Ok(fix)
}

/// An empty path means the current dir, which has to be resolved to get its name.
pub(crate) fn folder_name(dir: &Path) -> Result<String, String> {
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let abs = std::fs::canonicalize(dir).map_err(|e| format!("cannot resolve {}: {e}", dir.display()))?;
    abs.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("cannot get folder name of {}", abs.display()))
}
