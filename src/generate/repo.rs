//! files for the package's repository rather than Luanti: `.cdb.json`, `.luacheckrc` and `.gitattributes`.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde::Serialize;
use serde_json::{Map, Value};

use super::{Context, Files, append, display, package_name};
use crate::content_type::ContentType;
use crate::lint::LUANTI_GLOBALS;
use crate::parsers::mod_conf;
use crate::parsers::settings::Settings;

const MAX_SHORT_DESCRIPTION: usize = 200;

/// hosts with an issue tracker at `<repo>/issues`
const ISSUES_HOSTS: &[&str] = &["github.com", "gitlab.com", "codeberg.org"];

const GITATTRIBUTES: &str = "\
# normalize line endings
* text=auto eol=lf

# leave development files out of ContentDB releases, see https://content.luanti.org/help/package_config/#excluding-files
.* export-ignore
flint.toml export-ignore
";

pub(super) fn cdb_json(context: &Context, files: &mut Files) -> Result<(), String> {
    let path = context.root.join(".cdb.json");
    let (dir, kind) = &context.packages[0];
    let existing = files.read(&path)?;
    let (mut json, indent) = match &existing {
        Some(text) => match serde_json::from_str(text) {
            Ok(Value::Object(json)) => (json, indent_of(text)),
            Ok(_) => return Err(format!("{}: must be a JSON object", display(context.root, &path))),
            Err(e) => return Err(format!("{}: {e}", display(context.root, &path))),
        },
        None => {
            let mut json = Map::new();
            json.insert("$schema".to_owned(), "https://content.luanti.org/api/cdb_schema/".into());
            (json, "\t".to_owned())
        }
    };
    let before = json.clone();

    let conf_text = files.read(&dir.join(kind.conf_file().expect("packages have a conf")))?.unwrap_or_default();
    let conf = Settings::parse(&conf_text);
    let mut set = |key: &str, value: Option<String>| {
        if let Some(value) = value.filter(|v| !v.is_empty())
            && !json.contains_key(key)
        {
            json.insert(key.to_owned(), Value::String(value));
        }
    };
    let content_type = match kind {
        ContentType::Game => "GAME",
        ContentType::TexturePack => "TXP",
        _ => "MOD",
    };
    set("type", Some(content_type.to_owned()));
    set("title", conf.get("title").map(str::to_owned));
    let description = conf.get("description").map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "));
    match description {
        Some(d) if d.chars().count() > MAX_SHORT_DESCRIPTION => println!(
            "note: {}: not using `description` as `short_description`, ContentDB allows {MAX_SHORT_DESCRIPTION} characters",
            display(context.root, &path)
        ),
        d => set("short_description", d),
    }
    if let Some(repo) = git_remote(dir).as_deref().and_then(web_url) {
        set("issue_tracker", issue_tracker(&repo));
        set("repo", Some(repo));
    }

    if existing.is_some() && json == before {
        return Ok(());
    }
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes()));
    Value::Object(json).serialize(&mut serializer).map_err(|e| e.to_string())?;
    let mut out = String::from_utf8(out).map_err(|e| e.to_string())?;
    out.push('\n');
    files.write(path, out);
    Ok(())
}

fn indent_of(text: &str) -> String {
    text.lines()
        .find(|line| line.starts_with([' ', '\t']))
        .map(|line| line.chars().take_while(|c| *c == ' ' || *c == '\t').collect())
        .unwrap_or_else(|| "\t".to_owned())
}

fn git_remote(dir: &Path) -> Option<String> {
    let dir = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let output = Command::new("git").arg("-C").arg(dir).args(["remote", "get-url", "origin"]).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn web_url(remote: &str) -> Option<String> {
    let (authority, path) = match remote.split_once("://") {
        Some(("https" | "http" | "ssh" | "git", rest)) => rest.split_once('/')?,
        Some(_) => return None,
        None => remote.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?;
    let host = if remote.starts_with("http") { host } else { host.split(':').next()? };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (!host.is_empty() && !path.is_empty()).then(|| format!("https://{host}/{path}"))
}

fn issue_tracker(repo: &str) -> Option<String> {
    let host = repo.strip_prefix("https://")?.split('/').next()?;
    ISSUES_HOSTS.contains(&host).then(|| format!("{repo}/issues"))
}

pub(super) fn luacheckrc(context: &Context, files: &mut Files) -> Result<(), String> {
    let path = context.root.join(".luacheckrc");
    if files.read(&path)?.is_some() {
        return Ok(());
    }

    let mut own = BTreeSet::new();
    let mut depends = BTreeSet::new();
    for dir in context.packages(ContentType::Mod) {
        own.insert(package_name(files, dir, ContentType::Mod)?);
        let conf = mod_conf::parse(&files.read(&dir.join(mod_conf::FILE_NAME))?.unwrap_or_default());
        depends.extend(conf.depends.into_iter().flatten());
        depends.extend(conf.optional_depends.into_iter().flatten());
    }
    let depends: Vec<&String> = depends.iter().filter(|d| !own.contains(*d)).collect();
    let configured = context.flint.globals()?;

    let mut out = String::from("-- created by `flint generate`, which leaves it alone once it exists\n\nstd = \"luajit\"\n");
    let globals: Vec<&String> = own.iter().chain(&configured).collect();
    if !globals.is_empty() {
        out.push_str("\nglobals = {\n");
        for name in globals {
            out.push_str(&format!("\t\"{name}\",\n"));
        }
        out.push_str("}\n");
    }

    out.push_str("\nread_globals = {\n\t-- Luanti\n");
    let mut fields: Vec<(&str, Vec<&str>)> = Vec::new();
    for global in LUANTI_GLOBALS {
        match global.split_once('.') {
            Some((table, field)) => match fields.iter_mut().find(|(t, _)| *t == table) {
                Some((_, list)) => list.push(field),
                None => fields.push((table, vec![field])),
            },
            None => out.push_str(&format!("\t\"{global}\",\n")),
        }
    }
    for (table, list) in fields {
        let list: Vec<String> = list.iter().map(|f| format!("\"{f}\"")).collect();
        out.push_str(&format!("\t{table} = {{ fields = {{ {} }} }},\n", list.join(", ")));
    }
    if !depends.is_empty() {
        out.push_str("\t-- dependencies\n");
        for name in depends {
            out.push_str(&format!("\t\"{name}\",\n"));
        }
    }
    out.push_str("}\n");
    files.write(path, out);
    Ok(())
}

pub(super) fn gitattributes(context: &Context, files: &mut Files) -> Result<(), String> {
    let path = context.root.join(".gitattributes");
    let Some(text) = files.read(&path)? else {
        files.write(path, GITATTRIBUTES.to_owned());
        return Ok(());
    };
    let rules = |text: &str| -> Vec<Vec<String>> {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| line.split_whitespace().map(str::to_owned).collect())
            .collect()
    };
    let existing = rules(&text);
    let missing: Vec<Vec<String>> = rules(GITATTRIBUTES)
        .into_iter()
        .filter(|rule| !existing.iter().any(|e| e[0] == rule[0] && rule[1..].iter().all(|a| e[1..].contains(a))))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let lines: String = missing.iter().map(|rule| rule.join(" ") + "\n").collect();
    files.write(path, append(&text, &lines));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::super::tests::{dir_with, file, generated};
    use super::{GITATTRIBUTES, issue_tracker, web_url};

    #[test]
    fn remote_urls() {
        let github = Some("https://github.com/owner/repo".to_owned());
        assert_eq!(web_url("git@github.com:owner/repo.git"), github);
        assert_eq!(web_url("https://github.com/owner/repo.git"), github);
        assert_eq!(web_url("https://user:secret@github.com/owner/repo"), github);
        assert_eq!(web_url("ssh://git@github.com:22/owner/repo.git"), github);
        assert_eq!(web_url("https://example.com:8443/owner/repo/"), Some("https://example.com:8443/owner/repo".to_owned()));
        assert_eq!(web_url("/local/path"), None);
        assert_eq!(web_url("file:///local/path"), None);
        assert_eq!(issue_tracker("https://codeberg.org/a/b"), Some("https://codeberg.org/a/b/issues".to_owned()));
        assert_eq!(issue_tracker("https://example.com/a/b"), None);
    }

    #[test]
    fn cdb_json_is_filled_in() {
        let dir = dir_with(
            "cdb",
            &[
                ("mod.conf", "name = mymod\ntitle = My Mod\ndescription = Does\n  things\n"),
                ("init.lua", ""),
                (".cdb.json", "{\n  \"title\": \"Kept\",\n  \"tags\": [\"pvp\"]\n}\n"),
            ],
        );
        let json = file(&dir, &[Target::CdbJson], ".cdb.json").unwrap();
        assert!(json.starts_with("{\n  \"title\": \"Kept\",\n  \"tags\": [\n    \"pvp\"\n  ],\n  \"type\": \"MOD\",\n  \"short_description\": \"Does\""), "{json}");
    }

    #[test]
    fn new_cdb_json() {
        let dir = dir_with("cdb-new", &[("game.conf", "title = G\n")]);
        let json = file(&dir, &[Target::CdbJson], ".cdb.json").unwrap();
        assert!(json.starts_with("{\n\t\"$schema\": \"https://content.luanti.org/api/cdb_schema/\",\n\t\"type\": \"GAME\",\n\t\"title\": \"G\""), "{json}");
    }

    #[test]
    fn luacheckrc_is_only_created() {
        let dir = dir_with(
            "luacheckrc",
            &[
                ("modpack.conf", "name = pack\n"),
                ("a/mod.conf", "name = a\ndepends = b, default\n"),
                ("a/init.lua", ""),
                ("b/mod.conf", "name = b\noptional_depends = sfinv\n"),
                ("b/init.lua", ""),
            ],
        );
        let rc = file(&dir, &[Target::Luacheckrc], ".luacheckrc").unwrap();
        assert!(rc.contains("globals = {\n\t\"a\",\n\t\"b\",\n}\n"), "{rc}");
        assert!(rc.contains("\t\"core\",\n"), "{rc}");
        assert!(rc.contains("\ttable = { fields = { \"copy\", "), "{rc}");
        assert!(rc.contains("\t-- dependencies\n\t\"default\",\n\t\"sfinv\",\n}\n"), "{rc}");

        let dir = dir_with("luacheckrc-exists", &[("mod.conf", "name = a\n"), (".luacheckrc", "-- mine\n")]);
        assert_eq!(generated(&dir, &[Target::Luacheckrc]), []);
    }

    #[test]
    fn gitattributes() {
        let dir = dir_with("gitattributes-new", &[("mod.conf", "name = a\n")]);
        assert_eq!(file(&dir, &[Target::Gitattributes], ".gitattributes").unwrap(), GITATTRIBUTES);

        let dir = dir_with("gitattributes-merge", &[("mod.conf", "name = a\n"), (".gitattributes", "*   text=auto  eol=lf\n*.png binary")]);
        assert_eq!(
            file(&dir, &[Target::Gitattributes], ".gitattributes").unwrap(),
            "*   text=auto  eol=lf\n*.png binary\n.* export-ignore\nflint.toml export-ignore\n"
        );

        let dir = dir_with("gitattributes-crlf", &[("mod.conf", "name = a\n"), (".gitattributes", "* text=auto eol=lf\r\n")]);
        assert_eq!(
            file(&dir, &[Target::Gitattributes], ".gitattributes").unwrap(),
            "* text=auto eol=lf\r\n.* export-ignore\r\nflint.toml export-ignore\r\n"
        );

        let dir = dir_with("gitattributes-done", &[("mod.conf", "name = a\n"), (".gitattributes", GITATTRIBUTES)]);
        assert_eq!(generated(&dir, &[Target::Gitattributes]), []);
    }
}
