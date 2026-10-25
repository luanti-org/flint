//! `flint format`: formats every `.lua` file in a mod with StyLua.

use std::fs;
use std::path::Path;

use stylua_lib::{Config, OutputVerification};

use crate::annotate::{self, Level, Span};

pub fn run(flint: &crate::config::Config, check: bool) -> Result<(), String> {
    let config = flint.stylua()?;
    let files = flint.files(Path::new("."), crate::config::is_lua)?;

    let (mut unformatted, mut failures) = (0, 0);
    for path in &files {
        match format_file(path, config, check) {
            Ok(changed) => unformatted += changed as usize,
            Err(e) => {
                eprintln!("error: {}: {e}", path.display());
                annotate::emit(Level::Error, path, None, "flint format", &e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        return Err(format!("{failures} of {} files could not be formatted", files.len()));
    }
    if check && unformatted > 0 {
        return Err(format!("{unformatted} of {} files are not formatted", files.len()));
    }
    Ok(())
}

/// format `path` in place, or with `check` only report it, returning whether it needed formatting
fn format_file(path: &Path, config: Config, check: bool) -> Result<bool, String> {
    let code = fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
    let formatted = format_code(&code, config)?;
    if formatted == code {
        return Ok(false);
    }
    if check {
        println!("would format {}", path.display());
        let line = first_different_line(&code, &formatted);
        let span = Span { line, column: 1, end_line: line, end_column: 1 };
        annotate::emit(Level::Error, path, Some(&span), "flint format", "file is not formatted, run `flint format`");
    } else {
        fs::write(path, formatted).map_err(|e| format!("cannot write: {e}"))?;
        println!("formatted {}", path.display());
    }
    Ok(true)
}

pub fn format_code(code: &str, config: Config) -> Result<String, String> {
    // StyLua's optional output verification panics on some valid LuaJIT
    // (e.g. `12i`), so like its CLI leave it off, and guard against other panics.
    crate::panic_guard::catch(|| stylua_lib::format_code(code, config, None, OutputVerification::None))
        .map_err(|e| format!("StyLua crashed: {e}"))?
        .map_err(|e| e.to_string())
}

/// 1 based line of the first difference between `a` and `b`
fn first_different_line(a: &str, b: &str) -> usize {
    let mut a_lines = a.lines();
    let mut b_lines = b.lines();
    let mut line = 1;
    while let (Some(x), Some(y)) = (a_lines.next(), b_lines.next()) {
        if x != y {
            break;
        }
        line += 1;
    }
    line
}

#[cfg(test)]
mod tests {
    use super::{first_different_line, format_file};
    use std::fs;

    #[test]
    fn finds_first_different_line() {
        assert_eq!(first_different_line("a\nb\nc", "a\nB\nc"), 2);
        assert_eq!(first_different_line("a", "b"), 1);
        assert_eq!(first_different_line("a\nb", "a\nb\nc"), 3);
    }

    #[test]
    fn check_leaves_files_untouched() {
        let dir = std::env::temp_dir().join("flint-format-check");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("init.lua");
        let config = crate::config::Config::default().stylua().unwrap();

        fs::write(&path, "local   x=1\n").unwrap();
        assert!(format_file(&path, config, true).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "local   x=1\n");

        assert!(format_file(&path, config, false).unwrap());
        assert!(!format_file(&path, config, true).unwrap());
    }
}
