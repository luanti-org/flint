//! handles package translations and templates

use std::collections::HashMap;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use polib::catalog::Catalog;
use polib::message::Message;
use polib::metadata::CatalogMetadata;
use polib::po_file;

use super::{Context, Files, display, package_name};
use crate::content_type::ContentType;
use crate::lint::package::{list_dir, plural_forms};
use crate::parsers::settings::Settings;

const LOCALE_DIR: &str = "locale";

const PO_HEADER: &str = "msgid \"\"\nmsgstr \"Content-Type: text/plain; charset=UTF-8\\n\"\n";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    ctxt: Option<String>,
    msgid: String,
    plural: Option<String>,
}

#[derive(Default)]
struct Entries {
    order: Vec<Key>,
    found: HashMap<Key, (Vec<String>, Vec<String>)>,
}

impl Entries {
    fn add(&mut self, key: Key, source: String, comment: Option<String>) {
        let (sources, comments) = self.found.entry(key.clone()).or_insert_with(|| {
            self.order.push(key);
            Default::default()
        });
        if !sources.contains(&source) {
            sources.push(source);
        }
        if let Some(comment) = comment.filter(|c| !comments.contains(c)) {
            comments.push(comment);
        }
    }

    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

pub(super) fn generate(context: &Context, files: &mut Files) -> Result<(), String> {
    let packages: Vec<(PathBuf, ContentType)> = context
        .packages
        .iter()
        .filter(|(_, kind)| matches!(kind, ContentType::Mod | ContentType::TexturePack))
        .cloned()
        .collect();

    let mut shared = Vec::new();
    for (dir, kind) in context.packages.iter().filter(|(_, k)| matches!(k, ContentType::Game | ContentType::Modpack)) {
        let conf = dir.join(kind.conf_file().expect("packages have a conf"));
        let domain = textdomain(files, &conf, package_name(files, dir, *kind)?)?;
        let mut mods = context.packages(ContentType::Mod);
        if mods.any(|m| package_name(files, m, ContentType::Mod).is_ok_and(|name| name == domain)) {
            shared.push((conf, domain));
        } else if has_meta(files, &conf)? {
            println!(
                "note: {}: no mod is named after its textdomain `{domain}`, so its title and description can't be \
                 translated, set `textdomain` to one of its mods",
                display(context.root, &conf)
            );
        }
    }

    for (dir, kind) in packages {
        let name = package_name(files, &dir, kind)?;
        let ctxt = |domain: Option<String>| domain.filter(|d| *d != name);
        let mut entries = Entries::default();

        let own = dir.join(kind.conf_file().expect("packages have a conf"));
        let own_domain = textdomain(files, &own, name.clone())?;
        let metas = std::iter::once((own, own_domain)).chain(shared.iter().filter(|(_, d)| *d == name).cloned());
        for (conf, domain) in metas {
            let text = files.read(&conf)?.unwrap_or_default();
            let settings = Settings::parse(&text);
            for key in ["title", "description"] {
                if let Some(value) = settings.get(key).filter(|v| !v.trim().is_empty()) {
                    let key = Key { ctxt: ctxt(Some(domain.clone())), msgid: value.to_owned(), plural: None };
                    entries.add(key, relative(&dir, &conf), None);
                }
            }
        }

        for file in context.lua_in(&dir) {
            for string in &file.strings {
                let key = Key { ctxt: ctxt(string.domain.clone()), msgid: string.msgid.clone(), plural: string.plural.clone() };
                entries.add(key, format!("{}:{}", relative(&dir, &file.path), string.line), string.comment.clone());
            }
        }
        if entries.is_empty() {
            continue;
        }

        let locale = dir.join(LOCALE_DIR);
        files.write(locale.join(format!("{name}.pot")), render_template(&name, &entries)?);
        for po in list_dir(&locale)?.iter().filter(|p| p.extension().is_some_and(|ext| ext == "po")) {
            update_po(context, files, po, &name, &entries)?;
        }
    }
    Ok(())
}

fn textdomain(files: &Files, conf: &Path, default: String) -> Result<String, String> {
    let text = files.read(conf)?.unwrap_or_default();
    Ok(Settings::parse(&text).get("textdomain").filter(|d| !d.is_empty()).map_or(default, str::to_owned))
}

fn has_meta(files: &Files, conf: &Path) -> Result<bool, String> {
    let settings = Settings::parse(&files.read(conf)?.unwrap_or_default());
    Ok(["title", "description"].iter().any(|key| settings.get(key).is_some_and(|v| !v.trim().is_empty())))
}

fn relative(dir: &Path, path: &Path) -> String {
    if let Ok(inside) = path.strip_prefix(dir) {
        return inside.to_string_lossy().replace('\\', "/");
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let up = dir.strip_prefix(parent).map_or(0, |rest| rest.components().count());
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    format!("{}{name}", "../".repeat(up))
}

fn message(key: &Key, sources: &[String], comments: &[String], nplurals: usize) -> Message {
    let mut message = match &key.plural {
        Some(plural) => {
            let mut message = Message::build_plural();
            message.with_msgid_plural(plural.clone()).with_msgstr_plural(vec![String::new(); nplurals]);
            message
        }
        None => {
            let mut message = Message::build_singular();
            message.with_msgstr(String::new());
            message
        }
    };
    message.with_msgid(key.msgid.clone()).with_source(sources.join("\n"));
    if let Some(ctxt) = &key.ctxt {
        message.with_msgctxt(ctxt.clone());
    }
    if !comments.is_empty() {
        message.with_extracted_comments(comments.join("\n"));
    }
    message.done()
}

fn render_messages(catalog: &Catalog) -> Result<String, String> {
    let mut writer = BufWriter::new(Vec::new());
    po_file::write(catalog, &mut writer).map_err(|e| e.to_string())?;
    let bytes = writer.into_inner().map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let messages = text.split_once("\n\n").map_or("", |(_, messages)| messages);
    Ok(messages.trim_end().to_owned())
}

fn render_template(name: &str, entries: &Entries) -> Result<String, String> {
    let mut catalog = Catalog::new(CatalogMetadata::new());
    for key in &entries.order {
        let (sources, comments) = &entries.found[key];
        catalog.append_or_update(message(key, sources, comments, 2));
    }
    Ok(format!(
        "msgid \"\"\n\
         msgstr \"\"\n\
         \"Project-Id-Version: {name}\\n\"\n\
         \"MIME-Version: 1.0\\n\"\n\
         \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
         \"Content-Transfer-Encoding: 8bit\\n\"\n\
         \n\
         {}\n",
        render_messages(&catalog)?
    ))
}

fn update_po(context: &Context, files: &mut Files, path: &Path, name: &str, entries: &Entries) -> Result<(), String> {
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let stem = file_name.strip_suffix(".po").unwrap_or(&file_name);
    let (Some((file_domain, _)), Some((_, lang))) = (stem.split_once('.'), stem.rsplit_once('.')) else {
        return Ok(());
    };
    let Some(text) = files.read(path)? else { return Ok(()) };
    let parse = |text: &str| match crate::panic_guard::catch(|| po_file::parse_from_reader(text.as_bytes())) {
        Ok(Ok(catalog)) => Ok(catalog),
        Ok(Err(e)) => Err(format!("{}: {e}", display(context.root, path))),
        Err(_) => Err(format!("{}: malformed .po file", display(context.root, path))),
    };
    let existing = parse(&text).or_else(|e| parse(&format!("{PO_HEADER}\n{text}")).map_err(|_| e))?;
    let nplurals = match text.contains("Plural-Forms:") {
        true => existing.metadata.plural_rules.nplurals,
        false => plural_forms::for_language(lang)
            .strip_prefix("nplurals=")
            .and_then(|rest| rest.split(';').next()?.parse().ok())
            .unwrap_or(2),
    };
    let has = |msgid: &str| {
        existing.messages().any(|m| m.msgctxt().unwrap_or(file_domain) == file_domain && m.msgid() == msgid)
    };

    let mut missing = Catalog::new(CatalogMetadata::new());
    for key in &entries.order {
        if key.ctxt.as_deref().unwrap_or(name) != file_domain || has(&key.msgid) {
            continue;
        }
        let (sources, comments) = &entries.found[key];
        missing.append_or_update(message(&Key { ctxt: None, ..key.clone() }, sources, comments, nplurals.max(1)));
    }
    if missing.is_empty() {
        return Ok(());
    }

    let messages = render_messages(&missing)?;
    let text = crate::parsers::edit_normalized(&text, |mut text| {
        // a blank line between messages
        if !text.is_empty() && !text.ends_with("\n\n") {
            text.push_str(if text.ends_with('\n') { "\n" } else { "\n\n" });
        }
        text + &messages + "\n"
    });
    files.write(path.to_path_buf(), text);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::Target;
    use super::super::tests::{dir_with, file, generated};
    use super::relative;
    use std::path::Path;

    #[test]
    fn relative_paths() {
        assert_eq!(relative(Path::new("mods/a"), Path::new("mods/a/src/init.lua")), "src/init.lua");
        assert_eq!(relative(Path::new("mods/a"), Path::new("game.conf")), "../../game.conf");
        assert_eq!(relative(Path::new(""), Path::new("mod.conf")), "mod.conf");
    }

    #[test]
    fn template() {
        let code = "local S, PS = core.get_translator(\"mymod\")\n\
                    -- TRANSLATORS: a greeting\n\
                    print(S(\"Hello @1\", n), S(\"Hello @1\", m))\n\
                    print(PS(\"@1 apple\", \"@1 apples\", n, n), core.translate(\"other\", \"Elsewhere\"), S(\"A \\\"quote\\\"\"))\n";
        let dir = dir_with(
            "pot",
            &[("mod.conf", "name = mymod\ntitle = My Mod\ndescription = Does things\n"), ("init.lua", code)],
        );
        assert_eq!(
            file(&dir, &[Target::Pot], "locale/mymod.pot").unwrap(),
            "msgid \"\"\n\
             msgstr \"\"\n\
             \"Project-Id-Version: mymod\\n\"\n\
             \"MIME-Version: 1.0\\n\"\n\
             \"Content-Type: text/plain; charset=UTF-8\\n\"\n\
             \"Content-Transfer-Encoding: 8bit\\n\"\n\
             \n\
             #: mod.conf\n\
             msgid \"My Mod\"\n\
             msgstr \"\"\n\
             \n\
             #: mod.conf\n\
             msgid \"Does things\"\n\
             msgstr \"\"\n\
             \n\
             #. TRANSLATORS: a greeting\n\
             #: init.lua:3\n\
             msgid \"Hello @1\"\n\
             msgstr \"\"\n\
             \n\
             #: init.lua:4\n\
             msgid \"@1 apple\"\n\
             msgid_plural \"@1 apples\"\n\
             msgstr[0] \"\"\n\
             msgstr[1] \"\"\n\
             \n\
             #: init.lua:4\n\
             msgctxt \"other\"\n\
             msgid \"Elsewhere\"\n\
             msgstr \"\"\n\
             \n\
             #: init.lua:4\n\
             msgid \"A \\\"quote\\\"\"\n\
             msgstr \"\"\n"
        );
    }

    #[test]
    fn title_and_description_alone_get_a_template() {
        let dir = dir_with("pot-meta", &[("mod.conf", "name = mymod\ntitle = M\ndescription = D\n"), ("init.lua", "")]);
        let pot = file(&dir, &[Target::Pot], "locale/mymod.pot").unwrap();
        assert!(pot.ends_with("#: mod.conf\nmsgid \"M\"\nmsgstr \"\"\n\n#: mod.conf\nmsgid \"D\"\nmsgstr \"\"\n"), "{pot}");
        // with nothing to translate there's no template
        let dir = dir_with("pot-none", &[("mod.conf", "name = mymod\n"), ("init.lua", "")]);
        assert_eq!(generated(&dir, &[Target::Pot]), []);
    }

    #[test]
    fn po_files_get_missing_strings() {
        let po = "msgid \"\"\n\
                  msgstr \"\"\n\
                  \"Language: ru\\n\"\n\
                  \"Plural-Forms: nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);\\n\"\n\
                  \n\
                  # kept as is\n\
                  msgid \"Old\"\n\
                  msgstr \"Старый\"\n\
                  \n\
                  msgctxt \"mymod\"\n\
                  msgid \"Same domain\"\n\
                  msgstr \"x\"\n";
        let code = "local S, PS = core.get_translator(\"mymod\")\nprint(S(\"Old\"), S(\"Same domain\"), S(\"New\"), PS(\"@1 one\", \"@1 many\", n, n), core.translate(\"other\", \"O\"))";
        let dir = dir_with(
            "po",
            &[("mod.conf", "name = mymod\n"), ("init.lua", code), ("locale/mymod.ru.po", po), ("locale/other.de.po", "")],
        );
        let files = generated(&dir, &[Target::Pot]);
        let ru = &files.iter().find(|(p, _)| p == "locale/mymod.ru.po").unwrap().1;
        assert_eq!(
            ru.strip_prefix(po).unwrap(),
            "\n\
             #: init.lua:2\n\
             msgid \"New\"\n\
             msgstr \"\"\n\
             \n\
             #: init.lua:2\n\
             msgid \"@1 one\"\n\
             msgid_plural \"@1 many\"\n\
             msgstr[0] \"\"\n\
             msgstr[1] \"\"\n\
             msgstr[2] \"\"\n"
        );
        let other = &files.iter().find(|(p, _)| p == "locale/other.de.po").unwrap().1;
        assert_eq!(other, "#: init.lua:2\nmsgid \"O\"\nmsgstr \"\"\n");
    }

    #[test]
    fn po_files_without_a_header_are_read() {
        let po = "# comment\nmsgctxt \"mymod\"\nmsgid \"Old\"\nmsgstr \"Viejo\"\n";
        let dir = dir_with(
            "po-headerless",
            &[
                ("mod.conf", "name = mymod\n"),
                ("init.lua", "local S, PS = core.get_translator(\"mymod\")\nprint(S(\"Old\"), PS(\"New\", \"News\", n))"),
                ("locale/mymod.ja.po", po),
            ],
        );
        assert_eq!(
            file(&dir, &[Target::Pot], "locale/mymod.ja.po").unwrap(),
            format!("{po}\n#: init.lua:2\nmsgid \"New\"\nmsgid_plural \"News\"\nmsgstr[0] \"\"\n")
        );
    }

    #[test]
    fn game_meta_goes_to_the_mod_named_after_its_textdomain() {
        let dir = dir_with(
            "pot-game",
            &[
                ("game.conf", "title = My Game\ndescription = A game\ntextdomain = core_mod\n"),
                ("mods/core_mod/mod.conf", "name = core_mod\n"),
                ("mods/core_mod/init.lua", "print(S(\"x\"))"),
            ],
        );
        let pot = file(&dir, &[Target::Pot], "mods/core_mod/locale/core_mod.pot").unwrap();
        assert!(pot.contains("#: ../../game.conf\nmsgid \"My Game\"\n"), "{pot}");
        assert!(pot.contains("#: ../../game.conf\nmsgid \"A game\"\n"), "{pot}");
    }
}
