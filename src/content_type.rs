//! figure out what type of package a dir is

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    Game,
    Mod,
    Modpack,
    TexturePack,
    Unknown,
}

/// check in order, so more specific types are detected before more general ones
const MARKERS: &[(ContentType, &[&str])] = &[
    (ContentType::Game, &["game.conf"]),
    (ContentType::Modpack, &["modpack.conf", "modpack.txt"]),
    (ContentType::Mod, &["mod.conf", "init.lua"]),
    (ContentType::TexturePack, &["texture_pack.conf"]),
];

impl ContentType {
    /// the settings file that describes this type of package
    pub fn conf_file(self) -> Option<&'static str> {
        match self {
            ContentType::Game => Some("game.conf"),
            ContentType::Mod => Some(crate::parsers::mod_conf::FILE_NAME),
            ContentType::Modpack => Some("modpack.conf"),
            ContentType::TexturePack => Some("texture_pack.conf"),
            ContentType::Unknown => None,
        }
    }
}

pub fn detect(dir: impl AsRef<Path>) -> ContentType {
    let dir = dir.as_ref();
    MARKERS
        .iter()
        .find(|(_, files)| files.iter().any(|f| dir.join(f).is_file()))
        .map_or(ContentType::Unknown, |(kind, _)| *kind)
}

#[cfg(test)]
mod tests {
    use super::{ContentType, detect};
    use std::fs;
    use std::path::PathBuf;

    fn dir_with(name: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-content-type-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for f in files {
            fs::write(dir.join(f), "").unwrap();
        }
        dir
    }

    #[test]
    fn detects_sample_mods() {
        assert_eq!(detect("samplemods/unformatted"), ContentType::Mod);
        assert_eq!(detect("samplemods/premodconf"), ContentType::Mod);
    }

    #[test]
    fn detects_each_marker() {
        assert_eq!(detect(dir_with("game", &["game.conf"])), ContentType::Game);
        assert_eq!(detect(dir_with("modpack", &["modpack.conf"])), ContentType::Modpack);
        assert_eq!(detect(dir_with("modpack-txt", &["modpack.txt"])), ContentType::Modpack);
        assert_eq!(detect(dir_with("mod", &["mod.conf"])), ContentType::Mod);
        assert_eq!(detect(dir_with("texture-pack", &["texture_pack.conf"])), ContentType::TexturePack);
    }

    #[test]
    fn modpack_beats_mod() {
        let dir = dir_with("modpack-with-init", &["modpack.conf", "init.lua"]);
        assert_eq!(detect(dir), ContentType::Modpack);
    }

    #[test]
    fn unknown_without_markers() {
        assert_eq!(detect(dir_with("empty", &[])), ContentType::Unknown);
        assert_eq!(detect("does/not/exist"), ContentType::Unknown);
    }

    #[test]
    fn marker_must_be_a_file() {
        let dir = dir_with("game-dir", &[]);
        fs::create_dir(dir.join("game.conf")).unwrap();
        assert_eq!(detect(dir), ContentType::Unknown);
    }
}
