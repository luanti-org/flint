//! Luanti specific lints, add to selene

use selene_lib::lints::{AstContext, Context, Diagnostic, Lint, Severity};
use selene_lib::CheckerConfig;
use full_moon::node::Node;
use serde::Deserialize;

mod deprecated_env;
mod deprecated_function;
mod legacy_physics_override;
mod minetest_global;

/// a lint with its severity resolved from the config
pub trait Rule {
    fn name(&self) -> &'static str;
    fn severity(&self) -> Severity;
    fn pass(&self, ast: &full_moon::ast::Ast, context: &Context, ast_context: &AstContext) -> Vec<Diagnostic>;
}

struct Configured<L> {
    name: &'static str,
    lint: L,
    severity: Severity,
}

impl<L: Lint> Rule for Configured<L> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn pass(&self, ast: &full_moon::ast::Ast, context: &Context, ast_context: &AstContext) -> Vec<Diagnostic> {
        self.lint.pass(ast, context, ast_context)
    }
}

/// build every Luanti lint, taking their options out of `config` so selene doesn't see them
pub fn rules(config: &mut CheckerConfig<toml::Value>) -> Result<Vec<Box<dyn Rule>>, String> {
    let mut rules = vec![
        rule::<legacy_physics_override::LegacyPhysicsOverrideLint>(legacy_physics_override::NAME, config)?,
        rule::<deprecated_env::DeprecatedEnvLint>(deprecated_env::NAME, config)?,
        rule::<minetest_global::MinetestGlobalLint>(minetest_global::NAME, config)?,
    ];
    rules.extend(deprecated_function::rules(config)?);
    Ok(rules)
}

/// `core` or `minetest`, if `name` is one of them and the global rather than a local
fn luanti_namespace(name: &full_moon::tokenizer::TokenReference, ast_context: &AstContext) -> Option<String> {
    let namespace = name.token().to_string();
    if namespace != "core" && namespace != "minetest" {
        return None;
    }
    let start = name.start_position().map_or(0, |p| p.bytes());
    let global = ast_context
        .scope_manager
        .reference_at_byte(start)
        .is_none_or(|reference| reference.resolved.is_none());
    global.then_some(namespace)
}

fn rule<L>(name: &'static str, config: &mut CheckerConfig<toml::Value>) -> Result<Box<dyn Rule>, String>
where
    L: Lint + 'static,
    L::Config: Default,
{
    let options = match config.config.remove(name) {
        Some(value) => L::Config::deserialize(value).map_err(|e| format!("[linter.config] {name}: {e}"))?,
        None => L::Config::default(),
    };
    let lint = L::new(options).map_err(|e| format!("[linter.config] {name}: {e}"))?;
    let severity = config.lints.get(name).map_or(L::SEVERITY, |v| v.to_severity());
    Ok(Box::new(Configured { name, lint, severity }))
}
