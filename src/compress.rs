//! `flint compress`: losslessly compresses media files

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use optivorbis::{OggToOgg, Remuxer};
use oxipng::{Options, StripChunks};

use crate::annotate::{self, Level};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Media {
    Png,
    Ogg,
}

impl Media {
    fn of(path: &Path) -> Option<Media> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "png" => Some(Media::Png),
            "ogg" => Some(Media::Ogg),
            _ => None,
        }
    }
}

pub fn run(flint: &crate::config::Config, check: bool) -> Result<(), String> {
    let files = flint.files(Path::new("."), |path| Media::of(path).is_some())?;
    let options = png_options(flint.compress.level);

    let (mut before, mut after) = (0, 0);
    let (mut compressible, mut failures) = (0, 0);
    for path in &files {
        match compress_file(path, &options, check) {
            Ok((old, new)) => {
                before += old;
                after += new;
                if new < old {
                    compressible += 1;
                }
            }
            Err(e) => {
                eprintln!("error: {}: {e}", path.display());
                annotate::emit(Level::Error, path, None, "flint compress", &e);
                failures += 1;
            }
        }
    }
    if after < before {
        let verb = if check { "could save" } else { "saved" };
        println!("{verb} {} bytes ({before} -> {after})", before - after);
    }

    if failures > 0 {
        return Err(format!("{failures} of {} files could not be compressed", files.len()));
    }
    if check && compressible > 0 {
        return Err(format!("{compressible} of {} files could be compressed", files.len()));
    }
    Ok(())
}

fn png_options(level: u8) -> Options {
    Options {
        strip: StripChunks::Safe,
        ..Options::from_preset(level)
    }
}

/// compress `path` in place, or with `check` only report it, returning the size before and after
fn compress_file(path: &Path, options: &Options, check: bool) -> Result<(usize, usize), String> {
    let data = fs::read(path).map_err(|e| format!("cannot read: {e}"))?;
    let kind = Media::of(path).ok_or("not a png or ogg file")?;
    let compressed = compress(kind, &data, options)?;
    if compressed.len() >= data.len() {
        return Ok((data.len(), data.len()));
    }
    if check {
        let message = format!("could be compressed: {} -> {} bytes", data.len(), compressed.len());
        println!("{}: {message}", path.display());
        annotate::emit(Level::Error, path, None, "flint compress", &message);
    } else {
        write_replacing(path, &compressed)?;
        println!("compressed {}: {} -> {} bytes", path.display(), data.len(), compressed.len());
    }
    Ok((data.len(), compressed.len()))
}

fn compress(kind: Media, data: &[u8], options: &Options) -> Result<Vec<u8>, String> {
    match kind {
        Media::Png => oxipng::optimize_from_memory(data, options).map_err(|e| e.to_string()),
        Media::Ogg => OggToOgg::new_with_defaults()
            .remux(Cursor::new(data), Vec::new())
            .map_err(|e| e.to_string()),
    }
}

fn write_replacing(path: &Path, data: &[u8]) -> Result<(), String> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".flint-tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, data).map_err(|e| format!("cannot write: {e}"))?;
    fs::rename(&temp, path).map_err(|e| {
        let _ = fs::remove_file(&temp);
        format!("cannot replace: {e}")
    })
}

#[cfg(test)]
mod tests {
    use super::{Media, compress, compress_file, png_options};
    use std::fs;
    use std::path::{Path, PathBuf};

    const PNG: &[u8] = include_bytes!("../samplemods/media/textures/media_checker.png");
    const OGG: &[u8] = include_bytes!("../samplemods/media/sounds/media_tone.ogg");

    fn dir_with(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-compress-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    #[test]
    fn detects_media_by_extension() {
        assert_eq!(Media::of(Path::new("textures/a.png")), Some(Media::Png));
        assert_eq!(Media::of(Path::new("textures/a.PNG")), Some(Media::Png));
        assert_eq!(Media::of(Path::new("sounds/a.ogg")), Some(Media::Ogg));
        assert_eq!(Media::of(Path::new("init.lua")), None);
        assert_eq!(Media::of(Path::new("png")), None);
    }

    #[test]
    fn png_shrinks_and_strips_metadata() {
        let compressed = compress(Media::Png, PNG, &png_options(2)).unwrap();
        assert!(compressed.len() < PNG.len(), "{} >= {}", compressed.len(), PNG.len());
        assert!(compressed.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(!compressed.windows(4).any(|w| w == b"tEXt"));
    }

    #[test]
    fn ogg_shrinks() {
        let compressed = compress(Media::Ogg, OGG, &png_options(2)).unwrap();
        assert!(compressed.len() < OGG.len(), "{} >= {}", compressed.len(), OGG.len());
        assert!(compressed.starts_with(b"OggS"));
    }

    #[test]
    fn invalid_media_is_an_error() {
        assert!(compress(Media::Png, b"not a png", &png_options(2)).is_err());
        assert!(compress(Media::Ogg, b"not an ogg", &png_options(2)).is_err());
    }

    #[test]
    fn replaces_files_in_place() {
        let dir = dir_with("replace", &[("a.png", PNG), ("b.ogg", OGG)]);
        for name in ["a.png", "b.ogg"] {
            let path = dir.join(name);
            let (before, after) = compress_file(&path, &png_options(2), false).unwrap();
            assert!(after < before);
            assert_eq!(fs::metadata(&path).unwrap().len(), after as u64);
            // a second run has nothing left to do
            assert_eq!(compress_file(&path, &png_options(2), false).unwrap(), (after, after));
        }
        let mut names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, ["a.png", "b.ogg"], "no temp files are left behind");
    }

    #[test]
    fn check_leaves_files_untouched() {
        let dir = dir_with("check", &[("a.png", PNG), ("b.ogg", OGG)]);
        for (name, original) in [("a.png", PNG), ("b.ogg", OGG)] {
            let path = dir.join(name);
            let (before, after) = compress_file(&path, &png_options(2), true).unwrap();
            assert!(after < before);
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn broken_file_is_left_untouched() {
        let dir = dir_with("broken", &[("broken.png", b"not a png")]);
        let path = dir.join("broken.png");
        assert!(compress_file(&path, &png_options(2), false).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"not a png");
    }

    #[test]
    fn finds_media_respecting_config() {
        let dir = dir_with(
            "walk",
            &[
                (".gitignore", b"build/\n"),
                ("flint.toml", b"[files]\nincludes = [\"**\", \"!textures/vendor/**\"]\n"),
                ("textures/vendor/f.png", PNG),
                ("init.lua", b""),
                ("textures/a.png", PNG),
                ("sounds/b.OGG", OGG),
                ("models/c.b3d", b""),
                ("build/d.png", PNG),
                (".hidden/e.png", PNG),
            ],
        );
        let config = crate::config::load(&dir).unwrap();
        let files: Vec<_> = config
            .files(&dir, |p| Media::of(p).is_some())
            .unwrap()
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(files, ["sounds/b.OGG", "textures/a.png"]);
    }
}
