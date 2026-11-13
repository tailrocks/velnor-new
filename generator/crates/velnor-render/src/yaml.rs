// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) enum Yaml {
    Str(String),
    Expr(String),
    Block(String),
    Int(u64),
    Bool(bool),
    Seq(Vec<Yaml>),
    Map(Vec<(String, Yaml)>),
}

pub(crate) fn emit(root: &[(String, Yaml)]) -> String {
    let mut out = String::new();
    entries(&mut out, root, 0);
    out
}

fn entries(out: &mut String, items: &[(String, Yaml)], indent: usize) {
    for (key, value) in items {
        out.push_str(&" ".repeat(indent));
        out.push_str(&key_text(key));
        out.push(':');
        nested(out, value, indent + 2);
    }
}

fn nested(out: &mut String, value: &Yaml, indent: usize) {
    match value {
        Yaml::Map(items) if !items.is_empty() => {
            out.push('\n');
            entries(out, items, indent);
        }
        Yaml::Seq(items) if !items.is_empty() => {
            out.push('\n');
            for item in items {
                out.push_str(&" ".repeat(indent));
                out.push('-');
                let mut block = String::new();
                nested(&mut block, item, indent + 2);
                match block.strip_prefix('\n') {
                    Some(rest) => {
                        out.push(' ');
                        out.push_str(&rest[indent + 2..]);
                    }
                    None => out.push_str(&block),
                }
            }
        }
        Yaml::Block(text) => {
            out.push_str(" |\n");
            for line in text.replace("${{", "${{ '${{' }}").lines() {
                out.push_str(&" ".repeat(indent));
                out.push_str(line);
                out.push('\n');
            }
        }
        _ => {
            out.push(' ');
            out.push_str(&inline(value));
            out.push('\n');
        }
    }
}

fn inline(value: &Yaml) -> String {
    match value {
        Yaml::Str(text) => scalar(&text.replace("${{", "${{ '${{' }}")),
        Yaml::Expr(text) | Yaml::Block(text) => scalar(text),
        Yaml::Int(number) => number.to_string(),
        Yaml::Bool(flag) => flag.to_string(),
        Yaml::Seq(_) => "[]".to_owned(),
        Yaml::Map(_) => "{}".to_owned(),
    }
}

fn key_text(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        key.to_owned()
    } else {
        scalar(key)
    }
}

fn scalar(text: &str) -> String {
    if !text.chars().all(printable) {
        return double_quoted(text);
    }
    if plain(text) {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "''"))
    }
}

fn plain(text: &str) -> bool {
    const RESERVED: [&str; 11] = [
        "true", "false", "yes", "no", "on", "off", "null", "y", "n", "nan", "inf",
    ];
    text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && !text.ends_with(' ')
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " _./@-".contains(c))
        && !RESERVED.contains(&text.to_ascii_lowercase().as_str())
}

fn printable(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r' | ' '..='~' | '\u{85}' | '\u{a0}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..)
}

fn double_quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if !printable(c) => {
                out.push_str("\\u");
                for shift in [12, 8, 4, 0] {
                    out.extend(char::from_digit((u32::from(c) >> shift) & 0xf, 16));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub(crate) fn entry(key: &str, value: Yaml) -> (String, Yaml) {
    (key.to_owned(), value)
}

pub(crate) fn text(value: &str) -> Yaml {
    Yaml::Str(value.to_owned())
}
