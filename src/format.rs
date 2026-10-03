//! `flint format`: formats every `.lua` file in a mod with StyLua.

use std::fs;
use std::path::Path;

use stylua_lib::{Config, OutputVerification};

pub fn run(flint: &crate::config::Config) -> Result<(), String> {
    let config = flint.stylua()?;
    let files = flint.files(Path::new("."))?;

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
