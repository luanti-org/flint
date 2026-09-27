//! `description.txt`, `depends.txt` and `modpack.txt` into the package's `.conf` file.

use std::path::Path;

use super::{Plan, read_optional};
use crate::content_type::ContentType;
use crate::parsers::settings::{Settings, format_entry};
use crate::parsers::{depends_txt, description_txt, mod_conf};

const DESCRIPTION_TXT: &str = description_txt::FILE_NAME;
const DEPENDS_TXT: &str = depends_txt::FILE_NAME;
/// Legacy modpack marker, its contents are ignored by the engine.
const MODPACK_TXT: &str = "modpack.txt";

pub(super) fn plan(plan: &mut Plan, dir: &Path, kind: ContentType) -> Result<(), String> {
    let Some(conf_name) = kind.conf_file() else {
        return Ok(());
    };
    let conf_path = dir.join(conf_name);
    let conf_text = read_optional(&conf_path)?;
    let conf = Settings::parse(conf_text.as_deref().unwrap_or(""));
    let mut additions = String::new();
    let mut migrated = Vec::new();

    let description_path = dir.join(DESCRIPTION_TXT);
    if let Some(desc) = read_optional(&description_path)?.map(|s| description_txt::parse(&s)) {
        match conf.get("description") {
            Some(existing) if existing.trim() != desc => {
                return Err(format!(
                    "{} does not match description in {}",
                    description_path.display(),
                    conf_path.display()
                ));
            }
            Some(_) => {}
            None if desc.is_empty() => {}
            None => additions.push_str(&format_entry("description", &desc)),
        }
        migrated.push(description_path);
    }

    if kind == ContentType::Mod {
        let depends_path = dir.join(DEPENDS_TXT);
        if let Some(txt) = read_optional(&depends_path)?.map(|s| depends_txt::parse(&s)) {
            let conf = mod_conf::parse(conf_text.as_deref().unwrap_or(""));
            let pairs = [
                ("depends", &conf.depends, &txt.depends),
                ("optional_depends", &conf.optional_depends, &txt.optional_depends),
            ];
            for (key, existing, deps) in pairs {
                match existing {
                    Some(existing) if existing != deps => {
                        return Err(format!(
                            "{} does not match {key} in {}",
                            depends_path.display(),
                            conf_path.display()
                        ));
                    }
                    Some(_) => {}
                    None if deps.is_empty() => {}
                    None => additions.push_str(&format_entry(key, &mod_conf::format_list(deps))),
                }
            }
            migrated.push(depends_path);
        }
    }

    if kind == ContentType::Modpack {
        let marker = dir.join(MODPACK_TXT);
        if marker.is_file() {
            migrated.push(marker);
        }
    }

    if conf_text.is_none() && kind == ContentType::Mod {
        additions.insert_str(0, &format_entry("name", &folder_name(dir)?));
    }
    if conf_text.is_none() || !additions.is_empty() {
        let mut out = conf_text.unwrap_or_default();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&additions);
        plan.write(&conf_path, out);
    }
    for from in migrated {
        plan.migrate(from, &conf_path);
    }

    Ok(())
}

/// An empty path means the current dir, which has to be resolved to get its name.
fn folder_name(dir: &Path) -> Result<String, String> {
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let abs = std::fs::canonicalize(dir).map_err(|e| format!("cannot resolve {}: {e}", dir.display()))?;
    abs.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("cannot get folder name of {}", abs.display()))
}
