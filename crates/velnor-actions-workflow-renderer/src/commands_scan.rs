//! POSIX shell expansion spans used while quoting typed argv.

use crate::RenderError;

/// True when `bytes[index] == b'$'` starts a POSIX variable expansion.
pub(crate) fn is_expansion_at(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index + 1) {
        Some(b'{') => bytes.get(index + 2) != Some(&b'{'),
        Some(next) => {
            next.is_ascii_alphanumeric()
                || matches!(next, b'_' | b'@' | b'*' | b'#' | b'?' | b'$' | b'!' | b'-')
        }
        None => false,
    }
}

/// Return the byte after the complete expansion, or `None` for malformed input.
pub(crate) fn expansion_end(bytes: &[u8], index: usize) -> Option<usize> {
    if bytes.get(index + 1) != Some(&b'{') {
        let next = *bytes.get(index + 1)?;
        if next.is_ascii_alphabetic() || next == b'_' {
            let mut end = index + 2;
            while bytes
                .get(end)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                end += 1;
            }
            return Some(end);
        }
        return Some(index + 2);
    }

    let mut quotes = vec![0_u8];
    let mut offset = index + 2;
    while offset < bytes.len() {
        let quote = *quotes.last()?;
        let byte = bytes[offset];
        if quote != b'\'' && byte == b'\\' {
            offset = offset.checked_add(2)?.min(bytes.len());
            continue;
        }
        if quote != b'\'' && byte == b'$' && bytes.get(offset + 1) == Some(&b'{') {
            quotes.push(0);
            offset += 2;
            continue;
        }
        match quote {
            b'\'' if byte == b'\'' => *quotes.last_mut()? = 0,
            b'"' if byte == b'"' => *quotes.last_mut()? = 0,
            0 if byte == b'\'' || byte == b'"' => *quotes.last_mut()? = byte,
            0 if byte == b'}' => {
                quotes.pop()?;
                if quotes.is_empty() {
                    return Some(offset + 1);
                }
            }
            _ => {}
        }
        offset += 1;
    }
    None
}

/// Quote an expansion as one shell word and translate its fallback syntax.
pub(crate) fn quote_expansion(expansion: &str) -> Result<String, RenderError> {
    quote_expansion_at_depth(expansion, 0)
}

fn quote_expansion_at_depth(expansion: &str, depth: usize) -> Result<String, RenderError> {
    if depth > MAX_PARAMETER_NESTING {
        return Err(unsupported_expansion());
    }
    let bytes = expansion.as_bytes();
    if bytes.first() != Some(&b'$') || !is_expansion_at(bytes, 0) {
        return Err(unsupported_expansion());
    }
    if bytes.get(1) != Some(&b'{') {
        if bytes.get(1) == Some(&b'@') {
            return Err(unsupported_expansion());
        }
        return Ok(format!("\"{expansion}\""));
    }
    if expansion_end(bytes, 0) != Some(bytes.len()) {
        return Err(unsupported_expansion());
    }
    let fallback = braced_fallback_start(bytes).ok_or_else(unsupported_expansion)?;
    if fallback == bytes.len() - 1 {
        return Ok(format!("\"{expansion}\""));
    }
    validate_fallback_expansions(expansion, fallback, bytes.len() - 1, depth + 1)?;
    rewrite_braced_fallback(expansion, fallback)
}

fn validate_fallback_expansions(
    expansion: &str,
    start: usize,
    end: usize,
    depth: usize,
) -> Result<(), RenderError> {
    let bytes = expansion.as_bytes();
    let mut quote = 0_u8;
    let mut offset = start;
    while offset < end {
        let byte = bytes[offset];
        if quote != b'\'' && byte == b'\\' {
            offset = offset.checked_add(2).ok_or_else(unsupported_expansion)?;
            continue;
        }
        if quote != b'\'' && byte == b'$' && is_expansion_at(bytes, offset) {
            let nested_end = expansion_end(bytes, offset).ok_or_else(unsupported_expansion)?;
            quote_expansion_at_depth(&expansion[offset..nested_end], depth)?;
            offset = nested_end;
            continue;
        }
        match quote {
            b'\'' if byte == b'\'' => quote = 0,
            b'"' if byte == b'"' => quote = 0,
            0 if byte == b'\'' || byte == b'"' => quote = byte,
            _ => {}
        }
        offset += char_width(bytes, offset);
    }
    Ok(())
}

fn braced_fallback_start(bytes: &[u8]) -> Option<usize> {
    let mut offset = 2;
    if !bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
    {
        return None;
    }
    offset += 1;
    while bytes
        .get(offset)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        offset += 1;
    }
    if bytes.get(offset) == Some(&b'}') {
        return Some(offset);
    }
    if bytes.get(offset) == Some(&b':')
        && bytes
            .get(offset + 1)
            .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'=' | b'?'))
    {
        return Some(offset + 2);
    }
    if bytes
        .get(offset)
        .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'=' | b'?'))
    {
        return Some(offset + 1);
    }
    None
}

fn rewrite_braced_fallback(expansion: &str, fallback: usize) -> Result<String, RenderError> {
    let bytes = expansion.as_bytes();
    let mut quotes = vec![0_u8];
    let mut rewritten = String::with_capacity(expansion.len());
    rewritten.push_str(&expansion[..fallback]);
    let mut offset = fallback;
    while offset < bytes.len() {
        let quote = *quotes.last().ok_or_else(unsupported_expansion)?;
        let byte = bytes[offset];
        if quote == b'\'' {
            if byte == b'\'' {
                quotes.pop().ok_or_else(unsupported_expansion)?;
                offset += 1;
                continue;
            }
            if byte == b'}' {
                rewritten.push_str("\"}\"");
                offset += 1;
                continue;
            }
            if matches!(byte, b'$' | b'`' | b'"' | b'\\') {
                rewritten.push('\\');
            }
            push_source_char(&mut rewritten, expansion, offset);
            offset += char_width(bytes, offset);
            continue;
        }
        if byte == b'\\' {
            let next_index = offset.checked_add(1).ok_or_else(unsupported_expansion)?;
            let next = *bytes.get(next_index).ok_or_else(unsupported_expansion)?;
            if quote == 0 && next == b'}' {
                rewritten.push_str("\"}\"");
                offset += 2;
                continue;
            }
            if quote == 0 && next != b'\n' && !matches!(next, b'$' | b'`' | b'"' | b'\\') {
                push_source_char(&mut rewritten, expansion, next_index);
                offset = next_index + char_width(bytes, next_index);
                continue;
            }
            push_source_char(&mut rewritten, expansion, offset);
            push_source_char(&mut rewritten, expansion, next_index);
            offset = next_index + char_width(bytes, next_index);
            continue;
        }
        if quote != b'\'' && byte == b'$' && bytes.get(offset + 1) == Some(&b'{') {
            rewritten.push_str("${");
            quotes.push(0);
            offset += 2;
            continue;
        }
        match quote {
            0 if byte == b'\'' => {
                quotes.push(b'\'');
                offset += 1;
                continue;
            }
            0 if byte == b'"' => *quotes.last_mut().ok_or_else(unsupported_expansion)? = b'"',
            b'"' if byte == b'"' => *quotes.last_mut().ok_or_else(unsupported_expansion)? = 0,
            0 if byte == b'}' => {
                rewritten.push('}');
                quotes.pop().ok_or_else(unsupported_expansion)?;
                if quotes.is_empty() {
                    offset += 1;
                    break;
                }
                offset += 1;
                continue;
            }
            _ => {}
        }
        push_source_char(&mut rewritten, expansion, offset);
        offset += char_width(bytes, offset);
    }
    if !quotes.is_empty() || offset != bytes.len() {
        return Err(unsupported_expansion());
    }
    Ok(format!("\"{rewritten}\""))
}

fn char_width(bytes: &[u8], offset: usize) -> usize {
    if bytes[offset].is_ascii() {
        1
    } else {
        std::str::from_utf8(&bytes[offset..])
            .ok()
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8)
    }
}

fn push_source_char(out: &mut String, source: &str, offset: usize) {
    let width = char_width(source.as_bytes(), offset);
    out.push_str(&source[offset..offset + width]);
}

fn unsupported_expansion() -> RenderError {
    RenderError::BadCommand("unsupported_shell_expansion_quoting".to_owned())
}

const MAX_PARAMETER_NESTING: usize = 32;
