//! Bounded parser for the small Gradle settings subset used by workloads.
//! The generator never evaluates Kotlin; only a closed top-level subset is accepted.

const MAX_TOKENS: usize = 65_536;
/// Validate settings and, when requested, prove a project is literally included.
pub(super) fn validate(settings: &str, requested: Option<&str>) -> Result<(), &'static str> {
    let tokens = tokenize(settings)?;
    validate_top_level(&tokens)?;
    let requested = requested
        .map(normalize_project)
        .transpose()
        .map_err(|_| "gradle_project_selector_invalid")?;
    let mut includes = Vec::new();
    let mut braces = 0usize;
    let mut parens = 0usize;
    let mut index = 0usize;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Ident(name) if name == "project" => {
                return Err("gradle_settings_project_mapping_unsupported");
            }
            Token::Ident(name) if name == "projectDir" => {
                return Err("gradle_settings_project_mapping_unsupported");
            }
            Token::Ident(name) if name == "includeBuild" => {
                return Err("gradle_settings_composite_build_unsupported");
            }
            Token::Ident(name)
                if matches!(
                    name.as_str(),
                    "apply" | "for" | "fun" | "if" | "let" | "val" | "var" | "when" | "while"
                ) =>
            {
                return Err("gradle_settings_dynamic_logic_unsupported");
            }
            Token::Ident(name) if name == "include" && is_call(&tokens, index) => {
                if braces != 0 || parens != 0 || preceded_by_dot(&tokens, index) {
                    return Err("gradle_settings_include_not_top_level");
                }
                let (arguments, end) = include_arguments(&tokens, index + 2)?;
                includes.extend(arguments);
                index = end;
                continue;
            }
            Token::Ident(name) if name == "include" => {
                return Err("gradle_settings_include_dynamic");
            }
            Token::Punct('{') => {
                braces = braces.checked_add(1).ok_or("gradle_settings_unbalanced")?;
            }
            Token::Punct('}') => {
                braces = braces.checked_sub(1).ok_or("gradle_settings_unbalanced")?;
            }
            Token::Punct('(') => {
                parens = parens.checked_add(1).ok_or("gradle_settings_unbalanced")?;
            }
            Token::Punct(')') => {
                parens = parens.checked_sub(1).ok_or("gradle_settings_unbalanced")?;
            }
            Token::Punct(_) | Token::Literal(_) | Token::Ident(_) => {}
        }
        index += 1;
    }
    if braces != 0 || parens != 0 {
        return Err("gradle_settings_unbalanced");
    }
    if let Some(requested) = requested
        && !includes.iter().any(|entry| entry == &requested)
    {
        return Err("gradle_project_not_literal_include");
    }
    Ok(())
}
/// Normalize a configured selector to its module directory path.
pub(super) fn project_path(value: &str) -> Result<String, &'static str> {
    normalize_project(value)
}
fn is_call(tokens: &[Token], index: usize) -> bool {
    matches!(tokens.get(index + 1), Some(Token::Punct('(')))
}
fn validate_top_level(tokens: &[Token]) -> Result<(), &'static str> {
    let mut index = 0usize;
    while index < tokens.len() {
        if matches!(tokens[index], Token::Punct(';')) {
            index += 1;
            continue;
        }
        match tokens.get(index) {
            Some(Token::Ident(name)) if name == "include" && is_call(tokens, index) => {
                let (_, end) = include_arguments(tokens, index + 2)?;
                index = end;
            }
            Some(Token::Ident(name)) if name == "enableFeaturePreview" => {
                index = literal_call_end(tokens, index)?;
            }
            Some(Token::Ident(name)) if name == "rootProject" => {
                index = root_name_end(tokens, index)?;
            }
            Some(Token::Ident(name)) if allowed_block(name) => {
                let Some(Token::Punct('{')) = tokens.get(index + 1) else {
                    return Err("gradle_settings_top_level_unsupported");
                };
                let end = matching_brace(tokens, index + 1)?;
                scan_nested(&tokens[index + 2..end])?;
                index = end + 1;
            }
            _ => return Err("gradle_settings_top_level_unsupported"),
        }
    }
    Ok(())
}
fn allowed_block(name: &str) -> bool {
    matches!(
        name,
        "pluginManagement" | "dependencyResolutionManagement" | "plugins"
    )
}
fn literal_call_end(tokens: &[Token], index: usize) -> Result<usize, &'static str> {
    if !is_call(tokens, index) {
        return Err("gradle_settings_top_level_unsupported");
    }
    let Some(Token::Literal(value)) = tokens.get(index + 2) else {
        return Err("gradle_settings_dynamic_logic_unsupported");
    };
    if !value.plain || !matches!(tokens.get(index + 3), Some(Token::Punct(')'))) {
        return Err("gradle_settings_dynamic_logic_unsupported");
    }
    Ok(index + 4)
}
fn root_name_end(tokens: &[Token], index: usize) -> Result<usize, &'static str> {
    let valid_prefix = matches!(tokens.get(index), Some(Token::Ident(name)) if name == "rootProject")
        && matches!(tokens.get(index + 1), Some(Token::Punct('.')))
        && matches!(tokens.get(index + 2), Some(Token::Ident(name)) if name == "name")
        && matches!(tokens.get(index + 3), Some(Token::Punct('=')));
    if !valid_prefix {
        return Err("gradle_settings_top_level_unsupported");
    }
    let Some(Token::Literal(value)) = tokens.get(index + 4) else {
        return Err("gradle_settings_dynamic_logic_unsupported");
    };
    if !value.plain {
        return Err("gradle_settings_dynamic_logic_unsupported");
    }
    Ok(index + 5)
}
fn matching_brace(tokens: &[Token], start: usize) -> Result<usize, &'static str> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token {
            Token::Punct('{') => {
                depth = depth.checked_add(1).ok_or("gradle_settings_unbalanced")?;
            }
            Token::Punct('}') => {
                depth = depth.checked_sub(1).ok_or("gradle_settings_unbalanced")?;
                if depth == 0 {
                    return Ok(index);
                }
            }
            _ => {}
        }
    }
    Err("gradle_settings_unbalanced")
}
fn scan_nested(tokens: &[Token]) -> Result<(), &'static str> {
    for token in tokens {
        let Token::Ident(name) = token else { continue };
        if name == "include" {
            return Err("gradle_settings_include_not_top_level");
        }
        if forbidden_identifier(name) || name == "rootProject" {
            return Err(forbidden_error(name));
        }
    }
    Ok(())
}
fn forbidden_identifier(name: &str) -> bool {
    matches!(
        name,
        "project"
            | "projectDir"
            | "includeBuild"
            | "apply"
            | "for"
            | "fun"
            | "if"
            | "let"
            | "val"
            | "var"
            | "when"
            | "while"
    )
}
fn forbidden_error(name: &str) -> &'static str {
    match name {
        "includeBuild" => "gradle_settings_composite_build_unsupported",
        "project" | "projectDir" | "rootProject" => "gradle_settings_project_mapping_unsupported",
        _ => "gradle_settings_dynamic_logic_unsupported",
    }
}
fn preceded_by_dot(tokens: &[Token], index: usize) -> bool {
    index > 0 && matches!(tokens.get(index - 1), Some(Token::Punct('.')))
}
fn include_arguments(
    tokens: &[Token],
    mut index: usize,
) -> Result<(Vec<String>, usize), &'static str> {
    let mut entries = Vec::new();
    let mut expect_entry = true;
    loop {
        match tokens.get(index) {
            Some(Token::Punct(')')) if !entries.is_empty() => {
                return Ok((entries, index + 1));
            }
            Some(Token::Punct(')')) => return Err("gradle_settings_include_empty"),
            Some(Token::Literal(value)) if value.plain && expect_entry => {
                entries.push(normalize_project(&value.value)?);
                expect_entry = false;
                index += 1;
            }
            Some(Token::Punct(',')) if !expect_entry => {
                expect_entry = true;
                index += 1;
            }
            _ => return Err("gradle_settings_include_dynamic"),
        }
    }
}

fn normalize_project(value: &str) -> Result<String, &'static str> {
    let value = value.strip_prefix(':').unwrap_or(value);
    if value.is_empty() {
        return Err("gradle_project_selector_invalid");
    }
    let mut path = String::new();
    for (index, segment) in value.split(':').enumerate() {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.contains('/')
            || segment.contains('\\')
            || segment.starts_with('-')
            || !segment.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'.')
            })
        {
            return Err("gradle_project_selector_invalid");
        }
        if index != 0 {
            path.push('/');
        }
        path.push_str(segment);
    }
    Ok(path)
}

#[derive(Debug)]
enum Token {
    Ident(String),
    Literal(Literal),
    Punct(char),
}

#[derive(Debug)]
struct Literal {
    value: String,
    plain: bool,
}

fn tokenize(source: &str) -> Result<Vec<Token>, &'static str> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            byte if byte.is_ascii_whitespace() => index += 1,
            b'/' if bytes.get(index + 1) == Some(&b'/') => skip_line(bytes, &mut index),
            b'/' if bytes.get(index + 1) == Some(&b'*') => skip_block(bytes, &mut index)?,
            b'"' => {
                let (literal, end) = quoted(bytes, index)?;
                tokens.push(Token::Literal(literal));
                index = end;
            }
            b'\'' => {
                let ((), end) = character(bytes, index)?;
                tokens.push(Token::Punct('\''));
                index = end;
            }
            b'`' => return Err("gradle_settings_backtick_identifier_unsupported"),
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = index;
                index += 1;
                while bytes
                    .get(index)
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                {
                    index += 1;
                }
                let value = std::str::from_utf8(&bytes[start..index])
                    .map_err(|_| "gradle_settings_not_utf8")?;
                tokens.push(Token::Ident(value.to_owned()));
            }
            byte => {
                tokens.push(Token::Punct(byte as char));
                index += 1;
            }
        }
        if tokens.len() > MAX_TOKENS {
            return Err("gradle_settings_too_many_tokens");
        }
    }
    Ok(tokens)
}

fn skip_line(bytes: &[u8], index: &mut usize) {
    *index += 2;
    while *index < bytes.len() && bytes[*index] != b'\n' {
        *index += 1;
    }
}

fn skip_block(bytes: &[u8], index: &mut usize) -> Result<(), &'static str> {
    let mut depth = 1usize;
    *index += 2;
    while *index + 1 < bytes.len() {
        if bytes[*index] == b'/' && bytes[*index + 1] == b'*' {
            depth = depth.checked_add(1).ok_or("gradle_settings_unbalanced")?;
            *index += 2;
        } else if bytes[*index] == b'*' && bytes[*index + 1] == b'/' {
            depth -= 1;
            *index += 2;
            if depth == 0 {
                return Ok(());
            }
        } else {
            *index += 1;
        }
    }
    Err("gradle_settings_unterminated_comment")
}

fn quoted(bytes: &[u8], start: usize) -> Result<(Literal, usize), &'static str> {
    if bytes.get(start..start + 3) == Some(b"\"\"\"") {
        let mut index = start + 3;
        while index + 2 < bytes.len() {
            if bytes.get(index..index + 3) == Some(b"\"\"\"") {
                return Ok((
                    Literal {
                        value: String::new(),
                        plain: false,
                    },
                    index + 3,
                ));
            }
            index += 1;
        }
        return Err("gradle_settings_unterminated_literal");
    }
    let mut index = start + 1;
    let mut value = String::new();
    let mut plain = true;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => return Ok((Literal { value, plain }, index + 1)),
            b'\\' => {
                plain = false;
                index += 2;
            }
            b'$' if bytes.get(index + 1) == Some(&b'{') => {
                plain = false;
                index += 1;
            }
            byte => {
                value.push(byte as char);
                index += 1;
            }
        }
    }
    Err("gradle_settings_unterminated_literal")
}

fn character(bytes: &[u8], start: usize) -> Result<((), usize), &'static str> {
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'\'' => return Ok(((), index + 1)),
            _ => index += 1,
        }
    }
    Err("gradle_settings_unterminated_literal")
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
