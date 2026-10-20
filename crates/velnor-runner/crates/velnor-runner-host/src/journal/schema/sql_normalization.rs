/// Canonicalize SQLite schema formatting without changing quoted literals or identifiers.
pub(super) fn normalize(sql: &str) -> String {
    if has_sql_comment(sql) {
        return sql.to_owned();
    }
    let mut normalized = String::with_capacity(sql.len());
    let mut quote = None;
    let mut chars = sql.chars().peekable();
    while let Some(character) = chars.next() {
        if let Some(delimiter) = quote {
            normalized.push(character);
            let closing = if delimiter == '[' { ']' } else { delimiter };
            if character == closing {
                if delimiter != '[' && chars.peek() == Some(&closing) {
                    normalized.push(chars.next().unwrap_or(closing));
                } else {
                    quote = None;
                }
            }
        } else if matches!(character, '\'' | '"' | '`' | '[') {
            normalized.push(character);
            quote = Some(character);
        } else if character.is_whitespace() {
            let next = chars.clone().find(|next| !next.is_whitespace());
            let previous_is_punctuation = normalized.chars().last().is_some_and(is_sql_punctuation);
            let next_is_punctuation = next.is_some_and(is_sql_punctuation);
            if !previous_is_punctuation && !next_is_punctuation && !normalized.ends_with(' ') {
                normalized.push(' ');
            }
        } else {
            normalized.push(character.to_ascii_lowercase());
        }
    }
    let normalized = normalize_create_prefix(&normalized);
    let normalized = normalize_left_outer_join(&normalized);
    normalize_operator_spacing(&normalized)
}

fn has_sql_comment(sql: &str) -> bool {
    let bytes = sql.as_bytes();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            let closing = if delimiter == b'[' { b']' } else { delimiter };
            if bytes[index] == closing {
                if delimiter != b'[' && bytes.get(index + 1) == Some(&closing) {
                    index += 1;
                } else {
                    quote = None;
                }
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`' | b'[') {
            quote = Some(bytes[index]);
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"--") || bytes[index..].starts_with(b"/*") {
            return true;
        }
        index += 1;
    }
    false
}

fn normalize_operator_spacing(sql: &str) -> String {
    let operators = [
        "->>", "==", "!=", "<>", "<=", ">=", "||", "<<", ">>", "->", "=", "<", ">", "+", "-", "*",
        "/", "%", "&", "|", "~",
    ];
    let mut output = String::with_capacity(sql.len());
    let mut quote = None;
    let mut index = 0;
    let mut previous_operator = false;
    let mut whitespace_after_operator = false;
    while index < sql.len() {
        let bytes = sql.as_bytes();
        if let Some(delimiter) = quote {
            let closing = if delimiter == b'[' { b']' } else { delimiter };
            let character = sql[index..].chars().next().unwrap_or('\0');
            output.push(character);
            if character.is_ascii() && bytes[index] == closing {
                if delimiter != b'[' && bytes.get(index + 1) == Some(&closing) {
                    index += 1;
                    output.push(char::from(bytes[index]));
                } else {
                    quote = None;
                    previous_operator = false;
                    whitespace_after_operator = false;
                }
            }
            index += character.len_utf8();
            continue;
        }
        let character = sql[index..].chars().next().unwrap_or('\0');
        if character.is_ascii() && matches!(bytes[index], b'\'' | b'"' | b'`' | b'[') {
            quote = Some(bytes[index]);
            output.push(character);
            index += character.len_utf8();
            continue;
        }
        if let Some(operator) = operators
            .iter()
            .find(|operator| bytes[index..].starts_with(operator.as_bytes()))
        {
            if previous_operator && whitespace_after_operator {
                output.push(' ');
            } else {
                while output.ends_with(' ') {
                    output.pop();
                }
            }
            output.push_str(operator);
            index += operator.len();
            let whitespace_start = index;
            while sql
                .as_bytes()
                .get(index)
                .is_some_and(u8::is_ascii_whitespace)
            {
                index += 1;
            }
            previous_operator = true;
            whitespace_after_operator = index > whitespace_start;
            continue;
        }
        output.push(character);
        if !character.is_whitespace() {
            previous_operator = false;
            whitespace_after_operator = false;
        }
        index += character.len_utf8();
    }
    output
}

fn normalize_create_prefix(sql: &str) -> String {
    for prefix in [
        ("create table if not exists ", "create table "),
        ("create index if not exists ", "create index "),
        ("create trigger if not exists ", "create trigger "),
    ] {
        if let Some(rest) = sql.strip_prefix(prefix.0) {
            return format!("{}{}", prefix.1, rest);
        }
    }
    sql.to_owned()
}

fn normalize_left_outer_join(sql: &str) -> String {
    const CANONICAL: &str = "left outer join";
    let mut output = String::with_capacity(sql.len());
    let mut index = 0;
    let mut quote = None;
    let bytes = sql.as_bytes();
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            let closing = if delimiter == b'[' { b']' } else { delimiter };
            let character = sql[index..].chars().next().unwrap_or('\0');
            output.push(character);
            if character.is_ascii() && bytes[index] == closing {
                if delimiter != b'[' && bytes.get(index + 1) == Some(&closing) {
                    index += 1;
                    output.push(char::from(bytes[index]));
                } else {
                    quote = None;
                }
            }
            index += character.len_utf8();
            continue;
        }
        let character = sql[index..].chars().next().unwrap_or('\0');
        if character.is_ascii() && matches!(bytes[index], b'\'' | b'"' | b'`' | b'[') {
            quote = Some(bytes[index]);
            output.push(character);
            index += character.len_utf8();
            continue;
        }
        if bytes[index..].starts_with(CANONICAL.as_bytes()) {
            let before = output.as_bytes().last().copied();
            let after = bytes.get(index + CANONICAL.len()).copied();
            if before.is_none_or(|value| value.is_ascii_whitespace())
                && after.is_none_or(|value| value.is_ascii_whitespace())
            {
                output.push_str("left join");
                index += CANONICAL.len();
                continue;
            }
        }
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn is_sql_punctuation(character: char) -> bool {
    matches!(character, '(' | ')' | ',' | ';' | '.')
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn preserves_quoted_text_and_word_boundaries() {
        assert_ne!(
            normalize("CHECK(state='ready_held')"),
            normalize("CHECK(state='ready_ held')")
        );
        assert_ne!(
            normalize("CHECK(path='source-proof/inbox')"),
            normalize("CHECK(path='source-proof/in box')")
        );
        assert_ne!(
            normalize("CHECK(state='ready_held')"),
            normalize("CHECK(state='readyheld')")
        );
        assert_eq!(
            normalize("CREATE TABLE IF NOT EXISTS x (a TEXT)"),
            normalize("CREATE TABLE x ( a TEXT )")
        );
        assert_eq!(
            normalize("SELECT alpha  \n\t beta"),
            normalize("SELECT alpha beta")
        );
        assert_ne!(
            normalize("SELECT 'alpha  beta'"),
            normalize("SELECT 'alpha beta'")
        );
        assert_eq!(normalize("SELECT 'alpha  beta'"), "select 'alpha  beta'");
        assert_ne!(
            normalize("CREATE TABLE x (a TEXT CHECK(a = 'ready'))"),
            normalize("CREATE TABLE x (a TEXT)")
        );
        let unicode = "SELECT 'é ẞ', \"münchen\"";
        let normalized_unicode = normalize(unicode);
        assert_eq!(normalized_unicode, "select 'é ẞ',\"münchen\"");
        assert_eq!(normalize(&normalized_unicode), normalized_unicode);
        assert_eq!(
            normalize("SELECT 'a''é', \"a\"\"é\""),
            "select 'a''é',\"a\"\"é\""
        );
    }

    #[tokio::test]
    async fn operator_spacing_keeps_sql_tokens_and_comment_boundaries_distinct() {
        assert_eq!(normalize("SELECT a=b"), normalize("SELECT a = b"));
        assert_eq!(
            normalize("CHECK(a='x'||'y')"),
            normalize("CHECK(a = 'x' || 'y')")
        );
        assert_eq!(
            normalize("CHECK(a=\"x\"+1)"),
            normalize("CHECK(a = \"x\" + 1)")
        );
        assert_ne!(
            normalize("CREATE TABLE x(a INTEGER CHECK(a - -1 >0));"),
            normalize("CREATE TABLE x(a INTEGER CHECK(a--1 >0\n));")
        );
        assert_ne!(normalize("SELECT a==b"), normalize("SELECT a= =b"));
        assert_ne!(
            normalize("SELECT a/* comment */b"),
            normalize("SELECT a / * comment */b")
        );
        assert_ne!(normalize("SELECT 'a=b'"), normalize("SELECT 'a = b'"));

        let database = turso::Builder::new_local(":memory:")
            .build()
            .await
            .expect("build local SQLite database");
        let connection = database.connect().expect("open local connection");
        connection
            .execute(
                "CREATE TABLE spaced_check (value INTEGER CHECK(value - -1 > 0))",
                (),
            )
            .await
            .expect("create arithmetic check");
        connection
            .execute(
                "CREATE TABLE commented_check (value INTEGER CHECK(value--1 > 0\n))",
                (),
            )
            .await
            .expect("create line-comment check");
        assert!(
            connection
                .execute("INSERT INTO spaced_check VALUES (-1)", ())
                .await
                .is_err()
        );
        connection
            .execute("INSERT INTO commented_check VALUES (-1)", ())
            .await
            .expect("comment removes the apparent check expression");
    }
}
