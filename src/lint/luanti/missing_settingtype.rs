//! `luanti_missing_settingtype`: for when a mod setting is not listed in `settingtypes.txt`.

use full_moon::ast::{self, Call, Expression, FunctionArgs, Index, Prefix, Suffix};
use full_moon::visitors::Visitor;
use selene_lib::lints::{AstContext, Context, Diagnostic, Label, Severity};
use selene_lib::CheckerConfig;

use super::{Found, ModInfo, Rule};

pub const NAME: &str = "luanti_missing_settingtype";

/// `core.settings:method(name)`, with the matching settingtypes.txt type
const METHODS: &[(&str, &str)] = &[
    ("get", "string"),
    ("get_bool", "bool"),
    ("get_pos", "v3f"),
    ("get_flags", "flags"),
    ("get_np_group", "noise_params_3d"),
    ("has", "string"),
];

/// deprecated old `core.function(name)`, with matching type
const FUNCTIONS: &[(&str, &str)] = &[("setting_get", "string"), ("setting_getbool", "bool"), ("setting_get_pos", "v3f")];

pub fn rule(config: &mut CheckerConfig<toml::Value>) -> Result<Box<dyn Rule>, String> {
    if config.config.remove(NAME).is_some() {
        return Err(format!("[linter.config] {NAME}: takes no options"));
    }
    let severity = config.lints.get(NAME).map_or(Severity::Warning, |v| v.to_severity());
    Ok(Box::new(MissingSettingtypeRule { severity }))
}

struct MissingSettingtypeRule {
    severity: Severity,
}

impl Rule for MissingSettingtypeRule {
    fn name(&self) -> &'static str {
        NAME
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn pass(&self, ast: &ast::Ast, _: &Context, ast_context: &AstContext, owner: Option<&ModInfo>) -> Vec<Found> {
        let Some(owner) = owner else { return Vec::new() };
        let mut visitor = SettingsVisitor { owner, ast_context, diagnostics: Vec::new() };
        visitor.visit_ast(ast);
        visitor.diagnostics.into_iter().map(Found::from).collect()
    }
}

struct SettingsVisitor<'a> {
    owner: &'a ModInfo,
    ast_context: &'a AstContext,
    diagnostics: Vec<Diagnostic>,
}

impl SettingsVisitor<'_> {
    fn setting_read<'c>(&self, call: &'c ast::FunctionCall) -> Option<(&'c FunctionArgs, &'static str)> {
        let Prefix::Name(namespace) = call.prefix() else { return None };
        super::luanti_namespace(namespace, self.ast_context)?;
        let suffixes: Vec<&Suffix> = call.suffixes().collect();
        match suffixes.as_slice() {
            [Suffix::Index(Index::Dot { name, .. }), Suffix::Call(Call::MethodCall(method))]
                if name.token().to_string() == "settings" =>
            {
                let method_name = method.name().token().to_string();
                let (_, kind) = METHODS.iter().find(|(m, _)| *m == method_name)?;
                Some((method.args(), kind))
            }
            [Suffix::Index(Index::Dot { name, .. }), Suffix::Call(Call::AnonymousCall(args))] => {
                let function = name.token().to_string();
                let (_, kind) = FUNCTIONS.iter().find(|(f, _)| *f == function)?;
                Some((args, kind))
            }
            _ => None,
        }
    }

    fn is_own_setting(&self, setting: &str) -> bool {
        setting
            .strip_prefix(self.owner.name.as_str())
            .is_some_and(|rest| rest.len() > 1 && (rest.starts_with('.') || rest.starts_with('_')))
    }
}

impl Visitor for SettingsVisitor<'_> {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        let Some((args, kind)) = self.setting_read(call) else { return };
        let Some((setting, node)) = first_string(args) else { return };
        if !self.is_own_setting(&setting) || self.owner.settings.contains(&setting) {
            return;
        }
        self.diagnostics.push(Diagnostic::new_complete(
            NAME,
            format!("setting `{setting}` is not in settingtypes.txt"),
            Label::from_node(node, None),
            vec![
                "settings in settingtypes.txt show up in the settings menu, with a description and default".to_owned(),
                format!("try adding: {setting} (Readable name) {kind} <default>"),
            ],
            Vec::new(),
        ));
    }
}

fn first_string(args: &FunctionArgs) -> Option<(String, &full_moon::tokenizer::TokenReference)> {
    let token = match args {
        FunctionArgs::Parentheses { arguments, .. } => match arguments.iter().next()? {
            Expression::String(token) => token,
            _ => return None,
        },
        FunctionArgs::String(token) => token,
        _ => return None,
    };
    match token.token().token_type() {
        full_moon::tokenizer::TokenType::StringLiteral { literal, .. } => Some((literal.to_string(), token)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::ModInfo;
    use super::NAME;
    use selene_lib::lints::{AstContext, Context};
    use selene_lib::standard_library::StandardLibrary;
    use selene_lib::CheckerConfig;

    fn lint(code: &str, settings: &[&str]) -> Vec<String> {
        let ast = crate::lint::parse(code).unwrap();
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        let owner = ModInfo {
            name: "mymod".to_owned(),
            settings: settings.iter().map(|s| s.to_string()).collect(),
            ..ModInfo::default()
        };
        super::rule(&mut CheckerConfig::default())
            .unwrap()
            .pass(&ast, &context, &AstContext::from_ast(&ast), Some(&owner))
            .into_iter()
            .map(|f| f.diagnostic)
            .inspect(|d| assert_eq!(d.code, NAME))
            .map(|d| {
                let (start, end) = d.primary_label.range;
                code[start as usize..end as usize].to_owned()
            })
            .collect()
    }

    #[test]
    fn missing_own_settings_are_flagged() {
        assert_eq!(lint("local s = core.settings:get(\"mymod.speed\")", &[]), ["\"mymod.speed\""]);
        assert_eq!(lint("if minetest.settings:get_bool('mymod_enabled', true) then end", &[]), ["'mymod_enabled'"]);
        assert_eq!(lint("local p = core.settings:get_pos [[mymod.spawn]]", &[]), ["[[mymod.spawn]]"]);
        assert_eq!(lint("local s = minetest.setting_getbool(\"mymod.old\")", &[]), ["\"mymod.old\""]);
    }

    #[test]
    fn listed_settings_are_fine() {
        assert!(lint("local s = core.settings:get(\"mymod.speed\")", &["mymod.speed"]).is_empty());
    }

    #[test]
    fn other_settings_are_ignored() {
        // engine, other mods, a mod whose name starts with ours, and the mod name alone
        assert!(lint("local s = core.settings:get(\"enable_damage\")", &[]).is_empty());
        assert!(lint("local s = core.settings:get(\"default.speed\")", &[]).is_empty());
        assert!(lint("local s = core.settings:get(\"mymodextra.speed\")", &[]).is_empty());
        assert!(lint("local s = core.settings:get(\"mymod\")", &[]).is_empty());
        assert!(lint("local s = core.settings:get(\"mymod.\")", &[]).is_empty());
    }

    #[test]
    fn writes_and_dynamic_names_are_ignored() {
        assert!(lint("core.settings:set(\"mymod.speed\", \"1\")", &[]).is_empty());
        assert!(lint("local s = core.settings:get(modname .. \".speed\")", &[]).is_empty());
        assert!(lint("local s = Settings(path):get(\"mymod.speed\")", &[]).is_empty());
        assert!(lint("local core = {}\nlocal s = core.settings:get(\"mymod.speed\")", &[]).is_empty());
    }

    #[test]
    fn outside_a_mod_is_ignored() {
        let ast = crate::lint::parse("local s = core.settings:get(\"mymod.speed\")").unwrap();
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        let rule = super::rule(&mut CheckerConfig::default()).unwrap();
        assert!(rule.pass(&ast, &context, &AstContext::from_ast(&ast), None).is_empty());
    }
}
