//! `luanti_minetest_global`: the `minetest` global instead of `core`, fixed by renaming it.

use std::convert::Infallible;

use full_moon::ast;
use selene_lib::lints::{AstContext, Context, Diagnostic, Label, Lint, LintType, Severity};

use crate::lint::fix::Edit;

pub const NAME: &str = "luanti_minetest_global";

pub struct MinetestGlobalLint;

impl Lint for MinetestGlobalLint {
    type Config = ();
    type Error = Infallible;

    const SEVERITY: Severity = Severity::Warning;
    const LINT_TYPE: LintType = LintType::Style;

    fn new(_: Self::Config) -> Result<Self, Self::Error> {
        Ok(MinetestGlobalLint)
    }

    fn pass(&self, _: &ast::Ast, _: &Context, ast_context: &AstContext) -> Vec<Diagnostic> {
        let mut diagnostics: Vec<Diagnostic> = ast_context
            .scope_manager
            .references
            .iter()
            // a local named `minetest` isn't Luanti's
            .filter(|(_, reference)| reference.name == "minetest" && reference.resolved.is_none())
            .map(|(_, reference)| {
                Diagnostic::new(
                    NAME,
                    "use `core` instead of `minetest`".to_owned(),
                    Label::new(reference.identifier),
                )
            })
            .collect();
        diagnostics.sort_by_key(|d| d.primary_label.range);
        diagnostics.dedup_by_key(|d| d.primary_label.range);
        diagnostics
    }
}

pub fn fix(diagnostic: &Diagnostic, ast_context: &AstContext) -> Option<Vec<Edit>> {
    if ast_context.scope_manager.variables.iter().any(|(_, variable)| variable.name == "core") {
        return None;
    }
    let (start, end) = diagnostic.primary_label.range;
    Some(vec![Edit { range: start as usize..end as usize, text: "core".to_owned() }])
}

#[cfg(test)]
mod tests {
    use super::{fix, MinetestGlobalLint, NAME};
    use selene_lib::lints::{AstContext, Context, Lint};
    use selene_lib::standard_library::StandardLibrary;

    /// the start of each diagnostic for `code`
    fn lint(code: &str) -> Vec<u32> {
        let ast = crate::lint::parse(code).unwrap();
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        MinetestGlobalLint
            .pass(&ast, &context, &AstContext::from_ast(&ast))
            .into_iter()
            .inspect(|d| assert_eq!(d.code, NAME))
            .map(|d| d.primary_label.range.0)
            .collect()
    }

    #[test]
    fn core_is_fine() {
        assert!(lint("core.log(\"a\")").is_empty());
        assert!(lint("print(\"minetest\", t.minetest)").is_empty());
    }

    #[test]
    fn minetest_is_flagged() {
        assert_eq!(lint("minetest.log(\"a\")"), [0]);
        assert_eq!(lint("local mt = minetest\nminetest.register_node(\"a:b\", {})"), [11, 20]);
        assert_eq!(lint("minetest.env:get_node(pos)"), [0]);
    }

    fn fixed(code: &str) -> String {
        let ast = crate::lint::parse(code).unwrap();
        let ast_context = AstContext::from_ast(&ast);
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        let edits = MinetestGlobalLint
            .pass(&ast, &context, &ast_context)
            .iter()
            .filter_map(|d| fix(d, &ast_context))
            .flatten()
            .collect();
        crate::lint::fix::apply(code, edits).0
    }

    #[test]
    fn fix_renames_to_core() {
        assert_eq!(fixed("local mt = minetest
minetest.log(mt)"), "local mt = core
core.log(mt)");
    }

    #[test]
    fn no_fix_when_core_is_a_local() {
        let code = "local core = {}
minetest.log(core)";
        assert_eq!(fixed(code), code);
    }

    #[test]
    fn local_minetest_is_fine() {
        assert!(lint("local minetest = {}\nminetest.log(\"a\")").is_empty());
    }
}
