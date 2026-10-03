//! deprecated `core` functions and object methods, a lint each, e.g. `luanti_deprecated_get_perlin`.

use full_moon::ast::{self, Call, FunctionArgs, Index, Prefix, Suffix};
use full_moon::node::Node;
use full_moon::visitors::Visitor;
use selene_lib::lints::{AstContext, Context, Diagnostic, Label, Severity};
use selene_lib::CheckerConfig;

use super::Rule;

enum Kind {
    /// `core.name`, only deprecated when called with more arguments than `max_args`, if set
    Function { max_args: Option<usize> },
    /// `obj:name()`
    Method,
}

struct Deprecated {
    lint: &'static str,
    function: &'static str,
    kind: Kind,
    message: &'static str,
    note: Option<&'static str>,
}

const DEPRECATED: &[Deprecated] = &[
    Deprecated {
        lint: "luanti_deprecated_register_on_auth_fail",
        function: "register_on_auth_fail",
        kind: Kind::Function { max_args: None },
        message: "use `core.register_on_authplayer`",
        note: Some("its callback is `function(name, ip, is_success)`, check `is_success`"),
    },
    Deprecated {
        lint: "luanti_deprecated_setting_get_pos",
        function: "setting_get_pos",
        kind: Kind::Function { max_args: None },
        message: "use `core.settings:get_pos(name)`",
        note: None,
    },
    Deprecated {
        lint: "luanti_legacy_get_value_noise",
        function: "get_value_noise",
        kind: Kind::Function { max_args: Some(1) },
        message: "pass a noiseparams table",
        note: Some("try: core.get_value_noise({ seed = seeddiff, octaves = octaves, persistence = persistence, spread = { x = spread, y = spread, z = spread } })"),
    },
    Deprecated {
        lint: "luanti_deprecated_get_perlin",
        function: "get_perlin",
        kind: Kind::Function { max_args: None },
        message: "renamed to `core.get_value_noise`",
        note: Some("pass it a noiseparams table, positional arguments are deprecated too"),
    },
    Deprecated {
        lint: "luanti_deprecated_get_mapgen_params",
        function: "get_mapgen_params",
        kind: Kind::Function { max_args: None },
        message: "use `core.get_mapgen_setting(name)`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_set_mapgen_params",
        function: "set_mapgen_params",
        kind: Kind::Function { max_args: None },
        message: "use `core.set_mapgen_setting(name, value, override)`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_get_node_group",
        function: "get_node_group",
        kind: Kind::Function { max_args: None },
        message: "use `core.get_item_group(name, group)`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_get_entity_name",
        function: "get_entity_name",
        kind: Kind::Method,
        message: "use `obj:get_luaentity().name`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_get_player_velocity",
        function: "get_player_velocity",
        kind: Kind::Method,
        message: "use `player:get_velocity()`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_add_player_velocity",
        function: "add_player_velocity",
        kind: Kind::Method,
        message: "use `player:add_velocity(vel)`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_get_look_pitch",
        function: "get_look_pitch",
        kind: Kind::Method,
        message: "use `player:get_look_vertical()`",
        note: Some("`get_look_pitch` is inverted: it's positive looking up, `get_look_vertical` is positive looking down"),
    },
    Deprecated {
        lint: "luanti_deprecated_get_look_yaw",
        function: "get_look_yaw",
        kind: Kind::Method,
        message: "use `player:get_look_horizontal()`",
        note: Some("`get_look_yaw` is offset by pi/2 from `get_look_horizontal`"),
    },
    Deprecated {
        lint: "luanti_deprecated_set_look_pitch",
        function: "set_look_pitch",
        kind: Kind::Method,
        message: "use `player:set_look_vertical(radians)`",
        note: None,
    },
    Deprecated {
        lint: "luanti_deprecated_set_look_yaw",
        function: "set_look_yaw",
        kind: Kind::Method,
        message: "use `player:set_look_horizontal(radians)`",
        note: None,
    },
];

/// a lint for every deprecated function, with severities from `config`
pub fn rules(config: &mut CheckerConfig<toml::Value>) -> Result<Vec<Box<dyn Rule>>, String> {
    DEPRECATED
        .iter()
        .map(|deprecated| {
            if config.config.remove(deprecated.lint).is_some() {
                return Err(format!("[linter.config] {}: takes no options", deprecated.lint));
            }
            let severity = config.lints.get(deprecated.lint).map_or(Severity::Warning, |v| v.to_severity());
            Ok(Box::new(DeprecatedRule { deprecated, severity }) as Box<dyn Rule>)
        })
        .collect()
}

struct DeprecatedRule {
    deprecated: &'static Deprecated,
    severity: Severity,
}

impl Rule for DeprecatedRule {
    fn name(&self) -> &'static str {
        self.deprecated.lint
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn pass(&self, ast: &ast::Ast, _: &Context, ast_context: &AstContext) -> Vec<Diagnostic> {
        let mut visitor = DeprecatedVisitor { deprecated: self.deprecated, ast_context, diagnostics: Vec::new() };
        visitor.visit_ast(ast);
        visitor.diagnostics
    }
}

struct DeprecatedVisitor<'a> {
    deprecated: &'static Deprecated,
    ast_context: &'a AstContext,
    diagnostics: Vec<Diagnostic>,
}

impl DeprecatedVisitor<'_> {
    fn check(&mut self, prefix: &Prefix, suffixes: &[&Suffix]) {
        match self.deprecated.kind {
            Kind::Function { max_args } => self.check_function(prefix, suffixes, max_args),
            Kind::Method => self.check_methods(suffixes),
        }
    }

    /// `core.name`, called or not
    fn check_function(&mut self, prefix: &Prefix, suffixes: &[&Suffix], max_args: Option<usize>) {
        let deprecated = self.deprecated;
        let Prefix::Name(namespace) = prefix else { return };
        let [Suffix::Index(Index::Dot { name, .. }), rest @ ..] = suffixes else { return };
        if name.token().to_string() != deprecated.function {
            return;
        }
        if let Some(max_args) = max_args {
            let Some(Suffix::Call(Call::AnonymousCall(args))) = rest.first() else { return };
            if argument_count(args) <= max_args {
                return;
            }
        }
        let Some(namespace) = super::luanti_namespace(namespace, self.ast_context) else { return };

        let start = prefix.start_position().map_or(0, |p| p.bytes());
        let end = suffixes[0].end_position().map_or(start, |p| p.bytes());
        let message = match max_args {
            Some(_) => format!("`{namespace}.{}` with positional arguments is deprecated, {}", deprecated.function, deprecated.message),
            None => format!("`{namespace}.{}` is deprecated, {}", deprecated.function, deprecated.message),
        };
        self.push(message, Label::new((start, end)));
    }

    /// `obj:name()` anywhere in a chain
    fn check_methods(&mut self, suffixes: &[&Suffix]) {
        for suffix in suffixes {
            let Suffix::Call(Call::MethodCall(call)) = suffix else { continue };
            if call.name().token().to_string() == self.deprecated.function {
                let message = format!("`{}` is deprecated, {}", self.deprecated.function, self.deprecated.message);
                self.push(message, Label::from_node(call.name(), None));
            }
        }
    }

    fn push(&mut self, message: String, label: Label) {
        self.diagnostics.push(Diagnostic::new_complete(
            self.deprecated.lint,
            message,
            label,
            self.deprecated.note.map(str::to_owned).into_iter().collect(),
            Vec::new(),
        ));
    }
}

impl Visitor for DeprecatedVisitor<'_> {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        self.check(call.prefix(), &call.suffixes().collect::<Vec<_>>());
    }

    fn visit_var_expression(&mut self, var: &ast::VarExpression) {
        // methods can't be referenced without calling them
        if let Kind::Function { .. } = self.deprecated.kind {
            self.check(var.prefix(), &var.suffixes().collect::<Vec<_>>());
        }
    }
}

fn argument_count(args: &FunctionArgs) -> usize {
    match args {
        FunctionArgs::Parentheses { arguments, .. } => arguments.len(),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::DEPRECATED;
    use selene_lib::lints::{AstContext, Context, Severity};
    use selene_lib::standard_library::StandardLibrary;
    use selene_lib::{CheckerConfig, LintVariation};

    /// the lint and labelled code of each diagnostic for `code`, from every deprecated function lint
    fn lint(code: &str) -> Vec<(&'static str, String)> {
        let ast = crate::lint::parse(code).unwrap();
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        let ast_context = AstContext::from_ast(&ast);
        let mut diagnostics: Vec<_> = super::rules(&mut CheckerConfig::default())
            .unwrap()
            .iter()
            .flat_map(|rule| rule.pass(&ast, &context, &ast_context))
            .collect();
        diagnostics.sort_by_key(|d| d.primary_label.range);
        diagnostics
            .into_iter()
            .map(|d| {
                let (start, end) = d.primary_label.range;
                (d.code, code[start as usize..end as usize].to_owned())
            })
            .collect()
    }

    #[test]
    fn every_function_has_its_own_lint() {
        let mut lints: Vec<_> = DEPRECATED.iter().map(|d| d.lint).collect();
        lints.sort();
        lints.dedup();
        assert_eq!(lints.len(), DEPRECATED.len());
    }

    #[test]
    fn severity_is_configurable() {
        let mut config = CheckerConfig::<toml::Value>::default();
        config.lints.insert("luanti_deprecated_get_perlin".to_owned(), LintVariation::Deny);
        let rules = super::rules(&mut config).unwrap();
        for rule in rules {
            let expected = if rule.name() == "luanti_deprecated_get_perlin" { Severity::Error } else { Severity::Warning };
            assert_eq!(rule.severity(), expected, "{}", rule.name());
        }
    }

    #[test]
    fn replacements_are_fine() {
        assert!(lint("core.register_on_authplayer(f)").is_empty());
        assert!(lint("core.get_value_noise({ seed = 1 })").is_empty());
        assert!(lint("core.get_value_noise(np)").is_empty());
        assert!(lint("print(player:get_velocity(), player:get_look_vertical())").is_empty());
        assert!(lint("print(other.get_perlin(1, 2, 3, 4), get_node_group(a, b))").is_empty());
    }

    #[test]
    fn core_functions_are_flagged() {
        assert_eq!(
            lint("core.register_on_auth_fail(function() end)"),
            [("luanti_deprecated_register_on_auth_fail", "core.register_on_auth_fail".to_owned())]
        );
        assert_eq!(
            lint("local p = minetest.setting_get_pos(\"static_spawnpoint\")"),
            [("luanti_deprecated_setting_get_pos", "minetest.setting_get_pos".to_owned())]
        );
        assert_eq!(lint("local n = core.get_perlin(np)"), [("luanti_deprecated_get_perlin", "core.get_perlin".to_owned())]);
        assert_eq!(
            lint("core.set_mapgen_params(core.get_mapgen_params())"),
            [
                ("luanti_deprecated_set_mapgen_params", "core.set_mapgen_params".to_owned()),
                ("luanti_deprecated_get_mapgen_params", "core.get_mapgen_params".to_owned())
            ]
        );
        assert_eq!(
            lint("print(core.get_node_group(\"a:b\", \"c\"))"),
            [("luanti_deprecated_get_node_group", "core.get_node_group".to_owned())]
        );
    }

    #[test]
    fn uncalled_function_is_flagged() {
        assert_eq!(
            lint("local get = core.get_node_group"),
            [("luanti_deprecated_get_node_group", "core.get_node_group".to_owned())]
        );
    }

    #[test]
    fn value_noise_positional_arguments_are_flagged() {
        assert_eq!(
            lint("core.get_value_noise(1, 3, 0.5, 100)"),
            [("luanti_legacy_get_value_noise", "core.get_value_noise".to_owned())]
        );
        assert!(lint("local f = core.get_value_noise").is_empty());
    }

    #[test]
    fn methods_are_flagged() {
        assert_eq!(
            lint("print(obj:get_entity_name(), player:get_player_velocity())"),
            [
                ("luanti_deprecated_get_entity_name", "get_entity_name".to_owned()),
                ("luanti_deprecated_get_player_velocity", "get_player_velocity".to_owned())
            ]
        );
        assert_eq!(
            lint("player:add_player_velocity(v)"),
            [("luanti_deprecated_add_player_velocity", "add_player_velocity".to_owned())]
        );
        assert_eq!(
            lint("core.get_player_by_name(n):set_look_pitch(core.get_player_by_name(n):get_look_pitch())"),
            [
                ("luanti_deprecated_set_look_pitch", "set_look_pitch".to_owned()),
                ("luanti_deprecated_get_look_pitch", "get_look_pitch".to_owned())
            ]
        );
        assert_eq!(
            lint("p:set_look_yaw(p:get_look_yaw())"),
            [
                ("luanti_deprecated_set_look_yaw", "set_look_yaw".to_owned()),
                ("luanti_deprecated_get_look_yaw", "get_look_yaw".to_owned())
            ]
        );
    }

    #[test]
    fn local_namespace_is_fine() {
        assert!(lint("local core = {}\ncore.get_perlin(np)").is_empty());
    }
}
