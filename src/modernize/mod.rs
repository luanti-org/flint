//! `flint modernize`: migrates deprecated mod files to their current equivalents.

mod legacy_txt;
mod plural_forms;
mod translations;

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::content_type::{self, ContentType};

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
    let root = Path::new("");
    match content_type::detect(root) {
        ContentType::Unknown => {
            return Err("not a mod, modpack, game or texture pack".to_string());
        }
        kind => plan_package(&mut plan, root, kind)?,
    }
    plan.apply()
}

fn plan_package(plan: &mut Plan, dir: &Path, kind: ContentType) -> Result<(), String> {
    legacy_txt::plan(plan, dir, kind)?;
    translations::plan(plan, dir)?;
    match kind {
        ContentType::Modpack => plan_children(plan, dir),
        ContentType::Game => plan_children(plan, &dir.join("mods")),
        _ => Ok(()),
    }
}

/// Mods and modpacks inside a modpack or a game's `mods` dir.
fn plan_children(plan: &mut Plan, dir: &Path) -> Result<(), String> {
    for child in list_dir(dir)? {
        if child.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
            continue;
        }
        match content_type::detect(&child) {
            kind @ (ContentType::Mod | ContentType::Modpack) => plan_package(plan, &child, kind)?,
            _ => {}
        }
    }
    Ok(())
}

/// Sorted entries of `dir`, or nothing if it doesn't exist. An empty path means the current dir.
fn list_dir(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let read_path = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let entries = match fs::read_dir(read_path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", read_path.display())),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let name = entry.map_err(|e| format!("cannot read {}: {e}", read_path.display()))?.file_name();
        paths.push(dir.join(name));
    }
    paths.sort();
    Ok(paths)
}

fn read_optional(path: impl AsRef<Path>) -> Result<Option<String>, String> {
    let path = path.as_ref();
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}
