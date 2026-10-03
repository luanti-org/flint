//! `luanti_legacy_physics_override`: `set_physics_override(speed, jump, gravity)` instead of a table.

use std::convert::Infallible;

use full_moon::ast::{self, Expression, FunctionArgs};
use full_moon::visitors::Visitor;
use selene_lib::lints::{AstContext, Context, Diagnostic, Label, Lint, LintType, Severity};

pub const NAME: &str = "luanti_legacy_physics_override";

/// the order of the old positional arguments
const FIELDS: &[&str] = &["speed", "jump", "gravity"];

pub struct LegacyPhysicsOverrideLint;

impl Lint for LegacyPhysicsOverrideLint {
    type Config = ();
    type Error = Infallible;

    const SEVERITY: Severity = Severity::Error;
    const LINT_TYPE: LintType = LintType::Correctness;

    fn new(_: Self::Config) -> Result<Self, Self::Error> {
        Ok(LegacyPhysicsOverrideLint)
    }

    fn pass(&self, ast: &ast::Ast, _: &Context, _: &AstContext) -> Vec<Diagnostic> {
        let mut visitor = PhysicsOverrideVisitor { diagnostics: Vec::new() };
        visitor.visit_ast(ast);
        visitor.diagnostics
    }
}

struct PhysicsOverrideVisitor {
    diagnostics: Vec<Diagnostic>,
}

impl Visitor for PhysicsOverrideVisitor {
    fn visit_method_call(&mut self, call: &ast::MethodCall) {
        if call.name().token().to_string() != "set_physics_override" {
            return;
        }
        let FunctionArgs::Parentheses { arguments, .. } = call.args() else { return };
        let args: Vec<&Expression> = arguments.iter().collect();
        let legacy = match args.as_slice() {
            [Expression::Number(_)] => true,
            [_] | [] => false,
            _ => true,
        };
        if !legacy {
            return;
        }

        let mut notes = Vec::new();
        if args.len() <= FIELDS.len() {
            let fields: Vec<String> = FIELDS
                .iter()
                .zip(&args)
                // nil left the value unchanged
                .filter(|(_, arg)| !is_nil(arg))
                .map(|(field, arg)| format!("{field} = {}", arg.to_string().trim()))
                .collect();
            notes.push(format!("try: set_physics_override({{ {} }})", fields.join(", ")));
        }
        self.diagnostics.push(Diagnostic::new_complete(
            NAME,
            "set_physics_override no longer takes positional arguments, pass a table".to_owned(),
            Label::from_node(call.args(), None),
            notes,
            Vec::new(),
        ));
    }
}

fn is_nil(expression: &Expression) -> bool {
    matches!(expression, Expression::Symbol(token) if token.token().to_string() == "nil")
}

#[cfg(test)]
mod tests {
    use super::{LegacyPhysicsOverrideLint, NAME};
    use selene_lib::lints::{AstContext, Context, Lint};
    use selene_lib::standard_library::StandardLibrary;

    /// the notes of each diagnostic for `code`
    fn lint(code: &str) -> Vec<Vec<String>> {
        let ast = crate::lint::parse(code).unwrap();
        let context = Context {
            standard_library: StandardLibrary::from_name("lua51").unwrap(),
            user_set_standard_library: None,
        };
        LegacyPhysicsOverrideLint
            .pass(&ast, &context, &AstContext::from_ast(&ast))
            .into_iter()
            .inspect(|d| assert_eq!(d.code, NAME))
            .map(|d| d.notes)
            .collect()
    }

    #[test]
    fn table_is_fine() {
        assert!(lint("player:set_physics_override({ speed = 2 })").is_empty());
        assert!(lint("player:set_physics_override { speed = 2 }").is_empty());
        assert!(lint("player:set_physics_override(physics)").is_empty());
        assert!(lint("player:set_physics_override(get_physics())").is_empty());
        assert!(lint("player:set_physics(1, 2, 3)").is_empty());
    }

    #[test]
    fn positional_arguments_are_flagged() {
        assert_eq!(
            lint("player:set_physics_override(1.5, jump, 0.5)"),
            [["try: set_physics_override({ speed = 1.5, jump = jump, gravity = 0.5 })"]]
        );
        assert_eq!(lint("player:set_physics_override(2)"), [["try: set_physics_override({ speed = 2 })"]]);
    }

    #[test]
    fn nil_arguments_are_skipped() {
        assert_eq!(
            lint("local x = get_player():set_physics_override(nil, 2)"),
            [["try: set_physics_override({ jump = 2 })"]]
        );
    }

    #[test]
    fn extra_arguments_have_no_suggestion() {
        assert_eq!(lint("player:set_physics_override(1, 1, 1, true, false)"), [Vec::<String>::new()]);
    }
}
