//! GitHub Actions workflow commands, so issues show up inline on PRs
//!
//! see https://docs.github.com/en/actions/reference/workflow-commands-for-github-actions

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Notice,
}

#[derive(Debug, Default)]
pub struct Span {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

pub fn enabled() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some_and(|v| v == "true")
}

pub fn emit(level: Level, path: &Path, span: Option<&Span>, title: &str, message: &str) {
    if enabled() {
        println!("{}", command(level, &workspace_path(path), span, title, message));
    }
}

fn command(level: Level, file: &str, span: Option<&Span>, title: &str, message: &str) -> String {
    let name = match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Notice => "notice",
    };
    let mut properties = vec![format!("file={}", escape_property(file))];
    if let Some(span) = span {
        properties.push(format!("line={}", span.line));
        properties.push(format!("endLine={}", span.end_line));

        if span.line == span.end_line {
            properties.push(format!("col={}", span.column));
            properties.push(format!("endColumn={}", span.end_column));
        }
    }
    properties.push(format!("title={}", escape_property(title)));
    format!("::{name} {}::{}", properties.join(","), escape_data(message))
}

fn workspace_path(path: &Path) -> String {
    let relative = std::env::var_os("GITHUB_WORKSPACE")
        .and_then(|workspace| {
            let workspace = std::fs::canonicalize(workspace).ok()?;
            let absolute = std::fs::canonicalize(path).ok()?;
            absolute.strip_prefix(workspace).ok().map(Path::to_path_buf)
        })
        .unwrap_or_else(|| PathBuf::from(path.strip_prefix(".").unwrap_or(path)));
    relative.to_string_lossy().replace('\\', "/")
}

fn escape_data(s: &str) -> String {
    s.replace('%', "%25").replace('\r', "%0D").replace('\n', "%0A")
}

fn escape_property(s: &str) -> String {
    escape_data(s).replace(':', "%3A").replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::{command, workspace_path, Level, Span};
    use std::path::Path;

    #[test]
    fn formats_commands() {
        assert_eq!(
            command(Level::Error, "init.lua", None, "flint format", "not formatted"),
            "::error file=init.lua,title=flint format::not formatted"
        );
        let span = Span { line: 3, column: 5, end_line: 3, end_column: 9 };
        assert_eq!(
            command(Level::Warning, "src/a.lua", Some(&span), "unused_variable", "`x` is unused"),
            "::warning file=src/a.lua,line=3,endLine=3,col=5,endColumn=9,title=unused_variable::`x` is unused"
        );
        let span = Span { line: 3, column: 5, end_line: 4, end_column: 1 };
        assert_eq!(
            command(Level::Notice, "a.lua", Some(&span), "t", "m"),
            "::notice file=a.lua,line=3,endLine=4,title=t::m"
        );
    }

    #[test]
    fn escapes_special_characters() {
        assert_eq!(
            command(Level::Error, "a,b:c.lua", None, "x: y, z", "100%\nnext\r"),
            "::error file=a%2Cb%3Ac.lua,title=x%3A y%2C z::100%25%0Anext%0D"
        );
    }

    #[test]
    fn strips_leading_dot() {
        assert_eq!(workspace_path(Path::new("./does/not/exist.lua")), "does/not/exist.lua");
    }
}
