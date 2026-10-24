//! Luanti specific lints, add to selene

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use selene_lib::lints::{AstContext, Context, Diagnostic, Lint, Severity};
use selene_lib::CheckerConfig;
use full_moon::node::Node;
use serde::Deserialize;

use super::fix::Edit;
use crate::content_type::{self, ContentType};
use crate::parsers::{mod_conf, settingtypes_txt};

mod deprecated_env;
mod deprecated_function;
mod legacy_physics_override;
mod minetest_global;
mod missing_settingtype;

/// the mod a linted file belongs to
#[derive(Debug, Default)]
pub struct ModInfo {
    pub name: String,
    /// settings in the `settingtypes.txt` of the mod, and of the modpacks and game it's in
    pub settings: BTreeSet<String>,
}

impl ModInfo {
    /// the mod containing `file`, if it's in one
    pub fn find(file: &Path) -> Option<ModInfo> {
        let file = file.canonicalize().ok()?;
        let dir = file.ancestors().skip(1).find(|dir| content_type::detect(dir) == ContentType::Mod)?;
        let name = fs::read_to_string(dir.join(mod_conf::FILE_NAME))
            .ok()
            .and_then(|text| mod_conf::parse(&text).name)
            .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))?;

        let mut settings = BTreeSet::new();
        for (i, dir) in dir.ancestors().enumerate() {
            let kind = content_type::detect(dir);
            if i > 0 && !matches!(kind, ContentType::Modpack | ContentType::Game) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(dir.join(settingtypes_txt::FILE_NAME)) {
                settings.extend(settingtypes_txt::parse(&text));
            }
            if kind == ContentType::Game {
                break;
            }
        }
        Some(ModInfo { name, settings })
    }
}

/// a lint with its severity resolved from the config
pub trait Rule {
    fn name(&self) -> &'static str;
    fn severity(&self) -> Severity;
    fn pass(
        &self,
        ast: &full_moon::ast::Ast,
        context: &Context,
        ast_context: &AstContext,
        owner: Option<&ModInfo>,
    ) -> Vec<Diagnostic>;
    fn fix(&self, _diagnostic: &Diagnostic, _ast_context: &AstContext) -> Option<Vec<Edit>> {
        None
    }
}

type Fixer = fn(&Diagnostic, &AstContext) -> Option<Vec<Edit>>;

struct Configured<L> {
    name: &'static str,
    lint: L,
    severity: Severity,
    fix: Option<Fixer>,
}

impl<L> Configured<L> {
    fn with_fix(mut self, fix: Fixer) -> Self {
        self.fix = Some(fix);
        self
    }
}

impl<L: Lint> Rule for Configured<L> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn pass(&self, ast: &full_moon::ast::Ast, context: &Context, ast_context: &AstContext, _: Option<&ModInfo>) -> Vec<Diagnostic> {
        self.lint.pass(ast, context, ast_context)
    }

    fn fix(&self, diagnostic: &Diagnostic, ast_context: &AstContext) -> Option<Vec<Edit>> {
        self.fix?(diagnostic, ast_context)
    }
}

/// build every Luanti lint, taking their options out of `config` so selene doesn't see them
pub fn rules(config: &mut CheckerConfig<toml::Value>) -> Result<Vec<Box<dyn Rule>>, String> {
    let mut rules: Vec<Box<dyn Rule>> = vec![
        Box::new(rule::<legacy_physics_override::LegacyPhysicsOverrideLint>(legacy_physics_override::NAME, config)?),
        Box::new(rule::<deprecated_env::DeprecatedEnvLint>(deprecated_env::NAME, config)?),
        Box::new(rule::<minetest_global::MinetestGlobalLint>(minetest_global::NAME, config)?.with_fix(minetest_global::fix)),
    ];
    rules.push(missing_settingtype::rule(config)?);
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

fn rule<L>(name: &'static str, config: &mut CheckerConfig<toml::Value>) -> Result<Configured<L>, String>
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
    Ok(Configured { name, lint, severity, fix: None })
}
