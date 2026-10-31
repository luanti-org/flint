//! `luanti_minetest_domain`: minetest.net domains to update in code, package handled by package lint

use std::convert::Infallible;

use full_moon::ast;
use full_moon::node::Node;
use full_moon::tokenizer::{Token, TokenKind};
use selene_lib::lints::{AstContext, Context, Diagnostic, Label, Lint, LintType, Severity};

use super::{Fixable, Found};
use crate::lint::fix::Edit;

pub const NAME: &str = "luanti_minetest_domain";

pub(crate) const OLD: &str = "minetest.net";
pub(crate) const NEW: &str = "luanti.org";

pub struct MinetestDomainLint;

impl Lint for MinetestDomainLint {
    type Config = ();
    type Error = Infallible;

    const SEVERITY: Severity = Severity::Warning;
    const LINT_TYPE: LintType = LintType::Style;

    fn new(_: Self::Config) -> Result<Self, Self::Error> {
        Ok(MinetestDomainLint)
    }

    fn pass(&self, ast: &ast::Ast, context: &Context, ast_context: &AstContext) -> Vec<Diagnostic> {
        super::diagnostics(self.pass_with_fixes(ast, context, ast_context))
    }
}

impl Fixable for MinetestDomainLint {
    fn pass_with_fixes(&self, ast: &ast::Ast, _: &Context, _: &AstContext) -> Vec<Found> {
        let references = ast.nodes().tokens().chain(std::iter::once(ast.eof()));
        let tokens = references.flat_map(|r| r.leading_trivia().chain(std::iter::once(&**r)).chain(r.trailing_trivia()));
        tokens
            .filter(|token| is_text(token))
            .flat_map(|token| {
                let start = token.start_position().bytes();
                domains(&token.to_string()).into_iter().map(move |at| start + at..start + at + OLD.len())
            })
            .map(|range| Found {
                diagnostic: Diagnostic::new(
                    NAME,
                    format!("use `{NEW}` instead of `{OLD}`"),
                    Label::new((range.start, range.end)),
                ),
                fix: Some(vec![Edit { range, text: NEW.to_owned() }]),
            })
            .collect()
    }
}

/// strings and comments, where a domain can be
fn is_text(token: &Token) -> bool {
    matches!(
        token.token_kind(),
        TokenKind::StringLiteral | TokenKind::SingleLineComment | TokenKind::MultiLineComment
    )
}

pub(crate) fn domains(text: &str) -> Vec<usize> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let is_name = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    lower
        .match_indices(OLD)
        .map(|(at, _)| at)
        .filter(|&at| at == 0 || !is_name(bytes[at - 1]))
        .filter(|&at| {
            let rest = &bytes[at + OLD.len()..];
            match rest {
                [] => true,
                [b'.', next, ..] => !is_name(*next),
                [next, ..] => !is_name(*next),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{domains, Fixable, MinetestDomainLint, NAME};
    use selene_lib::lints::{AstContext, Context, Lint};
    use selene_lib::standard_library::StandardLibrary;

    fn context() -> Context {
        Context { standard_library: StandardLibrary::from_name("lua51").unwrap(), user_set_standard_library: None }
    }

    /// the start of each diagnostic for `code`
    fn lint(code: &str) -> Vec<u32> {
        let ast = crate::lint::parse(code).unwrap();
        MinetestDomainLint
            .pass(&ast, &context(), &AstContext::from_ast(&ast))
            .into_iter()
            .inspect(|d| assert_eq!(d.code, NAME))
            .map(|d| d.primary_label.range.0)
            .collect()
    }

    fn fixed(code: &str) -> String {
        let ast = crate::lint::parse(code).unwrap();
        let edits = MinetestDomainLint
            .pass_with_fixes(&ast, &context(), &AstContext::from_ast(&ast))
            .into_iter()
            .filter_map(|f| f.fix)
            .flatten()
            .collect();
        crate::lint::fix::apply(code, edits).0
    }

    #[test]
    fn finds_domains() {
        assert_eq!(domains("https://content.minetest.net/packages"), [16]);
        assert_eq!(domains("minetest.net"), [0]);
        assert_eq!(domains("see Minetest.NET."), [4]);
        assert_eq!(domains("a@minetest.net, b@forum.minetest.net"), [2, 24]);
    }

    #[test]
    fn ignores_longer_names() {
        assert!(domains("notminetest.net").is_empty());
        assert!(domains("minetest.network").is_empty());
        assert!(domains("minetest.net.example.com").is_empty());
        assert!(domains("minetest.net-mirror").is_empty());
    }

    #[test]
    fn strings_and_comments_are_flagged() {
        assert_eq!(lint("local url = \"https://minetest.net\""), [21]);
        assert_eq!(lint("-- see wiki.minetest.net\nprint(1)"), [12]);
        assert_eq!(lint("print([[forum.minetest.net]])\n--[[ dev.minetest.net ]]"), [14, 39]);
        assert_eq!(lint("print(1)\n-- minetest.net"), [12]);
    }

    #[test]
    fn code_is_fine() {
        assert!(lint("print(minetest.net)").is_empty());
        assert!(lint("print(\"luanti.org\")").is_empty());
    }

    #[test]
    fn fix_replaces_with_luanti() {
        assert_eq!(
            fixed("-- https://content.minetest.net/packages\nlocal u = 'https://minetest.net/a'"),
            "-- https://content.luanti.org/packages\nlocal u = 'https://luanti.org/a'"
        );
    }
}
