#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Ident(String),
    Punct(char),
    String(Option<String>),
    Literal,
}

type LexResult<T> = Result<T, &'static str>;

pub(super) fn extract_public_string_const(source: &str, name: &str) -> Result<String, String> {
    let tokens = rust_tokens(source)?;
    if has_conditional_crate_attribute(&tokens)? {
        return Err("conditional crate attribute".to_owned());
    }
    let matches = top_level_const_matches(&tokens, name)?;
    if matches.is_empty() {
        return Err(format!("missing const {name}"));
    }
    if matches.len() != 1 {
        return Err(format!("duplicate const {name}"));
    }

    let index = matches[0];
    if index == 0 || tokens[index - 1] != Token::Ident("pub".to_owned()) {
        return Err(format!("unsupported const {name} visibility"));
    }
    if index >= 2 && tokens[index - 2] == Token::Punct(']') {
        return Err("unsupported authority attribute".to_owned());
    }

    let declaration = &tokens[index + 2..];
    if declaration.len() < 6
        || !matches!(
            &declaration[..4],
            [Token::Punct(':'), Token::Punct('&'), Token::Ident(ty), Token::Punct('=')]
                if ty == "str"
        )
        || declaration[5] != Token::Punct(';')
    {
        return Err(format!("unsupported const {name} declaration"));
    }
    match &declaration[4] {
        Token::String(Some(value)) => Ok(value.clone()),
        _ => Err(format!("const {name} must use a plain string literal")),
    }
}

fn rust_tokens(source: &str) -> LexResult<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < source.len() {
        let rest = &source[index..];
        let current = char_at(source, index)?;
        if current.is_whitespace() {
            index += current.len_utf8();
            continue;
        }
        if rest.starts_with("//") {
            index = source[index + 2..]
                .find('\n')
                .map_or(source.len(), |end| index + 2 + end + 1);
            continue;
        }
        if rest.starts_with("/*") {
            index = skip_block_comment(source, index)?;
            continue;
        }
        if let Some((end, hash_count)) = raw_string_start(source, index) {
            let hashes = "#".repeat(hash_count);
            let content_start = index + end;
            let terminator = format!("\"{hashes}");
            let close = source[content_start..]
                .find(&terminator)
                .ok_or("unterminated raw string")?;
            tokens.push(Token::Literal);
            index = content_start + close + terminator.len();
            continue;
        }
        if rest.starts_with("b\"") || rest.starts_with("c\"") {
            let family = if current == 'b' { "byte" } else { "c" };
            index = read_string(source, index + 1, family)?.0;
            tokens.push(Token::Literal);
            continue;
        }
        if current == '"' {
            let (end, value) = read_string(source, index, "str")?;
            tokens.push(Token::String(value));
            index = end;
            continue;
        }
        if rest.starts_with("b'") {
            index = char_end(source, index + 1, true)?.ok_or("unterminated character literal")?;
            tokens.push(Token::Literal);
            continue;
        }
        if current == '\''
            && let Some(end) = char_end(source, index, false)?
        {
            tokens.push(Token::Literal);
            index = end;
            continue;
        }
        if current == '_' || current.is_alphabetic() {
            let raw_identifier = rest.starts_with("r#")
                && source[index + 2..]
                    .chars()
                    .next()
                    .is_some_and(|next| next == '_' || next.is_alphabetic());
            let start = if raw_identifier { index + 2 } else { index };
            let end = source[start..]
                .char_indices()
                .skip(1)
                .find(|(_, next)| *next != '_' && !next.is_alphanumeric())
                .map_or(source.len(), |(offset, _)| start + offset);
            tokens.push(Token::Ident(source[start..end].to_owned()));
            index = end;
            continue;
        }
        tokens.push(Token::Punct(current));
        index += current.len_utf8();
    }
    without_macro_bodies(&tokens)
}

fn raw_string_start(source: &str, index: usize) -> Option<(usize, usize)> {
    let rest = &source[index..];
    let prefix = if rest.starts_with("br") || rest.starts_with("cr") {
        2
    } else if rest.starts_with('r') {
        1
    } else {
        return None;
    };
    let mut cursor = index + prefix;
    let mut hash_count = 0;
    while source[cursor..].starts_with('#') {
        cursor += 1;
        hash_count += 1;
    }
    source[cursor..]
        .starts_with('"')
        .then_some((cursor + 1 - index, hash_count))
}

fn skip_block_comment(source: &str, mut index: usize) -> LexResult<usize> {
    let mut depth = 1_usize;
    index += 2;
    while index < source.len() && depth > 0 {
        if source[index..].starts_with("/*") {
            depth += 1;
            index += 2;
        } else if source[index..].starts_with("*/") {
            depth -= 1;
            index += 2;
        } else {
            let current = char_at(source, index)?;
            index += current.len_utf8();
        }
    }
    if depth > 0 {
        return Err("unterminated block comment");
    }
    Ok(index)
}

fn read_string(source: &str, index: usize, family: &str) -> LexResult<(usize, Option<String>)> {
    let mut cursor = index + 1;
    let start = cursor;
    let mut escaped = false;
    while cursor < source.len() {
        let current = char_at(source, cursor)?;
        if current == '\\' {
            cursor = skip_escape(source, cursor, family, true)?;
            escaped = true;
        } else if current == '"' {
            return Ok((
                cursor + 1,
                (!escaped).then(|| source[start..cursor].to_owned()),
            ));
        } else {
            cursor += current.len_utf8();
        }
    }
    Err("unterminated string")
}

fn skip_escape(source: &str, index: usize, family: &str, continuation: bool) -> LexResult<usize> {
    let cursor = index + 1;
    if cursor >= source.len() {
        return Err("unterminated escape");
    }
    let escape = char_at(source, cursor)?;
    if "nrt0\\\"'".contains(escape) && !(family == "c" && escape == '0') {
        return Ok(cursor + escape.len_utf8());
    }
    if escape == 'x' {
        let Some(digits) = source.get(cursor + 1..cursor + 3) else {
            return Err("invalid hexadecimal escape");
        };
        if digits.len() != 2 || !digits.chars().all(|value| value.is_ascii_hexdigit()) {
            return Err("invalid hexadecimal escape");
        }
        let value = u8::from_str_radix(digits, 16).map_err(|_| "invalid hexadecimal escape")?;
        if (family == "str" && value > 0x7f) || (family == "c" && value == 0) {
            return Err("invalid hexadecimal escape for literal family");
        }
        return Ok(cursor + 3);
    }
    if escape == 'u' && family != "byte" {
        if !source[cursor + 1..].starts_with('{') {
            return Err("invalid Unicode escape");
        }
        let start = cursor + 2;
        let end = source[start..]
            .find('}')
            .map(|relative| start + relative)
            .ok_or("unterminated Unicode escape")?;
        let digits = source[start..end].replace('_', "");
        if digits.is_empty()
            || digits.len() > 6
            || !digits.chars().all(|value| value.is_ascii_hexdigit())
        {
            return Err("invalid Unicode escape");
        }
        let scalar = u32::from_str_radix(&digits, 16).map_err(|_| "invalid Unicode escape")?;
        if scalar > 0x10_ffff || (0xd800..=0xdfff).contains(&scalar) {
            return Err("invalid Unicode scalar escape");
        }
        if family == "c" && scalar == 0 {
            return Err("NUL C string escape");
        }
        return Ok(end + 1);
    }
    if continuation && (escape == '\r' || escape == '\n') {
        let mut next = cursor + escape.len_utf8();
        if escape == '\r' && source[next..].starts_with('\n') {
            next += 1;
        }
        while next < source.len() {
            let current = char_at(source, next)?;
            if !current.is_whitespace() {
                break;
            }
            next += current.len_utf8();
        }
        return Ok(next);
    }
    Err("invalid escape")
}

fn char_end(source: &str, index: usize, byte: bool) -> LexResult<Option<usize>> {
    let mut cursor = index + 1;
    if cursor >= source.len() {
        return Ok(None);
    }
    let current = char_at(source, cursor)?;
    if current == '\\' {
        cursor = skip_escape(source, cursor, if byte { "byte" } else { "str" }, false)?;
    } else {
        if current == '\r' || current == '\n' || current == '\'' {
            return Ok(None);
        }
        if byte && !current.is_ascii() {
            return Err("non-ASCII byte character");
        }
        cursor += current.len_utf8();
    }
    Ok(source[cursor..].starts_with('\'').then_some(cursor + 1))
}

fn without_macro_bodies(tokens: &[Token]) -> LexResult<Vec<Token>> {
    let mut filtered = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if tokens.get(index) == Some(&Token::Ident("macro_rules".to_owned()))
            && tokens.get(index + 1) == Some(&Token::Punct('!'))
            && matches!(tokens.get(index + 2), Some(Token::Ident(_)))
            && matches!(tokens.get(index + 3), Some(Token::Punct(ch)) if closing(*ch).is_some())
        {
            let opening = index + 3;
            index = skip_token_group(tokens, opening)?;
            continue;
        }
        if tokens.get(index) == Some(&Token::Punct('!'))
            && (index == 0 || tokens.get(index - 1) != Some(&Token::Punct('#')))
            && matches!(tokens.get(index + 1), Some(Token::Punct(ch)) if closing(*ch).is_some())
        {
            index = skip_token_group(tokens, index + 1)?;
            continue;
        }
        filtered.push(tokens[index].clone());
        index += 1;
    }
    Ok(filtered)
}

fn closing(value: char) -> Option<char> {
    match value {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

fn skip_token_group(tokens: &[Token], index: usize) -> LexResult<usize> {
    let Some(Token::Punct(opening)) = tokens.get(index) else {
        return Err("invalid macro token tree");
    };
    let Some(first_close) = closing(*opening) else {
        return Err("invalid macro token tree");
    };
    let mut stack = vec![first_close];
    for (cursor, token) in tokens.iter().enumerate().skip(index + 1) {
        let Token::Punct(value) = token else {
            continue;
        };
        if let Some(close) = closing(*value) {
            stack.push(close);
        } else if matches!(*value, ')' | ']' | '}') {
            if stack.last() != Some(value) {
                return Err("mismatched macro token tree");
            }
            stack.pop().ok_or("invalid macro token tree")?;
            if stack.is_empty() {
                return Ok(cursor + 1);
            }
        }
    }
    Err("unterminated macro token tree")
}

fn top_level_const_matches(tokens: &[Token], name: &str) -> LexResult<Vec<usize>> {
    let mut matches = Vec::new();
    let mut stack = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if let Token::Punct(value) = token {
            if let Some(close) = closing(*value) {
                stack.push(close);
                continue;
            }
            if matches!(*value, ')' | ']' | '}') {
                if stack.pop() != Some(*value) {
                    return Err("mismatched Rust source delimiter");
                }
                continue;
            }
        }
        if stack.is_empty()
            && matches!(tokens.get(index), Some(Token::Ident(value)) if value == "const")
            && matches!(tokens.get(index + 1), Some(Token::Ident(value)) if value == name)
        {
            matches.push(index);
        }
    }
    if !stack.is_empty() {
        return Err("unterminated Rust source delimiter");
    }
    Ok(matches)
}

fn has_conditional_crate_attribute(tokens: &[Token]) -> LexResult<bool> {
    let mut index = 0;
    while index < tokens.len() {
        if tokens.get(index..index + 3)
            == Some(&[Token::Punct('#'), Token::Punct('!'), Token::Punct('[')])
        {
            if matches!(tokens.get(index + 3), Some(Token::Ident(name)) if name == "cfg" || name == "cfg_attr")
            {
                return Ok(true);
            }
            index = skip_token_group(tokens, index + 2)?;
            continue;
        }
        if matches!(tokens.get(index), Some(Token::Punct(value)) if closing(*value).is_some()) {
            index = skip_token_group(tokens, index)?;
            continue;
        }
        index += 1;
    }
    Ok(false)
}

fn char_at(s: &str, i: usize) -> LexResult<char> {
    s[i..].chars().next().ok_or("invalid Rust source cursor")
}

#[cfg(test)]
#[test]
fn extracts_public_plain_strings_and_rejects_private_constants() {
    let extract = |source| extract_public_string_const(source, "A");
    assert!(extract("pub const A:&str=\"x\";").is_ok_and(|v| v == "x"));
    assert!(extract("const A:&str=\"x\";").is_err());
    assert!(extract("pub const A:&str=\"x\\n\";").is_err());
    assert!(extract(r##"pub const A:&str=r#"x"#;"##).is_err());
    assert!(extract("#![cfg(x)] pub const A:&str=\"x\";").is_err());
    assert!(extract("pub const A:&str=\"\";pub const A:&str=\"\";").is_err());
}
