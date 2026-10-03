//! `-- flint: allow(lint)` comments, replacing selene `-- selene: allow(lint)` which is also supported

use full_moon::ast::{Ast, LastStmt, Stmt};
use full_moon::node::Node;
use full_moon::tokenizer::{Token, TokenReference, TokenType};
use full_moon::visitors::Visitor;
use selene_lib::lints::{Diagnostic, Label, Severity};
use selene_lib::{CheckerDiagnostic, LintVariation};

pub const INVALID: &str = "invalid_lint_filter";

const PREFIXES: &[&str] = &["flint:", "selene:"];

#[derive(Debug, PartialEq)]
struct Directive {
    global: bool,
    variation: LintVariation,
    lints: Vec<String>,
}

fn parse_line(line: &str) -> Option<Directive> {
    let text: String = line.split_whitespace().collect();
    let (global, text) = match text.strip_prefix('#') {
        Some(text) => (true, text),
        None => (false, text.as_str()),
    };
    let text = PREFIXES.iter().find_map(|prefix| text.strip_prefix(prefix))?;
    let (variation, rest) = text.split_once('(')?;
    let (lints, _) = rest.split_once(')')?;
    let variation = match variation {
        "allow" => LintVariation::Allow,
        "warn" => LintVariation::Warn,
        "deny" => LintVariation::Deny,
        _ => return None,
    };
    let lints: Vec<String> = lints.split(',').filter(|l| !l.is_empty()).map(str::to_owned).collect();
    (!lints.is_empty()).then_some(Directive { global, variation, lints })
}

struct Filter {
    lint: String,
    variation: LintVariation,
    scope: (usize, usize),
}

pub struct Filters {
    filters: Vec<Filter>,
    problems: Vec<Diagnostic>,
}

impl Filters {
    pub fn new(ast: &Ast, known: impl Fn(&str) -> bool) -> Filters {
        let mut statements = Statements(Vec::new());
        statements.visit_ast(ast);
        let first_code = ast.nodes().tokens().next().map(|t| t.token().start_position().bytes());

        let (mut filters, mut problems) = (Vec::new(), Vec::new());
        for (comment, directive) in directives(ast) {
            let range = (comment.start_position().bytes(), comment.end_position().bytes());
            let scope = if directive.global {
                if first_code.is_some_and(|code| range.0 >= code) {
                    problems.push(Diagnostic::new(
                        INVALID,
                        "global filters must come before any code".to_owned(),
                        Label::new(range),
                    ));
                    continue;
                }
                (0, usize::MAX)
            } else {
                match statements.0.iter().filter(|s| s.0 <= range.0 && range.1 <= s.1).min_by_key(|s| s.1 - s.0) {
                    Some(&scope) => scope,
                    None => continue,
                }
            };
            for lint in directive.lints {
                if !known(&lint) {
                    problems.push(Diagnostic::new(INVALID, format!("no lint named `{lint}` exists"), Label::new(range)));
                    continue;
                }
                filters.push(Filter { lint, variation: directive.variation, scope });
            }
        }
        Filters { filters, problems }
    }

    pub fn apply(self, diagnostics: Vec<CheckerDiagnostic>, invalid: Severity) -> Vec<CheckerDiagnostic> {
        let mut diagnostics: Vec<CheckerDiagnostic> = diagnostics
            .into_iter()
            .map(|mut d| {
                let start = d.diagnostic.start_position() as usize;
                let filter = self
                    .filters
                    .iter()
                    .rev()
                    .filter(|f| f.lint == d.diagnostic.code && f.scope.0 <= start && start < f.scope.1)
                    .min_by_key(|f| f.scope.1 - f.scope.0);
                if let Some(filter) = filter {
                    d.severity = filter.variation.to_severity();
                }
                d
            })
            .collect();
        diagnostics.extend(self.problems.into_iter().map(|diagnostic| CheckerDiagnostic { diagnostic, severity: invalid }));
        diagnostics
    }
}

pub fn hide_from_selene(code: &str, ast: &Ast) -> Option<String> {
    let mut bytes = code.as_bytes().to_vec();
    let mut changed = false;
    for (comment, _) in directives(ast) {
        let range = comment.start_position().bytes()..comment.end_position().bytes();
        for b in &mut bytes[range] {
            if *b == b':' {
                *b = b';';
                changed = true;
            }
        }
    }
    changed.then(|| String::from_utf8(bytes).expect("still utf-8"))
}

fn directives(ast: &Ast) -> Vec<(&Token, Directive)> {
    let tokens = ast.nodes().tokens().chain(std::iter::once(ast.eof()));
    let trivia = tokens.flat_map(|t| t.leading_trivia().chain(t.trailing_trivia()));
    let mut found = Vec::new();
    for token in trivia {
        let text = match token.token_type() {
            TokenType::SingleLineComment { comment } | TokenType::MultiLineComment { comment, .. } => comment,
            _ => continue,
        };
        found.extend(text.lines().filter_map(parse_line).map(|d| (token, d)));
    }
    found
}

struct Statements(Vec<(usize, usize)>);

impl Statements {
    fn push(&mut self, node: &impl Node) {
        let tokens: Vec<&TokenReference> = node.tokens().collect();
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else { return };
        let start = first.leading_trivia().next().unwrap_or(first.token()).start_position().bytes();
        let end = last.trailing_trivia().last().unwrap_or(last.token()).end_position().bytes();
        self.0.push((start, end));
    }
}

impl Visitor for Statements {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        self.push(stmt);
    }

    fn visit_last_stmt(&mut self, stmt: &LastStmt) {
        self.push(stmt);
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_line, Directive};
    use selene_lib::LintVariation;

    #[test]
    fn parses_directives() {
        assert_eq!(
            parse_line(" flint: allow(a, b)"),
            Some(Directive { global: false, variation: LintVariation::Allow, lints: vec!["a".into(), "b".into()] })
        );
        assert_eq!(
            parse_line("# selene : deny(a) trailing text"),
            Some(Directive { global: true, variation: LintVariation::Deny, lints: vec!["a".into()] })
        );
        assert_eq!(parse_line(" flint: ignore(a)"), None);
        assert_eq!(parse_line(" flint: allow()"), None);
        assert_eq!(parse_line(" flint: allow(a"), None);
        assert_eq!(parse_line(" just a comment: allow(a)"), None);
    }
}
