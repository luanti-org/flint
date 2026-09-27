//! `locale/*.tr` into gettext `locale/*.po`.

use std::collections::{BTreeMap, HashSet};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use polib::catalog::Catalog;
use polib::message::Message;
use polib::metadata::CatalogMetadata;
use polib::po_file;

use super::{Plan, list_dir, plural_forms, read_optional};
use crate::parsers::tr;

const LOCALE_DIR: &str = "locale";

pub(super) fn plan(plan: &mut Plan, dir: &Path) -> Result<(), String> {
    for tr_path in tr_files(&dir.join(LOCALE_DIR))? {
        let name = tr_path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let stem = name.strip_suffix(".tr").unwrap_or(name);
        let (Some(domain), Some((_, lang))) = (stem.split('.').next(), stem.rsplit_once('.')) else {
            return Err(format!("{}: name must be <textdomain>.<lang>.tr", tr_path.display()));
        };
        let po_path = tr_path.with_file_name(format!("{stem}.po"));

        let text = read_optional(&tr_path)?.unwrap_or_default();
        let tr = tr::parse(&text).map_err(|e| format!("{}: {e}", tr_path.display()))?;
        let catalog = build_catalog(&tr, domain, lang).map_err(|e| format!("{}: {e}", tr_path.display()))?;

        match read_optional(&po_path)? {
            None => plan.write(&po_path, render(&catalog)?),
            Some(existing) => {
                let existing = parse_po(&existing)
                    .map_err(|e| format!("cannot compare with {}: {e}", po_path.display()))?;
                let (ours, theirs) = (translations(&catalog, domain), translations(&existing, domain));
                if ours != theirs {
                    return Err(format!("{} does not match {}", tr_path.display(), po_path.display()));
                }
            }
        }
        plan.migrate(tr_path, po_path);
    }
    Ok(())
}

fn tr_files(locale_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = list_dir(locale_dir)?;
    files.retain(|path| path.is_file() && path.extension().is_some_and(|ext| ext == tr::EXTENSION));
    Ok(files)
}

fn build_catalog(tr: &tr::TrFile, file_domain: &str, lang: &str) -> Result<Catalog, String> {
    let header = format!(
        "MIME-Version: 1.0\n\
         Content-Type: text/plain; charset=UTF-8\n\
         Content-Transfer-Encoding: 8bit\n\
         Language: {lang}\n\
         Plural-Forms: {}\n",
        plural_forms::for_language(lang)
    );
    let metadata = CatalogMetadata::parse(&header).map_err(|e| e.to_string())?;
    let mut catalog = Catalog::new(metadata);
    let mut seen = HashSet::new();

    for entry in &tr.entries {
        let domain = match entry.textdomain.as_deref() {
            Some(d) if !d.is_empty() => d,
            _ => return Err(format!("line {}: entry has no `# textdomain:` above it", entry.line)),
        };
        if !seen.insert((domain, entry.source.as_str())) {
            continue;
        }
        let mut message = Message::build_singular();
        message.with_msgid(entry.source.clone()).with_msgstr(entry.translation.clone());
        if domain != file_domain {
            message.with_msgctxt(domain.to_string());
        }
        if !entry.comments.is_empty() {
            message.with_translator_comments(entry.comments.join("\n"));
        }
        catalog.append_or_update(message.done());
    }
    Ok(catalog)
}

fn parse_po(text: &str) -> Result<Catalog, String> {
    crate::panic_guard::catch(|| po_file::parse_from_reader(text.as_bytes()))
        .map_err(|_| "malformed .po file".to_string())?
        .map_err(|e| e.to_string())
}

fn render(catalog: &Catalog) -> Result<String, String> {
    let mut writer = BufWriter::new(Vec::new());
    po_file::write(catalog, &mut writer).map_err(|e| e.to_string())?;
    let bytes = writer.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn translations(catalog: &Catalog, file_domain: &str) -> BTreeMap<(String, String), String> {
    catalog
        .messages()
        .filter(|m| m.is_translated() && !m.is_fuzzy())
        .map(|m| {
            let domain = m.msgctxt().unwrap_or(file_domain).to_string();
            match (m.msgstr(), m.msgid_plural()) {
                (Ok(msgstr), _) => ((domain, m.msgid().to_string()), msgstr.to_string()),
                (_, Ok(plural)) => ((domain, m.msgid().to_string()), format!("<plural: {plural}>")),
                _ => unreachable!("a message is either singular or plural"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(tr_text: &str) -> String {
        let tr = tr::parse(tr_text).unwrap();
        render(&build_catalog(&tr, "mymod", "de").unwrap()).unwrap()
    }

    #[test]
    fn header() {
        let po = convert("# textdomain: mymod\na=b\n");
        assert!(po.starts_with("msgid \"\"\nmsgstr \"\"\n"), "{po}");
        assert!(po.contains("\"Content-Type: text/plain; charset=UTF-8\\n\""), "{po}");
        assert!(po.contains("\"Language: de\\n\""), "{po}");
        assert!(po.contains("\"Plural-Forms: nplurals=2; plural=(n != 1);\\n\""), "{po}");
    }

    #[test]
    fn entry_in_file_domain_has_no_msgctxt() {
        let po = convert("# textdomain: mymod\nHello @1=Hallo @1\n");
        assert!(po.contains("msgid \"Hello @1\"\nmsgstr \"Hallo @1\"\n"), "{po}");
        assert!(!po.contains("msgctxt"), "{po}");
    }

    #[test]
    fn other_domain_gets_msgctxt() {
        let po = convert("# textdomain: other\na=b\n");
        assert!(po.contains("msgctxt \"other\"\nmsgid \"a\"\nmsgstr \"b\"\n"), "{po}");
    }

    #[test]
    fn escapes_become_po_escapes() {
        let po = convert("# textdomain: mymod\na@=b \"q\"=x@ny\n");
        assert!(po.contains("msgid \"a=b \\\"q\\\"\"\n"), "{po}");
        assert!(po.contains("msgstr \"x\\ny\"\n"), "{po}");
    }

    #[test]
    fn untranslated_and_comments() {
        let po = convert("# textdomain: mymod\n# note\na=\n");
        assert!(po.contains("# note\nmsgid \"a\"\nmsgstr \"\"\n"), "{po}");
    }

    #[test]
    fn first_duplicate_wins() {
        let po = convert("# textdomain: mymod\na=first\na=second\n");
        assert!(po.contains("msgstr \"first\""), "{po}");
        assert!(!po.contains("second"), "{po}");
    }

    #[test]
    fn missing_textdomain_is_error() {
        let tr = tr::parse("a=b\n").unwrap();
        assert!(build_catalog(&tr, "mymod", "de").is_err());
        let tr = tr::parse("# textdomain:\na=b\n").unwrap();
        assert!(build_catalog(&tr, "mymod", "de").is_err());
    }

    #[test]
    fn output_round_trips_through_polib() {
        let tr = tr::parse("# textdomain: mymod\na@nb=c\n# textdomain: other\nd=e\nf=\n").unwrap();
        let catalog = build_catalog(&tr, "mymod", "de").unwrap();
        let parsed = po_file::parse_from_reader(render(&catalog).unwrap().as_bytes()).unwrap();
        assert_eq!(translations(&parsed, "mymod"), translations(&catalog, "mymod"));
        assert_eq!(translations(&catalog, "mymod").len(), 2);
    }

    #[test]
    fn malformed_po_is_error_not_panic() {
        let err = parse_po("msgid \"\"\nmsgstr \"unterminated\n\"\n").err().unwrap();
        assert_eq!(err, "malformed .po file");
    }

    #[test]
    fn fuzzy_and_msgctxt_defaults_are_compared_like_luanti() {
        let po = "msgid \"\"\nmsgstr \"Content-Type: text/plain; charset=UTF-8\\n\"\n\n\
                  msgctxt \"mymod\"\nmsgid \"a\"\nmsgstr \"b\"\n\n\
                  #, fuzzy\nmsgid \"c\"\nmsgstr \"d\"\n";
        let parsed = po_file::parse_from_reader(po.as_bytes()).unwrap();
        let tr = tr::parse("# textdomain: mymod\na=b\nc=\n").unwrap();
        let ours = build_catalog(&tr, "mymod", "de").unwrap();
        assert_eq!(translations(&parsed, "mymod"), translations(&ours, "mymod"));
    }
}

