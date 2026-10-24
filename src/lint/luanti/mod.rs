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
use crate::parsers::settings::Settings;
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
    /// the highest `min_minetest_version` of the mod, and of the modpacks and game it's in
    pub min_version: Option<Version>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32);

impl Version {
    pub fn parse(text: &str) -> Option<Version> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.trim().parse().ok()?;
        let minor = parts.next()?.trim().parse().ok()?;
        Some(Version(major, minor))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}", self.0, self.1)
    }
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

        let (mut settings, mut min_version) = (BTreeSet::new(), None);
        for (i, dir) in dir.ancestors().enumerate() {
            let kind = content_type::detect(dir);
            if i > 0 && !matches!(kind, ContentType::Modpack | ContentType::Game) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(dir.join(settingtypes_txt::FILE_NAME)) {
                settings.extend(settingtypes_txt::parse(&text));
            }
            let conf = kind.conf_file().and_then(|conf| fs::read_to_string(dir.join(conf)).ok());
            let version = conf.and_then(|text| Version::parse(Settings::parse(&text).get("min_minetest_version")?));
            min_version = min_version.max(version);
            if kind == ContentType::Game {
                break;
            }
        }
        Some(ModInfo { name, settings, min_version })
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
    ) -> Vec<Found>;
}

/// a diagnostic, with the edits that fix it if it can be fixed safely
pub struct Found {
    pub diagnostic: Diagnostic,
    pub fix: Option<Vec<Edit>>,
}

impl From<Diagnostic> for Found {
    fn from(diagnostic: Diagnostic) -> Self {
        Found { diagnostic, fix: None }
    }
}

trait Fixable: Lint {
    fn pass_with_fixes(&self, ast: &full_moon::ast::Ast, context: &Context, ast_context: &AstContext) -> Vec<Found>;
}

fn diagnostics(found: Vec<Found>) -> Vec<Diagnostic> {
    found.into_iter().map(|f| f.diagnostic).collect()
}

fn byte_range(node: &impl Node) -> std::ops::Range<usize> {
    let start = node.start_position().map_or(0, |p| p.bytes());
    start..node.end_position().map_or(start, |p| p.bytes())
}

struct Configured<L> {
    name: &'static str,
    lint: L,
    severity: Severity,
}

impl<L: Fixable> Rule for Configured<L> {
    fn name(&self) -> &'static str {
        self.name
    }

    fn severity(&self) -> Severity {
        self.severity
    }

    fn pass(&self, ast: &full_moon::ast::Ast, context: &Context, ast_context: &AstContext, _: Option<&ModInfo>) -> Vec<Found> {
        self.lint.pass_with_fixes(ast, context, ast_context)
    }
}

/// build every Luanti lint, taking their options out of `config` so selene doesn't see them
pub fn rules(config: &mut CheckerConfig<toml::Value>) -> Result<Vec<Box<dyn Rule>>, String> {
    let mut rules: Vec<Box<dyn Rule>> = vec![
        Box::new(rule::<legacy_physics_override::LegacyPhysicsOverrideLint>(legacy_physics_override::NAME, config)?),
        Box::new(rule::<deprecated_env::DeprecatedEnvLint>(deprecated_env::NAME, config)?),
        Box::new(rule::<minetest_global::MinetestGlobalLint>(minetest_global::NAME, config)?),
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
    Ok(Configured { name, lint, severity })
}
