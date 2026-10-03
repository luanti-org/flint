//! `flint format`: formats every `.lua` file in a mod with StyLua.

use std::fs;
use std::path::{Path, PathBuf};

use stylua_lib::{Config, LuaVersion, OutputVerification};

pub fn run() -> Result<(), String> {
    let config = Config {
        // luajit == 5.1 + extensions
        syntax: LuaVersion::LuaJIT,
        ..Config::default()
    };

    let mut files = Vec::new();
    collect_lua_files(Path::new("."), &mut files)?;
    files.sort();

    let mut failures = 0;
    for path in &files {
        if let Err(e) = format_file(path, config) {
            eprintln!("error: {}: {e}", path.display());
            failures += 1;
        }
    }

    match failures {
        0 => Ok(()),
        n => Err(format!("{n} of {} files could not be formatted", files.len())),
    }
}

fn format_file(path: &Path, config: Config) -> Result<(), String> {
    let code = fs::read_to_string(path).map_err(|e| format!("cannot read: {e}"))?;
    // StyLua's optional output verification panics on some valid LuaJIT
    // (e.g. `12i`), so like its CLI leave it off, and guard against other panics.
    let formatted = crate::panic_guard::catch(|| {
        stylua_lib::format_code(&code, config, None, OutputVerification::None)
    })
    .map_err(|e| format!("StyLua crashed: {e}"))?
    .map_err(|e| e.to_string())?;
    if formatted != code {
        fs::write(path, formatted).map_err(|e| format!("cannot write: {e}"))?;
        println!("formatted {}", path.display());
    }
    Ok(())
}

/// Recursively find `.lua` files, skipping hidden directories such as `.git`.
pub fn collect_lua_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if file_type.is_dir() {
            if !entry.file_name().to_string_lossy().starts_with('.') {
                collect_lua_files(&path, files)?;
            }
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "lua") {
            files.push(path);
        }
    }
    Ok(())
}
