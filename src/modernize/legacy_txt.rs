//! `description.txt` and `depends.txt` into `mod.conf`.

use super::{Plan, read_optional};
use crate::parsers::settings::format_entry;
use crate::parsers::{depends_txt, description_txt, mod_conf};

const MOD_CONF: &str = mod_conf::FILE_NAME;
const DESCRIPTION_TXT: &str = description_txt::FILE_NAME;
const DEPENDS_TXT: &str = depends_txt::FILE_NAME;

pub(super) fn plan(plan: &mut Plan) -> Result<(), String> {
    let conf_text = read_optional(MOD_CONF)?;
    let conf = mod_conf::parse(conf_text.as_deref().unwrap_or(""));
    let mut additions = String::new();

    let description = read_optional(DESCRIPTION_TXT)?.map(|s| description_txt::parse(&s));
    if let Some(desc) = &description {
        match &conf.description {
            Some(existing) if existing.trim() != desc => {
                return Err(format!("{DESCRIPTION_TXT} does not match description in {MOD_CONF}"));
            }
            Some(_) => {}
            None if desc.is_empty() => {}
            None => additions.push_str(&format_entry("description", desc)),
        }
    }

    let depends = read_optional(DEPENDS_TXT)?.map(|s| depends_txt::parse(&s));
    if let Some(txt) = &depends {
        let pairs = [
            ("depends", &conf.depends, &txt.depends),
            ("optional_depends", &conf.optional_depends, &txt.optional_depends),
        ];
        for (key, existing, deps) in pairs {
            match existing {
                Some(existing) if existing != deps => {
                    return Err(format!("{DEPENDS_TXT} does not match {key} in {MOD_CONF}"));
                }
                Some(_) => {}
                None if deps.is_empty() => {}
                None => additions.push_str(&format_entry(key, &mod_conf::format_list(deps))),
            }
        }
    }

    if conf_text.is_none() || !additions.is_empty() {
        let mut out = conf_text.unwrap_or_default();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&additions);
        plan.write(MOD_CONF, out);
    }
    if description.is_some() {
        plan.migrate(DESCRIPTION_TXT, MOD_CONF);
    }
    if depends.is_some() {
        plan.migrate(DEPENDS_TXT, MOD_CONF);
    }

    Ok(())
}
