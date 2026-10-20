use velnor_actions_bun::jsonc::to_json;

fn parse(text: &str) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&to_json(text)?)?)
}

#[test]
fn trailing_commas_and_comments_are_removed() -> Result<(), Box<dyn std::error::Error>> {
    let value = parse("{\n  // line\n  \"a\": [1, 2,], /* block */\n  \"b\": {\"c\": 3,},\n}\n")?;
    assert_eq!(value, serde_json::json!({ "a": [1, 2], "b": { "c": 3 } }));
    Ok(())
}

#[test]
fn string_contents_are_never_rewritten() -> Result<(), Box<dyn std::error::Error>> {
    let value = parse(r#"{"url": "http://x//y", "s": "a,]", "q": "\"//\"", "c": "/* no */",}"#)?;
    assert_eq!(value["url"], "http://x//y");
    assert_eq!(value["s"], "a,]");
    assert_eq!(value["q"], "\"//\"");
    assert_eq!(value["c"], "/* no */");
    Ok(())
}

#[test]
fn unterminated_input_fails_closed() {
    assert!(to_json("{\"a\": \"x").is_err());
    assert!(to_json("{ /* open").is_err());
}
