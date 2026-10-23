//! Shell quoting of project version references inside `run:` text.

use super::{RenderError, invalid};
use velnor_actions_contract::config::project_tool_refs::project_tool_ref_names;

/// Quote each `${VELNOR_TOOL_<NAME>}` in shell text as its own expansion:
/// unquoted becomes `"${..}"`, single-quoted becomes `'"${..}"'` (the
/// outer shell expands it before any nested `sh -c` sees the literal
/// version), double-quoted stays. Unbalanced quoting fails closed, and so
/// does a `case` command inside `$( )`: its `pattern)` arms are
/// indistinguishable from the end of the substitution without a shell
/// grammar, so the scanner refuses to guess.
pub(super) fn quote_run_refs(text: &str) -> Result<String, RenderError> {
    use velnor_actions_contract::config::project_tool_refs::PROJECT_TOOL_REF_PREFIX;
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Frame {
        /// Unquoted text (top level or inside `$( )`).
        Plain,
        /// Inside `"..."`.
        Double,
        /// Inside `'...'`.
        Single,
    }
    if project_tool_ref_names(text).is_empty() {
        return Ok(text.to_owned());
    }
    let mut out = String::with_capacity(text.len() + 8);
    // `$(` opens a fresh plain frame even inside double quotes.
    let mut stack = vec![Frame::Plain];
    let mut escaped = false;
    let mut rest = text;
    let mut previous = ' ';
    while let Some(ch) = rest.chars().next() {
        let frame = *stack.last().unwrap_or(&Frame::Plain);
        if frame == Frame::Plain
            && stack.len() > 1
            && !(previous.is_alphanumeric() || previous == '_')
            && rest
                .strip_prefix("case")
                .is_some_and(|after| after.starts_with(char::is_whitespace))
        {
            return Err(invalid("run_quoting_case_in_command_substitution"));
        }
        if !escaped && rest.starts_with(PROJECT_TOOL_REF_PREFIX) {
            let end = rest.find('}').ok_or_else(|| invalid("run_ref_unclosed"))? + 1;
            let reference = &rest[..end];
            match frame {
                Frame::Double => out.push_str(reference),
                Frame::Plain => {
                    for part in ["\"", reference, "\""] {
                        out.push_str(part);
                    }
                }
                Frame::Single => {
                    for part in ["'\"", reference, "\"'"] {
                        out.push_str(part);
                    }
                }
            }
            rest = &rest[end..];
            continue;
        }
        let mut width = ch.len_utf8();
        match (frame, ch) {
            _ if escaped => escaped = false,
            (Frame::Plain | Frame::Double, '\\') => escaped = true,
            (Frame::Plain | Frame::Double, '$') if rest.starts_with("$(") => {
                stack.push(Frame::Plain);
                width = 2;
            }
            (Frame::Plain, ')') if stack.len() > 1 => {
                stack.pop();
            }
            (Frame::Plain, '\'') => stack.push(Frame::Single),
            (Frame::Single, '\'') | (Frame::Double, '"') => {
                stack.pop();
            }
            (Frame::Plain, '"') => stack.push(Frame::Double),
            _ => {}
        }
        out.push_str(&rest[..width]);
        previous = ch;
        rest = &rest[width..];
    }
    if stack.len() != 1 || escaped {
        return Err(invalid("run_quoting_unbalanced"));
    }
    Ok(out)
}
