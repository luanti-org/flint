use std::fs;
use std::path::{Path, PathBuf};

// reads the commit hash straight from .git rather than running git, since
// cross builds run in containers that may not have git installed
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let hash = git_dir().and_then(|dir| head_hash(&dir));
    println!(
        "cargo:rustc-env=FLINT_GIT_HASH={}",
        hash.as_deref().unwrap_or("unknown")
    );
}

fn git_dir() -> Option<PathBuf> {
    let dot_git = Path::new(env!("CARGO_MANIFEST_DIR")).join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    // worktrees and submodules have a .git file pointing at the real dir
    let contents = fs::read_to_string(&dot_git).ok()?;
    let dir = contents.strip_prefix("gitdir:")?.trim();
    Some(dot_git.parent()?.join(dir))
}

fn head_hash(dir: &Path) -> Option<String> {
    let head_path = dir.join("HEAD");
    println!("cargo:rerun-if-changed={}", head_path.display());
    let head = fs::read_to_string(head_path).ok()?;
    let Some(reference) = head.trim().strip_prefix("ref:") else {
        // detached HEAD, e.g. a tag or PR checkout in CI
        return short(head.trim());
    };
    let reference = reference.trim();

    // worktrees keep their own HEAD but share refs with the main repo
    let common = fs::read_to_string(dir.join("commondir"))
        .map(|c| dir.join(c.trim()))
        .unwrap_or_else(|_| dir.to_path_buf());

    let ref_path = common.join(reference);
    let packed_path = common.join("packed-refs");
    println!("cargo:rerun-if-changed={}", ref_path.display());
    println!("cargo:rerun-if-changed={}", packed_path.display());

    if let Ok(hash) = fs::read_to_string(&ref_path) {
        return short(hash.trim());
    }
    let packed = fs::read_to_string(packed_path).ok()?;
    let hash = packed.lines().find_map(|line| match line.split_once(' ') {
        Some((hash, name)) if name == reference => Some(hash),
        _ => None,
    })?;
    short(hash)
}

fn short(hash: &str) -> Option<String> {
    let valid = hash.len() >= 7 && hash.chars().all(|c| c.is_ascii_hexdigit());
    valid.then(|| hash[..7].to_owned())
}
