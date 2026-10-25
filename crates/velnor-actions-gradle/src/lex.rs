#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Ident(String),

    Str(String, bool),

    Punct(char),

    Newline,
}

#[must_use]
pub fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            out.push(Tok::Newline);
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
        } else if c == '"' || c == '\'' {
            i = string(&chars, i, &mut out);
        } else if c == '`' {
            let end = chars[i + 1..].iter().position(|ch| *ch == '`');
            let end = end.map_or(chars.len(), |pos| i + 1 + pos);
            out.push(Tok::Ident(
                chars[i + 1..end.min(chars.len())].iter().collect(),
            ));
            i = end + 1;
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    out
}

fn string(chars: &[char], start: usize, out: &mut Vec<Tok>) -> usize {
    let quote = chars[start];
    let triple = chars.get(start + 1) == Some(&quote) && chars.get(start + 2) == Some(&quote);
    let mut i = start + if triple { 3 } else { 1 };
    let mut body = String::new();
    let mut interpolated = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && !triple {
            body.push(c);
            if let Some(next) = chars.get(i + 1) {
                body.push(*next);
            }
            i += 2;
            continue;
        }
        let closes = if triple {
            c == quote && chars.get(i + 1) == Some(&quote) && chars.get(i + 2) == Some(&quote)
        } else {
            c == quote
        };
        if closes {
            i += if triple { 3 } else { 1 };
            break;
        }
        if quote == '"' && c == '$' {
            interpolated = true;
        }
        body.push(c);
        i += 1;
    }
    out.push(Tok::Str(body, interpolated));
    i
}
