use velnor_actions_gradle::lex::*;

fn idents(text: &str) -> Vec<String> {
    tokenize(text)
        .into_iter()
        .filter_map(|t| if let Tok::Ident(s) = t { Some(s) } else { None })
        .collect()
}

#[test]
fn comments_and_strings_hide_code() {
    assert_eq!(idents("a // include(\"x\")\n/* include b */ c"), ["a", "c"]);
    let toks = tokenize("include(\"a\", 'b', \"c${x}\", \"\"\"d\"\"\")");
    let strs: Vec<_> = toks
        .iter()
        .filter_map(|t| {
            if let Tok::Str(s, i) = t {
                Some((s.as_str(), *i))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        strs,
        [("a", false), ("b", false), ("c${x}", true), ("d", false)]
    );
    assert_eq!(idents("`java-library`\nalias"), ["java-library", "alias"]);
}

#[test]
fn unterminated_input_does_not_panic() {
    for text in ["\"abc", "/* x", "`a", "'\\", "\"\"\"x", "$"] {
        assert!(tokenize(text).len() < 16);
    }
}
