//! `.tr`: old translation format, pre po

pub const EXTENSION: &str = "tr";

pub struct TrFile {
    pub entries: Vec<TrEntry>,
}

pub struct TrEntry {
    pub textdomain: Option<String>,
    pub source: String,
    pub translation: String,
    pub comments: Vec<String>,
    pub line: usize,
}

pub fn parse(text: &str) -> Result<TrFile, String> {
    let text = super::normalize(text);
    let mut lines = text.lines().enumerate();
    let mut textdomain = None;
    let mut comments = Vec::new();
    let mut entries = Vec::new();

    while let Some((index, mut line)) = lines.next() {
        if let Some(rest) = line.strip_prefix("# textdomain:") {
            // Luanti takes the text up to the next `:`.
            textdomain = Some(rest.split(':').next().unwrap_or("").trim().to_string());
            comments.clear();
            continue;
        }
        if line.trim().is_empty() {
            comments.clear();
            continue;
        }
        if let Some(comment) = line.strip_prefix('#') {
            comments.push(comment.strip_prefix(' ').unwrap_or(comment).to_string());
            continue;
        }

        let mut source = String::new();
        let mut translation = String::new();
        let mut in_source = true;
        loop {
            let mut continued = false;
            let mut chars = line.chars();
            while let Some(c) = chars.next() {
                let out = if in_source { &mut source } else { &mut translation };
                match c {
                    '@' => match chars.next() {
                        Some('=') => out.push('='),
                        Some('n') => out.push('\n'),
                        Some(other) => {
                            out.push('@');
                            out.push(other);
                        }
                        None => {
                            out.push('\n');
                            continued = true;
                        }
                    },
                    '=' if in_source => in_source = false,
                    c => out.push(c),
                }
            }
            if !continued {
                break;
            }
            match lines.next() {
                Some((_, next)) => line = next,
                None => break,
            }
        }

        if in_source {
            return Err(format!("line {}: no unescaped `=` between source and translation", index + 1));
        }
        entries.push(TrEntry {
            textdomain: textdomain.clone(),
            source,
            translation,
            comments: std::mem::take(&mut comments),
            line: index + 1,
        });
    }

    Ok(TrFile { entries })
}

#[cfg(test)]
mod tests {
    use super::parse;

    fn pairs(text: &str) -> Vec<(String, String)> {
        parse(text)
            .unwrap()
            .entries
            .into_iter()
            .map(|e| (e.source, e.translation))
            .collect()
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn basic_entry_and_textdomain() {
        let tr = parse("# textdomain: hello\nHello @1, how are you today?=Hallo @1, wie geht es dir heute?\n").unwrap();
        let e = &tr.entries[0];
        assert_eq!(e.textdomain.as_deref(), Some("hello"));
        assert_eq!(e.source, "Hello @1, how are you today?");
        assert_eq!(e.translation, "Hallo @1, wie geht es dir heute?");
        assert_eq!(e.line, 2);
    }

    #[test]
    fn no_textdomain() {
        let tr = parse("a=b\n").unwrap();
        assert_eq!(tr.entries[0].textdomain, None);
    }

    #[test]
    fn textdomain_switches_midway() {
        let tr = parse("# textdomain: one\na=b\n# textdomain: two\nc=d\n").unwrap();
        assert_eq!(tr.entries[0].textdomain.as_deref(), Some("one"));
        assert_eq!(tr.entries[1].textdomain.as_deref(), Some("two"));
    }

    #[test]
    fn textdomain_stops_at_colon() {
        let tr = parse("# textdomain: dom:extra\na=b\n").unwrap();
        assert_eq!(tr.entries[0].textdomain.as_deref(), Some("dom"));
    }

    #[test]
    fn untranslated() {
        assert_eq!(pairs("Hello=\n"), vec![pair("Hello", "")]);
    }

    #[test]
    fn escaped_equals() {
        assert_eq!(pairs("a@=b=c@=d\n"), vec![pair("a=b", "c=d")]);
    }

    #[test]
    fn unescaped_equals_in_translation_is_literal() {
        assert_eq!(pairs("a=b=c\n"), vec![pair("a", "b=c")]);
    }

    #[test]
    fn at_n_is_newline() {
        assert_eq!(pairs("one@ntwo=eins@nzwei\n"), vec![pair("one\ntwo", "eins\nzwei")]);
    }

    #[test]
    fn trailing_at_continues_line() {
        let tr = parse("one@\ntwo=eins@\nzwei\nnext=x\n").unwrap();
        assert_eq!(tr.entries[0].source, "one\ntwo");
        assert_eq!(tr.entries[0].translation, "eins\nzwei");
        assert_eq!(tr.entries[0].line, 1);
        assert_eq!(tr.entries[1].source, "next");
        assert_eq!(tr.entries[1].line, 4);
    }

    #[test]
    fn trailing_at_at_end_of_file() {
        assert_eq!(pairs("a=b@"), vec![pair("a", "b\n")]);
    }

    #[test]
    fn other_escapes_are_kept() {
        assert_eq!(pairs("@@ @1 @x=@@ @1 @x\n"), vec![pair("@@ @1 @x", "@@ @1 @x")]);
    }

    #[test]
    fn comments_attach_to_next_entry() {
        let tr = parse("# textdomain: d\n# header\n\n# first\n#second\na=b\nc=d\n").unwrap();
        assert_eq!(tr.entries[0].comments, vec!["first", "second"]);
        assert!(tr.entries[1].comments.is_empty());
    }

    #[test]
    fn blank_and_whitespace_lines_skipped() {
        assert_eq!(pairs("\n   \na=b\n\n"), vec![pair("a", "b")]);
    }

    #[test]
    fn crlf_and_bom() {
        assert_eq!(pairs("\u{feff}a=b\r\nc=d\r\n"), vec![pair("a", "b"), pair("c", "d")]);
    }

    #[test]
    fn missing_equals_is_error() {
        let err = parse("a=b\nno separator\n").err().unwrap();
        assert!(err.starts_with("line 2:"), "{err}");
        assert!(parse("only@=escaped\n").is_err());
    }
}
