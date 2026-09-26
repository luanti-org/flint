//! `flint modernize`: migrates deprecated mod files to their current equivalents.

mod legacy_txt;
mod plural_forms;
mod translations;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Plan {
    writes: Vec<(PathBuf, String)>,
    /// Old files to delete, and the file their content now lives in.
    migrations: Vec<(PathBuf, PathBuf)>,
}

impl Plan {
    fn write(&mut self, path: impl Into<PathBuf>, content: String) {
        self.writes.push((path.into(), content));
    }

    fn migrate(&mut self, from: impl Into<PathBuf>, into: impl Into<PathBuf>) {
        self.migrations.push((from.into(), into.into()));
    }

    fn apply(self) -> Result<(), String> {
        for (path, content) in self.writes {
            fs::write(&path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        }
        for (from, into) in self.migrations {
            fs::remove_file(&from).map_err(|e| format!("cannot remove {}: {e}", from.display()))?;
            println!("migrated {} into {}", from.display(), into.display());
        }
        Ok(())
    }
}

pub fn run() -> Result<(), String> {
    let mut plan = Plan::default();
    legacy_txt::plan(&mut plan)?;
    translations::plan(&mut plan)?;
    plan.apply()
}

fn read_optional(path: impl AsRef<Path>) -> Result<Option<String>, String> {
    let path = path.as_ref();
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}
