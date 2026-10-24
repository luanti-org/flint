//! translatable strings in Lua, like `xgettext -kS -kPS:1,2 -kcore.translate:1c,2 -kcore.translate_n:1c,2,3`
//! with `--add-comments=TRANSLATORS:`, plus Luanti's usual `NS`, `FS`, `NFS` and `FPS` helpers.

use std::collections::HashMap;

use full_moon::ast::{self, Call, Expression, FunctionArgs, Index, Prefix, Suffix};
use full_moon::node::Node;
use full_moon::tokenizer::{StringLiteralQuoteType, TokenReference, TokenType};
use full_moon::visitors::Visitor;
use selene_lib::lints::AstContext;

use crate::lint::luanti;

const SINGULAR: &[&str] = &["S", "NS", "FS", "NFS"];
const PLURAL: &[&str] = &["PS", "FPS"];

const TRANSLATORS: &str = "TRANSLATORS:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translatable {
    pub domain: Option<String>,
    pub msgid: String,
    pub plural: Option<String>,
    pub comment: Option<String>,
    pub line: usize,
}

pub fn extract(ast: &ast::Ast, ast_context: &AstContext) -> Vec<Translatable> {
    let mut translators = Translators { ast_context, domains: HashMap::new() };
    translators.visit_ast(ast);
    let mut visitor = StringVisitor { ast_context, domains: translators.domains, found: Vec::new() };
    visitor.visit_ast(ast);
    let mut found = visitor.found;
    found.sort_by_key(|(start, _)| *start);

    let mut comments = comment_blocks(ast).into_iter().peekable();
    let mut strings = Vec::with_capacity(found.len());
    for (start, mut string) in found {
        // a comment goes with the first string after it, on the same or the next line
        let mut block = None;
        while let Some(next) = comments.next_if(|c| c.end_byte <= start) {
            block = Some(next);
        }
        string.comment = block.filter(|b| string.line <= b.end_line + 1).and_then(|b| b.translators);
        strings.push(string);
    }
    strings
}

#[derive(Clone)]
struct Translator {
    domain: Option<String>,
    plural: bool,
}

struct Translators<'a> {
    ast_context: &'a AstContext,
    domains: HashMap<String, Translator>,
}

impl Translators<'_> {
    fn assign<'n>(&mut self, names: impl Iterator<Item = &'n TokenReference>, expressions: Vec<&Expression>) {
        let [Expression::FunctionCall(call)] = expressions[..] else { return };
        let Prefix::Name(namespace) = call.prefix() else { return };
        let [Suffix::Index(Index::Dot { name, .. }), Suffix::Call(Call::AnonymousCall(args))] =
            call.suffixes().collect::<Vec<_>>()[..]
        else {
            return;
        };
        if name.token().to_string() != "get_translator" || luanti::luanti_namespace(namespace, self.ast_context).is_none() {
            return;
        }
        let domain = string_arguments(args).into_iter().next().flatten();
        for (i, name) in names.take(2).enumerate() {
            self.domains.insert(name.token().to_string(), Translator { domain: domain.clone(), plural: i == 1 });
        }
    }
}

impl Visitor for Translators<'_> {
    fn visit_local_assignment(&mut self, assignment: &ast::LocalAssignment) {
        self.assign(assignment.names().iter(), assignment.expressions().iter().collect());
    }

    fn visit_assignment(&mut self, assignment: &ast::Assignment) {
        let names: Vec<&TokenReference> = assignment
            .variables()
            .iter()
            .filter_map(|var| match var {
                ast::Var::Name(name) => Some(name),
                _ => None,
            })
            .collect();
        if names.len() == assignment.variables().len() {
            self.assign(names.into_iter(), assignment.expressions().iter().collect());
        }
    }
}

struct StringVisitor<'a> {
    ast_context: &'a AstContext,
    domains: HashMap<String, Translator>,
    found: Vec<(usize, Translatable)>,
}

impl StringVisitor<'_> {
    fn domain(&self, name: &str) -> Option<String> {
        self.domains.get(name).or_else(|| self.domains.get("S")).and_then(|t| t.domain.clone())
    }
}

impl Visitor for StringVisitor<'_> {
    fn visit_function_call(&mut self, call: &ast::FunctionCall) {
        let Prefix::Name(prefix) = call.prefix() else { return };
        let prefix_name = prefix.token().to_string();
        let suffixes: Vec<&Suffix> = call.suffixes().collect();
        let (domain, mut args, plural) = match suffixes[..] {
            [Suffix::Call(Call::AnonymousCall(args))] => {
                let plural = match (prefix_name.as_str(), self.domains.get(&prefix_name)) {
                    (_, Some(translator)) => translator.plural,
                    (name, None) if SINGULAR.contains(&name) => false,
                    (name, None) if PLURAL.contains(&name) => true,
                    _ => return,
                };
                (self.domain(&prefix_name), string_arguments(args), plural)
            }
            [Suffix::Index(Index::Dot { name, .. }), Suffix::Call(Call::AnonymousCall(args))]
                if luanti::luanti_namespace(prefix, self.ast_context).is_some() =>
            {
                let plural = match name.token().to_string().as_str() {
                    "translate" => false,
                    "translate_n" => true,
                    _ => return,
                };
                let mut args = string_arguments(args).into_iter();
                let Some(domain) = args.next() else { return };
                (domain, args.collect(), plural)
            }
            _ => return,
        };

        args.resize(2, None);
        let Some(msgid) = args[0].take() else { return };
        let plural = match (plural, args[1].take()) {
            (false, _) => None,
            (true, Some(plural)) => Some(plural),
            (true, None) => return,
        };
        let start = call.start_position().map_or(0, |p| p.bytes());
        let line = call.start_position().map_or(0, |p| p.line());
        self.found.push((start, Translatable { domain, msgid, plural, comment: None, line }));
    }
}

fn string_arguments(args: &FunctionArgs) -> Vec<Option<String>> {
    match args {
        FunctionArgs::Parentheses { arguments, .. } => arguments.iter().map(constant_string).collect(),
        FunctionArgs::String(token) => vec![lua_string(token)],
        _ => Vec::new(),
    }
}

fn constant_string(expression: &Expression) -> Option<String> {
    match expression {
        Expression::String(token) => lua_string(token),
        Expression::Parentheses { expression, .. } => constant_string(expression),
        Expression::BinaryOperator { lhs, binop: ast::BinOp::TwoDots(_), rhs } => {
            Some(constant_string(lhs)? + &constant_string(rhs)?)
        }
        _ => None,
    }
}

pub fn lua_string(token: &TokenReference) -> Option<String> {
    let TokenType::StringLiteral { literal, quote_type, .. } = token.token().token_type() else { return None };
    if *quote_type == StringLiteralQuoteType::Brackets {
        let text = literal.strip_prefix("\r\n").or_else(|| literal.strip_prefix('\n')).unwrap_or(literal);
        return Some(text.to_string());
    }
    unescape(literal)
}

fn unescape(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            let mut buf = [0; 4];
            bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            continue;
        }
        let byte = match chars.next()? {
            'a' => 0x07,
            'b' => 0x08,
            'f' => 0x0c,
            'n' | '\n' => b'\n',
            'r' => b'\r',
            't' => b'\t',
            'v' => 0x0b,
            '\r' => {
                chars.next_if_eq(&'\n');
                b'\n'
            }
            'x' => {
                let hex: String = [chars.next()?, chars.next()?].iter().collect();
                u8::from_str_radix(&hex, 16).ok()?
            }
            'z' => {
                while chars.next_if(|c| c.is_whitespace()).is_some() {}
                continue;
            }
            'u' => {
                chars.next_if_eq(&'{')?;
                let mut hex = String::new();
                while let Some(c) = chars.next_if(|c| *c != '}') {
                    hex.push(c);
                }
                chars.next_if_eq(&'}')?;
                let c = char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?;
                let mut buf = [0; 4];
                bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                continue;
            }
            d if d.is_ascii_digit() => {
                let mut value = d.to_digit(10)?;
                for _ in 0..2 {
                    match chars.next_if(char::is_ascii_digit) {
                        Some(d) => value = value * 10 + d.to_digit(10)?,
                        None => break,
                    }
                }
                u8::try_from(value).ok()?
            }
            c @ ('\\' | '"' | '\'') => c as u8,
            _ => return None,
        };
        bytes.push(byte);
    }
    String::from_utf8(bytes).ok()
}

struct CommentBlock {
    end_byte: usize,
    end_line: usize,
    translators: Option<String>,
}

fn comment_blocks(ast: &ast::Ast) -> Vec<CommentBlock> {
    let tokens = ast.nodes().tokens().chain(std::iter::once(ast.eof()));
    let trivia = tokens.flat_map(|t| t.leading_trivia().chain(t.trailing_trivia()));
    let mut blocks: Vec<(usize, usize, Vec<String>)> = Vec::new();
    for token in trivia {
        let text = match token.token_type() {
            TokenType::SingleLineComment { comment } | TokenType::MultiLineComment { comment, .. } => comment,
            _ => continue,
        };
        let (start, end) = (token.start_position(), token.end_position());
        let lines = text.lines().map(|l| l.trim().to_owned());
        match blocks.last_mut() {
            Some((end_byte, end_line, texts)) if start.line() <= *end_line + 1 => {
                (*end_byte, *end_line) = (end.bytes(), end.line());
                texts.extend(lines);
            }
            _ => blocks.push((end.bytes(), end.line(), lines.collect())),
        }
    }
    blocks
        .into_iter()
        .map(|(end_byte, end_line, texts)| {
            let translators = texts.iter().position(|t| t.starts_with(TRANSLATORS)).map(|i| {
                texts[i..].iter().map(String::as_str).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n")
            });
            CommentBlock { end_byte, end_line, translators }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Translatable, extract, unescape};
    use selene_lib::lints::AstContext;

    fn strings(code: &str) -> Vec<Translatable> {
        let ast = crate::lint::parse(code).unwrap();
        extract(&ast, &AstContext::from_ast(&ast))
    }

    fn ids(code: &str) -> Vec<(Option<String>, String, Option<String>)> {
        strings(code).into_iter().map(|s| (s.domain, s.msgid, s.plural)).collect()
    }

    fn id(domain: Option<&str>, msgid: &str, plural: Option<&str>) -> (Option<String>, String, Option<String>) {
        (domain.map(str::to_owned), msgid.to_owned(), plural.map(str::to_owned))
    }

    #[test]
    fn helpers() {
        let code = r#"
            print(S("a"), NS("b"), FS("c @1", x), NFS"d", PS("e", "es", n), FPS("f", "fs", n), S[[g]])
        "#;
        assert_eq!(
            ids(code),
            [
                id(None, "a", None),
                id(None, "b", None),
                id(None, "c @1", None),
                id(None, "d", None),
                id(None, "e", Some("es")),
                id(None, "f", Some("fs")),
                id(None, "g", None),
            ]
        );
    }

    #[test]
    fn domain_from_get_translator() {
        let code = "local S, PS = core.get_translator(\"other\")\nprint(S(\"a\"), PS(\"b\", \"bs\", 2), FS(\"c\"))";
        assert_eq!(ids(code), [id(Some("other"), "a", None), id(Some("other"), "b", Some("bs")), id(Some("other"), "c", None)]);
        let code = "local T = minetest.get_translator(core.get_current_modname())\nprint(T(\"a\"))";
        assert_eq!(ids(code), [id(None, "a", None)]);
    }

    #[test]
    fn core_translate() {
        let code = "print(core.translate(\"dom\", \"a\"), core.translate_n(\"dom\", \"b\", \"bs\", n), core.translate(modname, \"c\"))";
        assert_eq!(ids(code), [id(Some("dom"), "a", None), id(Some("dom"), "b", Some("bs")), id(None, "c", None)]);
    }

    #[test]
    fn only_literals() {
        assert_eq!(ids("print(S(name), S(\"a\" .. b), PS(\"a\", plural, n), X(\"no\"))"), []);
        assert_eq!(ids("print(S(\"a\" .. \"b\" .. (\"c\")))"), [id(None, "abc", None)]);
    }

    #[test]
    fn escapes() {
        assert_eq!(unescape(r#"a\"b\'c\\d\ne\tf"#).as_deref(), Some("a\"b'c\\d\ne\tf"));
        assert_eq!(unescape(r"\65\x42\u{e9}\z   x").as_deref(), Some("ABéx"));
        assert_eq!(unescape(r"\q"), None);
        assert_eq!(ids("print(S[[\nline one\nline two]])"), [id(None, "line one\nline two", None)]);
    }

    #[test]
    fn translators_comments() {
        let code = "
-- TRANSLATORS: the stone
-- second line
local a = S(\"Stone\")
local b = S(\"Dirt\") -- TRANSLATORS: not before
-- TRANSLATORS: too far

local c = S(\"Sand\")
-- just a comment
local d = S(\"Wood\")
--[[ TRANSLATORS: long ]] print(S(\"Water\"), S(\"Lava\"))
";
        let comments: Vec<_> = strings(code).into_iter().map(|s| (s.msgid, s.comment)).collect();
        assert_eq!(
            comments,
            [
                ("Stone".to_owned(), Some("TRANSLATORS: the stone\nsecond line".to_owned())),
                ("Dirt".to_owned(), None),
                ("Sand".to_owned(), None),
                ("Wood".to_owned(), None),
                ("Water".to_owned(), Some("TRANSLATORS: long".to_owned())),
                ("Lava".to_owned(), None),
            ]
        );
        assert_eq!(strings(code)[0].line, 4);
    }
}
