//! `luanti_deprecated_env`: `core.env` or `minetest.env`

use std::convert::Infallible;

use full_moon::ast::{self, Expression, Index, Prefix, Suffix};
use full_moon::node::Node;
use full_moon::visitors::Visitor;
use selene_lib::lints::{AstContext, Context, Diagnostic, Label, Lint, LintType, Severity};

pub const NAME: &str = "luanti_deprecated_env";

pub struct DeprecatedEnvLint;

impl Lint for DeprecatedEnvLint {
    type Config = ();
    type Error = Infallible;

    const SEVERITY: Severity = Severity::Error;
    const LINT_TYPE: LintType = LintType::Correctness;

    fn new(_: Self::Config) -> Result<Self, Self::Error> {
        Ok(DeprecatedEnvLint)
    }

    fn pass(&self, ast: &ast::Ast, _: &Context, ast_context: &AstContext) -> Vec<Diagnostic> {
        let mut visitor = EnvVisitor { ast_context, diagnostics: Vec::new() };
        visitor.visit_ast(ast);
        visitor.diagnostics
    }
}

struct EnvVisitor<'a> {
    ast_context: &'a AstContext,
    diagnostics: Vec<Diagnostic>,
}

impl EnvVisitor<'_> {
    fn check<'s>(&mut self, prefix: &Prefix, mut suffixes: impl Iterator<Item = &'s Suffix>) {
        let Prefix::Name(name) = prefix else { return };
        let Some(env) = suffixes.next().filter(|suffix| is_env(suffix)) else { return };
        let Some(namespace) = super::luanti_namespace(name, self.ast_context) else { return };
        let start = name.start_position().map_or(0, |p| p.bytes());

        let note = match suffixes.next() {
            Some(Suffix::Call(ast::Call::MethodCall(call))) => format!(
                "try: core.{}{}",
                call.name().token(),
                call.args().to_string().trim_end()
            ),
            Some(Suffix::Index(Index::Dot { name, .. })) => format!("try: core.{}", name.token()),
            _ => "its methods are functions on `core`, e.g. `core.get_node(pos)` instead of `core.env:get_node(pos)`"
                .to_owned(),
        };
        let end = env.end_position().map_or(start, |p| p.bytes());
        self.diagnostics.push(Diagnostic::new_complete(
            NAME,
            format!("`{namespace}.env` is deprecated, use `core` instead"),
            Label::new((start, end)),
            vec![note],
            Vec::new(),
        ));
    }
}

impl Visitor for EnvVisitor<'_> {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        self.check(call.prefix(), call.suffixes());
    }

    fn visit_var_expression(&mut self, var: &ast::VarExpression) {
        self.check(var.prefix(), var.suffixes());
    }
}

/// `.env` or `["env"]`
fn is_env(suffix: &Suffix) -> bool {
    match suffix {
        Suffix::Index(Index::Dot { name, .. }) => name.token().to_string() == "env",
        Suffix::Index(Index::Brackets { expression: Expression::String(s), .. }) => {
            matches!(s.token().token_type(), full_moon::tokenizer::TokenType::StringLiteral { literal, .. } if literal.as_str() == "env")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{DeprecatedEnvLint, NAME};
    use selene_lib::lints::{AstContext, Context, Lint};
    use selene_lib::standard_library::StandardLibrary;

    fn lint(code: &str) -> Vec<(String, Vec<String>)> {
        let ast = crate::lint::parse(code).unwrap();
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        DeprecatedEnvLint
            .pass(&ast, &context, &AstContext::from_ast(&ast))
            .into_iter()
            .inspect(|d| assert_eq!(d.code, NAME))
            .map(|d| {
                let (start, end) = d.primary_label.range;
                (code[start as usize..end as usize].to_owned(), d.notes)
            })
            .collect()
    }

    #[test]
    fn core_is_fine() {
        assert!(lint("core.get_node(pos)").is_empty());
        assert!(lint("local env = core.environment").is_empty());
        assert!(lint("print(env, other.env:get_node(pos))").is_empty());
    }

    #[test]
    fn method_call_is_flagged() {
        assert_eq!(
            lint("local node = minetest.env:get_node(pos)"),
            [("minetest.env".to_owned(), vec!["try: core.get_node(pos)".to_owned()])]
        );
        assert_eq!(
            lint("core.env:set_node(pos, { name = \"air\" })"),
            [("core.env".to_owned(), vec!["try: core.set_node(pos, { name = \"air\" })".to_owned()])]
        );
    }

    #[test]
    fn index_is_flagged() {
        assert_eq!(lint("print(core.env.foo)"), [("core.env".to_owned(), vec!["try: core.foo".to_owned()])]);
        assert_eq!(lint("print(core[\"env\"])").len(), 1);
    }

    #[test]
    fn on_its_own_is_flagged() {
        let diagnostics = lint("local env = minetest.env\nenv:get_node(pos)");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].0, "minetest.env");
    }

    #[test]
    fn local_namespace_is_fine() {
        assert!(lint("local core = {}\ncore.env:get_node(pos)").is_empty());
    }
}
