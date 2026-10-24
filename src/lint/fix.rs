//! text edits that fix Lua lints, for `flint lint --write`

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// bytes of the source to replace
    pub range: Range<usize>,
    pub text: String,
}

pub fn apply(code: &str, mut edits: Vec<Edit>) -> (String, usize) {
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    let mut out = String::with_capacity(code.len());
    let (mut end, mut applied) = (0, 0);
    for edit in edits {
        if edit.range.start < end || edit.range.end > code.len() {
            continue;
        }
        out.push_str(&code[end..edit.range.start]);
        out.push_str(&edit.text);
        end = edit.range.end;
        applied += 1;
    }
    out.push_str(&code[end..]);
    (out, applied)
}

#[cfg(test)]
mod tests {
    use super::{apply, Edit};

    fn edit(start: usize, end: usize, text: &str) -> Edit {
        Edit { range: start..end, text: text.to_string() }
    }

    #[test]
    fn applies_in_order_of_position() {
        let edits = vec![edit(6, 7, "b"), edit(0, 1, "a")];
        assert_eq!(apply("x = 1 y", edits), ("a = 1 b".to_string(), 2));
    }

    #[test]
    fn skips_overlapping_edits() {
        let edits = vec![edit(0, 3, "abc"), edit(2, 4, "zz"), edit(4, 4, "!")];
        assert_eq!(apply("xxxx", edits), ("abcx!".to_string(), 2));
    }

    #[test]
    fn skips_edits_past_the_end() {
        assert_eq!(apply("x", vec![edit(0, 5, "y")]), ("x".to_string(), 0));
    }
}
