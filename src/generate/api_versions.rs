//! when parts of the Luanti API were added, to work out the `min_minetest_version` a mod's code needs.

use full_moon::ast::{self, Call, Index, Prefix, Suffix};
use full_moon::node::Node;
use full_moon::visitors::Visitor;
use selene_lib::lints::AstContext;

use crate::lint::luanti::{self, Version};

const fn v(minor: u32) -> Version {
    Version(5, minor)
}

/// `core.<name>`
const FUNCTIONS: &[(&str, Version)] = &[
    ("calculate_knockback", v(1)),
    ("format_chat_message", v(1)),
    ("read_schematic", v(1)),
    ("dynamic_add_media", v(3)),
    ("get_translated_string", v(3)),
    ("is_creative_enabled", v(3)),
    ("register_on_authplayer", v(3)),
    ("get_artificial_light", v(4)),
    ("get_natural_light", v(4)),
    ("get_objects_in_area", v(4)),
    ("register_on_chatcommand", v(4)),
    ("register_on_rightclickplayer", v(4)),
    ("colorspec_to_bytes", v(5)),
    ("colorspec_to_colorstring", v(5)),
    ("compare_block_status", v(5)),
    ("cpdir", v(5)),
    ("disconnect_player", v(5)),
    ("encode_png", v(5)),
    ("get_server_max_lag", v(5)),
    ("mvdir", v(5)),
    ("register_on_liquid_transformed", v(5)),
    ("rmdir", v(5)),
    ("get_tool_wear_after_use", v(6)),
    ("parse_relative_number", v(6)),
    ("register_async_dofile", v(6)),
    ("dir_to_fourdir", v(7)),
    ("fourdir_to_dir", v(7)),
    ("get_game_info", v(7)),
    ("get_mapgen_edges", v(7)),
    ("get_player_window_information", v(7)),
    ("item_pickup", v(7)),
    ("register_on_item_pickup", v(7)),
    ("register_on_mapblocks_changed", v(7)),
    ("urlencode", v(8)),
    ("change_player_privs", v(9)),
    ("get_mod_data_path", v(9)),
    ("get_node_boxes", v(9)),
    ("get_node_raw", v(9)),
    ("objects_in_area", v(9)),
    ("objects_inside_radius", v(9)),
    ("register_mapgen_script", v(9)),
    ("register_portable_metatable", v(9)),
    ("save_gen_notify", v(9)),
    ("sha256", v(9)),
    ("bulk_swap_node", v(10)),
    ("colorspec_to_table", v(10)),
    ("hypertext_escape", v(10)),
    ("ipc_cas", v(10)),
    ("ipc_get", v(10)),
    ("ipc_poll", v(10)),
    ("ipc_set", v(10)),
    ("is_valid_player_name", v(10)),
    ("show_death_screen", v(10)),
    ("time_to_day_night_ratio", v(10)),
    ("translate_n", v(10)),
    ("spawn_tree_on_vmanip", v(11)),
    ("get_value_noise", v(12)),
    ("get_value_noise_map", v(12)),
    ("get_mapgen_chunksize", v(13)),
    ("strip_escapes", v(14)),
    ("path_exists", v(15)),
    ("get_active_blocks", v(16)),
    ("get_loadable_blocks", v(16)),
    ("get_loaded_blocks", v(16)),
];

/// `vector.<name>`
const VECTOR: &[(&str, Version)] = &[
    ("angle", v(1)),
    ("cross", v(1)),
    ("dot", v(1)),
    ("dir_to_rotation", v(3)),
    ("rotate", v(3)),
    ("rotate_around_axis", v(3)),
    ("offset", v(4)),
    ("check", v(5)),
    ("copy", v(5)),
    ("from_string", v(5)),
    ("to_string", v(5)),
    ("zero", v(5)),
    ("combine", v(6)),
    ("in_area", v(8)),
    ("abs", v(10)),
    ("ceil", v(10)),
    ("random_direction", v(10)),
    ("random_in_area", v(10)),
    ("sign", v(10)),
];

/// global classes, e.g. `ValueNoise(params)`
const CLASSES: &[(&str, Version)] = &[("ValueNoise", v(12)), ("ValueNoiseMap", v(12))];

/// `core.settings:<name>()`
const SETTINGS_METHODS: &[(&str, Version)] = &[("get_flags", v(2)), ("has", v(8)), ("get_pos", v(11))];

/// `obj:<name>()`
const METHODS: &[(&str, Version)] = &[
    ("send_mapblock", v(1)),
    ("set_fov", v(1)),
    ("get_moon", v(2)),
    ("get_sky_color", v(2)),
    ("get_stars", v(2)),
    ("get_sun", v(2)),
    ("set_moon", v(2)),
    ("set_stars", v(2)),
    ("set_sun", v(2)),
    ("get_short_description", v(4)),
    ("set_minimap_modes", v(4)),
    ("add_wear_by_uses", v(6)),
    ("get_lighting", v(6)),
    ("get_bone_override", v(9)),
    ("get_bone_overrides", v(9)),
    ("get_wear_bar_params", v(9)),
    ("hud_get_all", v(9)),
    ("set_bone_override", v(9)),
    ("set_wear_bar_params", v(9)),
    ("get_effective_observers", v(10)),
    ("get_observers", v(10)),
    ("set_observers", v(10)),
    ("get_guid", v(13)),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub version: Version,
    pub what: String,
    pub line: usize,
}

pub fn requirements(ast: &ast::Ast, ast_context: &AstContext) -> Vec<Requirement> {
    let mut visitor = RequirementVisitor { ast_context, found: Vec::new() };
    visitor.visit_ast(ast);
    visitor.found.sort_by_key(|r| r.line);
    visitor.found
}

fn lookup(table: &[(&str, Version)], name: &str) -> Option<Version> {
    table.iter().find(|(n, _)| *n == name).map(|(_, version)| *version)
}

struct RequirementVisitor<'a> {
    ast_context: &'a AstContext,
    found: Vec<Requirement>,
}

impl RequirementVisitor<'_> {
    fn check(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) {
        if let Prefix::Name(name) = prefix {
            let first = match suffixes.first() {
                Some(Suffix::Index(Index::Dot { name, .. })) => Some(name.token().to_string()),
                _ => None,
            };
            let global = name.token().to_string();
            let found = match (global.as_str(), first) {
                (_, Some(first)) if luanti::luanti_namespace(name, self.ast_context).is_some() => {
                    match (first.as_str(), suffixes.get(1)) {
                        ("settings", Some(Suffix::Call(Call::MethodCall(method)))) => {
                            let method = method.name().token().to_string();
                            lookup(SETTINGS_METHODS, &method).map(|v| (v, format!("{global}.settings:{method}")))
                        }
                        _ => lookup(FUNCTIONS, &first).map(|v| (v, format!("{global}.{first}"))),
                    }
                }
                ("vector", Some(first)) if luanti::is_global(name, self.ast_context) => {
                    lookup(VECTOR, &first).map(|v| (v, format!("vector.{first}")))
                }
                (class, None) if matches!(suffixes.first(), Some(Suffix::Call(_))) => lookup(CLASSES, class)
                    .filter(|_| luanti::is_global(name, self.ast_context))
                    .map(|v| (v, class.to_owned())),
                _ => None,
            };
            if let Some((version, what)) = found {
                self.push(version, what, name);
            }
        }

        for suffix in suffixes {
            let Suffix::Call(Call::MethodCall(method)) = suffix else { continue };
            let name = method.name().token().to_string();
            if let Some(version) = lookup(METHODS, &name) {
                self.push(version, format!(":{name}()"), method.name());
            }
        }
    }

    fn push(&mut self, version: Version, what: String, node: &impl Node) {
        let line = node.start_position().map_or(0, |p| p.line());
        self.found.push(Requirement { version, what, line });
    }
}

impl Visitor for RequirementVisitor<'_> {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        self.check(call.prefix(), &call.suffixes().collect::<Vec<_>>());
    }

    fn visit_var_expression(&mut self, var: &ast::VarExpression) {
        self.check(var.prefix(), &var.suffixes().collect::<Vec<_>>());
    }
}

#[cfg(test)]
mod tests {
    use super::{CLASSES, FUNCTIONS, METHODS, SETTINGS_METHODS, VECTOR, requirements};
    use crate::lint::luanti::Version;
    use selene_lib::lints::AstContext;

    fn found(code: &str) -> Vec<(String, String, usize)> {
        let ast = crate::lint::parse(code).unwrap();
        requirements(&ast, &AstContext::from_ast(&ast))
            .into_iter()
            .map(|r| (r.version.to_string(), r.what, r.line))
            .collect()
    }

    fn entry(version: &str, what: &str, line: usize) -> (String, String, usize) {
        (version.to_owned(), what.to_owned(), line)
    }

    #[test]
    fn tables_are_sorted_and_unique() {
        for table in [FUNCTIONS, VECTOR, CLASSES, SETTINGS_METHODS, METHODS] {
            let mut names: Vec<_> = table.iter().map(|(n, _)| *n).collect();
            names.sort();
            names.dedup();
            assert_eq!(names.len(), table.len());
            assert!(table.is_sorted_by_key(|(_, v)| *v));
            assert!(table.iter().all(|(_, v)| *v > Version(5, 0)));
        }
    }

    #[test]
    fn core_functions() {
        assert_eq!(found("print(core.sha256(\"a\"))"), [entry("5.9", "core.sha256", 1)]);
        assert_eq!(found("local f = minetest.encode_png"), [entry("5.5", "minetest.encode_png", 1)]);
        assert_eq!(found("\ncore.register_on_authplayer(function() end)"), [entry("5.3", "core.register_on_authplayer", 2)]);
    }

    #[test]
    fn old_and_unknown_functions_need_nothing() {
        assert_eq!(found("core.register_node(\"a:b\", {})\ncore.get_node(pos)\nmymod.sha256()"), []);
    }

    #[test]
    fn locals_shadowing_globals_need_nothing() {
        assert_eq!(found("local core = {}\ncore.sha256()"), []);
        assert_eq!(found("local vector = {}\nvector.zero()"), []);
        assert_eq!(found("local ValueNoise = print\nValueNoise({})"), []);
    }

    #[test]
    fn vectors_classes_and_methods() {
        assert_eq!(found("local v = vector.zero()"), [entry("5.5", "vector.zero", 1)]);
        assert_eq!(found("local n = ValueNoise(np)"), [entry("5.12", "ValueNoise", 1)]);
        assert_eq!(found("print(obj:get_guid())"), [entry("5.13", ":get_guid()", 1)]);
        assert_eq!(found("local x = core.get_player_by_name(n):get_guid().x"), [entry("5.13", ":get_guid()", 1)]);
        assert_eq!(found("vm:set_lighting(l)\nobj:get_pos()"), []);
    }

    #[test]
    fn settings_methods() {
        assert_eq!(found("core.settings:get_pos(\"a\")"), [entry("5.11", "core.settings:get_pos", 1)]);
        assert_eq!(found("core.settings:get(\"a\")\nobj:get_pos()"), []);
    }
}
