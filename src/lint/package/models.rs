//! `luanti_legacy_b3d_model` and `luanti_legacy_x_model`: `.b3d` and `.x` models in a mod, fixed by converting them
//! to glTF `.glb` and renaming them in the Lua code, when the mod needs a Luanti that loads glTF.

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use full_moon::ast::{self, Call, Prefix, Suffix};
use full_moon::node::Node;
use full_moon::visitors::Visitor;

use super::{Fix, Problem, list_dir};
use crate::content_type::{self, ContentType};
use crate::convert;
use crate::lint::luanti::{ModInfo, Version};

pub const B3D_NAME: &str = "luanti_legacy_b3d_model";
pub const X_NAME: &str = "luanti_legacy_x_model";

/// the first version that loads glTF
const GLTF_SINCE: Version = Version(5, 10);

/// functions taking an entity's properties, or a model only entities use, e.g. `player_api.set_model`
const ENTITY_FUNCTIONS: &[&str] = &["register_entity", "set_properties", "register_mob", "register_model", "set_model"];

struct Lua {
    path: PathBuf,
    text: String,
    /// byte ranges of the arguments of `ENTITY_FUNCTIONS` calls, `None` if it can't be parsed
    entity_args: Option<Vec<Range<usize>>>,
}

pub(super) fn check_b3d(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    check(dir, kind, "b3d")
}

pub(super) fn check_x(dir: &Path, kind: ContentType) -> Result<Vec<Problem>, String> {
    check(dir, kind, "x")
}

fn check(dir: &Path, kind: ContentType, extension: &str) -> Result<Vec<Problem>, String> {
    if kind != ContentType::Mod {
        return Ok(Vec::new());
    }
    let models = media(&dir.join("models"), extension)?;
    let Some(first) = models.first() else {
        return Ok(Vec::new());
    };
    let min_version = ModInfo::find(first).and_then(|info| info.min_version);
    // the mod's code, and that of the game or modpack it's in, which can use its models
    let mut lua = Vec::new();
    for path in files(&outermost_package(dir), "lua")? {
        let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let entity_args = crate::lint::parse(&text).ok().map(|ast| {
            let mut visitor = EntityCalls(Vec::new());
            visitor.visit_ast(&ast);
            visitor.0
        });
        lua.push(Lua { path, text, entity_args });
    }

    Ok(models
        .into_iter()
        .map(|path| {
            let message = format!("`.{extension}` is a legacy model format, use glTF `.glb`");
            match convert(&path, extension, min_version, &lua) {
                Ok(fix) => Problem { path, message, fix: Some(fix) },
                Err(reason) => Problem { path, message: format!("{message}. it can't be converted automatically, as {reason}"), fix: None },
            }
        })
        .collect())
}

/// the fix converting `path`, or why there's none
fn convert(path: &Path, extension: &str, min_version: Option<Version>, lua: &[Lua]) -> Result<Fix, String> {
    if min_version.is_none_or(|v| v < GLTF_SINCE) {
        return Err(format!(
            "glTF needs {GLTF_SINCE} or later, set `min_minetest_version = {GLTF_SINCE}` in the mod's conf to fix this with `flint lint --write`"
        ));
    }
    let glb = path.with_extension("glb");
    if let Some(existing) = [glb.clone(), path.with_extension("gltf")].into_iter().find(|p| p.exists()) {
        return Err(format!("{} already exists", existing.display()));
    }

    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let new_name = glb.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let mut fix = Fix::default();
    // whether every use is passed to an entity function
    let mut entity_only = true;
    for file in lua {
        for quote in ['"', '\''] {
            let literal = format!("{quote}{name}{quote}");
            let mut uses = file.text.match_indices(&literal).map(|(at, _)| at).peekable();
            if uses.peek().is_none() {
                continue;
            }
            entity_only &= uses.all(|at| file.entity_args.as_ref().is_some_and(|args| args.iter().any(|r| r.contains(&at))));
            fix.replace(&file.path, literal, format!("{quote}{new_name}{quote}"));
        }
    }
    if fix.replacements.is_empty() {
        return Err(format!("`\"{name}\"` isn't in the code, so it can't be changed to use the `.glb`"));
    }

    let data = fs::read(path).map_err(|e| format!("it can't be read: {e}"))?;
    let (model, mut warnings) = convert::read(extension, &data).map_err(|e| format!("it can't be read: {e}"))?;
    let (converted, more) = model.finish().map_err(|e| format!("it can't be converted: {e}"))?;
    warnings.extend(more);
    // static b3d and x node meshes are 1 unit per node, but glTF ones 10. entities are 10 for both
    if !converted.is_animated() && !entity_only {
        let functions: Vec<_> = ENTITY_FUNCTIONS.iter().map(|f| format!("`{f}`")).collect();
        return Err(format!(
            "it's static and might be a node's `mesh`, which would be 10 times smaller. if only entities use it, pass \
             `\"{name}\"` straight to {}, otherwise convert it with `flint convert` and multiply the node's `visual_scale` by 10",
            functions.join(", ")
        ));
    }
    let changes: Vec<_> = warnings.into_iter().filter(|w| w.changes_model).map(|w| w.message).collect();
    if !changes.is_empty() {
        return Err(format!("it'd look different: {}", changes.join(", ")));
    }

    fix.write(&glb, converted.to_glb());
    fix.migrate(path, glb);
    Ok(fix)
}

/// the arguments of calls to `ENTITY_FUNCTIONS`, however they're called, e.g. `core.register_entity(...)` or
/// `self.object:set_properties(...)`
struct EntityCalls(Vec<Range<usize>>);

impl Visitor for EntityCalls {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        let suffixes: Vec<&Suffix> = call.suffixes().collect();
        for (i, suffix) in suffixes.iter().enumerate() {
            let (name, args) = match suffix {
                Suffix::Call(Call::MethodCall(method)) => (method.name(), method.args()),
                Suffix::Call(Call::AnonymousCall(args)) => {
                    let name = match i.checked_sub(1).map(|before| suffixes[before]) {
                        Some(Suffix::Index(ast::Index::Dot { name, .. })) => name,
                        None => match call.prefix() {
                            Prefix::Name(name) => name,
                            _ => continue,
                        },
                        _ => continue,
                    };
                    (name, &**args)
                }
                _ => continue,
            };
            if ENTITY_FUNCTIONS.contains(&name.token().to_string().as_str())
                && let Some((start, end)) = args.range()
            {
                self.0.push(start.bytes()..end.bytes());
            }
        }
    }
}

/// the game or modpack `dir` is in, or `dir` itself, relative to `dir`
fn outermost_package(dir: &Path) -> PathBuf {
    let readable = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let Ok(absolute) = readable.canonicalize() else {
        return dir.to_path_buf();
    };
    let mut levels = 0;
    for (up, ancestor) in absolute.ancestors().enumerate().skip(1) {
        match content_type::detect(ancestor) {
            ContentType::Modpack => levels = up,
            ContentType::Game => {
                levels = up;
                break;
            }
            // a game's mods are in its `mods` dir, which isn't a package
            _ if ancestor.file_name().is_some_and(|n| n == "mods") => {}
            _ => break,
        }
    }
    let mut outermost = dir.to_path_buf();
    outermost.extend(std::iter::repeat_n("..", levels));
    outermost
}

/// media files with `extension`, in `dir` and its subfolders, which Luanti ignores when starting with `_` or `.`
fn media(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, String> {
    walk(dir, extension, &['_', '.'])
}

fn files(dir: &Path, extension: &str) -> Result<Vec<PathBuf>, String> {
    walk(dir, extension, &['.'])
}

fn walk(dir: &Path, extension: &str, skipped: &[char]) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    for path in list_dir(dir)? {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if path.is_dir() {
            if !name.starts_with(skipped) {
                found.extend(walk(&path, extension, skipped)?);
            }
        } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case(extension)) {
            found.push(path);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::b3d::tests::b3d;

    /// a mesh with a bone and a keyframe
    fn animated() -> Vec<u8> {
        animated_at(&[1])
    }

    /// with a position keyframe at each of `frames`
    fn animated_at(frames: &[i32]) -> Vec<u8> {
        b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
                w.chunk("MESH", |w| {
                    w.int(-1);
                    w.chunk("VRTS", |w| {
                        w.int(0).int(0).int(0).floats(&[0.0; 9]);
                    });
                    w.chunk("TRIS", |w| {
                        w.int(-1).int(0).int(1).int(2);
                    });
                });
                w.chunk("NODE", |w| {
                    w.node_header("bone", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
                    w.chunk("BONE", |w| {
                        w.int(0).floats(&[1.0]);
                    });
                    w.chunk("KEYS", |w| {
                        w.int(1);
                        for &frame in frames {
                            w.int(frame).floats(&[0.0, 1.0, 0.0]);
                        }
                    });
                });
            });
        })
    }

    fn static_model() -> Vec<u8> {
        b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
            });
        })
    }

    fn dir_with(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flint-models-{name}"));
        let _ = fs::remove_dir_all(&dir);
        for (path, contents) in files {
            let path = dir.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
        dir
    }

    const MOD_510: (&str, &[u8]) = ("mod.conf", b"name = mobs\nmin_minetest_version = 5.10\n");

    fn messages(problems: &[Problem]) -> Vec<String> {
        problems.iter().map(|p| p.message.clone()).collect()
    }

    #[test]
    fn converts_and_renames_in_code() {
        let code = b"register(\"mobs_a.b3d\")\nregister('mobs_b.b3d')\nlocal again = \"mobs_a.b3d\"\nlocal not_it = \"xmobs_a.b3d\"\n";
        let dir = dir_with(
            "convert",
            &[MOD_510, ("init.lua", code), ("models/mobs_a.b3d", &animated()), ("models/sub/mobs_b.b3d", &animated())],
        );
        let problems = check_b3d(&dir, ContentType::Mod).unwrap();
        assert_eq!(problems.len(), 2);
        assert!(problems.iter().all(|p| p.fix.is_some()), "{:?}", messages(&problems));
        // both fixes change init.lua, the second must keep the first's change
        for problem in problems {
            problem.fix.unwrap().apply().unwrap();
        }
        assert_eq!(
            fs::read_to_string(dir.join("init.lua")).unwrap(),
            "register(\"mobs_a.glb\")\nregister('mobs_b.glb')\nlocal again = \"mobs_a.glb\"\nlocal not_it = \"xmobs_a.b3d\"\n"
        );
        assert!(!dir.join("models/mobs_a.b3d").exists() && !dir.join("models/sub/mobs_b.b3d").exists());
        assert!(fs::read(dir.join("models/mobs_a.glb")).unwrap().starts_with(b"glTF"));
        assert!(fs::read(dir.join("models/sub/mobs_b.glb")).unwrap().starts_with(b"glTF"));
        assert!(check_b3d(&dir, ContentType::Mod).unwrap().is_empty());
    }

    #[test]
    fn needs_a_min_version_with_gltf() {
        for (name, conf) in [("no-version", "name = mobs\n"), ("old-version", "name = mobs\nmin_minetest_version = 5.9\n")] {
            let dir = dir_with(name, &[("mod.conf", conf.as_bytes()), ("init.lua", b"m = \"m.b3d\""), ("models/m.b3d", &animated())]);
            let problems = check_b3d(&dir, ContentType::Mod).unwrap();
            assert!(problems[0].fix.is_none());
            assert!(problems[0].message.contains("set `min_minetest_version = 5.10`"), "{}", problems[0].message);
        }
    }

    #[test]
    fn unsafe_conversions_arent_fixable() {
        let not_fixable = |name: &str, files: &[(&str, &[u8])]| {
            let dir = dir_with(name, files);
            let problems = check_b3d(&dir, ContentType::Mod).unwrap();
            assert!(problems.iter().all(|p| p.fix.is_none()));
            messages(&problems)
        };
        let reason = not_fixable("static", &[MOD_510, ("init.lua", b"m = \"m.b3d\""), ("models/m.b3d", &static_model())]);
        assert!(reason[0].contains("it's static"), "{reason:?}");
        let reason = not_fixable("unnamed", &[MOD_510, ("init.lua", b"m = name .. \".b3d\""), ("models/m.b3d", &animated())]);
        assert!(reason[0].contains("`\"m.b3d\"` isn't in the code"), "{reason:?}");
        let reason = not_fixable("exists", &[MOD_510, ("init.lua", b"m = \"m.b3d\""), ("models/m.b3d", &animated()), ("models/m.glb", b"")]);
        assert!(reason[0].contains("m.glb already exists"), "{reason:?}");
        let reason = not_fixable("broken", &[MOD_510, ("init.lua", b"m = \"m.b3d\""), ("models/m.b3d", b"junk")]);
        assert!(reason[0].contains("it can't be read"), "{reason:?}");
    }

    fn fixable(name: &str, code: &str, model: &[u8]) -> Result<(), String> {
        let dir = dir_with(name, &[MOD_510, ("init.lua", code.as_bytes()), ("models/m.b3d", model)]);
        let problem = check_b3d(&dir, ContentType::Mod).unwrap().pop().unwrap();
        problem.fix.map(|_| ()).ok_or(problem.message)
    }

    #[test]
    fn static_models_only_entities_use_are_fixable() {
        let code = "core.register_entity(\"mobs:a\", {initial_properties = {mesh = \"m.b3d\"}})\n\
                    function e:on_step() self.object:set_properties({mesh = 'm.b3d'}) end\n\
                    player_api.register_model(\"m.b3d\", {})\nplayer_api.set_model(player, \"m.b3d\")\n\
                    mobs:register_mob(\"mobs:b\", {mesh = \"m.b3d\"})\n";
        assert_eq!(fixable("static-entities", code, &static_model()), Ok(()));
    }

    #[test]
    fn static_models_that_might_be_node_meshes_arent_fixable() {
        for (name, code) in [
            ("static-node", "core.register_node(\"a:b\", {mesh = \"m.b3d\"})\ncore.register_entity(\"a:c\", {mesh = \"m.b3d\"})\n"),
            // the table could be used for anything
            ("static-table", "local def = {mesh = \"m.b3d\"}\ncore.register_entity(\"a:c\", def)\n"),
            ("static-unparsable", "core.register_entity(\"a:c\", {mesh = \"m.b3d\"}) end end\n"),
        ] {
            let message = fixable(name, code, &static_model()).unwrap_err();
            assert!(message.contains("it's static and might be a node's `mesh`"), "{name}: {message}");
        }
    }

    #[test]
    fn only_warnings_that_change_the_model_block_the_fix() {
        let code = "core.register_entity(\"a:c\", {mesh = \"m.b3d\"})\n";
        // Luanti moves frame 0 to 1, and drops repeated frames, for both formats
        assert_eq!(fixable("frame-zero", code, &animated_at(&[0, 5])), Ok(()));
        assert_eq!(fixable("duplicate-frames", code, &animated_at(&[3, 3])), Ok(()));
        // too many vertices for Luanti's glTF loader
        let big = b3d(|w| {
            w.chunk("NODE", |w| {
                w.node_header("mesh", [0.0; 3], [0.0, 0.0, 0.0, 1.0]);
                w.chunk("MESH", |w| {
                    w.int(-1);
                    w.chunk("VRTS", |w| {
                        w.int(0).int(0).int(0);
                        for i in 0..65535 {
                            w.floats(&[i as f32, 0.0, 0.0]);
                        }
                    });
                    w.chunk("TRIS", |w| {
                        w.int(-1);
                        for i in 0..65535 {
                            w.int(i);
                        }
                    });
                });
                w.chunk("KEYS", |w| {
                    w.int(1).int(1).floats(&[0.0; 3]);
                });
            });
        });
        let message = fixable("big", code, &big).unwrap_err();
        assert!(message.contains("it'd look different: a mesh has 65535 vertices"), "{message}");
    }

    #[test]
    fn only_mods_and_their_media() {
        let dir = dir_with(
            "media",
            &[MOD_510, ("models/_old/a.b3d", &animated()), ("models/.git/b.b3d", &animated()), ("c.b3d", &animated())],
        );
        assert!(check_b3d(&dir, ContentType::Mod).unwrap().is_empty());
        let dir = dir_with("game-media", &[("game.conf", b""), ("models/a.b3d", &animated())]);
        assert!(check_b3d(&dir, ContentType::Game).unwrap().is_empty());
    }

    #[test]
    fn code_elsewhere_in_the_game_is_renamed_too() {
        let dir = dir_with(
            "game",
            &[
                ("game.conf", b"min_minetest_version = 5.10\n"),
                ("mods/player/init.lua", b""),
                ("mods/player/models/char.b3d", &animated()),
                ("mods/skins/init.lua", b"set_model(\"char.b3d\")\n"),
            ],
        );
        let problems = check_b3d(&dir.join("mods/player"), ContentType::Mod).unwrap();
        problems.into_iter().next().unwrap().fix.expect("fixable").apply().unwrap();
        assert_eq!(fs::read_to_string(dir.join("mods/skins/init.lua")).unwrap(), "set_model(\"char.glb\")\n");
    }

    const ANIMATED_X: &str = "xof 0303txt 0032\n\
        Frame Bone { FrameTransformMatrix { 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; } }\n\
        Mesh { 4; 0;0;0;, 1;0;0;, 1;1;0;, 0;1;0;; 1; 4;0,1,2,3;;\n\
          SkinWeights { \"Bone\"; 1; 2; 1.0; 1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1;; } }\n\
        AnimationSet { Animation { { Bone } AnimationKey { 2; 1; 5; 3; 0,1,0;;; } } }\n";

    const STATIC_X: &str = "xof 0303txt 0032\nMesh { 3; 0;0;0;, 1;0;0;, 1;1;0;; 1; 3;0,1,2;; }\n";

    fn x_problems(name: &str, code: &str, model: &str) -> (PathBuf, Vec<Problem>) {
        let dir = dir_with(name, &[MOD_510, ("init.lua", code.as_bytes()), ("models/m.x", model.as_bytes())]);
        let problems = check_x(&dir, ContentType::Mod).unwrap();
        (dir, problems)
    }

    #[test]
    fn x_models_are_converted_and_renamed_in_code() {
        let (dir, problems) = x_problems("x-convert", "mobs:register_mob(\"mobs:m\", {mesh = \"m.x\"})\n", ANIMATED_X);
        assert_eq!(messages(&problems), ["`.x` is a legacy model format, use glTF `.glb`"]);
        problems.into_iter().next().unwrap().fix.unwrap().apply().unwrap();
        assert_eq!(fs::read_to_string(dir.join("init.lua")).unwrap(), "mobs:register_mob(\"mobs:m\", {mesh = \"m.glb\"})\n");
        assert!(!dir.join("models/m.x").exists());
        assert!(fs::read(dir.join("models/m.glb")).unwrap().starts_with(b"glTF"));
        assert!(check_x(&dir, ContentType::Mod).unwrap().is_empty());
        assert!(check_b3d(&dir, ContentType::Mod).unwrap().is_empty());
    }

    #[test]
    fn x_models_follow_the_same_rules() {
        let entity = "core.register_entity(\"a:c\", {mesh = \"m.x\"})\n";
        let (_, problems) = x_problems("x-static-entity", entity, STATIC_X);
        assert!(problems[0].fix.is_some(), "{:?}", messages(&problems));
        let (_, problems) = x_problems("x-static-node", "core.register_node(\"a:b\", {drawtype = \"mesh\", mesh = \"m.x\"})\n", STATIC_X);
        assert!(problems[0].message.contains("it's static and might be a node's `mesh`"), "{:?}", messages(&problems));
        let (_, problems) = x_problems("x-unnamed", "m = name .. \".x\"\n", ANIMATED_X);
        assert!(problems[0].message.contains("`\"m.x\"` isn't in the code"), "{:?}", messages(&problems));
        let (_, problems) = x_problems("x-broken", entity, "xof 0303bin 0032");
        assert!(problems[0].message.contains("it can't be read: binary x files aren't supported"), "{:?}", messages(&problems));
        let decl = ANIMATED_X.replace("1; 4;0,1,2,3;;", "1; 4;0,1,2,3;; DeclData { 1; 2;0;3;0;; 12; 0,0,0,0,0,0,0,0,0,0,0,0;; }");
        let (_, problems) = x_problems("x-decl", entity, &decl);
        assert!(problems[0].message.contains("it'd look different: DeclData isn't supported"), "{:?}", messages(&problems));
    }

    #[test]
    fn x_models_need_a_min_version_with_gltf() {
        let dir = dir_with("x-old", &[("mod.conf", b"name = mobs\n"), ("init.lua", b"m = \"m.x\""), ("models/m.x", ANIMATED_X.as_bytes())]);
        let problems = check_x(&dir, ContentType::Mod).unwrap();
        assert!(problems[0].fix.is_none() && problems[0].message.contains("set `min_minetest_version = 5.10`"));
    }
}
